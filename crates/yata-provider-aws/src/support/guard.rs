//! Read-only guard: an SDK interceptor that stops write operations before they are sent.

use std::fmt;

use aws_sdk_sts::config::ConfigBag;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::interceptors::context::BeforeSerializationInterceptorContextRef;
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;

use super::guard_lists::{DENY_LIST, EXCEPTION_LIST};

/// Operation name prefixes that are read-only by convention.
const READ_PREFIXES: &[&str] = &["Describe", "List", "Get"];

/// Why an operation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DenyReason {
    /// The (service, operation) pair is in the deny list.
    DenyList,
    /// The operation name does not start with `Describe`, `List` or `Get` followed by an
    /// upper-case letter, and is not in the exception list.
    NotReadOperation,
    /// The SDK did not provide the operation metadata.
    MissingMetadata,
}

impl fmt::Display for DenyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            DenyReason::DenyList => "the operation is in the deny list",
            DenyReason::NotReadOperation => {
                "the operation name does not start with Describe, List or Get"
            }
            DenyReason::MissingMetadata => "the operation metadata is missing",
        };
        f.write_str(text)
    }
}

/// The result of checking one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// The operation may be sent.
    Allow,
    /// The operation must not be sent.
    Deny(DenyReason),
}

/// Decides whether an operation may be sent: deny list, then exception list, then prefix.
pub(crate) fn decide(service: &str, operation: &str) -> Decision {
    if DENY_LIST
        .iter()
        .any(|item| item.service == service && item.operation == operation)
    {
        return Decision::Deny(DenyReason::DenyList);
    }
    if EXCEPTION_LIST
        .iter()
        .any(|item| item.service == service && item.operation == operation)
    {
        return Decision::Allow;
    }
    if has_read_prefix(operation) {
        Decision::Allow
    } else {
        Decision::Deny(DenyReason::NotReadOperation)
    }
}

/// Returns true if `operation` starts with a read prefix followed by an upper-case letter.
fn has_read_prefix(operation: &str) -> bool {
    READ_PREFIXES.iter().any(|prefix| {
        operation
            .strip_prefix(prefix)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase())
    })
}

/// Decides from the optional operation metadata. A missing value is refused.
pub(crate) fn decide_metadata(metadata: Option<(&str, &str)>) -> Decision {
    match metadata {
        Some((service, operation)) => decide(service, operation),
        None => Decision::Deny(DenyReason::MissingMetadata),
    }
}

/// The error returned by the guard. It is the cause of the `SdkError` seen by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadOnlyViolation {
    /// Service name, or `unknown` if the metadata is missing.
    pub(crate) service: String,
    /// Operation name, or `unknown` if the metadata is missing.
    pub(crate) operation: String,
    /// Why the operation was refused.
    pub(crate) reason: DenyReason,
}

impl fmt::Display for ReadOnlyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "read-only guard blocked {}/{}: {}",
            self.service, self.operation, self.reason
        )
    }
}

impl std::error::Error for ReadOnlyViolation {}

/// SDK interceptor that refuses every operation that is not a read.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReadOnlyGuard;

