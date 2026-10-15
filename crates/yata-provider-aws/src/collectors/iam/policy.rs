//! Collector for `aws.iam.policy`: all IAM managed policies (AWS managed and customer managed)
//! with the document of their default version (`ListPolicies`, `GetPolicyVersion`).

use std::collections::BTreeMap;

use aws_sdk_iam::types::Policy;
use serde_json::{Map, Value, json};
use yata_core::{CollectionErrorKind, Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::convert::{datetime_to_rfc3339, opt_string};
use crate::support::errors::{CollectError, failure_mark};
use crate::support::fan_out::fan_out;

const RESOURCE_TYPE: &str = "aws.iam.policy";
const LIST_POLICIES: &str = "ListPolicies";
const GET_POLICY_VERSION: &str = "GetPolicyVersion";

/// Returns the collector of `aws.iam.policy`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(PolicyCollector)
}

struct PolicyCollector;

impl Collector for PolicyCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &["iam:ListPolicies", "iam:GetPolicyVersion"]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

/// What the per-policy task needs: the ARN and the default version id (if the API gave one).
struct VersionRequest {
    arn: String,
    default_version_id: Option<String>,
}

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_iam::Client = ctx.client(region);

    // Step 1: all pages of ListPolicies. Any failure fails the whole group.
    let listed = list_policies(&client).await?;
    let mut entries: Vec<(String, Option<String>, Value)> = Vec::with_capacity(listed.len());
    let mut requests: Vec<VersionRequest> = Vec::with_capacity(listed.len());
    for item in listed {
        entries.push((item.arn.clone(), item.name, item.value));
        requests.push(VersionRequest {
            arn: item.arn,
            default_version_id: item.default_version_id,
        });
    }

    // Step 2: the default version of every policy, concurrently with a bounded number of tasks.
    let versions: Vec<Value> = fan_out(ctx.fan_out_limit(), requests, {
        let client = client.clone();
        move |request: VersionRequest| {
            let client = client.clone();
            async move { get_policy_version(&client, request).await }
        }
    })
    .await?;
    warn_on_failure_marks(GET_POLICY_VERSION, &versions);

    // Step 3: assemble the resources.
    let resources = entries
        .into_iter()
        .zip(versions)
        .map(|((arn, name, policy), version)| {
            let mut configuration = Map::new();
            configuration.insert(LIST_POLICIES.to_owned(), policy);
            configuration.insert(GET_POLICY_VERSION.to_owned(), version);
            Resource {
                id: arn,
                resource_type: RESOURCE_TYPE.to_owned(),
                account: ctx.account_key().to_owned(),
                region: "global".to_owned(),
                name,
                tags: BTreeMap::new(),
                configuration,
            }
        })
        .collect();
    Ok(resources)
}

/// One listed policy: what the later steps need and the `ListPolicies` value of its resource.
struct ListedPolicy {
    arn: String,
    name: Option<String>,
    default_version_id: Option<String>,
    value: Value,
}

/// Fetches all pages of `ListPolicies` in the order the API returned them.
///
/// The value of each policy keeps the shape of the operation output: an object holding only the
/// array `Policies` with the element of that policy (paging fields are not included).
///
/// If no page returns `Policies` at all, the response is invalid: AWS managed policies always
/// exist, and treating it as zero policies would silently drop the input of the rules.
async fn list_policies(client: &aws_sdk_iam::Client) -> Result<Vec<ListedPolicy>, CollectError> {
    let mut listed = Vec::new();
    let mut any_page_with_policies = false;
    let mut pages = client.list_policies().into_paginator().send();
    while let Some(page) = pages.next().await {
        let page = page.map_err(|err| CollectError::from_sdk(LIST_POLICIES, &err))?;
        if let Some(page_policies) = &page.policies {
            any_page_with_policies = true;
            for policy in page_policies {
                let arn = policy
                    .arn()
                    .ok_or_else(|| invalid_response(LIST_POLICIES, "MissingArn"))?
                    .to_owned();
                listed.push(ListedPolicy {
                    arn,
                    name: policy.policy_name().map(str::to_owned),
                    default_version_id: policy.default_version_id().map(str::to_owned),
                    value: json!({ "Policies": [policy_element(policy)?] }),
                });
            }
        }
    }
    if !any_page_with_policies {
        return Err(invalid_response(LIST_POLICIES, "MissingPolicies"));
    }
    Ok(listed)
}

