//! Creation of read-only SDK clients.
//!
//! This is the only file that may call `Client::from_conf`. Every client is built from the
//! shared `SdkConfig`, pinned to one region, and gets the read-only guard.
//!
//! To support a new service, add the dependency (in a dedicated change) and add one line
//! to the `impl_guarded_client!` call below.

use aws_config::{Region, SdkConfig};
use aws_smithy_runtime_api::client::interceptors::SharedInterceptor;

use super::guard::ReadOnlyGuard;

/// Builds guarded SDK clients from a shared `SdkConfig`.
///
/// The `SdkConfig` (HTTP client, credential cache) is shared by all clients built here.
/// It is not exposed to collectors.
#[derive(Debug, Clone)]
pub(crate) struct ClientFactory {
    sdk_config: SdkConfig,
    #[cfg(test)]
    extra_interceptors: Vec<SharedInterceptor>,
}

impl ClientFactory {
    /// Creates a factory from a loaded `SdkConfig`.
    pub(crate) fn new(sdk_config: SdkConfig) -> Self {
        Self {
            sdk_config,
            #[cfg(test)]
            extra_interceptors: Vec::new(),
        }
    }

    /// Creates a factory that adds `extra` interceptors (for example mocks) to every client.
    #[cfg(test)]
    pub(crate) fn with_extra_interceptors(
        sdk_config: SdkConfig,
        extra: Vec<SharedInterceptor>,
    ) -> Self {
        Self {
            sdk_config,
            extra_interceptors: extra,
        }
    }

    /// Creates a guarded client of type `C` that sends requests to `region`.
    pub(crate) fn client<C: GuardedClient>(&self, region: &str) -> C {
        C::build(self, Region::new(region.to_owned()))
    }

    fn sdk_config(&self) -> &SdkConfig {
        &self.sdk_config
    }

    #[cfg(test)]
    fn extra_interceptors(&self) -> &[SharedInterceptor] {
        &self.extra_interceptors
    }

    #[cfg(not(test))]
    fn extra_interceptors(&self) -> &[SharedInterceptor] {
        &[]
    }
}

/// An SDK client type that can be created with the read-only guard attached.
pub(crate) trait GuardedClient {
    /// Builds the client for `region` from the factory's `SdkConfig`.
    fn build(factory: &ClientFactory, region: Region) -> Self;
}

macro_rules! impl_guarded_client {
    ($($sdk:ident),+ $(,)?) => {
        $(
            impl GuardedClient for $sdk::Client {
                fn build(factory: &ClientFactory, region: Region) -> Self {
                    let mut builder = $sdk::config::Builder::from(factory.sdk_config())
                        .region(region)
                        .interceptor(ReadOnlyGuard);
                    for interceptor in factory.extra_interceptors() {
                        builder.push_interceptor(interceptor.clone());
                    }
                    $sdk::Client::from_conf(builder.build())
                }
            }
        )+
    };
}

impl_guarded_client!(
    aws_sdk_sts,
    aws_sdk_ec2,
    aws_sdk_iam,
    aws_sdk_s3,
    aws_sdk_s3control,
    aws_sdk_cloudtrail,
    aws_sdk_cloudwatchlogs,
    aws_sdk_cloudwatch,
    aws_sdk_sns,
);

#[cfg(test)]
pub(crate) mod testing {
    //! Test support: factories whose clients answer from mock rules and never touch the network.

    use std::sync::{Arc, Mutex};

    use aws_config::{BehaviorVersion, Region, SdkConfig};
    use aws_sdk_sts::config::retry::RetryConfig;
    use aws_sdk_sts::config::{ConfigBag, Credentials, SharedCredentialsProvider};
    use aws_smithy_mocks::{MockResponseInterceptor, Rule, RuleMode, create_mock_http_client};
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef;
    use aws_smithy_runtime_api::client::interceptors::{Intercept, SharedInterceptor};
    use aws_smithy_runtime_api::client::orchestrator::Metadata;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;

    use super::ClientFactory;

    /// Records the names of the operations that reached the transmit stage.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct Sent(Arc<Mutex<Vec<String>>>);

    impl Sent {
        /// Returns the operation names recorded so far.
        pub(crate) fn operations(&self) -> Vec<String> {
            self.0.lock().expect("lock").clone()
        }
    }

    #[derive(Debug)]
    struct TransmitRecorder(Sent);

    impl Intercept for TransmitRecorder {
        fn name(&self) -> &'static str {
            "TransmitRecorder"
        }

