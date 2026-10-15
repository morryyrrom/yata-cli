//! Collector for `aws.s3.bucket`: S3 buckets of all regions with their bucket policy, bucket-level
//! Block Public Access settings and server access logging (`ListBuckets`, `GetBucketPolicy`,
//! `GetPublicAccessBlock`, `GetBucketLogging`).

use std::collections::BTreeMap;

use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::types::{Bucket, LoggingEnabled, Owner};
use serde_json::{Map, Value, json};
use yata_core::{CollectionErrorKind, Resource, Scope};

use crate::collector::{CollectFuture, Collector};
use crate::context::CollectContext;
use crate::support::convert::{datetime_to_rfc3339, opt_string};
use crate::support::errors::{CollectError, Failure, facts_of, failure_mark};
use crate::support::fan_out::fan_out;
use crate::support::partition::SUPPORTED_PARTITION;

const RESOURCE_TYPE: &str = "aws.s3.bucket";
const LIST_BUCKETS: &str = "ListBuckets";
const GET_POLICY: &str = "GetBucketPolicy";
const GET_PUBLIC_ACCESS_BLOCK: &str = "GetPublicAccessBlock";
const GET_LOGGING: &str = "GetBucketLogging";
const NO_SUCH_BUCKET_POLICY: &str = "NoSuchBucketPolicy";
const NO_SUCH_PUBLIC_ACCESS_BLOCK: &str = "NoSuchPublicAccessBlockConfiguration";
/// The page size of `ListBuckets` (the maximum the API accepts).
const MAX_BUCKETS: i32 = 10_000;

/// Returns the collector of `aws.s3.bucket`.
pub(crate) fn collector() -> Box<dyn Collector> {
    Box::new(BucketCollector)
}

struct BucketCollector;

impl Collector for BucketCollector {
    fn resource_type(&self) -> &'static str {
        RESOURCE_TYPE
    }

    fn scope(&self) -> Scope {
        Scope::Global
    }

    fn required_permissions(&self) -> &'static [&'static str] {
        &[
            "s3:ListAllMyBuckets",
            "s3:GetBucketPolicy",
            "s3:GetBucketPublicAccessBlock",
            "s3:GetBucketLogging",
        ]
    }

    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a> {
        Box::pin(collect(ctx, region))
    }
}

/// One listed bucket: its name, its region and the `ListBuckets` value of its resource.
struct BucketEntry {
    name: String,
    region: String,
    value: Value,
}

/// The values of the three per-bucket keys: policy, public access block, logging.
type BucketDetails = (Value, Value, Value);

async fn collect(ctx: &CollectContext, region: &str) -> Result<Vec<Resource>, CollectError> {
    let client: aws_sdk_s3::Client = ctx.client(region);

    // Step 1: all pages of ListBuckets. Any failure fails the whole group.
    let entries = list_buckets(&client).await?;

    // Step 2: one client per bucket region.
    let clients = clients_by_region(ctx, entries.iter().map(|entry| entry.region.as_str()));
    tracing::debug!(
        buckets = entries.len(),
        regions = clients.len(),
        "listed S3 buckets"
    );
    let jobs: Vec<(String, aws_sdk_s3::Client)> = entries
        .iter()
        .map(|entry| {
            let bucket_client = clients
                .get(&entry.region)
                .cloned()
                .unwrap_or_else(|| ctx.client(&entry.region));
            (entry.name.clone(), bucket_client)
        })
        .collect();

    // Step 3: the per-bucket calls, concurrently with a bounded number of tasks.
    let details: Vec<BucketDetails> = fan_out(
        ctx.fan_out_limit(),
        jobs,
        |(name, client): (String, aws_sdk_s3::Client)| async move {
            let policy = get_bucket_policy(&client, &name).await;
            let public_access_block = get_public_access_block(&client, &name).await;
            let logging = get_bucket_logging(&client, &name).await;
            (policy, public_access_block, logging)
        },
    )
    .await?;
    warn_on_failure_marks(GET_POLICY, details.iter().map(|d| &d.0));
    warn_on_failure_marks(GET_PUBLIC_ACCESS_BLOCK, details.iter().map(|d| &d.1));
    warn_on_failure_marks(GET_LOGGING, details.iter().map(|d| &d.2));

    // Step 4: assemble the resources.
    let resources = entries
        .into_iter()
        .zip(details)
        .map(|(entry, (policy, public_access_block, logging))| {
            let mut configuration = Map::new();
            configuration.insert(LIST_BUCKETS.to_owned(), entry.value);
            configuration.insert(GET_POLICY.to_owned(), policy);
            configuration.insert(GET_PUBLIC_ACCESS_BLOCK.to_owned(), public_access_block);
            configuration.insert(GET_LOGGING.to_owned(), logging);
            Resource {
                id: bucket_arn(SUPPORTED_PARTITION, &entry.name),
                resource_type: RESOURCE_TYPE.to_owned(),
                account: ctx.account_key().to_owned(),
                region: entry.region,
                name: Some(entry.name),
                tags: BTreeMap::new(),
                configuration,
            }
        })
        .collect();
    Ok(resources)
}

/// Builds the ARN of a bucket from its format: `arn:<partition>:s3:::<name>`.
fn bucket_arn(partition: &str, name: &str) -> String {
    format!("arn:{partition}:s3:::{name}")
}

