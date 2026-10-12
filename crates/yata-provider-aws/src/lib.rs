#![forbid(unsafe_code)]
//! AWS provider: collects resource configuration using read-only AWS APIs.
//!
//! The public API is small: [`supported_resource_types`], [`setup_permissions`] and
//! [`collect`]. Collectors, the read-only guard and client creation are internal. Every
//! SDK client is created with a read-only guard that refuses write operations before they
//! are sent.

mod collector;
mod collectors;
mod context;
mod error;
mod runner;
mod setup;
mod support;

use std::collections::BTreeSet;
use std::future::Future;
use std::num::NonZeroUsize;

use yata_core::{Inventory, Scope};

pub use error::{ProviderError, SetupError};

use collector::Collector;

/// The resource type of the account resource, which the provider itself creates.
pub(crate) const ACCOUNT_RESOURCE_TYPE: &str = "aws.account.account";

/// Permissions needed to create the account resource.
const ACCOUNT_PERMISSIONS: &[&str] = &["sts:GetCallerIdentity"];

/// Default value of the `--concurrency` option: the number of groups that run at the same time.
pub const DEFAULT_CONCURRENCY: NonZeroUsize = match NonZeroUsize::new(16) {
    Some(value) => value,
    // Evaluated at compile time: a zero value fails the build, never the program.
    None => panic!("the default concurrency must not be zero"),
};

/// A resource type that the provider can collect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTypeSpec {
    /// Resource type, e.g. `aws.s3.bucket`.
    pub resource_type: String,
    /// Whether the type is global or regional.
    pub scope: Scope,
    /// IAM actions required to collect the type, e.g. `s3:ListAllMyBuckets`.
    pub required_permissions: Vec<String>,
}

/// Permissions needed for the preparation of a collection, independent of the collectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPermissions {
    /// Always required.
    pub always: Vec<String>,
    /// Required only when `--region` is not given.
    pub without_region_option: Vec<String>,
}

/// Parameters of [`collect`].
#[derive(Debug, Clone)]
pub struct CollectRequest {
    /// Names of the resource types to collect. `aws.account.account` is always collected.
    pub resource_types: Vec<String>,
    /// The `--profile` value. Only the credentials of this profile are used.
    pub profile: Option<String>,
    /// The `--region` values. `None` means all regions that are enabled for the account.
    pub regions: Option<Vec<String>>,
    /// Maximum number of groups (resource type x region) that run at the same time.
    pub concurrency: NonZeroUsize,
}

/// Returns the resource types that can be collected, ascending by name.
///
/// The list always contains `aws.account.account`.
pub fn supported_resource_types() -> Vec<ResourceTypeSpec> {
    specs_of(&collectors::all_collectors())
}

/// Returns the permissions needed to prepare a collection.
pub fn setup_permissions() -> SetupPermissions {
    SetupPermissions {
        always: vec!["sts:GetCallerIdentity".to_owned()],
        without_region_option: vec!["ec2:DescribeRegions".to_owned()],
    }
}

/// Collects the requested resource types and returns the result.
///
/// When `cancel` completes, every running task is aborted and
/// [`ProviderError::Interrupted`] is returned. Must be called inside a tokio runtime.
pub async fn collect(
    request: CollectRequest,
    cancel: impl Future<Output = ()> + Send,
) -> Result<Inventory, ProviderError> {
    let collectors = collectors::all_collectors();
    let params = validate_request(&request, &collectors)?;
    let mut cancel = std::pin::pin!(cancel);
    let (factory, first_region) = tokio::select! {
        biased;
        () = &mut cancel => return Err(ProviderError::Interrupted),
        loaded = setup::load_environment(request.profile.as_deref()) => loaded,
    };
    runner::run(params, factory, &first_region, collectors, &mut cancel).await
}

/// Builds the list of supported types from a set of collectors, plus the account type.
fn specs_of(collectors: &[Box<dyn Collector>]) -> Vec<ResourceTypeSpec> {
    let mut specs: Vec<ResourceTypeSpec> = collectors
        .iter()
        .map(|collector| ResourceTypeSpec {
            resource_type: collector.resource_type().to_owned(),
            scope: collector.scope(),
            required_permissions: collector
                .required_permissions()
                .iter()
                .map(|permission| (*permission).to_owned())
                .collect(),
        })
        .collect();
    specs.push(ResourceTypeSpec {
        resource_type: ACCOUNT_RESOURCE_TYPE.to_owned(),
        scope: Scope::Global,
        required_permissions: ACCOUNT_PERMISSIONS
            .iter()
            .map(|p| (*p).to_owned())
            .collect(),
    });
    specs.sort_by(|a, b| a.resource_type.cmp(&b.resource_type));
    specs
}

