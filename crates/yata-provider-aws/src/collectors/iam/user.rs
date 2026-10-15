//! Collector for `aws.iam.user`: IAM users with their attached managed policies, the names of
//! their inline policies and their row of the credential report (`ListUsers`,
//! `ListAttachedUserPolicies`, `ListUserPolicies`, `GetCredentialReport`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use aws_sdk_iam::config::{AsyncSleep, SharedAsyncSleep};
use aws_sdk_iam::types::{ReportFormatType, ReportStateType, User};
use serde_json::{Map, Value, json};
use yata_core::{CollectionErrorKind, Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::convert::{datetime_to_rfc3339, opt_string};
use crate::support::csv::parse_csv;
use crate::support::errors::{CollectError, failure_mark};
use crate::support::fan_out::fan_out;

const RESOURCE_TYPE: &str = "aws.iam.user";
const LIST_USERS: &str = "ListUsers";
const LIST_ATTACHED: &str = "ListAttachedUserPolicies";
const LIST_INLINE: &str = "ListUserPolicies";
const GENERATE_REPORT: &str = "GenerateCredentialReport";
const GET_REPORT: &str = "GetCredentialReport";

/// The first wait between two `GenerateCredentialReport` calls.
const REPORT_POLL_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
/// The longest single wait. Each wait is twice the previous one, up to this value.
const REPORT_POLL_MAX_INTERVAL: Duration = Duration::from_secs(8);
/// The longest total wait. A wait that would exceed it is not made; polling times out instead.
const REPORT_POLL_MAX_TOTAL_WAIT: Duration = Duration::from_secs(120);

/// Returns the collector of `aws.iam.user`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(UserCollector)
}

struct UserCollector;

impl Collector for UserCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &[
            "iam:ListUsers",
            "iam:ListAttachedUserPolicies",
            "iam:ListUserPolicies",
            "iam:GenerateCredentialReport",
            "iam:GetCredentialReport",
        ]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

/// The values of the two per-user keys: `ListAttachedUserPolicies` and `ListUserPolicies`.
type UserDetails = (Value, Value);

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let sleep = {
        let client: aws_sdk_iam::Client = ctx.client(region);
        client.config().sleep_impl()
    };
    collect_with_sleep(ctx, region, sleep).await
}

/// Collects the users. `sleep` is the asynchronous sleep used between the polls of
/// `GenerateCredentialReport`; tests pass a fake one.
async fn collect_with_sleep(
    ctx: &CollectContext,
    region: &str,
    sleep: Option<SharedAsyncSleep>,
) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_iam::Client = ctx.client(region);

    // Step 1: all pages of ListUsers. Any failure fails the whole group.
    let entries = list_users(&client).await?;

    // Step 2: without users there is nothing to attach the report to, so do not generate it.
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    // Step 3: the per-user calls (bounded) and the credential report, concurrently.
    // Note: if the per-user calls fail (a task panic), the report polling still runs to its end
    // before the error is returned (`join!` waits for both).
    let names: Vec<String> = entries.iter().map(|(_, name, _)| name.clone()).collect();
    let (details, report) = tokio::join!(
        fan_out(ctx.fan_out_limit(), names, {
            let client = client.clone();
            move |name: String| {
                let client = client.clone();
                async move {
                    let attached = list_attached_policies(&client, &name).await;
                    let inline = list_inline_policies(&client, &name).await;
                    (attached, inline)
                }
            }
        }),
        fetch_report(&client, sleep.as_ref()),
    );
    let details: Vec<UserDetails> = details?;
    warn_on_failure_marks(LIST_ATTACHED, details.iter().map(|(attached, _)| attached));
    warn_on_failure_marks(LIST_INLINE, details.iter().map(|(_, inline)| inline));

    // Step 4: the value of `GetCredentialReport` of each user.
    let arns: Vec<&str> = entries.iter().map(|(arn, _, _)| arn.as_str()).collect();
    let report_values = report_values(&report, &arns);

    // Step 5: assemble the resources.
    let resources = entries
        .into_iter()
        .zip(details)
        .zip(report_values)
        .map(|(((arn, name, user), (attached, inline)), report)| {
            let mut configuration = Map::new();
            configuration.insert(LIST_USERS.to_owned(), user);
            configuration.insert(LIST_ATTACHED.to_owned(), attached);
            configuration.insert(LIST_INLINE.to_owned(), inline);
            configuration.insert(GET_REPORT.to_owned(), report);
            Resource {
                id: arn,
                resource_type: RESOURCE_TYPE.to_owned(),
                account: ctx.account_key().to_owned(),
                region: "global".to_owned(),
                name: Some(name),
                tags: BTreeMap::new(),
                configuration,
            }
        })
        .collect();
    Ok(resources)
}

/// The credential report after the checks and the CSV parsing.
struct ParsedReport {
    /// `GeneratedTime` as an RFC 3339 string, or `null` when the API returned none.
    generated_time: Value,
    /// `ReportFormat` as returned (always `text/csv` here).
    report_format: String,
    /// Column names of the header row (no duplicates).
    header: Vec<String>,
    /// Data rows; every row has as many values as `header`.
    rows: Vec<Vec<String>>,
    /// Position of the `arn` column in `header`.
    arn_column: usize,
}

/// Generates (if needed), gets and parses the credential report.
async fn fetch_report(
    client: &aws_sdk_iam::Client,
    sleep: Option<&SharedAsyncSleep>,
) -> Result<ParsedReport, CollectError> {
    wait_for_report(client, sleep).await?;
    let output = client
        .get_credential_report()
        .send()
        .await
        .map_err(|err| CollectError::from_sdk(GET_REPORT, &err))?;

    let report_format = match output.report_format() {
        None => return Err(invalid_response(GET_REPORT, "MissingReportFormat")),
        Some(ReportFormatType::TextCsv) => ReportFormatType::TextCsv.as_str().to_owned(),
        Some(_) => return Err(invalid_response(GET_REPORT, "UnsupportedReportFormat")),
    };
    let content = output
        .content()
        .ok_or_else(|| invalid_response(GET_REPORT, "MissingContent"))?;
    let generated_time = match output.generated_time() {
        Some(time) => Value::String(
            datetime_to_rfc3339(time)
                .ok_or_else(|| invalid_response(GET_REPORT, "InvalidDateTime"))?,
        ),
        None => Value::Null,
    };
    let text = std::str::from_utf8(content.as_ref())
        .map_err(|_| invalid_response(GET_REPORT, "InvalidUtf8"))?;

    let table = parse_csv(text).map_err(|err| invalid_response(GET_REPORT, err.code()))?;
    let mut seen = HashSet::new();
    if !table.header.iter().all(|name| seen.insert(name.as_str())) {
        return Err(invalid_response(GET_REPORT, "DuplicateColumn"));
    }
    let arn_column = table
        .header
        .iter()
        .position(|name| name == "arn")
        .ok_or_else(|| invalid_response(GET_REPORT, "MissingArnColumn"))?;
    Ok(ParsedReport {
        generated_time,
        report_format,
        header: table.header,
        rows: table.rows,
        arn_column,
    })
}