/// Converts an element of `ListPolicies` to JSON (all twelve members).
fn policy_element(policy: &Policy) -> Result<Value, CollectError> {
    let create_date = opt_datetime_strict(policy.create_date(), LIST_POLICIES)?;
    let update_date = opt_datetime_strict(policy.update_date(), LIST_POLICIES)?;
    let tags = match policy.tags.as_deref() {
        Some(tags) => Value::Array(
            tags.iter()
                .map(|tag| json!({ "Key": tag.key(), "Value": tag.value() }))
                .collect(),
        ),
        None => Value::Null,
    };
    Ok(json!({
        "Arn": opt_string(policy.arn()),
        "AttachmentCount": policy.attachment_count(),
        "CreateDate": create_date,
        "DefaultVersionId": opt_string(policy.default_version_id()),
        "Description": opt_string(policy.description()),
        "IsAttachable": policy.is_attachable(),
        "Path": opt_string(policy.path()),
        "PermissionsBoundaryUsageCount": policy.permissions_boundary_usage_count(),
        "PolicyId": opt_string(policy.policy_id()),
        "PolicyName": opt_string(policy.policy_name()),
        "Tags": tags,
        "UpdateDate": update_date,
    }))
}

/// Formats an optional timestamp; `None` is `null`, an unformattable value is an
/// `invalid_response` error for `operation`.
fn opt_datetime_strict(
    value: Option<&aws_sdk_iam::primitives::DateTime>,
    operation: &str,
) -> Result<Value, CollectError> {
    match value {
        Some(date) => datetime_to_rfc3339(date)
            .map(Value::String)
            .ok_or_else(|| invalid_response(operation, "InvalidDateTime")),
        None => Ok(Value::Null),
    }
}

/// Builds an `invalid_response` error: `"invalid response when calling <op> (<reason>)"`.
fn invalid_response(operation: &str, reason: &str) -> CollectError {
    CollectError::new(
        CollectionErrorKind::InvalidResponse,
        format!("invalid response when calling {operation} ({reason})"),
    )
}

/// Calls `GetPolicyVersion` for the default version of a policy. Returns the value, or a
/// failure mark.
async fn get_policy_version(client: &aws_sdk_iam::Client, request: VersionRequest) -> Value {
    let Some(version_id) = request.default_version_id else {
        tracing::debug!(policy_arn = %request.arn, "the policy has no DefaultVersionId");
        return invalid_response(LIST_POLICIES, "MissingDefaultVersionId").to_failure_mark();
    };
    let output = match client
        .get_policy_version()
        .policy_arn(&request.arn)
        .version_id(version_id)
        .send()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            tracing::debug!(policy_arn = %request.arn, "{GET_POLICY_VERSION} failed");
            return failure_mark(GET_POLICY_VERSION, &err);
        }
    };
    match output.policy_version() {
        Some(version) => {
            let create_date =
                match opt_datetime_strict(version.create_date(), GET_POLICY_VERSION) {
                    Ok(value) => value,
                    Err(err) => return err.to_failure_mark(),
                };
            let document = match version.document() {
                Some(document) => decode_document(document),
                None => Value::Null,
            };
            json!({
                "PolicyVersion": {
                    "CreateDate": create_date,
                    "Document": document,
                    "IsDefaultVersion": version.is_default_version(),
                    "VersionId": opt_string(version.version_id()),
                }
            })
        }
        None => json!({ "PolicyVersion": null }),
    }
}