/// Returns true if `region` has the accepted shape: lower-case letters, digits and `-`,
/// starting with a letter, ending with a digit, at most 32 characters.
///
/// This only guards against unexpected characters in an endpoint host name. It does not
/// check that the region exists.
pub(crate) fn is_valid_region_name(region: &str) -> bool {
    let bytes = region.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    bytes.len() <= 32
        && first.is_ascii_lowercase()
        && last.is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Checks the request and merges duplicates.
fn validate_request(
    request: &CollectRequest,
    collectors: &[Box<dyn Collector>],
) -> Result<runner::RunParams, ProviderError> {
    let supported: BTreeSet<&str> = collectors
        .iter()
        .map(|collector| collector.resource_type())
        .chain(std::iter::once(ACCOUNT_RESOURCE_TYPE))
        .collect();
    let mut resource_types = BTreeSet::new();
    for resource_type in &request.resource_types {
        if !supported.contains(resource_type.as_str()) {
            return Err(ProviderError::UnsupportedResourceType(
                resource_type.clone(),
            ));
        }
        resource_types.insert(resource_type.clone());
    }

    let regions = match &request.regions {
        None => None,
        Some(regions) if regions.is_empty() => {
            return Err(ProviderError::InvalidRegion(
                "at least one region is required when regions are specified".to_owned(),
            ));
        }
        Some(regions) => {
            let mut unique = BTreeSet::new();
            for region in regions {
                if !is_valid_region_name(region) {
                    return Err(ProviderError::InvalidRegion(format!(
                        "'{region}' is not a valid region name"
                    )));
                }
                unique.insert(region.clone());
            }
            Some(unique.into_iter().collect::<Vec<_>>())
        }
    };
    Ok(runner::RunParams {
        resource_types,
        regions,
        concurrency: request.concurrency.get(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::CollectFuture;
    use crate::context::CollectContext;
    use crate::support::guard_lists::{DENY_LIST, EXCEPTION_LIST};

    const REGO_RESERVED: &[&str] = &[
        "as", "contains", "default", "else", "every", "false", "if", "import", "in", "not", "null",
        "package", "some", "true", "with",
    ];

    fn is_name_part(part: &str) -> bool {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !REGO_RESERVED.contains(&part)
    }

    fn is_read_permission(permission: &str) -> bool {
        let Some((service, action)) = permission.split_once(':') else {
            return false;
        };
        let service_ok = !service.is_empty()
            && service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let action_ok = ["Describe", "List", "Get"].iter().any(|prefix| {
            action
                .strip_prefix(prefix)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_uppercase())
        });
        service_ok && action_ok && action.chars().all(|c| c.is_ascii_alphanumeric())
    }

    /// Checks the rules every resource type list must satisfy.
    fn check_specs(specs: &[ResourceTypeSpec]) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        let mut previous: Option<&str> = None;
        for spec in specs {
            let name = spec.resource_type.as_str();
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() != 3 || parts[0] != "aws" || !parts.iter().all(|p| is_name_part(p)) {
                return Err(format!("invalid resource type name: {name}"));
            }
            if !seen.insert(name) {
                return Err(format!("duplicate resource type: {name}"));
            }
            if previous.is_some_and(|p| p >= name) {
                return Err(format!("not ascending at: {name}"));
            }
            previous = Some(name);
            if spec.required_permissions.is_empty() {
                return Err(format!("{name}: no required permissions"));
            }
            for permission in &spec.required_permissions {
                if DENY_LIST.iter().any(|d| d.iam_action == permission) {
                    return Err(format!("{name}: denied permission {permission}"));
                }
                let excepted = EXCEPTION_LIST.iter().any(|e| e.iam_action == permission);
                if !excepted && !is_read_permission(permission) {
                    return Err(format!("{name}: not a read permission: {permission}"));
                }
            }
        }
        Ok(())
    }

    struct Fake {
        resource_type: &'static str,
        scope: Scope,
        permissions: &'static [&'static str],
    }

    impl Collector for Fake {
        fn resource_type(&self) -> &'static str {
            self.resource_type
        }
        fn scope(&self) -> Scope {
            self.scope
        }
        fn required_permissions(&self) -> &'static [&'static str] {
            self.permissions
        }
        fn collect<'a>(&'a self, _ctx: &'a CollectContext, _region: &'a str) -> CollectFuture<'a> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    fn fake(
        resource_type: &'static str,
        scope: Scope,
        permissions: &'static [&'static str],
    ) -> Box<dyn Collector> {
        Box::new(Fake {
            resource_type,
            scope,
            permissions,
        })
    }

    #[test]
    fn real_supported_types_satisfy_the_rules() {
        let specs = supported_resource_types();
        check_specs(&specs).expect("the list is valid");
        let account = specs
            .iter()
            .find(|s| s.resource_type == "aws.account.account")
            .expect("the account type is listed");
        assert_eq!(account.scope, Scope::Global);
        assert_eq!(account.required_permissions, ["sts:GetCallerIdentity"]);
    }

    #[test]
    fn collector_types_match_their_file_paths() {
        let collectors = collectors::all_collectors();
        assert_eq!(collectors.len(), collectors::COLLECTOR_PATH_TYPES.len());
        for (collector, path_type) in collectors.iter().zip(collectors::COLLECTOR_PATH_TYPES) {
            assert_eq!(collector.resource_type(), *path_type);
            assert_ne!(collector.resource_type(), ACCOUNT_RESOURCE_TYPE);
        }
        let unique: BTreeSet<_> = collectors::COLLECTOR_PATH_TYPES.iter().collect();
        assert_eq!(unique.len(), collectors::COLLECTOR_PATH_TYPES.len());
    }

    #[test]
    fn the_check_accepts_valid_fakes_and_rejects_bad_ones() {
        let good = specs_of(&[
            fake("aws.s3.bucket", Scope::Global, &["s3:ListAllMyBuckets"]),
            fake(
                "aws.iam.credential_report",
                Scope::Global,
                &["iam:GenerateCredentialReport", "iam:GetCredentialReport"],
            ),
        ]);
        check_specs(&good).expect("valid");

        let cases: Vec<(&str, Vec<Box<dyn Collector>>)> = vec![
            (
                "bad name",
                vec![fake("aws.s3", Scope::Global, &["s3:ListAllMyBuckets"])],
            ),
            (
                "upper case",
                vec![fake(
                    "aws.S3.bucket",
                    Scope::Global,
                    &["s3:ListAllMyBuckets"],
                )],
            ),
            (
                "reserved word",
                vec![fake("aws.iam.default", Scope::Global, &["iam:ListUsers"])],
            ),
            (
                "write permission",
                vec![fake(
                    "aws.s3.bucket",
                    Scope::Global,
                    &["s3:PutBucketPolicy"],
                )],
            ),
            (
                "denied permission",
                vec![fake("aws.s3.bucket", Scope::Global, &["s3:GetObject"])],
            ),
            (
                "no permission",
                vec![fake("aws.s3.bucket", Scope::Global, &[])],
            ),
            (
                "duplicate",
                vec![
                    fake("aws.s3.bucket", Scope::Global, &["s3:ListAllMyBuckets"]),
                    fake("aws.s3.bucket", Scope::Global, &["s3:ListAllMyBuckets"]),
                ],
            ),
        ];
        for (label, collectors) in cases {
            assert!(check_specs(&specs_of(&collectors)).is_err(), "{label}");
        }
    }

    #[test]
    fn specs_are_sorted_and_include_the_account_type() {
        let specs = specs_of(&[
            fake("aws.s3.bucket", Scope::Global, &["s3:ListAllMyBuckets"]),
            fake("aws.ec2.vpc", Scope::Regional, &["ec2:DescribeVpcs"]),
        ]);
        let names: Vec<&str> = specs.iter().map(|s| s.resource_type.as_str()).collect();
        assert_eq!(
            names,
            ["aws.account.account", "aws.ec2.vpc", "aws.s3.bucket"]
        );
    }

    #[test]
    fn setup_permissions_are_listed_separately() {
        let permissions = setup_permissions();
        assert_eq!(permissions.always, ["sts:GetCallerIdentity"]);
        assert_eq!(permissions.without_region_option, ["ec2:DescribeRegions"]);
    }

    #[test]
    fn region_name_format() {
        for valid in [
            "us-east-1",
            "ap-southeast-7",
            "eu-central-2",
            "us-gov-west-1",
        ] {
            assert!(is_valid_region_name(valid), "{valid}");
        }
        let too_long = format!("a{}1", "b".repeat(31));
        for invalid in [
            "",
            "US-EAST-1",
            "us-east-",
            "1-east-1",
            "us_east_1",
            "us east 1",
            "us-east-1.evil.example",
            "us-east-1/",
            "us-east-1\n",
            "-east-1",
            too_long.as_str(),
        ] {
            assert!(!is_valid_region_name(invalid), "{invalid:?}");
        }
        let longest = format!("a{}1", "b".repeat(30));
        assert_eq!(longest.len(), 32);
        assert!(is_valid_region_name(&longest));
    }

    fn request(types: &[&str], regions: Option<&[&str]>) -> CollectRequest {
        CollectRequest {
            resource_types: types.iter().map(|t| (*t).to_owned()).collect(),
            profile: None,
            regions: regions.map(|r| r.iter().map(|s| (*s).to_owned()).collect()),
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    fn collectors_for_validation() -> Vec<Box<dyn Collector>> {
        vec![
            fake("aws.s3.bucket", Scope::Global, &["s3:ListAllMyBuckets"]),
            fake("aws.ec2.vpc", Scope::Regional, &["ec2:DescribeVpcs"]),
        ]
    }

    #[test]
    fn validation_merges_duplicates_and_sorts_regions() {
        let params = validate_request(
            &request(
                &[
                    "aws.s3.bucket",
                    "aws.ec2.vpc",
                    "aws.s3.bucket",
                    "aws.account.account",
                ],
                Some(&["us-west-2", "ap-northeast-1", "us-west-2"]),
            ),
            &collectors_for_validation(),
        )
        .expect("valid");
        let types: Vec<&str> = params.resource_types.iter().map(String::as_str).collect();
        assert_eq!(
            types,
            ["aws.account.account", "aws.ec2.vpc", "aws.s3.bucket"]
        );
        assert_eq!(
            params.regions,
            Some(vec!["ap-northeast-1".to_owned(), "us-west-2".to_owned()])
        );
        assert_eq!(params.concurrency, 16);
    }

    #[test]
    fn validation_rejects_unsupported_types_and_bad_regions() {
        let collectors = collectors_for_validation();
        let err = validate_request(&request(&["aws.nope.thing"], None), &collectors)
            .expect_err("unsupported");
        assert!(
            matches!(err, ProviderError::UnsupportedResourceType(ref t) if t == "aws.nope.thing")
        );

        let err = validate_request(&request(&[], Some(&["US-EAST-1"])), &collectors)
            .expect_err("bad region");
        assert!(matches!(err, ProviderError::InvalidRegion(_)));

        let err = validate_request(&request(&[], Some(&[])), &collectors).expect_err("empty");
        assert!(matches!(err, ProviderError::InvalidRegion(_)));

        let params = validate_request(&request(&[], None), &collectors).expect("none is fine");
        assert_eq!(params.regions, None);
    }

    #[tokio::test]
    async fn collect_rejects_invalid_requests_before_touching_the_environment() {
        let err = collect(request(&["aws.nope.thing"], None), std::future::pending())
            .await
            .expect_err("unsupported");
        assert!(matches!(err, ProviderError::UnsupportedResourceType(_)));
        assert_eq!(err.exit_code(), 2);
        let err = collect(request(&[], Some(&[])), std::future::pending())
            .await
            .expect_err("empty regions");
        assert!(matches!(err, ProviderError::InvalidRegion(_)));
    }

    #[tokio::test]
    async fn collect_returns_interrupted_when_cancelled_during_setup() {
        let err = collect(request(&[], Some(&["us-east-1"])), std::future::ready(()))
            .await
            .expect_err("interrupted");
        assert!(matches!(err, ProviderError::Interrupted));
        assert_eq!(err.exit_code(), 130);
    }

    #[test]
    fn default_concurrency_is_16() {
        assert_eq!(DEFAULT_CONCURRENCY.get(), 16);
    }
}
