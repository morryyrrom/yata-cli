//! Errors returned by collectors and the mapping from SDK errors to error kinds.
//!
//! Messages never contain the message body returned by the service, because it may contain
//! the caller's ARN and these messages end up in reports and snapshots. The full SDK error
//! is written to the `debug` log instead.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use std::error::Error;
use std::fmt;

use aws_sdk_sts::error::{ProvideErrorMetadata, SdkError};
use serde_json::{Value, json};
use yata_core::CollectionErrorKind;

use super::guard::ReadOnlyViolation;

/// Error codes that mean the caller is not allowed to call the API.
const ACCESS_DENIED_CODES: &[&str] = &[
    "AccessDenied",
    "AccessDeniedException",
    "UnauthorizedOperation",
    "AuthorizationError",
];

/// Error codes that the SDK classifies as throttling.
const THROTTLING_CODES: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

/// Error code that means the region is not enabled for the account.
const OPT_IN_REQUIRED_CODE: &str = "OptInRequired";

/// The shape of an SDK error, reduced to the facts that decide the error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The request could not be built (including refusals by interceptors).
    Construction,
    /// The operation timed out.
    Timeout,
    /// The request could not be sent.
    Dispatch {
        /// The failure was a timeout.
        timeout: bool,
        /// The failure was an I/O error (DNS, connection).
        io: bool,
    },
    /// The response could not be interpreted.
    Response,
    /// The service returned an error.
    Service,
}

/// Facts extracted from an SDK error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorFacts {
    /// The cause chain contains a read-only guard refusal.
    pub(crate) guard_blocked: bool,
    /// The error variant.
    pub(crate) failure: Failure,
    /// The error code returned by the service, if any.
    pub(crate) code: Option<String>,
}

/// Decides the error kind from the facts. The rows are checked from the top.
pub(crate) fn classify(facts: &ErrorFacts) -> CollectionErrorKind {
    if facts.guard_blocked {
        return CollectionErrorKind::Service;
    }
    match facts.failure {
        Failure::Timeout => return CollectionErrorKind::Timeout,
        Failure::Dispatch { timeout: true, .. } => return CollectionErrorKind::Timeout,
        Failure::Dispatch { io: true, .. } => return CollectionErrorKind::RegionUnavailable,
        Failure::Response => return CollectionErrorKind::InvalidResponse,
        Failure::Service => {
            if let Some(code) = facts.code.as_deref() {
                if ACCESS_DENIED_CODES.contains(&code) {
                    return CollectionErrorKind::AccessDenied;
                }
                if THROTTLING_CODES.contains(&code) {
                    return CollectionErrorKind::Throttled;
                }
                if code == OPT_IN_REQUIRED_CODE {
                    return CollectionErrorKind::RegionUnavailable;
                }
            }
        }
        Failure::Construction | Failure::Dispatch { .. } => {}
    }
    CollectionErrorKind::Service
}

/// Extracts the facts from an SDK error.
pub(crate) fn facts_of<E, R>(err: &SdkError<E, R>) -> ErrorFacts
where
    E: Error + ProvideErrorMetadata + 'static,
    R: fmt::Debug + 'static,
{
    let failure = match err {
        SdkError::TimeoutError(_) => Failure::Timeout,
        SdkError::DispatchFailure(dispatch) => Failure::Dispatch {
            timeout: dispatch.is_timeout(),
            io: dispatch.is_io(),
        },
        SdkError::ResponseError(_) => Failure::Response,
        SdkError::ServiceError(_) => Failure::Service,
        _ => Failure::Construction,
    };
    ErrorFacts {
        guard_blocked: has_guard_cause(err),
        failure,
        code: ProvideErrorMetadata::code(err).map(str::to_owned),
    }
}