/// Creates one client for each distinct region in `regions`.
fn clients_by_region<'a>(
    ctx: &CollectContext,
    regions: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, aws_sdk_s3::Client> {
    let mut clients = BTreeMap::new();
    for region in regions {
        if !clients.contains_key(region) {
            clients.insert(region.to_owned(), ctx.client::<aws_sdk_s3::Client>(region));
        }
    }
    clients
}

/// Fetches all pages of `ListBuckets` in the order the API returned them.
///
/// The value of each bucket keeps the shape of the operation output: an object holding the
/// array `Buckets` with the element of that bucket, and the `Owner` and `Prefix` of the page
/// that returned it (the paging field `ContinuationToken` is not included).
async fn list_buckets(client: &aws_sdk_s3::Client) -> Result<Vec<BucketEntry>, CollectError> {
    let mut entries = Vec::new();
    let mut pages = client
        .list_buckets()
        .into_paginator()
        .page_size(MAX_BUCKETS)
        .send();
    while let Some(page) = pages.next().await {
        let page = page.map_err(|err| CollectError::from_sdk(LIST_BUCKETS, &err))?;
        let owner = owner_value(page.owner());
        let prefix = opt_string(page.prefix());
        for bucket in page.buckets.as_deref().unwrap_or_default() {
            let (name, region, element) = bucket_element(bucket)?;
            let value = json!({
                "Buckets": [element],
                "Owner": owner,
                "Prefix": prefix,
            });
            entries.push(BucketEntry {
                name,
                region,
                value,
            });
        }
    }
    Ok(entries)
}

/// Converts the `Owner` of a page to JSON (`null` when the page has none).
fn owner_value(owner: Option<&Owner>) -> Value {
    match owner {
        Some(owner) => json!({
            "DisplayName": opt_string(owner.display_name()),
            "ID": opt_string(owner.id()),
        }),
        None => Value::Null,
    }
}

/// Converts an element of `ListBuckets` to JSON (all four members) and returns its name and
/// region with it.
fn bucket_element(bucket: &Bucket) -> Result<(String, String, Value), CollectError> {
    let name = bucket
        .name()
        .ok_or_else(|| invalid_response("MissingName"))?;
    let region = bucket.bucket_region().ok_or_else(|| {
        tracing::debug!(bucket = name, "ListBuckets element has no BucketRegion");
        invalid_response("MissingBucketRegion")
    })?;
    if !crate::is_valid_region_name(region) {
        tracing::debug!(bucket = name, region, "ListBuckets element has an invalid BucketRegion");
        return Err(invalid_response("InvalidBucketRegion"));
    }
    let creation_date = match bucket.creation_date() {
        Some(date) => Value::String(datetime_to_rfc3339(date).ok_or_else(|| {
            tracing::debug!(bucket = name, "ListBuckets element has an unformattable CreationDate");
            invalid_response("InvalidDateTime")
        })?),
        None => Value::Null,
    };
    let expected_arn = bucket_arn(SUPPORTED_PARTITION, name);
    if let Some(returned) = bucket.bucket_arn()
        && returned != expected_arn
    {
        tracing::debug!(bucket = name, "BucketArn differs from the built ARN");
    }
    let element = json!({
        "BucketArn": opt_string(bucket.bucket_arn()),
        "BucketRegion": region,
        "CreationDate": creation_date,
        "Name": name,
    });
    Ok((name.to_owned(), region.to_owned(), element))
}

/// The error for an element of `ListBuckets` that cannot be converted.
fn invalid_response(reason: &str) -> CollectError {
    CollectError::new(
        CollectionErrorKind::InvalidResponse,
        format!("invalid response when calling {LIST_BUCKETS} ({reason})"),
    )
}

/// Returns true if `err` is a service error with exactly `code`.
fn is_service_error_with_code<E, R>(err: &SdkError<E, R>, code: &str) -> bool
where
    E: std::error::Error + ProvideErrorMetadata + 'static,
    R: std::fmt::Debug + 'static,
{
    let facts = facts_of(err);
    facts.failure == Failure::Service && facts.code.as_deref() == Some(code)
}

/// Calls `GetBucketPolicy`. `NoSuchBucketPolicy` gives `null`; other failures give a mark.
async fn get_bucket_policy(client: &aws_sdk_s3::Client, bucket: &str) -> Value {
    match client.get_bucket_policy().bucket(bucket).send().await {
        Ok(output) => json!({ "Policy": output.policy().map_or(Value::Null, parse_policy) }),
        Err(err) => {
            if is_service_error_with_code(&err, NO_SUCH_BUCKET_POLICY) {
                Value::Null
            } else {
                tracing::debug!(bucket, "{GET_POLICY} failed");
                failure_mark(GET_POLICY, &err)
            }
        }
    }
}

/// Parses the bucket policy as JSON. A body that is not JSON is kept as the returned string.
fn parse_policy(policy: &str) -> Value {
    match serde_json::from_str::<Value>(policy) {
        Ok(value) => value,
        Err(err) => {
            tracing::debug!(error = %err, "the bucket policy is not valid JSON");
            Value::String(policy.to_owned())
        }
    }
}