/// Calls `GenerateCredentialReport` until its state is `COMPLETE`, waiting between calls
/// (1, 2, 4, 8, 8, ... seconds, at most 120 seconds in total).
async fn wait_for_report(
    client: &aws_sdk_iam::Client,
    sleep: Option<&SharedAsyncSleep>,
) -> Result<(), CollectError> {
    let mut interval = REPORT_POLL_INITIAL_INTERVAL;
    let mut waited = Duration::ZERO;
    loop {
        let output = client
            .generate_credential_report()
            .send()
            .await
            .map_err(|err| CollectError::from_sdk(GENERATE_REPORT, &err))?;
        match output.state() {
            Some(ReportStateType::Complete) => return Ok(()),
            Some(ReportStateType::Started | ReportStateType::Inprogress) => {
                if waited + interval > REPORT_POLL_MAX_TOTAL_WAIT {
                    tracing::debug!(state = ?output.state(), "credential report is not ready; giving up");
                    return Err(CollectError::new(
                        CollectionErrorKind::Timeout,
                        format!("timeout when calling {GENERATE_REPORT} (PollingLimitReached)"),
                    ));
                }
                tracing::debug!(
                    state = ?output.state(),
                    next_wait_secs = interval.as_secs(),
                    "credential report is not ready"
                );
                let Some(sleep) = sleep else {
                    return Err(CollectError::new(
                        CollectionErrorKind::Service,
                        format!(
                            "service error when calling {GENERATE_REPORT} (NoSleepImplementation)"
                        ),
                    ));
                };
                sleep.sleep(interval).await;
                waited += interval;
                interval = (interval * 2).min(REPORT_POLL_MAX_INTERVAL);
            }
            None => return Err(invalid_response(GENERATE_REPORT, "MissingState")),
            Some(other) => {
                tracing::debug!(state = other.as_str(), "unknown credential report state");
                return Err(invalid_response(GENERATE_REPORT, "UnknownState"));
            }
        }
    }
}

/// An `invalid_response` error for `operation` with the reason `code`.
fn invalid_response(operation: &str, code: &str) -> CollectError {
    CollectError::new(
        CollectionErrorKind::InvalidResponse,
        format!("invalid response when calling {operation} ({code})"),
    )
}

/// The `warn` message for a report that could not be fetched (no ARNs, only a count).
fn unavailable_message(reason: &str, users: usize) -> String {
    format!("credential report unavailable: {reason}; marked {users} users")
}

/// The `warn` message for users whose row is missing or duplicated (no ARNs, only counts).
fn user_row_message(count: usize, users: usize, problem: &str, generated: &str) -> String {
    format!(
        "{count} of {users} users have {problem} the credential report generated at {generated}"
    )
}

/// Decides the `GetCredentialReport` value of each user (in the order of `arns`).
///
/// A report that could not be fetched marks every user. Otherwise a user gets its row, or a
/// failure mark when it has no row or more than one. Rows without a user are not stored.
fn report_values(report: &Result<ParsedReport, CollectError>, arns: &[&str]) -> Vec<Value> {
    let report = match report {
        Ok(report) => report,
        Err(err) => {
            tracing::warn!("{}", unavailable_message(err.message(), arns.len()));
            return vec![err.to_failure_mark(); arns.len()];
        }
    };

    let mut by_arn: HashMap<&str, Vec<&Vec<String>>> = HashMap::new();
    for row in &report.rows {
        if let Some(arn) = row.get(report.arn_column) {
            by_arn.entry(arn.as_str()).or_default().push(row);
        }
    }

    let mut missing: Vec<&str> = Vec::new();
    let mut duplicated: Vec<&str> = Vec::new();
    let values: Vec<Value> = arns
        .iter()
        .map(|arn| match by_arn.get(arn).map(Vec::as_slice) {
            Some([row]) => {
                let content: Map<String, Value> = report
                    .header
                    .iter()
                    .zip(row.iter())
                    .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                    .collect();
                json!({
                    "GeneratedTime": report.generated_time,
                    "ReportFormat": report.report_format,
                    "Content": content,
                })
            }
            Some([_, _, ..]) => {
                duplicated.push(arn);
                invalid_response(GET_REPORT, "DuplicateArn").to_failure_mark()
            }
            _ => {
                missing.push(arn);
                invalid_response(GET_REPORT, "MissingUserRow").to_failure_mark()
            }
        })
        .collect();

    let generated = report.generated_time.as_str().unwrap_or("unknown time");
    if !missing.is_empty() {
        tracing::warn!(
            "{}",
            user_row_message(missing.len(), arns.len(), "no row in", generated)
        );
        tracing::debug!(arns = ?missing, "users without a row in the credential report");
    }
    if !duplicated.is_empty() {
        tracing::warn!(
            "{}",
            user_row_message(duplicated.len(), arns.len(), "more than one row in", generated)
        );
        tracing::debug!(arns = ?duplicated, "users with several rows in the credential report");
    }

    let user_arns: HashSet<&str> = arns.iter().copied().collect();
    // Count rows (not distinct ARNs); sort the ARNs so that the log is deterministic.
    let mut root_rows = 0usize;
    let mut other_rows = 0usize;
    let mut other_arns: Vec<&str> = Vec::new();
    for (arn, rows) in by_arn.iter().filter(|(arn, _)| !user_arns.contains(**arn)) {
        if arn.ends_with(":root") {
            root_rows += rows.len();
        } else {
            other_rows += rows.len();
            other_arns.push(arn);
        }
    }
    other_arns.sort_unstable();
    if root_rows > 0 || other_rows > 0 {
        tracing::debug!(
            root_rows,
            other_rows,
            arns = ?other_arns,
            "credential report rows without a user are not stored"
        );
    }
    values
}

/// One listed user: its ARN, its name, and the `ListUsers` value of its resource.
type UserEntry = (String, String, Value);

/// Fetches all pages of `ListUsers` in the order the API returned them.
///
/// The value of each user keeps the shape of the operation output: an object holding only the
/// array `Users` with the element of that user (paging fields are not included).
async fn list_users(client: &aws_sdk_iam::Client) -> Result<Vec<UserEntry>, CollectError> {
    let mut entries = Vec::new();
    let mut pages = client.list_users().into_paginator().send();
    while let Some(page) = pages.next().await {
        let page = page.map_err(|err| CollectError::from_sdk(LIST_USERS, &err))?;
        for user in &page.users {
            entries.push((
                user.arn().to_owned(),
                user.user_name().to_owned(),
                json!({ "Users": [user_element(user)?] }),
            ));
        }
    }
    Ok(entries)
}

/// Converts an element of `ListUsers` to JSON (all eight members).
fn user_element(user: &User) -> Result<Value, CollectError> {
    let create_date = datetime_to_rfc3339(user.create_date())
        .ok_or_else(|| invalid_datetime(LIST_USERS))?;
    let password_last_used = match user.password_last_used() {
        Some(date) => {
            Value::String(datetime_to_rfc3339(date).ok_or_else(|| invalid_datetime(LIST_USERS))?)
        }
        None => Value::Null,
    };
    let permissions_boundary = match user.permissions_boundary() {
        Some(boundary) => json!({
            "PermissionsBoundaryArn": opt_string(boundary.permissions_boundary_arn()),
            "PermissionsBoundaryType": opt_string(
                boundary.permissions_boundary_type().map(|t| t.as_str())
            ),
        }),
        None => Value::Null,
    };
    let tags = match user.tags.as_deref() {
        Some(tags) => Value::Array(
            tags.iter()
                .map(|tag| json!({ "Key": tag.key(), "Value": tag.value() }))
                .collect(),
        ),
        None => Value::Null,
    };
    Ok(json!({
        "Arn": user.arn(),
        "CreateDate": create_date,
        "PasswordLastUsed": password_last_used,
        "Path": user.path(),
        "PermissionsBoundary": permissions_boundary,
        "Tags": tags,
        "UserId": user.user_id(),
        "UserName": user.user_name(),
    }))
}

/// The error for a timestamp that cannot be formatted.
fn invalid_datetime(operation: &str) -> CollectError {
    CollectError::new(
        CollectionErrorKind::InvalidResponse,
        format!("invalid response when calling {operation} (InvalidDateTime)"),
    )
}

