//! Collector for `aws.iam.account_summary`: the IAM account summary (`GetAccountSummary`).

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value, json};
use yata_core::{Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::convert::map_to_sorted_object;
use crate::support::errors::CollectError;

const RESOURCE_TYPE: &str = "aws.iam.account_summary";
const OPERATION: &str = "GetAccountSummary";

/// Returns the collector of `aws.iam.account_summary`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(AccountSummaryCollector)
}

struct AccountSummaryCollector;

impl Collector for AccountSummaryCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &["iam:GetAccountSummary"]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_iam::Client = ctx.client(region);
    let output = client
        .get_account_summary()
        .send()
        .await
        .map_err(|err| CollectError::from_sdk(OPERATION, &err))?;

    let summary_map = match output.summary_map() {
        Some(map) => {
            let by_name: HashMap<String, i32> = map
                .iter()
                .map(|(key, value)| (key.as_str().to_owned(), *value))
                .collect();
            map_to_sorted_object(&by_name, |value| json!(value))
        }
        None => Value::Null,
    };

    let mut configuration = Map::new();
    configuration.insert(OPERATION.to_owned(), json!({ "SummaryMap": summary_map }));
    Ok(vec![Resource {
        id: format!("{RESOURCE_TYPE}:{}:global", ctx.account_id()),
        resource_type: RESOURCE_TYPE.to_owned(),
        account: ctx.account_key().to_owned(),
        region: "global".to_owned(),
        name: None,
        tags: BTreeMap::new(),
        configuration,
    }])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::support::clients::testing::{mock_factory, mock_factory_counted};
    use crate::support::golden::{assert_golden, golden_path};
    use aws_sdk_iam::error::ErrorMetadata;
    use aws_sdk_iam::operation::get_account_summary::{
        GetAccountSummaryError, GetAccountSummaryOutput,
    };
    use aws_sdk_s3::primitives::SdkBody;
    use aws_sdk_iam::types::SummaryKeyType;
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;
    use yata_core::CollectionErrorKind;

    fn context(rules: &[Rule]) -> CollectContext {
        CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            mock_factory(rules),
        )
    }

    fn summary_output() -> GetAccountSummaryOutput {
        GetAccountSummaryOutput::builder()
            .summary_map(SummaryKeyType::Users, 3)
            .summary_map(SummaryKeyType::UsersQuota, 5000)
            .summary_map(SummaryKeyType::AccountMfaEnabled, 1)
            .summary_map(SummaryKeyType::AccountAccessKeysPresent, 0)
            .summary_map(SummaryKeyType::AccountPasswordPresent, 1)
            .build()
    }

    fn xml_response(body: &'static str) -> HttpResponse {
        HttpResponse::new(StatusCode::try_from(200u16).unwrap(), SdkBody::from(body))
    }

    fn error_rule(code: &'static str) -> Rule {
        mock!(aws_sdk_iam::Client::get_account_summary).then_error(move || {
            GetAccountSummaryError::generic(
                ErrorMetadata::builder()
                    .code(code)
                    .message("User arn:aws:iam::123456789012:user/example-user is not allowed")
                    .build(),
            )
        })
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let rule = mock!(aws_sdk_iam::Client::get_account_summary).then_output(summary_output);
        let ctx = context(std::slice::from_ref(&rule));
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(rule.num_calls(), 1);
        assert_eq!(resources.len(), 1);
        let resource = &resources[0];
        assert_eq!(resource.id, "aws.iam.account_summary:123456789012:global");
        assert_eq!(resource.resource_type, "aws.iam.account_summary");
        assert_eq!(resource.account, "aws-123456789012");
        assert_eq!(resource.region, "global");
        assert_eq!(resource.name, None);
        assert!(resource.tags.is_empty());
        assert_golden(
            &golden_path("iam", "account_summary"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn summary_map_keys_are_ascending() {
        let rule = mock!(aws_sdk_iam::Client::get_account_summary).then_output(summary_output);
        let ctx = context(&[rule]);
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        let map = resources[0].configuration["GetAccountSummary"]["SummaryMap"]
            .as_object()
            .unwrap();
        let keys: Vec<&String> = map.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(map["AccountMFAEnabled"], json!(1));
        assert_eq!(map["AccountAccessKeysPresent"], json!(0));
    }

    #[tokio::test]
    async fn unknown_keys_are_kept_as_strings() {
        let rule = mock!(aws_sdk_iam::Client::get_account_summary).then_http_response(|| {
            xml_response(
                "<GetAccountSummaryResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
                 <GetAccountSummaryResult><SummaryMap>\
                 <entry><key>Users</key><value>3</value></entry>\
                 <entry><key>ExampleFutureKey</key><value>7</value></entry>\
                 </SummaryMap></GetAccountSummaryResult>\
                 <ResponseMetadata><RequestId>example</RequestId></ResponseMetadata>\
                 </GetAccountSummaryResponse>",
            )
        });
        let ctx = context(&[rule]);
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        let map = &resources[0].configuration["GetAccountSummary"]["SummaryMap"];
        assert_eq!(map["ExampleFutureKey"], json!(7));
        assert_eq!(map["Users"], json!(3));
    }

    #[tokio::test]
    async fn missing_summary_map_is_null() {
        let rule = mock!(aws_sdk_iam::Client::get_account_summary)
            .then_output(|| GetAccountSummaryOutput::builder().build());
        let ctx = context(&[rule]);
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetAccountSummary"],
            json!({ "SummaryMap": null })
        );
    }

    #[tokio::test]
    async fn failures_fail_the_whole_group() {
        let cases = [
            ("AccessDenied", CollectionErrorKind::AccessDenied),
            ("Throttling", CollectionErrorKind::Throttled),
            ("ServiceFailure", CollectionErrorKind::Service),
            ("NotAuthorized", CollectionErrorKind::AccessDenied),
        ];
        for (code, kind) in cases {
            let ctx = context(&[error_rule(code)]);
            let err = collect(&ctx, "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("GetAccountSummary"), "{code}");
            assert!(err.message().contains(code), "{code}");
            assert!(!err.message().contains("arn:aws"), "{code}");
        }
    }

    #[tokio::test]
    async fn the_operation_reaches_the_transmit_stage() {
        let rule = mock!(aws_sdk_iam::Client::get_account_summary).then_output(summary_output);
        let (factory, sent) = mock_factory_counted(&[rule]);
        let ctx = CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            factory,
        );
        collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(sent.operations(), ["GetAccountSummary"]);
    }

    #[test]
    fn metadata() {
        let c = collector();
        assert_eq!(c.resource_type(), "aws.iam.account_summary");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(c.required_permissions(), ["iam:GetAccountSummary"]);
    }
}
