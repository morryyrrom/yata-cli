//! Collector for `aws.s3control.account_public_access_block`: the account-level S3 Block Public
//! Access settings (S3 Control `GetPublicAccessBlock`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use yata_core::{Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::errors::{CollectError, Failure, facts_of};

const RESOURCE_TYPE: &str = "aws.s3control.account_public_access_block";
const OPERATION: &str = "GetPublicAccessBlock";
const NO_SUCH_CONFIGURATION: &str = "NoSuchPublicAccessBlockConfiguration";

/// Returns the collector of `aws.s3control.account_public_access_block`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(AccountPublicAccessBlockCollector)
}

struct AccountPublicAccessBlockCollector;

impl Collector for AccountPublicAccessBlockCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &["s3:GetAccountPublicAccessBlock"]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_s3control::Client = ctx.client(region);
    let value = match client
        .get_public_access_block()
        .account_id(ctx.account_id())
        .send()
        .await
    {
        Ok(output) => {
            let configuration = match output.public_access_block_configuration() {
                Some(config) => json!({
                    "BlockPublicAcls": config.block_public_acls(),
                    "BlockPublicPolicy": config.block_public_policy(),
                    "IgnorePublicAcls": config.ignore_public_acls(),
                    "RestrictPublicBuckets": config.restrict_public_buckets(),
                }),
                None => Value::Null,
            };
            json!({ "PublicAccessBlockConfiguration": configuration })
        }
        Err(err) => {
            // The account has no setting: a fact, not a failure.
            let facts = facts_of(&err);
            if facts.failure == Failure::Service
                && facts.code.as_deref() == Some(NO_SUCH_CONFIGURATION)
            {
                Value::Null
            } else {
                return Err(CollectError::from_sdk(OPERATION, &err));
            }
        }
    };

    let mut configuration = Map::new();
    configuration.insert(OPERATION.to_owned(), value);
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
    use aws_sdk_s3control::error::ErrorMetadata;
    use aws_sdk_s3control::operation::get_public_access_block::{
        GetPublicAccessBlockError, GetPublicAccessBlockInput, GetPublicAccessBlockOutput,
    };
    use aws_sdk_s3control::types::PublicAccessBlockConfiguration;
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

    fn account_rule(
        then: impl Fn() -> GetPublicAccessBlockOutput + Send + Sync + 'static,
    ) -> Rule {
        mock!(aws_sdk_s3control::Client::get_public_access_block)
            .match_requests(|input: &GetPublicAccessBlockInput| {
                input.account_id() == Some("123456789012")
            })
            .then_output(then)
    }

    fn full_output() -> GetPublicAccessBlockOutput {
        GetPublicAccessBlockOutput::builder()
            .public_access_block_configuration(
                PublicAccessBlockConfiguration::builder()
                    .block_public_acls(true)
                    .block_public_policy(true)
                    .ignore_public_acls(true)
                    .restrict_public_buckets(false)
                    .build(),
            )
            .build()
    }

    fn error_rule(code: &'static str) -> Rule {
        mock!(aws_sdk_s3control::Client::get_public_access_block).then_error(move || {
            GetPublicAccessBlockError::generic(
                ErrorMetadata::builder()
                    .code(code)
                    .message("failure for arn:aws:iam::123456789012:user/example-user")
                    .build(),
            )
        })
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let rule = account_rule(full_output);
        let (factory, sent) = mock_factory_counted(std::slice::from_ref(&rule));
        let ctx = CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            factory,
        );
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        // The account id matcher accepted the request, so the rule was used once.
        assert_eq!(rule.num_calls(), 1);
        assert_eq!(sent.operations(), ["GetPublicAccessBlock"]);
        assert_eq!(resources.len(), 1);
        let resource = &resources[0];
        assert_eq!(
            resource.id,
            "aws.s3control.account_public_access_block:123456789012:global"
        );
        assert_eq!(
            resource.resource_type,
            "aws.s3control.account_public_access_block"
        );
        assert_eq!(resource.account, "aws-123456789012");
        assert_eq!(resource.region, "global");
        assert_eq!(resource.name, None);
        assert!(resource.tags.is_empty());
        assert_golden(
            &golden_path("s3control", "account_public_access_block"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn four_members_are_present_and_missing_ones_are_null() {
        let full = collect(&context(&[account_rule(full_output)]), "us-east-1")
            .await
            .unwrap();
        let value = &full[0].configuration["GetPublicAccessBlock"]["PublicAccessBlockConfiguration"];
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(value["RestrictPublicBuckets"], json!(false));

        let partial = account_rule(|| {
            GetPublicAccessBlockOutput::builder()
                .public_access_block_configuration(
                    PublicAccessBlockConfiguration::builder()
                        .block_public_acls(true)
                        .build(),
                )
                .build()
        });
        let resources = collect(&context(&[partial]), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetPublicAccessBlock"],
            json!({
                "PublicAccessBlockConfiguration": {
                    "BlockPublicAcls": true,
                    "BlockPublicPolicy": null,
                    "IgnorePublicAcls": null,
                    "RestrictPublicBuckets": null,
                }
            })
        );
    }

    #[tokio::test]
    async fn no_such_configuration_gives_a_null_value() {
        let xml = mock!(aws_sdk_s3control::Client::get_public_access_block).then_http_response(|| {
            HttpResponse::new(
                StatusCode::try_from(404u16).unwrap(),
                aws_sdk_s3::primitives::SdkBody::from(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                     <ErrorResponse><Error><Code>NoSuchPublicAccessBlockConfiguration</Code>\
                     <Message>The public access block configuration was not found</Message>\
                     <AccountId>123456789012</AccountId></Error>\
                     <RequestId>example</RequestId></ErrorResponse>",
                ),
            )
        });
        let resources = collect(&context(&[xml]), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].configuration["GetPublicAccessBlock"],
            Value::Null
        );
        let generic = collect(&context(&[error_rule(NO_SUCH_CONFIGURATION)]), "us-east-1")
            .await
            .unwrap();
        assert_eq!(generic.len(), 1);
        assert_eq!(generic[0].configuration["GetPublicAccessBlock"], Value::Null);
    }

    #[tokio::test]
    async fn missing_configuration_in_a_success_is_distinct_from_null() {
        let rule = account_rule(|| GetPublicAccessBlockOutput::builder().build());
        let resources = collect(&context(&[rule]), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetPublicAccessBlock"],
            json!({ "PublicAccessBlockConfiguration": null })
        );
    }

    #[tokio::test]
    async fn other_errors_fail_the_whole_group() {
        let cases = [
            ("AccessDenied", CollectionErrorKind::AccessDenied),
            ("SlowDown", CollectionErrorKind::Throttled),
            ("InternalError", CollectionErrorKind::Service),
        ];
        for (code, kind) in cases {
            let err = collect(&context(&[error_rule(code)]), "us-east-1")
                .await
                .unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("GetPublicAccessBlock"), "{code}");
            assert!(err.message().contains(code), "{code}");
            assert!(!err.message().contains("arn:aws"), "{code}");
        }
    }

    #[test]
    fn metadata() {
        let c = collector();
        assert_eq!(c.resource_type(), "aws.s3control.account_public_access_block");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(c.required_permissions(), ["s3:GetAccountPublicAccessBlock"]);
    }
}
