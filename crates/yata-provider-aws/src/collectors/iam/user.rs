//! Collector for `aws.iam.user`: IAM users with their attached managed policies and the names
//! of their inline policies (`ListUsers`, `ListAttachedUserPolicies`, `ListUserPolicies`).

use std::collections::BTreeMap;

use aws_sdk_iam::types::User;
use serde_json::{Map, Value, json};
use yata_core::{CollectionErrorKind, Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::convert::{datetime_to_rfc3339, opt_string};
use crate::support::errors::{CollectError, failure_mark};
use crate::support::fan_out::fan_out;

const RESOURCE_TYPE: &str = "aws.iam.user";
const LIST_USERS: &str = "ListUsers";
const LIST_ATTACHED: &str = "ListAttachedUserPolicies";
const LIST_INLINE: &str = "ListUserPolicies";

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
        ]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

/// The values of the two per-user keys: `ListAttachedUserPolicies` and `ListUserPolicies`.
type UserDetails = (Value, Value);

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_iam::Client = ctx.client(region);

    // Step 1: all pages of ListUsers. Any failure fails the whole group.
    let entries = list_users(&client).await?;

    // Step 2: the per-user calls, concurrently with a bounded number of tasks.
    let names: Vec<String> = entries.iter().map(|(_, name, _)| name.clone()).collect();
    let details: Vec<UserDetails> = fan_out(ctx.fan_out_limit(), names, {
        let client = client.clone();
        move |name: String| {
            let client = client.clone();
            async move {
                let attached = list_attached_policies(&client, &name).await;
                let inline = list_inline_policies(&client, &name).await;
                (attached, inline)
            }
        }
    })
    .await?;
    warn_on_failure_marks(LIST_ATTACHED, details.iter().map(|(attached, _)| attached));
    warn_on_failure_marks(LIST_INLINE, details.iter().map(|(_, inline)| inline));

    // Step 3: assemble the resources.
    let resources = entries
        .into_iter()
        .zip(details)
        .map(|((arn, name, user), (attached, inline))| {
            let mut configuration = Map::new();
            configuration.insert(LIST_USERS.to_owned(), user);
            configuration.insert(LIST_ATTACHED.to_owned(), attached);
            configuration.insert(LIST_INLINE.to_owned(), inline);
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
    use crate::support::clients::testing::{mock_factory, mock_factory_counted};
    use crate::support::golden::{assert_golden, golden_path};
    use aws_sdk_iam::error::ErrorMetadata;
    use aws_sdk_iam::operation::list_attached_user_policies::{
        ListAttachedUserPoliciesError, ListAttachedUserPoliciesInput,
        ListAttachedUserPoliciesOutput,
    };
    use aws_sdk_iam::operation::list_user_policies::{
        ListUserPoliciesError, ListUserPoliciesInput, ListUserPoliciesOutput,
    };
    use aws_sdk_iam::operation::list_users::{ListUsersError, ListUsersInput, ListUsersOutput};
    use aws_sdk_iam::primitives::DateTime;
    use aws_sdk_iam::types::{AttachedPermissionsBoundary, AttachedPolicy, PermissionsBoundaryAttachmentType, Tag};
    use aws_sdk_s3::primitives::SdkBody;
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;
    use std::num::NonZeroUsize;

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
        rules
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
        let (factory, sent) = mock_factory_counted(&rules);
        let ctx = context_with(factory);
        let resources = sorted(collect(&ctx, "us-east-1").await.unwrap());

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
        for name in ["ListUsers", "ListAttachedUserPolicies", "ListUserPolicies"] {
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
            let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
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
            let err = collect(&context(&[rule]), "us-east-1").await.unwrap_err();
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
        let err = collect(&context(&[first, second]), "us-east-1")
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
        let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
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
            let resources = collect(&ctx, "us-east-1").await.unwrap();
            results.push(serde_json::to_value(&resources).unwrap());
        }
        assert_eq!(results[0], results[1]);
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
                "iam:ListUserPolicies"
            ]
        );
    }
}
