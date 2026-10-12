//! Preparation of a collection: SDK configuration, account and regions.
//!
//! The SDK configuration is built by `aws-config`; this crate never reads the AWS
//! environment variables itself. With `--profile`, only the credentials of that profile
//! are used (the default chain, which looks at environment variables first, is skipped).

use std::collections::BTreeMap;

use aws_config::environment::region::EnvironmentVariableRegionProvider;
use aws_config::meta::region::{ProvideRegion, RegionProviderChain};
use aws_config::profile::{ProfileFileCredentialsProvider, ProfileFileRegionProvider};
use aws_config::provider_config::ProviderConfig;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use serde_json::{Map, json};
use yata_core::Resource;

use crate::ACCOUNT_RESOURCE_TYPE;
use crate::error::SetupError;
use crate::support::clients::ClientFactory;
use crate::support::errors::{ErrorFacts, code_label, facts_of};
use crate::support::partition::{DEFAULT_GLOBAL_REGION, SUPPORTED_PARTITION, partition_of_arn};

#[cfg(test)]
#[allow(deprecated)]
use aws_config::profile::profile_file::ProfileFiles;

/// Where credentials come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CredentialsSource {
    /// The default provider chain of `aws-config`.
    DefaultChain,
    /// Only the credentials of the named profile; environment credentials are not used.
    ProfileOnly(String),
}

/// One step of the resolution of the first API region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegionStep {
    /// The default region chain of `aws-config`.
    SdkDefaultChain,
    /// `AWS_REGION`, then `AWS_DEFAULT_REGION`.
    EnvironmentVariables,
    /// The `region` of the named profile.
    ProfileRegion(String),
    /// A fixed fallback region.
    Fallback(&'static str),
}

/// How the SDK configuration is assembled for a given `--profile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigPlan {
    /// Source of credentials.
    pub(crate) credentials: CredentialsSource,
    /// Profile name given to the config loader (also used for other profile settings).
    pub(crate) profile_name: Option<String>,
    /// Region resolution steps, tried in order.
    pub(crate) region_steps: Vec<RegionStep>,
}

/// Builds the plan for the given `--profile` value.
pub(crate) fn config_plan(profile: Option<&str>) -> ConfigPlan {
    match profile {
        None => ConfigPlan {
            credentials: CredentialsSource::DefaultChain,
            profile_name: None,
            region_steps: vec![
                RegionStep::SdkDefaultChain,
                RegionStep::Fallback(DEFAULT_GLOBAL_REGION),
            ],
        },
        Some(name) => ConfigPlan {
            credentials: CredentialsSource::ProfileOnly(name.to_owned()),
            profile_name: Some(name.to_owned()),
            region_steps: vec![
                RegionStep::EnvironmentVariables,
                RegionStep::ProfileRegion(name.to_owned()),
                RegionStep::Fallback(DEFAULT_GLOBAL_REGION),
            ],
        },
    }
}

/// Test-only overrides of the configuration sources. Empty in production builds.
#[derive(Clone, Default)]
pub(crate) struct Overrides {
    #[cfg(test)]
    #[allow(deprecated)]
    pub(crate) files: Option<ProfileFiles>,
}

impl Overrides {
    #[cfg(test)]
    fn region_builder(
        &self,
        builder: aws_config::profile::region::Builder,
    ) -> aws_config::profile::region::Builder {
        match &self.files {
            Some(files) => builder.profile_files(files.clone()),
            None => builder,
        }
    }

    #[cfg(not(test))]
    fn region_builder(
        &self,
        builder: aws_config::profile::region::Builder,
    ) -> aws_config::profile::region::Builder {
        builder
    }

    #[cfg(test)]
    fn credentials_builder(
        &self,
        builder: aws_config::profile::credentials::Builder,
    ) -> aws_config::profile::credentials::Builder {
        match &self.files {
            Some(files) => builder.profile_files(files.clone()),
            None => builder,
        }
    }

    #[cfg(not(test))]
    fn credentials_builder(
        &self,
        builder: aws_config::profile::credentials::Builder,
    ) -> aws_config::profile::credentials::Builder {
        builder
    }

    #[cfg(test)]
    fn loader(&self, loader: aws_config::ConfigLoader) -> aws_config::ConfigLoader {
        match &self.files {
            Some(files) => loader.profile_files(files.clone()),
            None => loader,
        }
    }

    #[cfg(not(test))]
    fn loader(&self, loader: aws_config::ConfigLoader) -> aws_config::ConfigLoader {
        loader
    }
}

