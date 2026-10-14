//! The `RuleOutcome` type and its small companions: the result of one rule for one account.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::rule_metadata::RuleMetadata;
use crate::rule_result::RuleResult;

/// State of a rule for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    /// The rule was evaluated and all inputs were complete.
    Evaluated,
    /// The rule was evaluated, but part of the data was missing or marked as failed.
    Incomplete,
    /// The rule was not evaluated (see [`SkipReason`]).
    Skipped,
    /// An error occurred while evaluating the rule.
    Error,
}

impl RuleStatus {
    /// Returns the serialized name of the status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Evaluated => "evaluated",
            Self::Incomplete => "incomplete",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for RuleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a rule was not evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// At least one required group was never collected.
    NotCollected,
    /// All groups of at least one required resource type failed to be collected.
    CollectionFailed,
}

impl SkipReason {
    /// Returns the serialized name of the reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotCollected => "not_collected",
            Self::CollectionFailed => "collection_failed",
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The kind of an evaluation error of a rule (status `error`).
///
/// The kind is safe to log at any level: it says what went wrong without naming resources or
/// values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleErrorKind {
    /// A complete rule or a function produced two different values (a Rego conflict).
    Conflict,
    /// The rule broke the rule conventions (for example `fail` is not a boolean).
    Type,
    /// The evaluation exceeded the time limit.
    Timeout,
    /// Any other error of the Rego engine.
    Engine,
    /// The input given to the engine was wrong. This should not happen.
    Internal,
}

impl RuleErrorKind {
    /// Returns the serialized name of the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflict => "conflict",
            Self::Type => "type",
            Self::Timeout => "timeout",
            Self::Engine => "engine",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for RuleErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A resource whose configuration contains a failure marker (`{"yata_error": {...}}`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IncompleteResource {
    /// Resource ID.
    pub resource_id: String,
    /// Resource type, e.g. `aws.s3.bucket`.
    pub resource_type: String,
    /// Region of the resource, or `global`.
    pub region: String,
    /// Name of the operation that failed (the key in `configuration`).
    pub operation: String,
    /// Kind of the failure, or `unknown` when the marker has no string kind.
    pub kind: String,
}

/// The outcome of one rule for one account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleOutcome {
    /// Rule ID.
    pub rule_id: String,
    /// Metadata of the rule.
    pub metadata: RuleMetadata,
    /// Account key of the evaluated account.
    pub account: String,
    /// State of the rule.
    pub status: RuleStatus,
    /// Set only when `status` is `Skipped`.
    pub skip_reason: Option<SkipReason>,
    /// Set only when `status` is `Error`: a one-line English description. It may contain the
    /// message of the Rego engine, which can contain values from the rule.
    pub error: Option<String>,
    /// Set only when `status` is `Error`: the kind of the error. Not serialized: it is an
    /// internal value for logs and for `--omit-values`; how reports show it is decided later.
    #[serde(skip)]
    pub error_kind: Option<RuleErrorKind>,
    /// Set only when `status` is `Error`: the description without the message of the Rego
    /// engine. It names the resource but never contains a value, so it stays when values are
    /// omitted from a report. Not serialized, like `error_kind`.
    #[serde(skip)]
    pub error_summary: Option<String>,
    /// Per-resource results. Empty when `status` is `Skipped` or `Error`.
    pub results: Vec<RuleResult>,
    /// Resources that carried a failure marker. Empty when `status` is `Skipped` or `Error`.
    pub incomplete_resources: Vec<IncompleteResource>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_value(RuleStatus::Incomplete).unwrap(),
            serde_json::json!("incomplete")
        );
        assert_eq!(
            serde_json::to_value(SkipReason::CollectionFailed).unwrap(),
            serde_json::json!("collection_failed")
        );
        assert_eq!(
            serde_json::to_value(RuleErrorKind::Conflict).unwrap(),
            serde_json::json!("conflict")
        );
        for kind in [
            RuleErrorKind::Conflict,
            RuleErrorKind::Type,
            RuleErrorKind::Timeout,
            RuleErrorKind::Engine,
            RuleErrorKind::Internal,
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::json!(kind.as_str())
            );
        }
        assert_eq!(RuleStatus::Error.to_string(), "error");
        assert_eq!(SkipReason::NotCollected.to_string(), "not_collected");
    }
}
