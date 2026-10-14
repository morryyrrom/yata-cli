//! Collector for `aws.iam.password_policy`: the account password policy
//! (`GetAccountPasswordPolicy`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use yata_core::{Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::errors::{CollectError, Failure, facts_of};

const RESOURCE_TYPE: &str = "aws.iam.password_policy";
const OPERATION: &str = "GetAccountPasswordPolicy";
const NO_SUCH_ENTITY: &str = "NoSuchEntity";

/// Returns the collector of `aws.iam.password_policy`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(PasswordPolicyCollector)
}

struct PasswordPolicyCollector;

impl Collector for PasswordPolicyCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &["iam:GetAccountPasswordPolicy"]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_iam::Client = ctx.client(region);
    let value = match client.get_account_password_policy().send().await {
        Ok(output) => {
            let policy = match output.password_policy() {
                Some(policy) => json!({
                    "AllowUsersToChangePassword": policy.allow_users_to_change_password(),
                    "ExpirePasswords": policy.expire_passwords(),
                    "HardExpiry": policy.hard_expiry(),
                    "MaxPasswordAge": policy.max_password_age(),
                    "MinimumPasswordLength": policy.minimum_password_length(),
                    "PasswordReusePrevention": policy.password_reuse_prevention(),
                    "RequireLowercaseCharacters": policy.require_lowercase_characters(),
                    "RequireNumbers": policy.require_numbers(),
                    "RequireSymbols": policy.require_symbols(),
                    "RequireUppercaseCharacters": policy.require_uppercase_characters(),
                }),
                None => Value::Null,
            };
            json!({ "PasswordPolicy": policy })
        }
        Err(err) => {
            // The account has no password policy: a fact, not a failure.
            let facts = facts_of(&err);
            if facts.failure == Failure::Service && facts.code.as_deref() == Some(NO_SUCH_ENTITY) {
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
    use aws_sdk_iam::error::ErrorMetadata;
    use aws_sdk_iam::operation::get_account_password_policy::{
        GetAccountPasswordPolicyError, GetAccountPasswordPolicyOutput,
    };
    use aws_sdk_iam::types::PasswordPolicy;
    use aws_sdk_s3::primitives::SdkBody;
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

    fn policy_output() -> GetAccountPasswordPolicyOutput {
        GetAccountPasswordPolicyOutput::builder()
            .password_policy(
                PasswordPolicy::builder()
                    .allow_users_to_change_password(true)
                    .expire_passwords(false)
                    .minimum_password_length(14)
                    .password_reuse_prevention(24)
                    .require_lowercase_characters(true)
                    .require_numbers(true)
                    .require_symbols(true)
                    .require_uppercase_characters(true)
                    .build(),
            )
            .build()
    }

    fn error_rule(code: &'static str) -> Rule {
        mock!(aws_sdk_iam::Client::get_account_password_policy).then_error(move || {
            GetAccountPasswordPolicyError::generic(
                ErrorMetadata::builder()
                    .code(code)
                    .message("User arn:aws:iam::123456789012:user/example-user failed")
                    .build(),
            )
        })
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let rule =
            mock!(aws_sdk_iam::Client::get_account_password_policy).then_output(policy_output);
        let ctx = context(std::slice::from_ref(&rule));
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(rule.num_calls(), 1);
        assert_eq!(resources.len(), 1);
        let resource = &resources[0];
        assert_eq!(resource.id, "aws.iam.password_policy:123456789012:global");
        assert_eq!(resource.resource_type, "aws.iam.password_policy");
        assert_eq!(resource.account, "aws-123456789012");
        assert_eq!(resource.region, "global");
        assert_eq!(resource.name, None);
        assert!(resource.tags.is_empty());
        let policy = &resource.configuration["GetAccountPasswordPolicy"]["PasswordPolicy"];
        assert_eq!(policy.as_object().unwrap().len(), 10);
        assert_eq!(policy["HardExpiry"], Value::Null);
        assert_eq!(policy["MaxPasswordAge"], Value::Null);
        assert_eq!(policy["MinimumPasswordLength"], json!(14));
        assert_golden(
            &golden_path("iam", "password_policy"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn no_such_entity_gives_a_null_value() {
        let rule = mock!(aws_sdk_iam::Client::get_account_password_policy).then_http_response(|| {
            HttpResponse::new(
                StatusCode::try_from(404u16).unwrap(),
                SdkBody::from(
                    "<ErrorResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
                     <Error><Type>Sender</Type><Code>NoSuchEntity</Code>\
                     <Message>The Password Policy with domain name 123456789012 cannot be found.</Message>\
                     </Error><RequestId>example</RequestId></ErrorResponse>",
                ),
            )
        });
        let ctx = context(&[rule]);
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].configuration["GetAccountPasswordPolicy"],
            Value::Null
        );
        let mock_error = collect(&context(&[error_rule("NoSuchEntity")]), "us-east-1")
            .await
            .unwrap();
        assert_eq!(
            mock_error[0].configuration["GetAccountPasswordPolicy"],
            Value::Null
        );
    }

    #[tokio::test]
    async fn missing_password_policy_in_a_success_is_distinct_from_null() {
        let rule = mock!(aws_sdk_iam::Client::get_account_password_policy)
            .then_output(|| GetAccountPasswordPolicyOutput::builder().build());
        let ctx = context(&[rule]);
        let resources = collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetAccountPasswordPolicy"],
            json!({ "PasswordPolicy": null })
        );
    }

    #[tokio::test]
    async fn other_errors_fail_the_whole_group() {
        let cases = [
            ("AccessDenied", CollectionErrorKind::AccessDenied),
            ("ServiceFailure", CollectionErrorKind::Service),
            ("Throttling", CollectionErrorKind::Throttled),
            ("NotAuthorized", CollectionErrorKind::AccessDenied),
        ];
        for (code, kind) in cases {
            let err = collect(&context(&[error_rule(code)]), "us-east-1")
                .await
                .unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("GetAccountPasswordPolicy"), "{code}");
            assert!(err.message().contains(code), "{code}");
            assert!(!err.message().contains("arn:aws"), "{code}");
        }
    }

    #[tokio::test]
    async fn the_operation_reaches_the_transmit_stage() {
        let rule =
            mock!(aws_sdk_iam::Client::get_account_password_policy).then_output(policy_output);
        let (factory, sent) = mock_factory_counted(&[rule]);
        let ctx = CollectContext::new(
            "123456789012".to_owned(),
            "aws-123456789012".to_owned(),
            factory,
        );
        collect(&ctx, "us-east-1").await.unwrap();
        assert_eq!(sent.operations(), ["GetAccountPasswordPolicy"]);
    }

    #[test]
    fn metadata() {
        let c = collector();
        assert_eq!(c.resource_type(), "aws.iam.password_policy");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(c.required_permissions(), ["iam:GetAccountPasswordPolicy"]);
    }
}