/// Builds the region provider chain described by the plan.
fn region_chain(plan: &ConfigPlan, overrides: &Overrides) -> RegionProviderChain {
    let mut chain: Option<RegionProviderChain> = None;
    for step in &plan.region_steps {
        let provider: Box<dyn ProvideRegion> = match step {
            RegionStep::SdkDefaultChain => {
                Box::new(aws_config::default_provider::region::default_provider())
            }
            RegionStep::EnvironmentVariables => Box::new(EnvironmentVariableRegionProvider::new()),
            RegionStep::ProfileRegion(name) => {
                let builder = ProfileFileRegionProvider::builder().profile_name(name.clone());
                Box::new(overrides.region_builder(builder).build())
            }
            RegionStep::Fallback(region) => Box::new(Region::from_static(region)),
        };
        chain = Some(match chain {
            None => RegionProviderChain::first_try(provider),
            Some(chain) => chain.or_else(provider),
        });
    }
    chain.unwrap_or_else(|| {
        RegionProviderChain::first_try(Region::from_static(DEFAULT_GLOBAL_REGION))
    })
}

/// Loads the SDK configuration and resolves the first API region.
pub(crate) async fn load_sdk_config(
    profile: Option<&str>,
    overrides: &Overrides,
) -> (SdkConfig, Region) {
    let plan = config_plan(profile);
    let region = region_chain(&plan, overrides)
        .region()
        .await
        .unwrap_or_else(|| Region::from_static(DEFAULT_GLOBAL_REGION));

    let mut loader = overrides
        .loader(aws_config::defaults(BehaviorVersion::latest()))
        .region(region.clone());
    if let Some(name) = &plan.profile_name {
        loader = loader.profile_name(name.clone());
    }
    if let CredentialsSource::ProfileOnly(name) = &plan.credentials {
        let provider_config = ProviderConfig::default().with_region(Some(region.clone()));
        let builder = ProfileFileCredentialsProvider::builder()
            .configure(&provider_config)
            .profile_name(name.clone());
        loader = loader.credentials_provider(overrides.credentials_builder(builder).build());
    }
    (loader.load().await, region)
}
/// Loads the SDK configuration for production use.
///
/// Returns the client factory and the first API region (used for `GetCallerIdentity`,
/// `DescribeRegions` and AssumeRole only; never as a scan region).
pub(crate) async fn load_environment(profile: Option<&str>) -> (ClientFactory, String) {
    let (sdk_config, region) = load_sdk_config(profile, &Overrides::default()).await;
    tracing::debug!(region = %region, profile = ?profile, "resolved the first API region");
    (ClientFactory::new(sdk_config), region.to_string())
}

/// The account that is being collected.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Identity {
    /// Account id (12 digits).
    pub(crate) account_id: String,
    /// Account key (`aws-<account id>`).
    pub(crate) account_key: String,
    /// The `aws.account.account` resource.
    pub(crate) resource: Resource,
}

fn setup_code(facts: &ErrorFacts) -> String {
    code_label(facts)
}

/// Calls `sts:GetCallerIdentity` and builds the account key and the account resource.
pub(crate) async fn identify(
    factory: &ClientFactory,
    region: &str,
) -> Result<Identity, SetupError> {
    let client = factory.client::<aws_sdk_sts::Client>(region);
    let output = client.get_caller_identity().send().await.map_err(|err| {
        let code = setup_code(&facts_of(&err));
        tracing::debug!(error = ?err, "GetCallerIdentity failed");
        SetupError::CallerIdentity {
            code,
            source: Box::new(err),
        }
    })?;

    let account_id = output.account().unwrap_or_default();
    if account_id.len() != 12 || !account_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(SetupError::InvalidAccountId);
    }
    let arn = output.arn().ok_or(SetupError::MissingArn)?;
    match partition_of_arn(arn) {
        Some(SUPPORTED_PARTITION) => {}
        other => {
            return Err(SetupError::UnsupportedPartition {
                partition: other.unwrap_or("unknown").to_owned(),
            });
        }
    }

    let account_key = format!("aws-{account_id}");
    let mut configuration = Map::new();
    configuration.insert(
        "GetCallerIdentity".to_owned(),
        json!({ "Account": account_id }),
    );
    let resource = Resource {
        id: format!("{ACCOUNT_RESOURCE_TYPE}:{account_id}:global"),
        resource_type: ACCOUNT_RESOURCE_TYPE.to_owned(),
        account: account_key.clone(),
        region: "global".to_owned(),
        name: None,
        tags: BTreeMap::new(),
        configuration,
    };
    Ok(Identity {
        account_id: account_id.to_owned(),
        account_key,
        resource,
    })
}