        fn read_before_transmit(
            &self,
            _context: &BeforeTransmitInterceptorContextRef<'_>,
            _runtime_components: &RuntimeComponents,
            cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            if let Some(metadata) = cfg.load::<Metadata>() {
                self.0
                    .0
                    .lock()
                    .expect("lock")
                    .push(metadata.name().to_owned());
            }
            Ok(())
        }
    }

    /// Creates a factory with dummy credentials and a mock HTTP client. Every client it
    /// builds answers from `rules` (matched in any order) and has retries disabled.
    pub(crate) fn mock_factory(rules: &[Rule]) -> ClientFactory {
        mock_factory_counted(rules).0
    }

    /// Like [`mock_factory`], and also returns a record of the operations that were sent.
    pub(crate) fn mock_factory_counted(rules: &[Rule]) -> (ClientFactory, Sent) {
        let sent = Sent::default();
        let mut interceptor = MockResponseInterceptor::new().rule_mode(RuleMode::MatchAny);
        for rule in rules {
            interceptor = interceptor.with_rule(rule);
        }
        let credentials = Credentials::new("AKIDEXAMPLE", "SECRETEXAMPLE", None, None, "test");
        let sdk_config = SdkConfig::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(SharedCredentialsProvider::new(credentials))
            .http_client(create_mock_http_client())
            .retry_config(RetryConfig::disabled())
            .build();
        let factory = ClientFactory::with_extra_interceptors(
            sdk_config,
            vec![
                SharedInterceptor::new(interceptor),
                SharedInterceptor::new(TransmitRecorder(sent.clone())),
            ],
        );
        (factory, sent)
    }
}

#[cfg(test)]
mod tests {
    use aws_smithy_mocks::mock;
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;

    use super::testing::{Sent, mock_factory_counted};
    use crate::support::errors::facts_of;
    use crate::support::guard::ReadOnlyViolation;

    fn empty_ok() -> HttpResponse {
        HttpResponse::new(
            StatusCode::try_from(200u16).expect("valid status"),
            aws_sdk_s3::primitives::SdkBody::from(""),
        )
    }