impl Intercept for ReadOnlyGuard {
    fn name(&self) -> &'static str {
        "ReadOnlyGuard"
    }

    // The client-level `read_before_execution` hook runs before the operation configuration
    // (which holds the `Metadata`) is applied, so the check is done in
    // `read_before_serialization`: it is the first hook that sees the metadata, and an error
    // here skips serialization, signing and sending.
    fn read_before_serialization(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let metadata = cfg.load::<Metadata>();
        let pair = metadata.map(|m| (m.service(), m.name()));
        match decide_metadata(pair) {
            Decision::Allow => Ok(()),
            Decision::Deny(reason) => {
                let (service, operation) = pair.unwrap_or(("unknown", "unknown"));
                tracing::warn!("read-only guard blocked {service}/{operation}");
                Err(Box::new(ReadOnlyViolation {
                    service: service.to_owned(),
                    operation: operation.to_owned(),
                    reason,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::guard_lists::ListedOperation;

    #[test]
    fn read_prefixes_are_allowed() {
        for (service, operation) in [
            ("STS", "GetCallerIdentity"),
            ("EC2", "DescribeRegions"),
            ("IAM", "ListUsers"),
            ("S3", "ListBuckets"),
            ("S3 Control", "GetPublicAccessBlock"),
            ("CloudTrail", "DescribeTrails"),
        ] {
            assert_eq!(decide(service, operation), Decision::Allow, "{operation}");
        }
    }

    #[test]
    fn write_operations_are_denied() {
        for operation in [
            "CreateUser",
            "PutBucketPolicy",
            "DeleteLogGroup",
            "UpdateTrail",
            "StartLogging",
            "StopLogging",
            "Publish",
            "TagResource",
        ] {
            assert_eq!(
                decide("IAM", operation),
                Decision::Deny(DenyReason::NotReadOperation),
                "{operation}"
            );
        }
    }

    #[test]
    fn prefix_must_be_followed_by_an_upper_case_letter() {
        for operation in [
            "Getaway",
            "Listing",
            "Describes",
            "Get",
            "List",
            "Describe",
            "get",
        ] {
            assert_eq!(
                decide("EC2", operation),
                Decision::Deny(DenyReason::NotReadOperation),
                "{operation}"
            );
        }
    }

    #[test]
    fn every_deny_list_item_is_denied() {
        assert_eq!(DENY_LIST.len(), 9);
        for item in DENY_LIST {
            assert_eq!(
                decide(item.service, item.operation),
                Decision::Deny(DenyReason::DenyList),
                "{}/{}",
                item.service,
                item.operation
            );
        }
    }

    #[test]
    fn exception_list_matches_service_and_operation() {
        assert_eq!(decide("IAM", "GenerateCredentialReport"), Decision::Allow);
        assert_eq!(
            decide("IAM", "GenerateServiceLastAccessedDetails"),
            Decision::Deny(DenyReason::NotReadOperation)
        );
        assert_eq!(
            decide("Other", "GenerateCredentialReport"),
            Decision::Deny(DenyReason::NotReadOperation)
        );
    }

    #[test]
    fn missing_metadata_is_denied() {
        assert_eq!(
            decide_metadata(None),
            Decision::Deny(DenyReason::MissingMetadata)
        );
        assert_eq!(
            decide_metadata(Some(("STS", "GetCallerIdentity"))),
            Decision::Allow
        );
    }

    #[test]
    fn violation_message_names_service_and_operation_only() {
        let violation = ReadOnlyViolation {
            service: "STS".to_owned(),
            operation: "GetSessionToken".to_owned(),
            reason: DenyReason::DenyList,
        };
        assert_eq!(
            violation.to_string(),
            "read-only guard blocked STS/GetSessionToken: the operation is in the deny list"
        );
    }

    fn iam_action_parts(item: &ListedOperation) -> (&str, &str) {
        item.iam_action
            .split_once(':')
            .expect("iam action has a prefix")
    }

    #[test]
    fn lists_are_consistent() {
        for item in DENY_LIST.iter().chain(EXCEPTION_LIST) {
            let (prefix, operation) = iam_action_parts(item);
            assert!(
                !prefix.is_empty()
                    && prefix
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "bad IAM prefix in {}",
                item.iam_action
            );
            assert_eq!(operation, item.operation, "{}", item.iam_action);
            assert!(!item.reason.trim().is_empty(), "{}", item.iam_action);
        }
        for deny in DENY_LIST {
            assert!(
                !EXCEPTION_LIST
                    .iter()
                    .any(|e| e.service == deny.service && e.operation == deny.operation),
                "{}/{} is in both lists",
                deny.service,
                deny.operation
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for item in DENY_LIST.iter().chain(EXCEPTION_LIST) {
            assert!(
                seen.insert(item.iam_action),
                "duplicate {}",
                item.iam_action
            );
        }
    }
}