/// Converts the `Document` of a policy version to JSON.
///
/// The API returns the document URL-encoded (RFC 3986). It is percent-decoded (`+` stays a
/// plus sign), read as UTF-8 and parsed as JSON. If any step fails, the string returned by the
/// API is returned unchanged.
fn decode_document(encoded: &str) -> Value {
    match try_decode_document(encoded) {
        Ok(value) => value,
        Err(reason) => {
            tracing::debug!(reason, "the policy document could not be decoded; keeping it as is");
            Value::String(encoded.to_owned())
        }
    }
}

fn try_decode_document(encoded: &str) -> Result<Value, &'static str> {
    let bytes = percent_decode(encoded).ok_or("invalid percent-encoding")?;
    let text = String::from_utf8(bytes).map_err(|_| "not UTF-8")?;
    serde_json::from_str(&text).map_err(|_| "not JSON")
}

/// Decodes `%XX` sequences into bytes. Other characters (including `+`) are kept as they are.
/// Returns `None` if a `%` is not followed by two hexadecimal digits.
fn percent_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            out.push(high << 4 | low);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    Some(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Logs one `warn` line for an operation when at least one of its values is a failure mark.
fn warn_on_failure_marks(operation: &str, values: &[Value]) {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        if let Some(kind) = value["yata_error"]["kind"].as_str() {
            *by_kind.entry(kind.to_owned()).or_insert(0) += 1;
        }
    }
    let failed: usize = by_kind.values().sum();
    if failed > 0 {
        let kinds: Vec<String> = by_kind.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        tracing::warn!(
            "{failed} of {} {operation} calls failed ({})",
            values.len(),
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
    use aws_sdk_iam::operation::get_policy_version::{
        GetPolicyVersionError, GetPolicyVersionInput, GetPolicyVersionOutput,
    };
    use aws_sdk_iam::operation::list_policies::{
        ListPoliciesError, ListPoliciesInput, ListPoliciesOutput,
    };
    use aws_sdk_iam::primitives::DateTime;
    use aws_sdk_iam::types::PolicyVersion;
    use aws_sdk_s3::primitives::SdkBody;
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;
    use std::num::NonZeroUsize;

    const ADMIN_ARN: &str = "arn:aws:iam::aws:policy/AdministratorAccess";
    const CUSTOM_ARN: &str = "arn:aws:iam::123456789012:policy/example-policy";
    const UNATTACHED_ARN: &str = "arn:aws:iam::123456789012:policy/unattached-policy";

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
            .message("failure for arn:aws:iam::123456789012:policy/example-policy")
            .build()
    }

    fn policy(name: &str, arn: &str, attachments: i32) -> Policy {
        Policy::builder()
            .policy_name(name)
            .policy_id(format!("ANPAEXAMPLE{}", name.to_uppercase().replace('-', "")))
            .arn(arn)
            .path("/")
            .default_version_id("v1")
            .attachment_count(attachments)
            .permissions_boundary_usage_count(0)
            .is_attachable(true)
            .create_date(DateTime::from_secs(1_423_247_986))
            .update_date(DateTime::from_secs(1_423_247_986))
            .build()
    }

    fn marker_of(index: usize) -> Option<String> {
        (index > 0).then(|| format!("marker-{index}"))
    }

    /// Rules for `ListPolicies` that return `pages` in order (`None`: no `Policies` member).
    /// Each rule also checks that no filtering parameter is set.
    fn list_policies_rules(pages: Vec<Option<Vec<Policy>>>) -> Vec<Rule> {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(index, policies)| {
                let expected = marker_of(index);
                mock!(aws_sdk_iam::Client::list_policies)
                    .match_requests(move |input: &ListPoliciesInput| {
                        input.marker() == expected.as_deref()
                            && input.scope().is_none()
                            && input.only_attached().is_none()
                            && input.path_prefix().is_none()
                            && input.policy_usage_filter().is_none()
                            && input.max_items().is_none()
                    })
                    .then_output(move || {
                        let last = index + 1 == count;
                        ListPoliciesOutput::builder()
                            .set_policies(policies.clone())
                            .is_truncated(!last)
                            .set_marker((!last).then(|| format!("marker-{}", index + 1)))
                            .build()
                    })
            })
            .collect()
    }

    fn percent_encode(text: &str) -> String {
        let mut out = String::new();
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+') {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    fn admin_document() -> Value {
        json!({
            "Version": "2012-10-17",
            "Statement": [{ "Effect": "Allow", "Action": "*", "Resource": "*" }],
        })
    }

    /// A rule for `GetPolicyVersion` of `arn` at version `v1` that returns `document`.
    fn version_rule(arn: &str, document: &Value) -> Rule {
        let arn = arn.to_owned();
        let encoded = percent_encode(&document.to_string());
        mock!(aws_sdk_iam::Client::get_policy_version)
            .match_requests(move |input: &GetPolicyVersionInput| {
                input.policy_arn() == Some(arn.as_str()) && input.version_id() == Some("v1")
            })
            .then_output(move || {
                GetPolicyVersionOutput::builder()
                    .policy_version(
                        PolicyVersion::builder()
                            .document(encoded.clone())
                            .version_id("v1")
                            .is_default_version(true)
                            .create_date(DateTime::from_secs(1_423_247_986))
                            .build(),
                    )
                    .build()
            })
    }

    fn version_error_rule(arn: &str, code: &'static str) -> Rule {
        let arn = arn.to_owned();
        mock!(aws_sdk_iam::Client::get_policy_version)
            .match_requests(move |input: &GetPolicyVersionInput| {
                input.policy_arn() == Some(arn.as_str())
            })
            .then_error(move || GetPolicyVersionError::generic(metadata(code)))
    }

    fn standard_rules() -> Vec<Rule> {
        let mut rules = list_policies_rules(vec![
            Some(vec![
                policy("AdministratorAccess", ADMIN_ARN, 1),
                policy("example-policy", CUSTOM_ARN, 2),
            ]),
            Some(vec![policy("unattached-policy", UNATTACHED_ARN, 0)]),
        ]);
        rules.push(version_rule(ADMIN_ARN, &admin_document()));
        rules.push(version_rule(CUSTOM_ARN, &admin_document()));
        rules.push(version_rule(UNATTACHED_ARN, &admin_document()));
        rules
    }

    fn sorted(mut resources: Vec<Resource>) -> Vec<Resource> {
        resources.sort_by(|a, b| a.id.cmp(&b.id));
        resources
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let rules = standard_rules();
        let (factory, sent) = mock_factory_counted(&rules);
        let resources = sorted(collect(&context_with(factory), "us-east-1").await.unwrap());

        // AWS managed, unattached and customer managed policies are all kept.
        assert_eq!(resources.len(), 3);
        let ids: Vec<&str> = resources.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, [CUSTOM_ARN, UNATTACHED_ARN, ADMIN_ARN]);
        for resource in &resources {
            assert_eq!(resource.resource_type, "aws.iam.policy");
            assert_eq!(resource.account, "aws-123456789012");
            assert_eq!(resource.region, "global");
            assert!(resource.tags.is_empty());
        }
        assert_eq!(resources[2].name.as_deref(), Some("AdministratorAccess"));
        assert_eq!(
            resources[1].configuration["ListPolicies"]["Policies"][0]["AttachmentCount"],
            json!(0)
        );
        assert_eq!(
            resources[2].configuration["GetPolicyVersion"]["PolicyVersion"]["Document"],
            admin_document()
        );
        let operations = sent.operations();
        assert!(operations.iter().any(|o| o == "ListPolicies"));
        assert!(operations.iter().any(|o| o == "GetPolicyVersion"));
        assert_golden(
            &golden_path("iam", "policy"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn the_document_is_decoded_from_an_http_response() {
        let document = json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Sid": "Mixed100% a+b \u{65e5}\u{672c}\u{8a9e}",
                "Effect": "Allow",
                "Action": "s3:GetObject",
                "Resource": "arn:aws:s3:::example-bucket/*",
            }],
        });
        let encoded = percent_encode(&document.to_string());
        // The API may leave a plus sign as it is; it must not become a space.
        assert!(encoded.contains("a+b"));
        let xml = format!(
            "<GetPolicyVersionResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
             <GetPolicyVersionResult><PolicyVersion><Document>{encoded}</Document>\
             <VersionId>v1</VersionId><IsDefaultVersion>true</IsDefaultVersion>\
             <CreateDate>2015-02-06T18:39:46Z</CreateDate></PolicyVersion>\
             </GetPolicyVersionResult><ResponseMetadata><RequestId>example</RequestId>\
             </ResponseMetadata></GetPolicyVersionResponse>"
        );
        let mut rules = list_policies_rules(vec![Some(vec![policy("example-policy", CUSTOM_ARN, 1)])]);
        rules.push(
            mock!(aws_sdk_iam::Client::get_policy_version).then_http_response(move || {
                HttpResponse::new(
                    StatusCode::try_from(200u16).unwrap(),
                    SdkBody::from(xml.clone()),
                )
            }),
        );
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let version = &resources[0].configuration["GetPolicyVersion"]["PolicyVersion"];
        // A second decoding by the SDK would break the `%25` and the document would stay a string.
        assert_eq!(version["Document"], document);
        assert_eq!(version["CreateDate"], json!("2015-02-06T18:39:46Z"));
        assert_eq!(version["IsDefaultVersion"], json!(true));
        assert_eq!(version["VersionId"], json!("v1"));
    }

    #[test]
    fn decode_document_cases() {
        let original = |text: &str| Value::String(text.to_owned());
        // Decoded and parsed.
        assert_eq!(decode_document("%7B%22a%22%3A1%7D"), json!({ "a": 1 }));
        assert_eq!(
            decode_document("%7B%22a%22%3A%22%E6%97%A5%22%7D"),
            json!({ "a": "\u{65e5}" })
        );
        // `+` is not a space.
        assert_eq!(decode_document("%22a+b%22"), json!("a+b"));
        assert_eq!(decode_document("%22a%2Bb%22"), json!("a+b"));
        // Failures keep the string returned by the API.
        for bad in [
            "%",
            "%7",
            "%zz",
            "%7B%22a%22%3A1%7",
            "%FF%FE",
            "%7B",
            "not json",
            "",
        ] {
            assert_eq!(decode_document(bad), original(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn the_listing_value_is_an_object_with_only_the_element_array() {
        let rules = standard_rules();
        let resources = sorted(collect(&context(&rules), "us-east-1").await.unwrap());
        assert_eq!(resources.len(), 3);
        for resource in &resources {
            let value = resource.configuration["ListPolicies"].as_object().unwrap();
            assert_eq!(value.keys().collect::<Vec<_>>(), ["Policies"]);
            assert!(!value.contains_key("IsTruncated") && !value.contains_key("Marker"));
            let policies = value["Policies"].as_array().unwrap();
            assert_eq!(policies.len(), 1);
            assert_eq!(policies[0]["Arn"], json!(resource.id));
        }
    }

    #[tokio::test]
    async fn a_failed_version_call_marks_only_that_policy() {
        for (code, kind) in [
            ("Throttling", "throttled"),
            ("AccessDenied", "access_denied"),
            ("NoSuchEntity", "service"),
        ] {
            let mut rules = list_policies_rules(vec![Some(vec![
                policy("AdministratorAccess", ADMIN_ARN, 1),
                policy("example-policy", CUSTOM_ARN, 2),
            ])]);
            rules.push(version_rule(ADMIN_ARN, &admin_document()));
            rules.push(version_error_rule(CUSTOM_ARN, code));
            let resources = collect(&context(&rules), "us-east-1").await.unwrap();
            assert_eq!(
                resources[0].configuration["GetPolicyVersion"]["PolicyVersion"]["VersionId"],
                json!("v1")
            );
            let mark = &resources[1].configuration["GetPolicyVersion"];
            assert_eq!(mark["yata_error"]["kind"], json!(kind), "{code}");
            let message = mark["yata_error"]["message"].as_str().unwrap();
            assert!(message.contains("GetPolicyVersion") && message.contains(code));
            assert!(!message.contains("arn:aws"));
            assert_eq!(mark.as_object().unwrap().len(), 1);
            // The listing data is still there.
            assert_eq!(
                resources[1].configuration["ListPolicies"]["Policies"][0]["Arn"],
                json!(CUSTOM_ARN)
            );
        }
    }

    #[tokio::test]
    async fn a_policy_without_a_default_version_is_not_fetched() {
        let no_version = Policy::builder()
            .policy_name("example-policy")
            .arn(CUSTOM_ARN)
            .build();
        let mut rules = list_policies_rules(vec![Some(vec![no_version])]);
        let version = mock!(aws_sdk_iam::Client::get_policy_version).then_output(|| {
            GetPolicyVersionOutput::builder().build()
        });
        rules.push(version.clone());
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(version.num_calls(), 0);
        let mark = &resources[0].configuration["GetPolicyVersion"]["yata_error"];
        assert_eq!(mark["kind"], json!("invalid_response"));
        assert_eq!(
            mark["message"],
            json!("invalid response when calling ListPolicies (MissingDefaultVersionId)")
        );
        assert_eq!(
            resources[0].configuration["ListPolicies"]["Policies"][0]["DefaultVersionId"],
            Value::Null
        );
    }

    #[tokio::test]
    async fn a_missing_policy_version_is_null() {
        let mut rules = list_policies_rules(vec![Some(vec![policy("example-policy", CUSTOM_ARN, 1)])]);
        rules.push(
            mock!(aws_sdk_iam::Client::get_policy_version)
                .then_output(|| GetPolicyVersionOutput::builder().build()),
        );
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetPolicyVersion"],
            json!({ "PolicyVersion": null })
        );
    }

    #[tokio::test]
    async fn a_missing_document_is_null_and_a_bad_date_marks_the_key() {
        let mut rules = list_policies_rules(vec![Some(vec![
            policy("AdministratorAccess", ADMIN_ARN, 1),
            policy("example-policy", CUSTOM_ARN, 2),
        ])]);
        rules.push(
            mock!(aws_sdk_iam::Client::get_policy_version)
                .match_requests(|input: &GetPolicyVersionInput| {
                    input.policy_arn() == Some(ADMIN_ARN)
                })
                .then_output(|| {
                    GetPolicyVersionOutput::builder()
                        .policy_version(
                            PolicyVersion::builder()
                                .version_id("v1")
                                .is_default_version(true)
                                .build(),
                        )
                        .build()
                }),
        );
        rules.push(
            mock!(aws_sdk_iam::Client::get_policy_version)
                .match_requests(|input: &GetPolicyVersionInput| {
                    input.policy_arn() == Some(CUSTOM_ARN)
                })
                .then_output(|| {
                    GetPolicyVersionOutput::builder()
                        .policy_version(
                            PolicyVersion::builder()
                                .version_id("v1")
                                .create_date(DateTime::from_secs(i64::MAX))
                                .build(),
                        )
                        .build()
                }),
        );
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let first = &resources[0].configuration["GetPolicyVersion"]["PolicyVersion"];
        assert_eq!(first["Document"], Value::Null);
        assert_eq!(first["CreateDate"], Value::Null);
        let mark = &resources[1].configuration["GetPolicyVersion"]["yata_error"];
        assert_eq!(mark["kind"], json!("invalid_response"));
        assert_eq!(
            mark["message"],
            json!("invalid response when calling GetPolicyVersion (InvalidDateTime)")
        );
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
            let rule = mock!(aws_sdk_iam::Client::list_policies)
                .then_error(move || ListPoliciesError::generic(metadata(code)));
            let err = collect(&context(&[rule]), "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("ListPolicies") && err.message().contains(code));
            assert!(!err.message().contains("arn:aws"));
        }
    }

    #[tokio::test]
    async fn a_failure_on_the_second_page_fails_the_whole_group() {
        let first = list_policies_rules(vec![
            Some(vec![policy("AdministratorAccess", ADMIN_ARN, 1)]),
            Some(Vec::new()),
        ])
        .remove(0);
        let second = mock!(aws_sdk_iam::Client::list_policies)
            .match_requests(|input: &ListPoliciesInput| input.marker() == Some("marker-1"))
            .then_error(|| ListPoliciesError::generic(metadata("ServiceFailure")));
        let err = collect(&context(&[first, second]), "us-east-1")
            .await
            .unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::Service);
    }

    #[tokio::test]
    async fn an_element_without_an_arn_fails_the_whole_group() {
        let nameless = Policy::builder().policy_name("no-arn").build();
        let rules = list_policies_rules(vec![Some(vec![nameless])]);
        let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::InvalidResponse);
        assert_eq!(
            err.message(),
            "invalid response when calling ListPolicies (MissingArn)"
        );
    }

    #[tokio::test]
    async fn an_unformattable_listing_date_fails_the_whole_group() {
        let broken = Policy::builder()
            .policy_name("example-policy")
            .arn(CUSTOM_ARN)
            .default_version_id("v1")
            .create_date(DateTime::from_secs(i64::MAX))
            .build();
        let rules = list_policies_rules(vec![Some(vec![broken])]);
        let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
        assert_eq!(
            err.message(),
            "invalid response when calling ListPolicies (InvalidDateTime)"
        );
    }

    #[tokio::test]
    async fn no_policies_member_on_any_page_fails_the_whole_group() {
        let rules = list_policies_rules(vec![None, None]);
        let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
        assert_eq!(err.kind(), CollectionErrorKind::InvalidResponse);
        assert_eq!(
            err.message(),
            "invalid response when calling ListPolicies (MissingPolicies)"
        );

        // The same through an HTTP response that has no `Policies` element.
        let rule = mock!(aws_sdk_iam::Client::list_policies).then_http_response(|| {
            HttpResponse::new(
                StatusCode::try_from(200u16).unwrap(),
                SdkBody::from(
                    "<ListPoliciesResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
                     <ListPoliciesResult><IsTruncated>false</IsTruncated></ListPoliciesResult>\
                     <ResponseMetadata><RequestId>example</RequestId></ResponseMetadata>\
                     </ListPoliciesResponse>",
                ),
            )
        });
        let err = collect(&context(&[rule]), "us-east-1").await.unwrap_err();
        assert_eq!(
            err.message(),
            "invalid response when calling ListPolicies (MissingPolicies)"
        );
    }

    #[tokio::test]
    async fn one_page_with_policies_is_enough() {
        let mut rules = list_policies_rules(vec![
            None,
            Some(vec![policy("example-policy", CUSTOM_ARN, 1)]),
        ]);
        rules.push(version_rule(CUSTOM_ARN, &admin_document()));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].id, CUSTOM_ARN);
    }

    #[tokio::test]
    async fn the_fan_out_limit_does_not_change_the_result() {
        let rules = standard_rules();
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
        assert_eq!(c.resource_type(), "aws.iam.policy");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(
            c.required_permissions(),
            ["iam:ListPolicies", "iam:GetPolicyVersion"]
        );
    }
}