    /// Asserts that the call failed because the read-only guard refused it, and that the
    /// operation never reached the transmit stage.
    fn assert_blocked<T, E, R>(result: Result<T, aws_sdk_sts::error::SdkError<E, R>>, sent: &Sent)
    where
        E: std::error::Error + aws_sdk_sts::error::ProvideErrorMetadata + 'static,
        R: std::fmt::Debug + 'static,
        T: std::fmt::Debug,
    {
        let err = result.expect_err("the guard must refuse the operation");
        assert!(
            facts_of(&err).guard_blocked,
            "not refused by the guard: {err:?}"
        );
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&err);
        let mut violation = None;
        while let Some(error) = current {
            if let Some(found) = error.downcast_ref::<ReadOnlyViolation>() {
                violation = Some(found.clone());
                break;
            }
            current = error.source();
        }
        let violation = violation.expect("the cause chain holds a ReadOnlyViolation");
        assert!(
            !sent.operations().contains(&violation.operation),
            "{} reached the transmit stage",
            violation.operation
        );
    }

    #[tokio::test]
    async fn sts_guard() {
        let read = mock!(aws_sdk_sts::Client::get_caller_identity).then_output(|| {
            aws_sdk_sts::operation::get_caller_identity::GetCallerIdentityOutput::builder().build()
        });
        let write = mock!(aws_sdk_sts::Client::get_session_token).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_sts::Client>("us-east-1");
        client
            .get_caller_identity()
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_eq!(sent.operations(), ["GetCallerIdentity"]);
        assert_blocked(client.get_session_token().send().await, &sent);
    }

    #[tokio::test]
    async fn ec2_guard() {
        let read = mock!(aws_sdk_ec2::Client::describe_regions).then_output(|| {
            aws_sdk_ec2::operation::describe_regions::DescribeRegionsOutput::builder().build()
        });
        let write = mock!(aws_sdk_ec2::Client::create_tags).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_ec2::Client>("us-east-1");
        client
            .describe_regions()
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client
                .create_tags()
                .resources("i-0123456789abcdef0")
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn iam_guard() {
        let read = mock!(aws_sdk_iam::Client::list_users).then_output(|| {
            aws_sdk_iam::operation::list_users::ListUsersOutput::builder()
                .set_users(Some(Vec::new()))
                .build()
                .expect("valid output")
        });
        let write = mock!(aws_sdk_iam::Client::create_user).then_http_response(empty_ok);
        let report = mock!(aws_sdk_iam::Client::generate_credential_report).then_output(|| {
            aws_sdk_iam::operation::generate_credential_report::GenerateCredentialReportOutput::builder().build()
        });
        let denied = mock!(aws_sdk_iam::Client::generate_service_last_accessed_details)
            .then_http_response(empty_ok);
        let (factory, sent) =
            mock_factory_counted(&[read.clone(), write.clone(), report.clone(), denied.clone()]);
        let client = factory.client::<aws_sdk_iam::Client>("us-east-1");
        client.list_users().send().await.expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client.create_user().user_name("example").send().await,
            &sent,
        );
        client
            .generate_credential_report()
            .send()
            .await
            .expect("the exception list allows GenerateCredentialReport");
        assert_eq!(report.num_calls(), 1);
        assert_blocked(
            client
                .generate_service_last_accessed_details()
                .arn("arn:aws:iam::123456789012:user/example")
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn s3_guard() {
        let read = mock!(aws_sdk_s3::Client::list_buckets).then_output(|| {
            aws_sdk_s3::operation::list_buckets::ListBucketsOutput::builder().build()
        });
        let write = mock!(aws_sdk_s3::Client::put_bucket_policy).then_http_response(empty_ok);
        let object = mock!(aws_sdk_s3::Client::get_object).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone(), object.clone()]);
        let client = factory.client::<aws_sdk_s3::Client>("us-east-1");
        client.list_buckets().send().await.expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client
                .put_bucket_policy()
                .bucket("example-bucket")
                .policy("{}")
                .send()
                .await,
            &sent,
        );
        assert_blocked(
            client
                .get_object()
                .bucket("example-bucket")
                .key("key")
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn s3control_guard() {
        let read = mock!(aws_sdk_s3control::Client::get_public_access_block).then_output(|| {
            aws_sdk_s3control::operation::get_public_access_block::GetPublicAccessBlockOutput::builder().build()
        });
        let write =
            mock!(aws_sdk_s3control::Client::put_public_access_block).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_s3control::Client>("us-east-1");
        client
            .get_public_access_block()
            .account_id("123456789012")
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        let config = aws_sdk_s3control::types::PublicAccessBlockConfiguration::builder().build();
        assert_blocked(
            client
                .put_public_access_block()
                .account_id("123456789012")
                .public_access_block_configuration(config)
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn cloudtrail_guard() {
        let read = mock!(aws_sdk_cloudtrail::Client::describe_trails).then_output(|| {
            aws_sdk_cloudtrail::operation::describe_trails::DescribeTrailsOutput::builder().build()
        });
        let write = mock!(aws_sdk_cloudtrail::Client::stop_logging).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_cloudtrail::Client>("us-east-1");
        client
            .describe_trails()
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client.stop_logging().name("example-trail").send().await,
            &sent,
        );
    }

    #[tokio::test]
    async fn cloudwatchlogs_guard() {
        let read = mock!(aws_sdk_cloudwatchlogs::Client::describe_metric_filters).then_output(|| {
            aws_sdk_cloudwatchlogs::operation::describe_metric_filters::DescribeMetricFiltersOutput::builder().build()
        });
        let write =
            mock!(aws_sdk_cloudwatchlogs::Client::delete_log_group).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_cloudwatchlogs::Client>("us-east-1");
        client
            .describe_metric_filters()
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client
                .delete_log_group()
                .log_group_name("example")
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn cloudwatch_guard() {
        let read = mock!(aws_sdk_cloudwatch::Client::describe_alarms).then_output(|| {
            aws_sdk_cloudwatch::operation::describe_alarms::DescribeAlarmsOutput::builder().build()
        });
        let write =
            mock!(aws_sdk_cloudwatch::Client::put_metric_alarm).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_cloudwatch::Client>("us-east-1");
        client
            .describe_alarms()
            .send()
            .await
            .expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(
            client
                .put_metric_alarm()
                .alarm_name("example")
                .evaluation_periods(1)
                .comparison_operator(
                    aws_sdk_cloudwatch::types::ComparisonOperator::GreaterThanThreshold,
                )
                .send()
                .await,
            &sent,
        );
    }

    #[tokio::test]
    async fn sns_guard() {
        let read = mock!(aws_sdk_sns::Client::list_topics).then_output(|| {
            aws_sdk_sns::operation::list_topics::ListTopicsOutput::builder().build()
        });
        let write = mock!(aws_sdk_sns::Client::publish).then_http_response(empty_ok);
        let (factory, sent) = mock_factory_counted(&[read.clone(), write.clone()]);
        let client = factory.client::<aws_sdk_sns::Client>("us-east-1");
        client.list_topics().send().await.expect("read is allowed");
        assert_eq!(read.num_calls(), 1);
        assert_blocked(client.publish().message("hello").send().await, &sent);
    }

    #[tokio::test]
    async fn clients_use_the_requested_region() {
        let read = mock!(aws_sdk_sts::Client::get_caller_identity).then_output(|| {
            aws_sdk_sts::operation::get_caller_identity::GetCallerIdentityOutput::builder().build()
        });
        let (factory, _sent) = mock_factory_counted(&[read]);
        let client = factory.client::<aws_sdk_sts::Client>("eu-west-1");
        let region = client.config().region().map(|r| r.to_string());
        assert_eq!(region.as_deref(), Some("eu-west-1"));
    }
}