/// Calls `ListAttachedUserPolicies` for a user (all pages). Returns the value, or a failure
/// mark if any page fails.
async fn list_attached_policies(client: &aws_sdk_iam::Client, user_name: &str) -> Value {
    // `None` until a page returns the array; pages without it do not turn it into `[]`.
    let mut policies: Option<Vec<Value>> = None;
    let mut pages = client
        .list_attached_user_policies()
        .user_name(user_name)
        .into_paginator()
        .send();
    while let Some(page) = pages.next().await {
        match page {
            Ok(page) => {
                if let Some(attached) = page.attached_policies {
                    policies.get_or_insert_with(Vec::new).extend(attached.iter().map(
                        |policy| {
                            json!({
                                "PolicyArn": policy.policy_arn(),
                                "PolicyName": policy.policy_name(),
                            })
                        },
                    ));
                }
            }
            Err(err) => {
                tracing::debug!(user_name, "{LIST_ATTACHED} failed");
                return failure_mark(LIST_ATTACHED, &err);
            }
        }
    }
    json!({ "AttachedPolicies": policies })
}

/// Calls `ListUserPolicies` for a user (all pages). Returns the value, or a failure mark if
/// any page fails.
async fn list_inline_policies(client: &aws_sdk_iam::Client, user_name: &str) -> Value {
    let mut names: Vec<String> = Vec::new();
    let mut pages = client
        .list_user_policies()
        .user_name(user_name)
        .into_paginator()
        .send();
    while let Some(page) = pages.next().await {
        match page {
            Ok(page) => names.extend(page.policy_names),
            Err(err) => {
                tracing::debug!(user_name, "{LIST_INLINE} failed");
                return failure_mark(LIST_INLINE, &err);
            }
        }
    }
    json!({ "PolicyNames": names })
}