/// Calls `GetPublicAccessBlock` of a bucket. `NoSuchPublicAccessBlockConfiguration` gives `null`.
async fn get_public_access_block(client: &aws_sdk_s3::Client, bucket: &str) -> Value {
    match client.get_public_access_block().bucket(bucket).send().await {
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
            if is_service_error_with_code(&err, NO_SUCH_PUBLIC_ACCESS_BLOCK) {
                Value::Null
            } else {
                tracing::debug!(bucket, "{GET_PUBLIC_ACCESS_BLOCK} failed");
                failure_mark(GET_PUBLIC_ACCESS_BLOCK, &err)
            }
        }
    }
}

/// Calls `GetBucketLogging`. A bucket without logging gives `{"LoggingEnabled": null}`.
async fn get_bucket_logging(client: &aws_sdk_s3::Client, bucket: &str) -> Value {
    match client.get_bucket_logging().bucket(bucket).send().await {
        Ok(output) => {
            let logging = output.logging_enabled().map_or(Value::Null, logging_value);
            json!({ "LoggingEnabled": logging })
        }
        Err(err) => {
            tracing::debug!(bucket, "{GET_LOGGING} failed");
            failure_mark(GET_LOGGING, &err)
        }
    }
}

/// Converts `LoggingEnabled` to JSON (all four members).
///
/// `TargetBucket`, `TargetPrefix` and the grantee `Type` are never `null`: when the API does
/// not return them the SDK fills in `""` and `"no value was set"`, and they are kept as is.
fn logging_value(logging: &LoggingEnabled) -> Value {
    let grants = match logging.target_grants.as_deref() {
        Some(grants) => Value::Array(
            grants
                .iter()
                .map(|grant| {
                    let grantee = match grant.grantee() {
                        Some(grantee) => json!({
                            "DisplayName": opt_string(grantee.display_name()),
                            "EmailAddress": opt_string(grantee.email_address()),
                            "ID": opt_string(grantee.id()),
                            "Type": grantee.r#type().as_str(),
                            "URI": opt_string(grantee.uri()),
                        }),
                        None => Value::Null,
                    };
                    json!({
                        "Grantee": grantee,
                        "Permission": opt_string(grant.permission().map(|p| p.as_str())),
                    })
                })
                .collect(),
        ),
        None => Value::Null,
    };
    let key_format = match logging.target_object_key_format() {
        Some(format) => json!({
            "PartitionedPrefix": match format.partitioned_prefix() {
                Some(prefix) => json!({
                    "PartitionDateSource": opt_string(
                        prefix.partition_date_source().map(|s| s.as_str())
                    ),
                }),
                None => Value::Null,
            },
            "SimplePrefix": format.simple_prefix().map_or(Value::Null, |_| json!({})),
        }),
        None => Value::Null,
    };
    json!({
        "TargetBucket": logging.target_bucket(),
        "TargetGrants": grants,
        "TargetObjectKeyFormat": key_format,
        "TargetPrefix": logging.target_prefix(),
    })
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
    use aws_sdk_s3::error::ErrorMetadata;
    use aws_sdk_s3::operation::get_bucket_logging::{
        GetBucketLoggingError, GetBucketLoggingInput, GetBucketLoggingOutput,
    };
    use aws_sdk_s3::operation::get_bucket_policy::{GetBucketPolicyError, GetBucketPolicyInput};
    use aws_sdk_s3::operation::get_public_access_block::{
        GetPublicAccessBlockError, GetPublicAccessBlockInput, GetPublicAccessBlockOutput,
    };
    use aws_sdk_s3::operation::list_buckets::{
        ListBucketsError, ListBucketsInput, ListBucketsOutput,
    };
    use aws_sdk_s3::primitives::{DateTime, SdkBody};
    use aws_sdk_s3::types::{
        BucketLogsPermission, Grantee, PartitionDateSource, PartitionedPrefix,
        PublicAccessBlockConfiguration, SimplePrefix, TargetGrant, TargetObjectKeyFormat, Type,
    };
    use aws_smithy_mocks::{Rule, mock};
    use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
    use aws_smithy_runtime_api::http::StatusCode;
    use std::num::NonZeroUsize;

    const OWNER_ID_1: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OWNER_ID_2: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

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
            .message("failure for arn:aws:s3:::example-bucket")
            .build()
    }

    fn bucket(name: &str, region: &str) -> Bucket {
        Bucket::builder()
            .name(name)
            .bucket_region(region)
            .creation_date(DateTime::from_secs(1_767_225_600))
            .build()
    }

    fn owner(id: &str, display_name: Option<&str>) -> Owner {
        Owner::builder()
            .id(id)
            .set_display_name(display_name.map(str::to_owned))
            .build()
    }

    /// One `ListBuckets` page of the mock.
    struct Page {
        buckets: Option<Vec<Bucket>>,
        owner: Option<Owner>,
        prefix: Option<String>,
    }

    fn page(buckets: Vec<Bucket>, owner: Option<Owner>) -> Page {
        Page {
            buckets: Some(buckets),
            owner,
            prefix: None,
        }
    }

    fn token_of(index: usize) -> Option<String> {
        (index > 0).then(|| format!("token-{index}"))
    }

    /// Rules for `ListBuckets` that return `pages` in order. Each rule also checks the
    /// default parameters.
    fn list_rules(pages: Vec<Page>) -> Vec<Rule> {
        let count = pages.len();
        pages
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                let expected = token_of(index);
                let buckets = page.buckets;
                let owner = page.owner;
                let prefix = page.prefix;
                mock!(aws_sdk_s3::Client::list_buckets)
                    .match_requests(move |input: &ListBucketsInput| {
                        input.continuation_token() == expected.as_deref()
                            && input.max_buckets() == Some(10_000)
                            && input.prefix().is_none()
                            && input.bucket_region().is_none()
                    })
                    .then_output(move || {
                        let last = index + 1 == count;
                        ListBucketsOutput::builder()
                            .set_buckets(buckets.clone())
                            .set_owner(owner.clone())
                            .set_prefix(prefix.clone())
                            .set_continuation_token((!last).then(|| format!("token-{}", index + 1)))
                            .build()
                    })
            })
            .collect()
    }

    fn policy_error_rule(name: &str, code: &'static str) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_bucket_policy)
            .match_requests(move |input: &GetBucketPolicyInput| {
                input.bucket() == Some(name.as_str()) && input.expected_bucket_owner().is_none()
            })
            .then_error(move || GetBucketPolicyError::generic(metadata(code)))
    }

    fn policy_body_rule(name: &str, body: &'static str) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_bucket_policy)
            .match_requests(move |input: &GetBucketPolicyInput| {
                input.bucket() == Some(name.as_str()) && input.expected_bucket_owner().is_none()
            })
            .then_http_response(move || {
                HttpResponse::new(StatusCode::try_from(200u16).unwrap(), SdkBody::from(body))
            })
    }

    fn pab_error_rule(name: &str, code: &'static str) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_public_access_block)
            .match_requests(move |input: &GetPublicAccessBlockInput| {
                input.bucket() == Some(name.as_str()) && input.expected_bucket_owner().is_none()
            })
            .then_error(move || GetPublicAccessBlockError::generic(metadata(code)))
    }

    fn pab_output_rule(name: &str, config: Option<PublicAccessBlockConfiguration>) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_public_access_block)
            .match_requests(move |input: &GetPublicAccessBlockInput| {
                input.bucket() == Some(name.as_str()) && input.expected_bucket_owner().is_none()
            })
            .then_output(move || {
                GetPublicAccessBlockOutput::builder()
                    .set_public_access_block_configuration(config.clone())
                    .build()
            })
    }

    fn logging_rule(name: &str, logging: Option<LoggingEnabled>) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_bucket_logging)
            .match_requests(move |input: &GetBucketLoggingInput| {
                input.bucket() == Some(name.as_str()) && input.expected_bucket_owner().is_none()
            })
            .then_output(move || {
                GetBucketLoggingOutput::builder()
                    .set_logging_enabled(logging.clone())
                    .build()
            })
    }

    fn logging_error_rule(name: &str, code: &'static str) -> Rule {
        let name = name.to_owned();
        mock!(aws_sdk_s3::Client::get_bucket_logging)
            .match_requests(move |input: &GetBucketLoggingInput| {
                input.bucket() == Some(name.as_str())
            })
            .then_error(move || GetBucketLoggingError::generic(metadata(code)))
    }

    /// The three per-bucket rules of a bucket with nothing configured.
    fn unconfigured_rules(name: &str) -> Vec<Rule> {
        vec![
            policy_error_rule(name, NO_SUCH_BUCKET_POLICY),
            pab_error_rule(name, NO_SUCH_PUBLIC_ACCESS_BLOCK),
            logging_rule(name, None),
        ]
    }

    fn all_four(value: bool) -> PublicAccessBlockConfiguration {
        PublicAccessBlockConfiguration::builder()
            .block_public_acls(value)
            .block_public_policy(value)
            .ignore_public_acls(value)
            .restrict_public_buckets(value)
            .build()
    }

    fn full_logging() -> LoggingEnabled {
        LoggingEnabled::builder()
            .target_bucket("example-logs-bucket")
            .target_prefix("example-bucket/")
            .target_grants(
                TargetGrant::builder()
                    .grantee(
                        Grantee::builder()
                            .r#type(Type::CanonicalUser)
                            .id(OWNER_ID_1)
                            .display_name("example-user")
                            .build()
                            .unwrap(),
                    )
                    .permission(BucketLogsPermission::FullControl)
                    .build(),
            )
            .target_object_key_format(
                TargetObjectKeyFormat::builder()
                    .partitioned_prefix(
                        PartitionedPrefix::builder()
                            .partition_date_source(PartitionDateSource::EventTime)
                            .build(),
                    )
                    .simple_prefix(SimplePrefix::builder().build())
                    .build(),
            )
            .build()
            .unwrap()
    }

    const POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Sid":"DenyInsecureTransport","Effect":"Deny","Principal":"*","Action":"s3:*","Resource":["arn:aws:s3:::alpha-bucket","arn:aws:s3:::alpha-bucket/*"],"Condition":{"Bool":{"aws:SecureTransport":"false"}}}]}"#;

    /// Four buckets in three regions on two pages.
    fn standard_rules() -> Vec<Rule> {
        let mut rules = list_rules(vec![
            page(
                vec![
                    bucket("alpha-bucket", "us-east-1"),
                    bucket("beta-bucket", "eu-west-1"),
                ],
                Some(owner(OWNER_ID_1, Some("example-owner"))),
            ),
            Page {
                buckets: Some(vec![
                    bucket("gamma-bucket", "ap-northeast-1"),
                    bucket("delta-bucket", "eu-west-1"),
                ]),
                owner: Some(owner(OWNER_ID_2, None)),
                prefix: Some("example-".to_owned()),
            },
        ]);
        rules.push(policy_body_rule("alpha-bucket", POLICY));
        rules.push(pab_output_rule("alpha-bucket", Some(all_four(true))));
        rules.push(logging_rule("alpha-bucket", Some(full_logging())));
        rules.extend(unconfigured_rules("beta-bucket"));
        rules.push(policy_error_rule("gamma-bucket", NO_SUCH_BUCKET_POLICY));
        rules.push(pab_output_rule(
            "gamma-bucket",
            Some(
                PublicAccessBlockConfiguration::builder()
                    .block_public_acls(true)
                    .build(),
            ),
        ));
        rules.push(logging_rule("gamma-bucket", None));
        rules.extend(unconfigured_rules("delta-bucket"));
        rules
    }

    fn sorted(mut resources: Vec<Resource>) -> Vec<Resource> {
        resources.sort_by(|a, b| a.id.cmp(&b.id));
        resources
    }

    fn by_name<'a>(resources: &'a [Resource], name: &str) -> &'a Resource {
        resources
            .iter()
            .find(|r| r.name.as_deref() == Some(name))
            .unwrap()
    }

    #[tokio::test]
    async fn golden_and_resource_fields() {
        let rules = standard_rules();
        let (factory, sent) = mock_factory_counted(&rules);
        let ctx = context_with(factory);
        let resources = sorted(collect(&ctx, "us-east-1").await.unwrap());

        // Every listed bucket becomes a resource, across pages and regions.
        assert_eq!(resources.len(), 4);
        let ids: Vec<&str> = resources.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "arn:aws:s3:::alpha-bucket",
                "arn:aws:s3:::beta-bucket",
                "arn:aws:s3:::delta-bucket",
                "arn:aws:s3:::gamma-bucket",
            ]
        );
        for resource in &resources {
            assert_eq!(resource.resource_type, "aws.s3.bucket");
            assert_eq!(resource.account, "aws-123456789012");
            assert!(resource.tags.is_empty());
            let element = &resource.configuration["ListBuckets"]["Buckets"][0];
            assert_eq!(resource.name.as_deref(), element["Name"].as_str());
            assert_eq!(Some(resource.region.as_str()), element["BucketRegion"].as_str());
        }
        let regions: Vec<&str> = resources.iter().map(|r| r.region.as_str()).collect();
        assert_eq!(
            regions,
            ["us-east-1", "eu-west-1", "eu-west-1", "ap-northeast-1"]
        );
        // The operations all reached the transmit stage.
        let operations = sent.operations();
        for name in [
            "ListBuckets",
            "GetBucketPolicy",
            "GetPublicAccessBlock",
            "GetBucketLogging",
        ] {
            assert!(operations.iter().any(|o| o == name), "{name} was not sent");
        }
        assert_golden(
            &golden_path("s3", "bucket"),
            &serde_json::to_value(&resources).unwrap(),
        );
    }

    #[tokio::test]
    async fn request_parameters_are_the_defaults() {
        // The matchers of the rules check MaxBuckets, Prefix, BucketRegion, the continuation
        // token and ExpectedBucketOwner; a request that does not match fails the collection.
        let rules = standard_rules();
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 4);
        for rule in &rules {
            assert_eq!(rule.num_calls(), 1);
        }
    }

    #[tokio::test]
    async fn the_listing_value_has_only_the_three_keys() {
        let rules = standard_rules();
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        for resource in &resources {
            let value = resource.configuration["ListBuckets"].as_object().unwrap();
            let mut keys: Vec<&str> = value.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, ["Buckets", "Owner", "Prefix"]);
            assert_eq!(value["Buckets"].as_array().unwrap().len(), 1);
            let element = value["Buckets"][0].as_object().unwrap();
            assert_eq!(element.len(), 4);
            assert_eq!(element["BucketArn"], Value::Null);
        }
    }

    #[tokio::test]
    async fn owner_and_prefix_are_those_of_the_page_of_the_bucket() {
        let rules = standard_rules();
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let alpha = &by_name(&resources, "alpha-bucket").configuration["ListBuckets"];
        assert_eq!(alpha["Owner"]["ID"], json!(OWNER_ID_1));
        assert_eq!(alpha["Owner"]["DisplayName"], json!("example-owner"));
        assert_eq!(alpha["Prefix"], Value::Null);
        let gamma = &by_name(&resources, "gamma-bucket").configuration["ListBuckets"];
        assert_eq!(gamma["Owner"]["ID"], json!(OWNER_ID_2));
        assert_eq!(gamma["Owner"]["DisplayName"], Value::Null);
        assert_eq!(gamma["Prefix"], json!("example-"));
    }

    #[tokio::test]
    async fn a_page_without_an_owner_gives_a_null_owner() {
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.extend(unconfigured_rules("alpha-bucket"));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources[0].configuration["ListBuckets"]["Owner"], Value::Null);
    }

    #[tokio::test]
    async fn a_bucket_arn_that_is_returned_is_kept_but_not_used_as_the_id() {
        let listed = Bucket::builder()
            .name("alpha-bucket")
            .bucket_region("us-east-1")
            .bucket_arn("arn:aws:s3express:us-east-1:123456789012:bucket/other")
            .build();
        let mut rules = list_rules(vec![page(vec![listed], None)]);
        rules.extend(unconfigured_rules("alpha-bucket"));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources[0].id, "arn:aws:s3:::alpha-bucket");
        let element = &resources[0].configuration["ListBuckets"]["Buckets"][0];
        assert_eq!(
            element["BucketArn"],
            json!("arn:aws:s3express:us-east-1:123456789012:bucket/other")
        );
        assert_eq!(element["CreationDate"], Value::Null);
    }

    #[tokio::test]
    async fn the_policy_json_is_parsed() {
        let resources = collect(&context(&standard_rules()), "us-east-1")
            .await
            .unwrap();
        let policy = &by_name(&resources, "alpha-bucket").configuration["GetBucketPolicy"];
        assert_eq!(
            policy["Policy"],
            serde_json::from_str::<Value>(POLICY).unwrap()
        );
        assert_eq!(
            policy["Policy"]["Statement"][0]["Condition"]["Bool"]["aws:SecureTransport"],
            json!("false")
        );
    }

    #[tokio::test]
    async fn a_policy_that_is_not_json_is_kept_as_the_returned_string() {
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.push(policy_body_rule("alpha-bucket", "not json {"));
        rules.push(pab_error_rule("alpha-bucket", NO_SUCH_PUBLIC_ACCESS_BLOCK));
        rules.push(logging_rule("alpha-bucket", None));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(
            resources[0].configuration["GetBucketPolicy"],
            json!({ "Policy": "not json {" })
        );
    }

    #[tokio::test]
    async fn no_such_bucket_policy_gives_a_null_value() {
        // A generic error with the code.
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.extend(unconfigured_rules("alpha-bucket"));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources[0].configuration["GetBucketPolicy"], Value::Null);

        // The S3 XML error response.
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.push(
            mock!(aws_sdk_s3::Client::get_bucket_policy).then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(404u16).unwrap(),
                    SdkBody::from(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                         <Error><Code>NoSuchBucketPolicy</Code>\
                         <Message>The bucket policy does not exist</Message>\
                         <BucketName>alpha-bucket</BucketName>\
                         <RequestId>example</RequestId></Error>",
                    ),
                )
            }),
        );
        rules.push(pab_error_rule("alpha-bucket", NO_SUCH_PUBLIC_ACCESS_BLOCK));
        rules.push(logging_rule("alpha-bucket", None));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources[0].configuration["GetBucketPolicy"], Value::Null);
    }

    #[tokio::test]
    async fn a_success_without_a_policy_is_distinct_from_null() {
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.push(
            mock!(aws_sdk_s3::Client::get_bucket_policy)
                .then_output(|| aws_sdk_s3::operation::get_bucket_policy::GetBucketPolicyOutput::builder().build()),
        );
        rules.push(pab_output_rule("alpha-bucket", None));
        rules.push(logging_rule("alpha-bucket", None));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let configuration = &resources[0].configuration;
        // A success without a policy is not the same as NoSuchBucketPolicy.
        assert_eq!(configuration["GetBucketPolicy"], json!({ "Policy": null }));
        assert_eq!(
            configuration["GetPublicAccessBlock"],
            json!({ "PublicAccessBlockConfiguration": null })
        );
    }

    #[tokio::test]
    async fn public_access_block_members_and_the_missing_configuration() {
        let resources = collect(&context(&standard_rules()), "us-east-1")
            .await
            .unwrap();
        let alpha = &by_name(&resources, "alpha-bucket").configuration["GetPublicAccessBlock"];
        assert_eq!(
            alpha,
            &json!({
                "PublicAccessBlockConfiguration": {
                    "BlockPublicAcls": true,
                    "BlockPublicPolicy": true,
                    "IgnorePublicAcls": true,
                    "RestrictPublicBuckets": true,
                }
            })
        );
        let gamma = &by_name(&resources, "gamma-bucket").configuration["GetPublicAccessBlock"];
        assert_eq!(
            gamma["PublicAccessBlockConfiguration"],
            json!({
                "BlockPublicAcls": true,
                "BlockPublicPolicy": null,
                "IgnorePublicAcls": null,
                "RestrictPublicBuckets": null,
            })
        );
        let beta = &by_name(&resources, "beta-bucket").configuration["GetPublicAccessBlock"];
        assert_eq!(beta, &Value::Null);
    }

    #[tokio::test]
    async fn logging_members_are_converted() {
        let resources = collect(&context(&standard_rules()), "us-east-1")
            .await
            .unwrap();
        let alpha = &by_name(&resources, "alpha-bucket").configuration["GetBucketLogging"];
        assert_eq!(
            alpha,
            &json!({
                "LoggingEnabled": {
                    "TargetBucket": "example-logs-bucket",
                    "TargetGrants": [{
                        "Grantee": {
                            "DisplayName": "example-user",
                            "EmailAddress": null,
                            "ID": OWNER_ID_1,
                            "Type": "CanonicalUser",
                            "URI": null,
                        },
                        "Permission": "FULL_CONTROL",
                    }],
                    "TargetObjectKeyFormat": {
                        "PartitionedPrefix": { "PartitionDateSource": "EventTime" },
                        "SimplePrefix": {},
                    },
                    "TargetPrefix": "example-bucket/",
                }
            })
        );
        let beta = &by_name(&resources, "beta-bucket").configuration["GetBucketLogging"];
        assert_eq!(beta, &json!({ "LoggingEnabled": null }));
    }

    #[tokio::test]
    async fn values_the_sdk_fills_in_are_kept_as_they_are() {
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.push(policy_error_rule("alpha-bucket", NO_SUCH_BUCKET_POLICY));
        rules.push(pab_error_rule("alpha-bucket", NO_SUCH_PUBLIC_ACCESS_BLOCK));
        rules.push(
            mock!(aws_sdk_s3::Client::get_bucket_logging).then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(200u16).unwrap(),
                    SdkBody::from(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                         <BucketLoggingStatus xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                         <LoggingEnabled><TargetGrants><Grant><Grantee>\
                         <ID>0123456789abcdef</ID></Grantee><Permission>READ</Permission>\
                         </Grant></TargetGrants></LoggingEnabled></BucketLoggingStatus>",
                    ),
                )
            }),
        );
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let logging = &resources[0].configuration["GetBucketLogging"]["LoggingEnabled"];
        assert_eq!(logging["TargetBucket"], json!(""));
        assert_eq!(logging["TargetPrefix"], json!(""));
        assert_eq!(logging["TargetGrants"][0]["Grantee"]["Type"], json!("no value was set"));
        assert_eq!(logging["TargetGrants"][0]["Grantee"]["ID"], json!("0123456789abcdef"));
        assert_eq!(logging["TargetGrants"][0]["Permission"], json!("READ"));
        assert_eq!(logging["TargetObjectKeyFormat"], Value::Null);
    }

    #[tokio::test]
    async fn a_code_of_another_operation_does_not_give_null() {
        let mut rules = list_rules(vec![page(vec![bucket("alpha-bucket", "us-east-1")], None)]);
        rules.push(policy_error_rule("alpha-bucket", NO_SUCH_PUBLIC_ACCESS_BLOCK));
        rules.push(pab_error_rule("alpha-bucket", NO_SUCH_BUCKET_POLICY));
        rules.push(logging_rule("alpha-bucket", None));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        let configuration = &resources[0].configuration;
        assert_eq!(configuration["GetBucketPolicy"]["yata_error"]["kind"], json!("service"));
        assert_eq!(
            configuration["GetPublicAccessBlock"]["yata_error"]["kind"],
            json!("service")
        );
        assert_eq!(configuration["GetBucketLogging"], json!({ "LoggingEnabled": null }));
    }

    #[tokio::test]
    async fn a_failed_call_marks_only_that_bucket_and_key() {
        let cases: [(&str, &str, &str, usize); 4] = [
            ("AccessDenied", "access_denied", GET_POLICY, 0),
            ("SlowDown", "throttled", GET_PUBLIC_ACCESS_BLOCK, 1),
            ("NoSuchBucket", "service", GET_LOGGING, 2),
            ("PermanentRedirect", "service", GET_POLICY, 0),
        ];
        for (code, kind, operation, position) in cases {
            let mut rules = list_rules(vec![page(
                vec![bucket("alpha-bucket", "us-east-1"), bucket("beta-bucket", "eu-west-1")],
                None,
            )]);
            let mut alpha = unconfigured_rules("alpha-bucket");
            alpha[position] = match position {
                0 => policy_error_rule("alpha-bucket", code),
                1 => pab_error_rule("alpha-bucket", code),
                _ => logging_error_rule("alpha-bucket", code),
            };
            rules.extend(alpha);
            rules.extend(unconfigured_rules("beta-bucket"));
            let resources = sorted(collect(&context(&rules), "us-east-1").await.unwrap());
            let alpha = &resources[0].configuration;
            let mark = &alpha[operation];
            assert_eq!(mark["yata_error"]["kind"], json!(kind), "{code}");
            let message = mark["yata_error"]["message"].as_str().unwrap();
            assert!(message.contains(operation) && message.contains(code), "{code}");
            assert!(!message.contains("arn:aws"), "{code}");
            assert_eq!(mark.as_object().unwrap().len(), 1);
            // The other two keys of the same bucket hold values.
            for other in [GET_POLICY, GET_PUBLIC_ACCESS_BLOCK, GET_LOGGING] {
                if other != operation {
                    assert!(alpha[other].get("yata_error").is_none(), "{other}");
                }
            }
            // The other bucket is not affected.
            let beta = &resources[1].configuration;
            for key in [GET_POLICY, GET_PUBLIC_ACCESS_BLOCK, GET_LOGGING] {
                assert!(beta[key].get("yata_error").is_none(), "{key}");
            }
        }
    }

    #[tokio::test]
    async fn the_client_of_each_bucket_uses_its_region() {
        let ctx = context(&[]);
        let clients = clients_by_region(
            &ctx,
            ["us-east-1", "eu-west-1", "us-east-1", "ap-northeast-1", "eu-west-1"],
        );
        assert_eq!(clients.len(), 3);
        for (region, client) in &clients {
            assert_eq!(
                client.config().region().map(|r| r.to_string()).as_deref(),
                Some(region.as_str())
            );
        }
        assert!(clients_by_region(&ctx, []).is_empty());
    }

    #[tokio::test]
    async fn listing_failures_fail_the_whole_group() {
        let cases = [
            ("AccessDenied", CollectionErrorKind::AccessDenied),
            ("SlowDown", CollectionErrorKind::Throttled),
            ("InternalError", CollectionErrorKind::Service),
        ];
        for (code, kind) in cases {
            // The first page fails.
            let rule = mock!(aws_sdk_s3::Client::list_buckets)
                .then_error(move || ListBucketsError::generic(metadata(code)));
            let err = collect(&context(&[rule]), "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("ListBuckets") && err.message().contains(code));
            assert!(!err.message().contains("arn:aws"));

            // The second page fails.
            let first = list_rules(vec![
                page(vec![bucket("alpha-bucket", "us-east-1")], None),
                page(vec![bucket("beta-bucket", "us-east-1")], None),
            ])
            .remove(0);
            let second = mock!(aws_sdk_s3::Client::list_buckets)
                .match_requests(|input: &ListBucketsInput| {
                    input.continuation_token() == Some("token-1")
                })
                .then_error(move || ListBucketsError::generic(metadata(code)));
            let mut rules = vec![first, second];
            rules.extend(unconfigured_rules("alpha-bucket"));
            let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), kind, "{code}");
            assert!(err.message().contains("ListBuckets") && err.message().contains(code));
        }
    }

    #[tokio::test]
    async fn elements_that_cannot_be_converted_fail_the_whole_group() {
        let cases: Vec<(Bucket, &str)> = vec![
            (
                Bucket::builder().bucket_region("us-east-1").build(),
                "MissingName",
            ),
            (Bucket::builder().name("alpha-bucket").build(), "MissingBucketRegion"),
            (
                Bucket::builder()
                    .name("alpha-bucket")
                    .bucket_region("not a region")
                    .build(),
                "InvalidBucketRegion",
            ),
            (
                Bucket::builder()
                    .name("alpha-bucket")
                    .bucket_region("us-east-1")
                    .creation_date(DateTime::from_secs(i64::MAX))
                    .build(),
                "InvalidDateTime",
            ),
        ];
        for (broken, reason) in cases {
            let mut rules = list_rules(vec![page(
                vec![bucket("beta-bucket", "us-east-1"), broken],
                None,
            )]);
            let calls = vec![
                policy_error_rule("beta-bucket", NO_SUCH_BUCKET_POLICY),
                pab_error_rule("beta-bucket", NO_SUCH_PUBLIC_ACCESS_BLOCK),
                logging_rule("beta-bucket", None),
            ];
            rules.extend(calls.iter().cloned());
            let err = collect(&context(&rules), "us-east-1").await.unwrap_err();
            assert_eq!(err.kind(), CollectionErrorKind::InvalidResponse, "{reason}");
            assert_eq!(
                err.message(),
                format!("invalid response when calling ListBuckets ({reason})")
            );
            for rule in &calls {
                assert_eq!(rule.num_calls(), 0, "{reason}");
            }
        }
    }

    #[tokio::test]
    async fn no_buckets_gives_an_empty_list() {
        let calls = unconfigured_rules("alpha-bucket");
        let cases = [
            vec![Page {
                buckets: Some(Vec::new()),
                owner: Some(owner(OWNER_ID_1, None)),
                prefix: None,
            }],
            vec![Page {
                buckets: None,
                owner: None,
                prefix: None,
            }],
        ];
        for pages in cases {
            let mut rules = list_rules(pages);
            rules.extend(calls.iter().cloned());
            let resources = collect(&context(&rules), "us-east-1").await.unwrap();
            assert!(resources.is_empty());
        }
        for rule in &calls {
            assert_eq!(rule.num_calls(), 0);
        }
    }

    #[tokio::test]
    async fn pages_without_buckets_are_skipped() {
        let mut rules = list_rules(vec![
            Page {
                buckets: None,
                owner: Some(owner(OWNER_ID_1, None)),
                prefix: None,
            },
            page(vec![bucket("alpha-bucket", "us-east-1")], Some(owner(OWNER_ID_2, None))),
            Page {
                buckets: Some(Vec::new()),
                owner: None,
                prefix: None,
            },
        ]);
        rules.extend(unconfigured_rules("alpha-bucket"));
        let resources = collect(&context(&rules), "us-east-1").await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].configuration["ListBuckets"]["Owner"]["ID"],
            json!(OWNER_ID_2)
        );
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
    fn the_arn_is_built_from_the_partition_and_the_name() {
        assert_eq!(bucket_arn("aws", "example-bucket"), "arn:aws:s3:::example-bucket");
        assert_eq!(bucket_arn("aws-cn", "example-bucket"), "arn:aws-cn:s3:::example-bucket");
    }

    #[test]
    fn metadata_of_the_collector() {
        let c = collector();
        assert_eq!(c.resource_type(), "aws.s3.bucket");
        assert_eq!(c.scope(), Scope::Global);
        assert_eq!(
            c.required_permissions(),
            [
                "s3:ListAllMyBuckets",
                "s3:GetBucketPolicy",
                "s3:GetBucketPublicAccessBlock",
                "s3:GetBucketLogging",
            ]
        );
    }
}