/// Returns true if a read-only guard refusal is in the cause chain of `err`.
fn has_guard_cause(err: &(dyn Error + 'static)) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(err);
    while let Some(error) = current {
        if error.downcast_ref::<ReadOnlyViolation>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
}

/// Builds the message for an error that has been classified.
///
/// The form is `"<description> when calling <operation> (<code>)"`; a refusal by the
/// read-only guard is described as such.
pub(crate) fn describe(kind: CollectionErrorKind, facts: &ErrorFacts, operation: &str) -> String {
    if facts.guard_blocked {
        return format!("blocked by the read-only guard when calling {operation}");
    }
    let description = match kind {
        CollectionErrorKind::AccessDenied => "access denied",
        CollectionErrorKind::Throttled => "throttled",
        CollectionErrorKind::RegionUnavailable => "region unavailable",
        CollectionErrorKind::Timeout => "timeout",
        CollectionErrorKind::InvalidResponse => "invalid response",
        _ => "service error",
    };
    let code = code_label(facts);
    format!("{description} when calling {operation} ({code})")
}

/// Returns the service error code, or the name of the error variant when there is no code.
pub(crate) fn code_label(facts: &ErrorFacts) -> String {
    match facts.code.as_deref() {
        Some(code) => code.to_owned(),
        None => match facts.failure {
            Failure::Construction => "ConstructionFailure",
            Failure::Timeout => "TimeoutError",
            Failure::Dispatch { .. } => "DispatchFailure",
            Failure::Response => "ResponseError",
            Failure::Service => "Unknown",
        }
        .to_owned(),
    }
}

/// The failure of a whole collection group, returned by a collector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectError {
    kind: CollectionErrorKind,
    message: String,
}

impl CollectError {
    /// Creates an error with an explicit kind and message (English, without secrets).
    pub(crate) fn new(kind: CollectionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Creates an error from a failed SDK call to `operation` that enumerates resources.
    pub(crate) fn from_sdk<E, R>(operation: &str, err: &SdkError<E, R>) -> Self
    where
        E: Error + ProvideErrorMetadata + 'static,
        R: fmt::Debug + 'static,
    {
        let facts = facts_of(err);
        let kind = classify(&facts);
        tracing::debug!(operation, ?kind, error = ?err, "SDK call failed");
        Self {
            kind,
            message: describe(kind, &facts, operation),
        }
    }

    /// Returns the kind of the failure.
    pub(crate) fn kind(&self) -> CollectionErrorKind {
        self.kind
    }

    /// Returns the message.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CollectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl Error for CollectError {}

/// Builds the failure mark for a per-resource operation that failed.
///
/// The result is `{"yata_error": {"kind": ..., "message": ...}}`, to be stored under the
/// operation name in the resource configuration.
pub(crate) fn failure_mark<E, R>(operation: &str, err: &SdkError<E, R>) -> Value
where
    E: Error + ProvideErrorMetadata + 'static,
    R: fmt::Debug + 'static,
{
    let error = CollectError::from_sdk(operation, err);
    json!({
        "yata_error": {
            "kind": error.kind().as_str(),
            "message": error.message(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(failure: Failure, code: Option<&str>) -> ErrorFacts {
        ErrorFacts {
            guard_blocked: false,
            failure,
            code: code.map(str::to_owned),
        }
    }

    #[test]
    fn classification_table() {
        use CollectionErrorKind as K;
        let cases = [
            (facts(Failure::Timeout, None), K::Timeout),
            (
                facts(
                    Failure::Dispatch {
                        timeout: true,
                        io: false,
                    },
                    None,
                ),
                K::Timeout,
            ),
            (
                facts(
                    Failure::Dispatch {
                        timeout: false,
                        io: true,
                    },
                    None,
                ),
                K::RegionUnavailable,
            ),
            (
                facts(
                    Failure::Dispatch {
                        timeout: false,
                        io: false,
                    },
                    None,
                ),
                K::Service,
            ),
            (facts(Failure::Response, None), K::InvalidResponse),
            (facts(Failure::Construction, None), K::Service),
            (
                facts(Failure::Service, Some("AccessDenied")),
                K::AccessDenied,
            ),
            (
                facts(Failure::Service, Some("AccessDeniedException")),
                K::AccessDenied,
            ),
            (
                facts(Failure::Service, Some("UnauthorizedOperation")),
                K::AccessDenied,
            ),
            (
                facts(Failure::Service, Some("AuthorizationError")),
                K::AccessDenied,
            ),
            (facts(Failure::Service, Some("Throttling")), K::Throttled),
            (
                facts(Failure::Service, Some("ThrottlingException")),
                K::Throttled,
            ),
            (
                facts(Failure::Service, Some("RequestLimitExceeded")),
                K::Throttled,
            ),
            (facts(Failure::Service, Some("SlowDown")), K::Throttled),
            (
                facts(Failure::Service, Some("OptInRequired")),
                K::RegionUnavailable,
            ),
            (facts(Failure::Service, Some("NoSuchEntity")), K::Service),
            (facts(Failure::Service, None), K::Service),
        ];
        for (input, expected) in cases {
            assert_eq!(classify(&input), expected, "{input:?}");
        }
    }

    #[test]
    fn guard_refusal_wins_over_everything() {
        let input = ErrorFacts {
            guard_blocked: true,
            failure: Failure::Timeout,
            code: Some("AccessDenied".to_owned()),
        };
        assert_eq!(classify(&input), CollectionErrorKind::Service);
        let message = describe(CollectionErrorKind::Service, &input, "PutBucketPolicy");
        assert_eq!(
            message,
            "blocked by the read-only guard when calling PutBucketPolicy"
        );
    }

    #[test]
    fn message_form() {
        let input = facts(Failure::Service, Some("AccessDenied"));
        assert_eq!(
            describe(CollectionErrorKind::AccessDenied, &input, "ListBuckets"),
            "access denied when calling ListBuckets (AccessDenied)"
        );
        let input = facts(Failure::Timeout, None);
        assert_eq!(
            describe(CollectionErrorKind::Timeout, &input, "ListBuckets"),
            "timeout when calling ListBuckets (TimeoutError)"
        );
    }

    #[test]
    fn guard_cause_is_found_through_the_source_chain() {
        use crate::support::guard::DenyReason;
        let violation = ReadOnlyViolation {
            service: "IAM".to_owned(),
            operation: "CreateUser".to_owned(),
            reason: DenyReason::NotReadOperation,
        };
        assert!(has_guard_cause(&violation));
        let other = std::io::Error::other("other");
        assert!(!has_guard_cause(&other));
    }

    fn sts_error_rule(status: u16, code: &str) -> aws_smithy_mocks::Rule {
        let body = format!(
            "<ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code><Message>arn:aws:iam::123456789012:user/secret-name body</Message></Error><RequestId>r</RequestId></ErrorResponse>"
        );
        aws_smithy_mocks::mock!(aws_sdk_sts::Client::get_caller_identity).then_http_response(
            move || {
                aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
                    aws_smithy_runtime_api::http::StatusCode::try_from(status)
                        .expect("valid status"),
                    aws_sdk_s3::primitives::SdkBody::from(body.clone()),
                )
            },
        )
    }

    #[tokio::test]
    async fn from_sdk_and_failure_mark_map_codes_without_the_service_body() {
        let cases = [
            (403, "AccessDenied", CollectionErrorKind::AccessDenied),
            (400, "ThrottlingException", CollectionErrorKind::Throttled),
            (403, "OptInRequired", CollectionErrorKind::RegionUnavailable),
            (400, "ValidationError", CollectionErrorKind::Service),
        ];
        for (status, code, expected) in cases {
            let factory =
                crate::support::clients::testing::mock_factory(&[sts_error_rule(status, code)]);
            let client = factory.client::<aws_sdk_sts::Client>("us-east-1");
            let err = client
                .get_caller_identity()
                .send()
                .await
                .expect_err("the mock returns an error");

            let collect_error = CollectError::from_sdk("GetCallerIdentity", &err);
            assert_eq!(collect_error.kind(), expected, "{code}");
            let message = collect_error.message();
            assert!(message.contains("GetCallerIdentity"), "{message}");
            assert!(message.contains(code), "{message}");
            assert!(!message.contains("secret-name"), "{message}");

            let mark = failure_mark("GetCallerIdentity", &err);
            assert_eq!(mark["yata_error"]["kind"], json!(expected.as_str()));
            assert_eq!(mark["yata_error"]["message"], json!(message));
            assert_eq!(mark.as_object().map(|o| o.len()), Some(1));
        }
    }

    #[tokio::test]
    async fn guard_refusals_are_service_errors_naming_the_operation() {
        let rule =
            aws_smithy_mocks::mock!(aws_sdk_sts::Client::get_session_token).then_output(|| {
                aws_sdk_sts::operation::get_session_token::GetSessionTokenOutput::builder().build()
            });
        let factory = crate::support::clients::testing::mock_factory(&[rule]);
        let client = factory.client::<aws_sdk_sts::Client>("us-east-1");
        let err = client
            .get_session_token()
            .send()
            .await
            .expect_err("blocked");
        let collect_error = CollectError::from_sdk("GetSessionToken", &err);
        assert_eq!(collect_error.kind(), CollectionErrorKind::Service);
        assert!(collect_error.message().contains("read-only guard"));
        assert!(collect_error.message().contains("GetSessionToken"));
    }
}