/// Logs one `warn` line for an operation when at least one of its values is a failure mark.
fn warn_on_failure_marks<'a>(operation: &str, values: impl Iterator<Item = &'a Value>) {
    let mut total = 0usize;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        total += 1;
        if let Some(kind) = value["yata_error"]["kind"].as_str() {
            *by_kind.entry(kind.to_owned()).or_insert(0) += 1;
        }
    }
    let failed: usize = by_kind.values().sum();
    if failed > 0 {
        let kinds: Vec<String> = by_kind.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        tracing::warn!(
            "{failed} of {total} {operation} calls failed ({})",
            kinds.join(", ")
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::support::clients::testing::{Sent, mock_factory, mock_factory_counted};
    use crate::support::golden::{assert_golden, golden_path};
    use aws_sdk_iam::config::Sleep;
    use aws_sdk_iam::error::ErrorMetadata;
    use aws_sdk_iam::operation::generate_credential_report::{
        GenerateCredentialReportError, GenerateCredentialReportOutput,
    };
    use aws_sdk_iam::operation::get_credential_report::{
        GetCredentialReportError, GetCredentialReportOutput,
    };
    use aws_sdk_iam::operation::list_attached_user_policies::{
        ListAttachedUserPoliciesError, ListAttachedUserPoliciesInput,
        ListAttachedUserPoliciesOutput,
    };
    use aws_sdk_iam::operation::list_user_policies::{
        ListUserPoliciesError, ListUserPoliciesInput, ListUserPoliciesOutput,
    };
    use aws_sdk_iam::operation::list_users::{ListUsersError, ListUsersInput, ListUsersOutput};
    use aws_sdk_iam::primitives::{Blob, DateTime};
    use aws_sdk_iam::types::{AttachedPermissionsBoundary, AttachedPolicy, PermissionsBoundaryAttachmentType, Tag};
    use aws_sdk_s3::primitives::SdkBody;
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};

    fn context_with(factory: crate::support::clients::ClientFactory) -> CollectContext {
        CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            factory,
        )
    }

    fn context(rules: &[Rule]) -> CollectContext {
        context_with(mock_factory(rules))
    }

    /// A sleep that records the requested durations and returns at once.
    #[derive(Debug, Clone, Default)]
    struct RecordingSleep(Arc<Mutex<Vec<Duration>>>);

    impl RecordingSleep {
        fn waits(&self) -> Vec<Duration> {
            self.0.lock().unwrap().clone()
        }
    }

    impl AsyncSleep for RecordingSleep {
        fn sleep(&self, duration: Duration) -> Sleep {
            self.0.lock().unwrap().push(duration);
            Sleep::new(async {})
        }
    }

    /// Runs the collector with a fake sleep, so that tests never wait.
    async fn collect_fast(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
        let sleep = SharedAsyncSleep::new(RecordingSleep::default());
        collect_with_sleep(ctx, region, Some(sleep)).await
    }

    fn metadata(code: &'static str) -> ErrorMetadata {
        ErrorMetadata::builder()
            .code(code)
            .message("failure for arn:aws:iam::123456789012:user/example-user")
            .build()
    }

    fn arn_of(path: &str, name: &str) -> String {
        format!("arn:aws:iam::123456789012:user{path}{name}")
    }

    fn user(name: &str, path: &str) -> User {
        User::builder()
            .path(path)
            .user_name(name)
            .user_id(format!("AIDAEXAMPLE{}", name.to_uppercase()))
            .arn(arn_of(path, name))
            .create_date(DateTime::from_secs(1_767_225_600))
            .build()
            .unwrap()
    }

    fn marker_of(index: usize) -> Option<String> {
        (index > 0).then(|| format!("marker-{index}"))
    }

    /// Rules for `ListUsers` that return `pages` in order. Each rule also checks that the
    /// default parameters are used.
    fn list_users_rules(pages: Vec<Vec<User>>) -> Vec<Rule> {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(index, users)| {
                let expected = marker_of(index);
                mock!(aws_sdk_iam::Client::list_users)
                    .match_requests(move |input: &ListUsersInput| {
                        input.marker() == expected.as_deref()
                            && input.path_prefix().is_none()
                            && input.max_items().is_none()
                    })
                    .then_output(move || {
                        let last = index + 1 == count;
                        ListUsersOutput::builder()
                            .set_users(Some(users.clone()))
                            .is_truncated(!last)
                            .set_marker((!last).then(|| format!("marker-{}", index + 1)))
                            .build()
                            .unwrap()
                    })
            })
            .collect()
    }

    fn attached(name: &str) -> AttachedPolicy {
        AttachedPolicy::builder()
            .policy_name(name)
            .policy_arn(format!("arn:aws:iam::aws:policy/{name}"))
            .build()
    }

    /// Rules for `ListAttachedUserPolicies` of one user; `None` is a page without the array.
    fn attached_rules(user_name: &str, pages: Vec<Option<Vec<AttachedPolicy>>>) -> Vec<Rule> {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(index, policies)| {
                let expected = marker_of(index);
                let name = user_name.to_owned();
                mock!(aws_sdk_iam::Client::list_attached_user_policies)
                    .match_requests(move |input: &ListAttachedUserPoliciesInput| {
                        input.user_name() == Some(name.as_str())
                            && input.marker() == expected.as_deref()
                            && input.path_prefix().is_none()
                            && input.max_items().is_none()
                    })
                    .then_output(move || {
                        let last = index + 1 == count;
                        ListAttachedUserPoliciesOutput::builder()
                            .set_attached_policies(policies.clone())
                            .is_truncated(!last)
                            .set_marker((!last).then(|| format!("marker-{}", index + 1)))
                            .build()
                    })
            })
            .collect()
    }

    /// Rules for `ListUserPolicies` of one user.
    fn inline_rules(user_name: &str, pages: Vec<Vec<&str>>) -> Vec<Rule> {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(index, names)| {
                let expected = marker_of(index);
                let name = user_name.to_owned();
                let names: Vec<String> = names.into_iter().map(str::to_owned).collect();
                mock!(aws_sdk_iam::Client::list_user_policies)
                    .match_requests(move |input: &ListUserPoliciesInput| {
                        input.user_name() == Some(name.as_str())
                            && input.marker() == expected.as_deref()
                            && input.max_items().is_none()
                    })
                    .then_output(move || {
                        let last = index + 1 == count;
                        ListUserPoliciesOutput::builder()
                            .set_policy_names(Some(names.clone()))
                            .is_truncated(!last)
                            .set_marker((!last).then(|| format!("marker-{}", index + 1)))
                            .build()
                            .unwrap()
                    })
            })
            .collect()
    }

    fn attached_error_rule(user_name: &str, code: &'static str) -> Rule {
        let name = user_name.to_owned();
        mock!(aws_sdk_iam::Client::list_attached_user_policies)
            .match_requests(move |input: &ListAttachedUserPoliciesInput| {
                input.user_name() == Some(name.as_str())
            })
            .then_error(move || ListAttachedUserPoliciesError::generic(metadata(code)))
    }

    fn inline_error_rule(user_name: &str, code: &'static str) -> Rule {
        let name = user_name.to_owned();
        mock!(aws_sdk_iam::Client::list_user_policies)
            .match_requests(move |input: &ListUserPoliciesInput| {
                input.user_name() == Some(name.as_str())
            })
            .then_error(move || ListUserPoliciesError::generic(metadata(code)))
    }

    /// Three users on two `ListUsers` pages, all with empty policy lists except where
    /// the test adds rules.
    fn three_user_rules() -> Vec<Rule> {
        let mut rules = list_users_rules(vec![
            vec![user("alice", "/"), user("bob", "/team/")],
            vec![user("carol", "/")],
        ]);
        rules.extend(attached_rules(
            "alice",
            vec![Some(vec![attached("ReadOnlyAccess")])],
        ));
        rules.extend(attached_rules("bob", vec![Some(Vec::new())]));
        rules.extend(attached_rules("carol", vec![None]));
        rules.extend(inline_rules("alice", vec![vec!["example-inline-policy"]]));
        rules.extend(inline_rules("bob", vec![Vec::new()]));
        rules.extend(inline_rules("carol", vec![Vec::new()]));
        rules.extend(report_rules(&[("alice", "/"), ("bob", "/team/"), ("carol", "/")]));
        rules
    }

    // ----- credential report helpers -----

    /// The columns of the credential report (dummy data in the documented layout).
    const REPORT_HEADER: &str = "user,arn,user_creation_time,password_enabled,password_last_used,\
        password_last_changed,password_next_rotation,mfa_active,access_key_1_active,\
        access_key_1_last_rotated,access_key_1_last_used_date,access_key_1_last_used_region,\
        access_key_1_last_used_service,access_key_2_active,access_key_2_last_rotated,\
        access_key_2_last_used_date,access_key_2_last_used_region,access_key_2_last_used_service,\
        cert_1_active,cert_1_last_rotated,cert_2_active,cert_2_last_rotated";

    /// One CSV row of the report. `user` and `arn` are CSV fields (already quoted if needed).
    fn report_row(user: &str, arn: &str) -> String {
        format!(
            "{user},{arn},2026-01-01T00:00:00+00:00,TRUE,no_information,\
             2026-01-01T00:00:00+00:00,N/A,FALSE,TRUE,2026-01-01T00:00:00+00:00,N/A,N/A,N/A,\
             FALSE,N/A,N/A,N/A,N/A,FALSE,N/A,FALSE,N/A"
        )
    }

    fn report_csv(rows: &[String]) -> String {
        let mut csv = format!("{REPORT_HEADER}\n");
        for row in rows {
            csv.push_str(row);
            csv.push('\n');
        }
        csv
    }

    /// The report text with one row per `(name, path)` user.
    fn report_csv_of(users: &[(&str, &str)]) -> String {
        let rows: Vec<String> = users
            .iter()
            .map(|(name, path)| report_row(name, &arn_of(path, name)))
            .collect();
        report_csv(&rows)
    }

    fn root_row() -> String {
        report_row("<root_account>", "arn:aws:iam::123456789012:root")
    }

    fn generated_time() -> DateTime {
        DateTime::from_secs(1_767_225_600)
    }

    /// A `GenerateCredentialReport` rule that returns `states` in order.
    fn generate_sequence(states: Vec<ReportStateType>) -> Rule {
        let mut sequence = mock!(aws_sdk_iam::Client::generate_credential_report).sequence();
        for state in states {
            sequence = sequence
                .output(move || GenerateCredentialReportOutput::builder().state(state.clone()).build());
        }
        sequence.build()
    }

    /// A `GenerateCredentialReport` rule that returns `state` for every call.
    fn generate_forever(state: ReportStateType) -> Rule {
        mock!(aws_sdk_iam::Client::generate_credential_report)
            .sequence()
            .output(move || GenerateCredentialReportOutput::builder().state(state.clone()).build())
            .repeatedly()
            .build()
    }

    fn generate_error_rule(code: &'static str) -> Rule {
        mock!(aws_sdk_iam::Client::generate_credential_report)
            .then_error(move || GenerateCredentialReportError::generic(metadata(code)))
    }

    /// A `GetCredentialReport` rule that returns `content` as text/csv.
    fn get_rule_bytes(content: Vec<u8>) -> Rule {
        mock!(aws_sdk_iam::Client::get_credential_report).then_output(move || {
            GetCredentialReportOutput::builder()
                .content(Blob::new(content.clone()))
                .report_format(ReportFormatType::TextCsv)
                .generated_time(generated_time())
                .build()
        })
    }

    fn get_rule(csv: &str) -> Rule {
        get_rule_bytes(csv.as_bytes().to_vec())
    }

    fn get_error_rule(code: &'static str) -> Rule {
        mock!(aws_sdk_iam::Client::get_credential_report)
            .then_error(move || GetCredentialReportError::generic(metadata(code)))
    }

    fn xml_rule_generate(state: &str) -> Rule {
        let body = format!(
            "<GenerateCredentialReportResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
             <GenerateCredentialReportResult><State>{state}</State></GenerateCredentialReportResult>\
             <ResponseMetadata><RequestId>example</RequestId></ResponseMetadata>\
             </GenerateCredentialReportResponse>"
        );
        mock!(aws_sdk_iam::Client::generate_credential_report).then_http_response(move || {
            HttpResponse::new(StatusCode::try_from(200u16).unwrap(), SdkBody::from(body.clone()))
        })
    }

    /// A `GetCredentialReport` rule answering with the given XML members.
    fn xml_rule_get(members: String) -> Rule {
        let body = format!(
            "<GetCredentialReportResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
             <GetCredentialReportResult>{members}</GetCredentialReportResult>\
             <ResponseMetadata><RequestId>example</RequestId></ResponseMetadata>\
             </GetCredentialReportResponse>"
        );
        mock!(aws_sdk_iam::Client::get_credential_report).then_http_response(move || {
            HttpResponse::new(StatusCode::try_from(200u16).unwrap(), SdkBody::from(body.clone()))
        })
    }

    fn base64_encode(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// `ListUsers` (one page) and empty policy lists for each `(name, path)` user.
    fn users_rules(users: &[(&str, &str)]) -> Vec<Rule> {
        let mut rules = list_users_rules(vec![users.iter().map(|(n, p)| user(n, p)).collect()]);
        for (name, _) in users {
            rules.extend(attached_rules(name, vec![Some(Vec::new())]));
            rules.extend(inline_rules(name, vec![Vec::new()]));
        }
        rules
    }

    /// A completed generation and a report with one row for each of `users`.
    fn report_rules(users: &[(&str, &str)]) -> Vec<Rule> {
        vec![
            mock!(aws_sdk_iam::Client::generate_credential_report).then_output(|| {
                GenerateCredentialReportOutput::builder()
                    .state(ReportStateType::Complete)
                    .build()
            }),
            get_rule(&report_csv_of(users)),
        ]
    }

    const TWO_USERS: [(&str, &str); 2] = [("alice", "/"), ("bob", "/")];

    struct Run {
        resources: Vec<Resource>,
        waits: Vec<Duration>,
        sent: Sent,
    }

    async fn run_collect(rules: &[Rule]) -> Run {
        let sleep = RecordingSleep::default();
        let (factory, sent) = mock_factory_counted(rules);
        let ctx = context_with(factory);
        let resources =
            collect_with_sleep(&ctx, "us-east-1", Some(SharedAsyncSleep::new(sleep.clone())))
                .await
                .unwrap();
        Run {
            resources,
            waits: sleep.waits(),
            sent,
        }
    }

    fn report_of(resource: &Resource) -> &Value {
        &resource.configuration["GetCredentialReport"]
    }

    fn secs(values: &[u64]) -> Vec<Duration> {
        values.iter().map(|s| Duration::from_secs(*s)).collect()
    }

    /// Asserts that `mark` is a failure mark with `kind` and a message of the documented
    /// form that names `operation` and `code`.
    fn assert_mark(mark: &Value, kind: &str, operation: &str, code: &str) {
        assert_eq!(mark.as_object().unwrap().len(), 1, "{mark}");
        assert_eq!(mark["yata_error"]["kind"], json!(kind), "{mark}");
        let message = mark["yata_error"]["message"].as_str().unwrap();
        assert!(message.contains(&format!("when calling {operation} ({code})")), "{message}");
        assert!(!message.contains("arn:aws"), "{message}");
    }

    fn assert_all_marked(resources: &[Resource], kind: &str, operation: &str, code: &str) {
        assert!(!resources.is_empty());
        for resource in resources {
            assert_mark(report_of(resource), kind, operation, code);
        }
    }

    fn sorted(mut resources: Vec<Resource>) -> Vec<Resource> {
        resources.sort_by(|a, b| a.id.cmp(&b.id));
        resources
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let mut rules = list_users_rules(vec![
            vec![
                User::builder()
                    .path("/")
                    .user_name("alice")
                    .user_id("AIDAEXAMPLEALICE")
                    .arn(arn_of("/", "alice"))
                    .create_date(DateTime::from_secs(1_767_225_600))
                    .password_last_used(DateTime::from_secs(1_769_904_000))
                    .permissions_boundary(
                        AttachedPermissionsBoundary::builder()
                            .permissions_boundary_type(
                                PermissionsBoundaryAttachmentType::Policy,
                            )
                            .permissions_boundary_arn("arn:aws:iam::123456789012:policy/boundary")
                            .build(),
                    )
                    .tags(Tag::builder().key("team").value("example").build().unwrap())
                    .build()
                    .unwrap(),
                user("bob", "/team/"),
            ],
            vec![user("carol", "/")],
        ]);
        rules.extend(attached_rules(
            "alice",
            vec![Some(vec![attached("ReadOnlyAccess")]), Some(vec![attached("ViewOnlyAccess")])],
        ));
        rules.extend(attached_rules("bob", vec![Some(Vec::new())]));
        rules.extend(attached_rules("carol", vec![None]));
        rules.extend(inline_rules("alice", vec![vec!["example-inline-policy"], vec!["second"]]));
        rules.extend(inline_rules("bob", vec![Vec::new()]));
        rules.extend(inline_rules("carol", vec![Vec::new()]));
        // The report has rows for alice and bob only; carol gets a failure mark.
        rules.extend(report_rules(&[("alice", "/"), ("bob", "/team/")]));
        let (factory, sent) = mock_factory_counted(&rules);
        let ctx = context_with(factory);
        let resources = sorted(collect_fast(&ctx, "us-east-1").await.unwrap());

        assert_eq!(resources.len(), 3);
        let alice = &resources[0];
        assert_eq!(alice.id, "arn:aws:iam::123456789012:user/alice");
        assert_eq!(alice.name.as_deref(), Some("alice"));
        assert_eq!(alice.resource_type, "aws.iam.user");
        assert_eq!(alice.account, "aws-123456789012");
        assert_eq!(alice.region, "global");
        assert!(alice.tags.is_empty());
        assert!(resources.iter().all(|r| r.tags.is_empty()));
        // Users with another path are kept too.
        assert!(resources.iter().any(|r| r.id.contains("/team/")));
        // Pages are joined in order.
        assert_eq!(
            alice.configuration["ListAttachedUserPolicies"]["AttachedPolicies"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            alice.configuration["ListUserPolicies"]["PolicyNames"],
            json!(["example-inline-policy", "second"])
        );
        // A page without the array does not become an empty array.
        assert_eq!(
            resources[1].configuration["ListAttachedUserPolicies"],
            json!({ "AttachedPolicies": null })
        );
        let operations = sent.operations();
        for name in [
            "ListUsers",
            "ListAttachedUserPolicies",
            "ListUserPolicies",
            "GenerateCredentialReport",
            "GetCredentialReport",
        ] {
            assert!(operations.iter().any(|o| o == name), "{name} was not sent");
        }
        assert_golden(
            &golden_path("iam", "user"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn the_listing_value_is_an_object_with_only_the_element_array() {
        let rules = three_user_rules();
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 3);
        for (resource, name) in resources.iter().zip(["alice", "bob", "carol"]) {
            let value = resource.configuration["ListUsers"].as_object().unwrap();
            assert_eq!(value.keys().collect::<Vec<_>>(), ["Users"]);
            assert!(!value.contains_key("IsTruncated") && !value.contains_key("Marker"));
            let users = value["Users"].as_array().unwrap();
            assert_eq!(users.len(), 1);
            assert_eq!(users[0]["UserName"], json!(name));
        }
    }

    #[tokio::test]
    async fn every_listed_user_becomes_a_resource() {
        let rules = three_user_rules();
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 3);
        let ids: Vec<&str> = resources.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "arn:aws:iam::123456789012:user/alice",
                "arn:aws:iam::123456789012:user/team/bob",
                "arn:aws:iam::123456789012:user/carol",
            ]
        );
    }

    #[tokio::test]
    async fn no_users_gives_an_empty_list() {
        let rules = list_users_rules(vec![Vec::new()]);
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        assert!(resources.is_empty());
    }

    #[tokio::test]
    async fn attached_policies_missing_on_every_page_is_null() {
        let mut rules = list_users_rules(vec![vec![user("alice", "/")]]);
        rules.push(
            mock!(aws_sdk_iam::Client::list_attached_user_policies).then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(200u16).unwrap(),
                    SdkBody::from(
                        "<ListAttachedUserPoliciesResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
                         <ListAttachedUserPoliciesResult><IsTruncated>false</IsTruncated>\
                         </ListAttachedUserPoliciesResult>\
                         <ResponseMetadata><RequestId>example</RequestId></ResponseMetadata>\
                         </ListAttachedUserPoliciesResponse>",
                    ),
                )
            }),
        );
        rules.extend(inline_rules("alice", vec![Vec::new()]));
        rules.extend(report_rules(&[("alice", "/")]));
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["ListAttachedUserPolicies"],
            json!({ "AttachedPolicies": null })
        );
        assert_eq!(
            resources[0].configuration["ListUserPolicies"],
            json!({ "PolicyNames": [] })
        );
    }

    #[tokio::test]
    async fn attached_policies_returned_by_the_first_page_only_are_kept() {
        let mut rules = list_users_rules(vec![vec![user("alice", "/")]]);
        rules.extend(attached_rules(
            "alice",
            vec![Some(vec![attached("ReadOnlyAccess")]), None],
        ));
        rules.extend(inline_rules("alice", vec![Vec::new()]));
        rules.extend(report_rules(&[("alice", "/")]));
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["ListAttachedUserPolicies"]["AttachedPolicies"],
            json!([{
                "PolicyArn": "arn:aws:iam::aws:policy/ReadOnlyAccess",
                "PolicyName": "ReadOnlyAccess",
            }])
        );
    }

    #[tokio::test]
    async fn a_failed_call_marks_only_that_user_and_key() {
        let mut rules = list_users_rules(vec![vec![user("alice", "/"), user("bob", "/")]]);
        rules.extend(attached_rules("alice", vec![Some(vec![attached("ReadOnlyAccess")])]));
        rules.push(inline_error_rule("alice", "AccessDenied"));
        rules.extend(attached_rules("bob", vec![Some(Vec::new())]));
        rules.extend(inline_rules("bob", vec![vec!["p"]]));
        rules.extend(report_rules(&[("alice", "/"), ("bob", "/")]));
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        let alice = &resources[0].configuration;
        assert_eq!(
            alice["ListUserPolicies"]["yata_error"]["kind"],
            json!("access_denied")
        );
        assert_eq!(alice["ListAttachedUserPolicies"]["AttachedPolicies"][0]["PolicyName"], json!("ReadOnlyAccess"));
        let bob = &resources[1].configuration;
        assert_eq!(bob["ListUserPolicies"], json!({ "PolicyNames": ["p"] }));
        assert_eq!(bob["ListAttachedUserPolicies"], json!({ "AttachedPolicies": [] }));
    }

    #[tokio::test]
    async fn failure_marks_use_the_error_kind() {
        let cases = [
            ("NoSuchEntity", "service"),
            ("Throttling", "throttled"),
            ("AccessDenied", "access_denied"),
        ];
        for (code, kind) in cases {
            let mut rules = list_users_rules(vec![vec![user("alice", "/")]]);
            rules.push(attached_error_rule("alice", code));
            rules.extend(inline_rules("alice", vec![Vec::new()]));
            rules.extend(report_rules(&[("alice", "/")]));
            let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
            let mark = &resources[0].configuration["ListAttachedUserPolicies"];
            assert_eq!(mark["yata_error"]["kind"], json!(kind), "{code}");
            let message = mark["yata_error"]["message"].as_str().unwrap();
            assert!(message.contains("ListAttachedUserPolicies") && message.contains(code));
            assert!(!message.contains("arn:aws"));
            assert_eq!(mark.as_object().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn a_failure_on_a_later_page_drops_the_earlier_pages() {
        let mut rules = list_users_rules(vec![vec![user("alice", "/")]]);
        let mut pages = attached_rules(
            "alice",
            vec![Some(vec![attached("ReadOnlyAccess")]), Some(vec![attached("Other")])],
        );
        // Replace the second page by an error.
        pages.truncate(1);
        let first_page = pages.remove(0);
        rules.push(first_page);
        rules.push(
            mock!(aws_sdk_iam::Client::list_attached_user_policies)
                .match_requests(|input: &ListAttachedUserPoliciesInput| {
                    input.marker() == Some("marker-1")
                })
                .then_error(|| ListAttachedUserPoliciesError::generic(metadata("AccessDenied"))),
        );
        rules.extend(inline_rules("alice", vec![Vec::new()]));
        rules.extend(report_rules(&[("alice", "/")]));
        let resources = collect_fast(&context(&rules), "us-east-1").await.unwrap();
        let value = &resources[0].configuration["ListAttachedUserPolicies"];
        assert_eq!(value["yata_error"]["kind"], json!("access_denied"));
        assert!(value.get("AttachedPolicies").is_none());
    }

    #[tokio::test]
    async fn listing_failures_fail_the_whole_group() {
        let cases = [
            ("AccessDenied", CollectionErrorKind::AccessDenied),
            ("Throttling", CollectionErrorKind::Throttled),
            ("ServiceFailure", CollectionErrorKind::Service),
            ("NotAuthorized", CollectionErrorKind::AccessDenied),
        ];
        for (code, kind) in cases {
            let rule = mock!(aws_sdk_iam::Client::list_users)
                .then_error(move || ListUsersError::generic(metadata(code)));
            let err = collect_fast(&context(&[rule]), "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("ListUsers") && err.message().contains(code));
            assert!(!err.message().contains("arn:aws"));
        }
    }

    #[tokio::test]
    async fn a_failure_on_the_second_users_page_fails_the_whole_group() {
        let first = list_users_rules(vec![vec![user("alice", "/")], vec![user("bob", "/")]])
            .remove(0);
        let second = mock!(aws_sdk_iam::Client::list_users)
            .match_requests(|input: &ListUsersInput| input.marker() == Some("marker-1"))
            .then_error(|| ListUsersError::generic(metadata("ServiceFailure")));
        let err = collect_fast(&context(&[first, second]), "us-east-1")
            .await
            .unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::Service);
    }

    #[tokio::test]
    async fn an_unformattable_date_fails_the_whole_group() {
        let broken = User::builder()
            .path("/")
            .user_name("alice")
            .user_id("AIDAEXAMPLEALICE")
            .arn(arn_of("/", "alice"))
            .create_date(DateTime::from_secs(i64::MAX))
            .build()
            .unwrap();
        let rules = list_users_rules(vec![vec![broken]]);
        let err = collect_fast(&context(&rules), "us-east-1").await.unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::InvalidResponse);
        assert_eq!(
            err.message(),
            "invalid response when calling ListUsers (InvalidDateTime)"
        );
    }

    #[tokio::test]
    async fn the_fan_out_limit_does_not_change_the_result() {
        let rules = three_user_rules();
        let mut results = Vec::new();
        for limit in [1usize, 8] {
            let ctx = context(&rules).with_fan_out_limit(NonZeroUsize::new(limit).unwrap());
            let resources = collect_fast(&ctx, "us-east-1").await.unwrap();
            results.push(serde_json::to_value(&resources).unwrap());
        }
        assert_eq!(results[0], results[1]);
    }

    // ----- credential report -----

    #[tokio::test]
    async fn waits_until_the_report_is_complete() {
        let generate = generate_sequence(vec![
            ReportStateType::Started,
            ReportStateType::Inprogress,
            ReportStateType::Complete,
        ]);
        let get = get_rule(&report_csv_of(&TWO_USERS));
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([generate.clone(), get.clone()]);
        let run = run_collect(&rules).await;
        assert_eq!(generate.num_calls(), 3);
        assert_eq!(get.num_calls(), 1);
        assert_eq!(run.waits, secs(&[1, 2]));
        for resource in &run.resources {
            assert!(report_of(resource).get("yata_error").is_none());
            assert_eq!(report_of(resource)["Content"]["arn"], json!(resource.id));
        }
    }

    #[tokio::test]
    async fn a_reused_report_is_used_without_waiting() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend(report_rules(&TWO_USERS));
        let run = run_collect(&rules).await;
        assert!(run.waits.is_empty());
        assert!(run.resources.iter().all(|r| report_of(r)["Content"].is_object()));
    }

    #[tokio::test]
    async fn polling_times_out_after_the_total_wait() {
        let generate = generate_forever(ReportStateType::Inprogress);
        let get = get_rule(&report_csv_of(&TWO_USERS));
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([generate.clone(), get.clone()]);
        let run = run_collect(&rules).await;

        let mut expected = vec![1, 2, 4];
        expected.extend([8; 14]);
        assert_eq!(run.waits, secs(&expected));
        assert_eq!(run.waits.iter().sum::<Duration>(), Duration::from_secs(119));
        assert_eq!(generate.num_calls(), 18);
        assert_eq!(get.num_calls(), 0);
        assert_all_marked(&run.resources, "timeout", "GenerateCredentialReport", "PollingLimitReached");
        // The other keys keep their values.
        for resource in &run.resources {
            assert_eq!(
                resource.configuration["ListUserPolicies"],
                json!({ "PolicyNames": [] })
            );
        }
    }

    #[tokio::test]
    async fn a_missing_state_fails_without_waiting() {
        let generate = mock!(aws_sdk_iam::Client::generate_credential_report)
            .then_output(|| GenerateCredentialReportOutput::builder().build());
        let get = get_rule(&report_csv_of(&TWO_USERS));
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([generate.clone(), get.clone()]);
        let run = run_collect(&rules).await;
        assert!(run.waits.is_empty());
        assert_eq!(get.num_calls(), 0);
        assert_all_marked(&run.resources, "invalid_response", "GenerateCredentialReport", "MissingState");
    }

    #[tokio::test]
    async fn an_unknown_state_fails_without_waiting() {
        let get = get_rule(&report_csv_of(&TWO_USERS));
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([xml_rule_generate("FAILED"), get.clone()]);
        let run = run_collect(&rules).await;
        assert!(run.waits.is_empty());
        assert_eq!(get.num_calls(), 0);
        assert_all_marked(&run.resources, "invalid_response", "GenerateCredentialReport", "UnknownState");
    }

    #[tokio::test]
    async fn no_sleep_implementation_fails_when_a_wait_is_needed() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([
            generate_sequence(vec![ReportStateType::Started, ReportStateType::Complete]),
            get_rule(&report_csv_of(&TWO_USERS)),
        ]);
        let ctx = context(&rules);
        let resources = collect_with_sleep(&ctx, "us-east-1", None).await.unwrap();
        assert_all_marked(&resources, "service", "GenerateCredentialReport", "NoSleepImplementation");

        // Without a wait, no sleep implementation is needed.
        let mut rules = users_rules(&TWO_USERS);
        rules.extend(report_rules(&TWO_USERS));
        let ctx = context(&rules);
        let resources = collect_with_sleep(&ctx, "us-east-1", None).await.unwrap();
        assert!(resources.iter().all(|r| report_of(r)["Content"].is_object()));
    }

    #[tokio::test]
    async fn generate_failures_mark_only_the_report_key() {
        let cases = [
            ("AccessDenied", "access_denied"),
            ("Throttling", "throttled"),
            ("LimitExceeded", "service"),
            ("ServiceFailure", "service"),
        ];
        for (code, kind) in cases {
            let get = get_rule(&report_csv_of(&TWO_USERS));
            let mut rules = users_rules(&TWO_USERS);
            rules.extend([generate_error_rule(code), get.clone()]);
            let run = run_collect(&rules).await;
            assert_all_marked(&run.resources, kind, "GenerateCredentialReport", code);
            assert_eq!(get.num_calls(), 0, "{code}");
            assert!(!run.sent.operations().iter().any(|o| o == "GetCredentialReport"));
            for resource in &run.resources {
                assert_eq!(
                    resource.configuration["ListAttachedUserPolicies"],
                    json!({ "AttachedPolicies": [] })
                );
                assert_eq!(
                    resource.configuration["ListUserPolicies"],
                    json!({ "PolicyNames": [] })
                );
            }
        }
    }

    #[tokio::test]
    async fn get_failures_mark_every_user() {
        let cases = [
            ("ReportNotPresent", "service"),
            ("ReportExpired", "service"),
            ("ReportInProgress", "service"),
            ("AccessDenied", "access_denied"),
        ];
        for (code, kind) in cases {
            let mut rules = users_rules(&TWO_USERS);
            rules.extend([
                generate_sequence(vec![ReportStateType::Complete]),
                get_error_rule(code),
            ]);
            let run = run_collect(&rules).await;
            assert_all_marked(&run.resources, kind, "GetCredentialReport", code);
            assert!(run.resources.iter().all(|r| !report_of(r).is_null()));
        }
    }

    #[tokio::test]
    async fn invalid_outputs_mark_every_user() {
        let csv = report_csv_of(&TWO_USERS);
        let b64 = base64_encode(csv.as_bytes());
        let cases: Vec<(Rule, &str)> = vec![
            (
                mock!(aws_sdk_iam::Client::get_credential_report).then_output(|| {
                    GetCredentialReportOutput::builder()
                        .content(Blob::new(b"x".to_vec()))
                        .build()
                }),
                "MissingReportFormat",
            ),
            (
                xml_rule_get(format!(
                    "<Content>{b64}</Content><ReportFormat>text/tsv</ReportFormat>"
                )),
                "UnsupportedReportFormat",
            ),
            (
                mock!(aws_sdk_iam::Client::get_credential_report).then_output(|| {
                    GetCredentialReportOutput::builder()
                        .report_format(ReportFormatType::TextCsv)
                        .build()
                }),
                "MissingContent",
            ),
            (
                mock!(aws_sdk_iam::Client::get_credential_report).then_output(|| {
                    GetCredentialReportOutput::builder()
                        .content(Blob::new(b"user,arn\n\xff\xfe,x\n".to_vec()))
                        .report_format(ReportFormatType::TextCsv)
                        .build()
                }),
                "InvalidUtf8",
            ),
            (
                mock!(aws_sdk_iam::Client::get_credential_report).then_output(|| {
                    GetCredentialReportOutput::builder()
                        .content(Blob::new(b"user,arn\n".to_vec()))
                        .report_format(ReportFormatType::TextCsv)
                        .generated_time(DateTime::from_secs(i64::MAX))
                        .build()
                }),
                "InvalidDateTime",
            ),
        ];
        for (get, code) in cases {
            let mut rules = users_rules(&TWO_USERS);
            rules.extend([generate_sequence(vec![ReportStateType::Complete]), get]);
            let run = run_collect(&rules).await;
            assert_all_marked(&run.resources, "invalid_response", "GetCredentialReport", code);
        }
    }

    #[tokio::test]
    async fn a_missing_generated_time_is_null() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([
            generate_sequence(vec![ReportStateType::Complete]),
            xml_rule_get(format!(
                "<Content>{}</Content><ReportFormat>text/csv</ReportFormat>",
                base64_encode(report_csv_of(&TWO_USERS).as_bytes())
            )),
        ]);
        let run = run_collect(&rules).await;
        for resource in &run.resources {
            let value = report_of(resource);
            assert_eq!(value["GeneratedTime"], Value::Null);
            assert!(value["Content"].is_object());
        }
    }

    #[tokio::test]
    async fn base64_content_is_decoded_once_by_the_sdk() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([
            generate_sequence(vec![ReportStateType::Complete]),
            xml_rule_get(format!(
                "<Content>{}</Content><ReportFormat>text/csv</ReportFormat>\
                 <GeneratedTime>2026-01-01T00:00:00Z</GeneratedTime>",
                base64_encode(report_csv_of(&TWO_USERS).as_bytes())
            )),
        ]);
        let run = run_collect(&rules).await;
        for resource in &run.resources {
            let value = report_of(resource);
            assert_eq!(value["Content"]["arn"], json!(resource.id));
            assert_eq!(value["Content"]["user"], json!(resource.name));
            assert_eq!(value["GeneratedTime"], json!("2026-01-01T00:00:00Z"));
        }
    }

    #[tokio::test]
    async fn a_user_without_a_row_is_marked_alone() {
        let users = [("alice", "/"), ("bob", "/"), ("carol", "/")];
        let mut rules = users_rules(&users);
        rules.extend(report_rules(&[("alice", "/"), ("carol", "/")]));
        let run = run_collect(&rules).await;
        assert!(report_of(&run.resources[0])["Content"].is_object());
        assert_mark(report_of(&run.resources[1]), "invalid_response", "GetCredentialReport", "MissingUserRow");
        assert!(report_of(&run.resources[2])["Content"].is_object());
    }

    #[tokio::test]
    async fn a_duplicated_arn_marks_only_that_user() {
        let users = [("alice", "/"), ("bob", "/")];
        let alice = report_row("alice", &arn_of("/", "alice"));
        let bob = report_row("bob", &arn_of("/", "bob"));
        let csv = report_csv(&[alice.clone(), bob, alice]);
        let mut rules = users_rules(&users);
        rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
        let run = run_collect(&rules).await;
        assert_mark(report_of(&run.resources[0]), "invalid_response", "GetCredentialReport", "DuplicateArn");
        assert!(report_of(&run.resources[1])["Content"].is_object());
    }

    #[tokio::test]
    async fn rows_without_a_user_are_not_stored() {
        let users = [("alice", "/"), ("bob", "/"), ("carol", "/")];
        let user_rows: Vec<String> = users
            .iter()
            .map(|(n, p)| report_row(n, &arn_of(p, n)))
            .collect();

        // The root row plus one row per user: rows without the root row = users with a row.
        let mut rows = vec![root_row()];
        rows.extend(user_rows.clone());
        let csv = report_csv(&rows);
        let mut rules = users_rules(&users);
        rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
        let run = run_collect(&rules).await;
        let table = parse_csv(&csv).unwrap();
        let rows_without_root = table
            .rows
            .iter()
            .filter(|row| !row[1].ends_with(":root"))
            .count();
        let with_row = run
            .resources
            .iter()
            .filter(|r| report_of(r).get("Content").is_some())
            .count();
        assert_eq!(rows_without_root, with_row);
        assert_eq!(with_row, users.len());
        let text = serde_json::to_string(&run.resources).unwrap();
        assert!(!text.contains("<root_account>") && !text.contains(":root"));

        // A row of a deleted user is not stored either.
        rows.push(report_row("ghost", &arn_of("/", "ghost")));
        let csv = report_csv(&rows);
        let mut rules = users_rules(&users);
        rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
        let run = run_collect(&rules).await;
        assert_eq!(run.resources.len(), 3);
        let text = serde_json::to_string(&run.resources).unwrap();
        assert!(!text.contains("ghost"));
        assert!(run.resources.iter().all(|r| report_of(r)["Content"].is_object()));
    }

    #[tokio::test]
    async fn report_values_are_stored_as_written() {
        let alice = arn_of("/", "alice");
        let csv = format!(
            "user,arn,password_enabled,password_last_used,mfa_active,access_key_1_last_used_date,\
             cert_1_active,empty_column,padded,future_column\n\
             alice,{alice},TRUE,not_supported,FALSE,N/A,no_information,,  x y ,2026-01-01T00:00:00+00:00\n"
        );
        let mut rules = users_rules(&[("alice", "/")]);
        rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
        let run = run_collect(&rules).await;
        assert_eq!(
            *report_of(&run.resources[0]),
            json!({
                "GeneratedTime": "2026-01-01T00:00:00Z",
                "ReportFormat": "text/csv",
                "Content": {
                    "user": "alice",
                    "arn": alice,
                    "password_enabled": "TRUE",
                    "password_last_used": "not_supported",
                    "mfa_active": "FALSE",
                    "access_key_1_last_used_date": "N/A",
                    "cert_1_active": "no_information",
                    "empty_column": "",
                    "padded": "  x y ",
                    "future_column": "2026-01-01T00:00:00+00:00",
                },
            })
        );
    }

    #[tokio::test]
    async fn quoted_values_are_matched_by_arn_and_unquoted() {
        let name = "a,b";
        let path = "/x\",y/";
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let arn = arn_of(path, name);
        let csv = report_csv(&[report_row(&quote(name), &quote(&arn))]);
        let mut rules = users_rules(&[(name, path)]);
        rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
        let run = run_collect(&rules).await;
        let content = &report_of(&run.resources[0])["Content"];
        assert_eq!(content["user"], json!(name));
        assert_eq!(content["arn"], json!(arn));
        assert_eq!(run.resources[0].id, arn);
    }

    #[tokio::test]
    async fn unparsable_reports_mark_every_user() {
        let alice = report_row("alice", &arn_of("/", "alice"));
        let cases: Vec<(String, &str)> = vec![
            // One bad row fails the whole report, including the good rows.
            (
                format!("{REPORT_HEADER}\n{alice}\n{}\n", "bob,only-two"),
                "FieldCountMismatch",
            ),
            (String::new(), "MissingHeader"),
            ("user,name\nalice,x\n".to_owned(), "MissingArnColumn"),
            ("user,arn,user\nalice,x,y\n".to_owned(), "DuplicateColumn"),
            ("user,arn\n\"alice,x\n".to_owned(), "InvalidCsv"),
        ];
        for (csv, code) in cases {
            let mut rules = users_rules(&TWO_USERS);
            rules.extend([generate_sequence(vec![ReportStateType::Complete]), get_rule(&csv)]);
            let run = run_collect(&rules).await;
            assert_all_marked(&run.resources, "invalid_response", "GetCredentialReport", code);
        }
    }

    #[tokio::test]
    async fn a_header_only_report_leaves_every_user_without_a_row() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend([
            generate_sequence(vec![ReportStateType::Complete]),
            get_rule(&report_csv(&[])),
        ]);
        let run = run_collect(&rules).await;
        assert_all_marked(&run.resources, "invalid_response", "GetCredentialReport", "MissingUserRow");
    }

    #[tokio::test]
    async fn the_report_is_not_requested_without_users() {
        let generate = generate_sequence(vec![ReportStateType::Complete]);
        let get = get_rule(&report_csv(&[]));
        let mut rules = list_users_rules(vec![Vec::new()]);
        rules.extend([generate.clone(), get.clone()]);
        let (factory, sent) = mock_factory_counted(&rules);
        let resources = collect_fast(&context_with(factory), "us-east-1").await.unwrap();
        assert!(resources.is_empty());
        assert_eq!(generate.num_calls() + get.num_calls(), 0);
        assert!(!sent.operations().iter().any(|o| o.contains("CredentialReport")));
    }

    #[tokio::test]
    async fn the_report_is_not_requested_when_listing_users_fails() {
        let generate = generate_sequence(vec![ReportStateType::Complete]);
        let get = get_rule(&report_csv(&[]));
        let rules = vec![
            mock!(aws_sdk_iam::Client::list_users)
                .then_error(|| ListUsersError::generic(metadata("AccessDenied"))),
            generate.clone(),
            get.clone(),
        ];
        let (factory, sent) = mock_factory_counted(&rules);
        let err = collect_fast(&context_with(factory), "us-east-1").await.unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::AccessDenied);
        assert_eq!(generate.num_calls() + get.num_calls(), 0);
        assert!(!sent.operations().iter().any(|o| o.contains("CredentialReport")));
    }

    #[tokio::test]
    async fn the_report_operations_pass_the_read_only_guard() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend(report_rules(&TWO_USERS));
        let run = run_collect(&rules).await;
        let operations = run.sent.operations();
        for name in ["GenerateCredentialReport", "GetCredentialReport"] {
            assert!(operations.iter().any(|o| o == name), "{name} was not sent");
        }
        let text = serde_json::to_string(&run.resources).unwrap();
        assert!(!text.contains("read-only guard"));
        assert!(!text.contains("yata_error"));
    }

    /// The real `collect` (which takes the sleep from the client) works when no wait is needed.
    #[tokio::test]
    async fn the_real_collect_runs_without_a_wait() {
        let mut rules = users_rules(&TWO_USERS);
        rules.extend(report_rules(&TWO_USERS));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert!(resources.iter().all(|r| report_of(r)["Content"].is_object()));
    }

    #[test]
    fn warn_messages_have_counts_and_no_arn() {
        let unavailable = unavailable_message(
            "timeout when calling GenerateCredentialReport (PollingLimitReached)",
            25,
        );
        assert_eq!(
            unavailable,
            "credential report unavailable: timeout when calling GenerateCredentialReport \
             (PollingLimitReached); marked 25 users"
        );
        let missing = user_row_message(3, 25, "no row in", "2026-01-01T00:00:00Z");
        assert_eq!(
            missing,
            "3 of 25 users have no row in the credential report generated at 2026-01-01T00:00:00Z"
        );
        let duplicated = user_row_message(1, 25, "more than one row in", "unknown time");
        assert!(duplicated.starts_with("1 of 25 users have more than one row in"));
        for message in [unavailable, missing, duplicated] {
            assert!(!message.contains("arn:"));
        }
    }

    #[test]
    fn the_polling_constants_match_the_design() {
        assert_eq!(REPORT_POLL_INITIAL_INTERVAL, Duration::from_secs(1));
        assert_eq!(REPORT_POLL_MAX_INTERVAL, Duration::from_secs(8));
        assert_eq!(REPORT_POLL_MAX_TOTAL_WAIT, Duration::from_secs(120));
    }

    #[test]
    fn metadata_of_the_collector() {
        let c = collector();
        assert_eq!(c.resource_type(), "aws.iam.user");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(
            c.required_permissions(),
            [
                "iam:ListUsers",
                "iam:ListAttachedUserPolicies",
                "iam:ListUserPolicies",
                "iam:GenerateCredentialReport",
                "iam:GetCredentialReport"
            ]
        );
    }
}