/// Calls `ec2:DescribeRegions` with default parameters and returns the region names, ascending.
pub(crate) async fn discover_regions(
    factory: &ClientFactory,
    region: &str,
) -> Result<Vec<String>, SetupError> {
    let client = factory.client::<aws_sdk_ec2::Client>(region);
    let output = client.describe_regions().send().await.map_err(|err| {
        let code = setup_code(&facts_of(&err));
        tracing::debug!(error = ?err, "DescribeRegions failed");
        SetupError::DescribeRegions {
            code,
            source: Box::new(err),
        }
    })?;
    let mut regions: Vec<String> = output
        .regions()
        .iter()
        .filter_map(|r| r.region_name().map(str::to_owned))
        .collect();
    regions.sort();
    regions.dedup();
    Ok(regions)
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use aws_config::profile::profile_file::ProfileFileKind;
    use aws_sdk_ec2::operation::describe_regions::DescribeRegionsOutput;
    use aws_sdk_ec2::types::Region as Ec2Region;
    use aws_sdk_sts::operation::get_caller_identity::GetCallerIdentityOutput;
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;

    use super::*;
    use crate::support::clients::testing::mock_factory;

    fn identity_rule(account: &str, arn: &str) -> Rule {
        let account = account.to_owned();
        let arn = arn.to_owned();
        mock!(aws_sdk_sts::Client::get_caller_identity).then_output(move || {
            GetCallerIdentityOutput::builder()
                .account(account.clone())
                .arn(arn.clone())
                .user_id("AIDAEXAMPLE")
                .build()
        })
    }

    fn http_error_rule_sts(status: u16, code: &str) -> Rule {
        let body = format!(
            "<ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code><Message>arn:aws:iam::123456789012:user/secret-name is not allowed</Message></Error><RequestId>r</RequestId></ErrorResponse>"
        );
        mock!(aws_sdk_sts::Client::get_caller_identity).then_http_response(move || {
            HttpResponse::new(
                StatusCode::try_from(status).expect("valid status"),
                aws_sdk_s3::primitives::SdkBody::from(body.clone()),
            )
        })
    }

    #[tokio::test]
    async fn identify_builds_the_account_resource_without_arn_and_user_id() {
        let rule = identity_rule("123456789012", "arn:aws:iam::123456789012:user/example");
        let factory = mock_factory(&[rule]);
        let identity = identify(&factory, "us-east-1").await.expect("identified");
        assert_eq!(identity.account_id, "123456789012");
        assert_eq!(identity.account_key, "aws-123456789012");
        let value = serde_json::to_value(&identity.resource).expect("serializes");
        assert_eq!(
            value,
            json!({
                "id": "aws.account.account:123456789012:global",
                "type": "aws.account.account",
                "account": "aws-123456789012",
                "region": "global",
                "tags": {},
                "configuration": { "GetCallerIdentity": { "Account": "123456789012" } }
            })
        );
        let text = value.to_string();
        assert!(!text.contains("arn:aws"));
        assert!(!text.contains("AIDAEXAMPLE"));
    }

    #[tokio::test]
    async fn identify_rejects_other_partitions() {
        let rule = identity_rule("123456789012", "arn:aws-cn:iam::123456789012:user/example");
        let factory = mock_factory(&[rule]);
        let err = identify(&factory, "us-east-1").await.expect_err("rejected");
        assert!(
            matches!(err, SetupError::UnsupportedPartition { ref partition } if partition == "aws-cn"),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn identify_rejects_account_ids_that_are_not_12_digits() {
        for account in ["12345", "12345678901a", "1234567890123", ""] {
            let rule = identity_rule(account, "arn:aws:iam::123456789012:user/example");
            let factory = mock_factory(&[rule]);
            let err = identify(&factory, "us-east-1").await.expect_err("rejected");
            assert!(
                matches!(err, SetupError::InvalidAccountId),
                "{account}: {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn identify_failure_is_a_setup_error_without_the_service_message() {
        let rule = http_error_rule_sts(403, "AccessDenied");
        let factory = mock_factory(&[rule]);
        let err = identify(&factory, "us-east-1").await.expect_err("fails");
        assert!(matches!(err, SetupError::CallerIdentity { .. }), "{err:?}");
        let message = err.to_string();
        assert!(message.contains("AccessDenied"), "{message}");
        assert!(!message.contains("secret-name"), "{message}");
    }

    fn regions_output(names: &[&str]) -> DescribeRegionsOutput {
        let regions = names
            .iter()
            .map(|name| Ec2Region::builder().region_name(*name).build())
            .collect::<Vec<_>>();
        DescribeRegionsOutput::builder()
            .set_regions(Some(regions))
            .build()
    }

    #[tokio::test]
    async fn describe_regions_uses_default_parameters_and_sorts() {
        let rule = mock!(aws_sdk_ec2::Client::describe_regions)
            .match_requests(|req| {
                req.all_regions().is_none()
                    && req.filters().is_empty()
                    && req.region_names().is_empty()
            })
            .then_output(|| regions_output(&["us-west-2", "ap-northeast-1", "eu-west-1"]));
        let factory = mock_factory(std::slice::from_ref(&rule));
        let regions = discover_regions(&factory, "us-east-1").await.expect("ok");
        assert_eq!(regions, ["ap-northeast-1", "eu-west-1", "us-west-2"]);
        assert_eq!(rule.num_calls(), 1);
    }

    #[tokio::test]
    async fn describe_regions_failure_suggests_the_region_option() {
        let rule = mock!(aws_sdk_ec2::Client::describe_regions).then_http_response(|| {
            HttpResponse::new(
                StatusCode::try_from(403u16).expect("valid status"),
                aws_sdk_s3::primitives::SdkBody::from(
                    "<Response><Errors><Error><Code>UnauthorizedOperation</Code><Message>not allowed</Message></Error></Errors><RequestID>r</RequestID></Response>",
                ),
            )
        });
        let factory = mock_factory(&[rule]);
        let err = discover_regions(&factory, "us-east-1")
            .await
            .expect_err("fails");
        assert!(matches!(err, SetupError::DescribeRegions { .. }), "{err:?}");
        let message = err.to_string();
        assert!(message.contains("--region"), "{message}");
        assert!(message.contains("UnauthorizedOperation"), "{message}");
    }

    #[test]
    fn plan_without_profile_uses_the_default_chain() {
        let plan = config_plan(None);
        assert_eq!(plan.credentials, CredentialsSource::DefaultChain);
        assert_eq!(plan.profile_name, None);
        assert_eq!(
            plan.region_steps,
            [
                RegionStep::SdkDefaultChain,
                RegionStep::Fallback("us-east-1")
            ]
        );
    }

    #[test]
    fn plan_with_profile_never_uses_the_default_credentials_chain() {
        let plan = config_plan(Some("audit"));
        assert_eq!(
            plan.credentials,
            CredentialsSource::ProfileOnly("audit".to_owned())
        );
        assert_eq!(plan.profile_name.as_deref(), Some("audit"));
        assert_eq!(
            plan.region_steps,
            [
                RegionStep::EnvironmentVariables,
                RegionStep::ProfileRegion("audit".to_owned()),
                RegionStep::Fallback("us-east-1")
            ]
        );
    }

    fn profile_files(contents: &str) -> ProfileFiles {
        ProfileFiles::builder()
            .include_default_config_file(false)
            .include_default_credentials_file(false)
            .with_contents(ProfileFileKind::Config, contents)
            .build()
    }

    #[tokio::test]
    async fn profile_credentials_come_from_the_profile_file() {
        use aws_sdk_sts::config::ProvideCredentials;
        let files = profile_files(
            "[profile audit]\naws_access_key_id = AKIDPROFILE\naws_secret_access_key = SECRETPROFILE\nregion = eu-central-1\n",
        );
        let (config, region) =
            load_sdk_config(Some("audit"), &Overrides { files: Some(files) }).await;
        let provider = config.credentials_provider().expect("a provider is set");
        let credentials = provider
            .provide_credentials()
            .await
            .expect("credentials resolve");
        assert_eq!(credentials.access_key_id(), "AKIDPROFILE");
        // The profile region is used unless AWS_REGION or AWS_DEFAULT_REGION is set.
        let env_set = std::env::var_os("AWS_REGION").is_some()
            || std::env::var_os("AWS_DEFAULT_REGION").is_some();
        if !env_set {
            assert_eq!(region.as_ref(), "eu-central-1");
        }
    }

    #[tokio::test]
    async fn profile_without_region_falls_back_to_us_east_1() {
        let files = profile_files(
            "[profile audit]\naws_access_key_id = AKIDPROFILE\naws_secret_access_key = SECRETPROFILE\n",
        );
        let (_, region) = load_sdk_config(Some("audit"), &Overrides { files: Some(files) }).await;
        let env_set = std::env::var_os("AWS_REGION").is_some()
            || std::env::var_os("AWS_DEFAULT_REGION").is_some();
        if !env_set {
            assert_eq!(region.as_ref(), "us-east-1");
        }
    }
}
