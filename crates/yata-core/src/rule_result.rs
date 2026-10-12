//! The `RuleResult` type: one per-resource result of a rule.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::severity::Severity;

/// Whether a resource passed or failed a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    /// The resource satisfies the rule.
    Pass,
    /// The resource violates the rule.
    Fail,
}

impl ResultStatus {
    /// Returns the serialized name of the status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

impl fmt::Display for ResultStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The resource a result belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultResource {
    /// Resource ID.
    pub id: String,
    /// Resource type, e.g. `aws.s3.bucket`.
    #[serde(rename = "type")]
    pub resource_type: String,
    /// Account key, e.g. `aws-123456789012`.
    pub account: String,
    /// Region of the resource, or `global`.
    pub region: String,
}

/// A per-resource result of a rule.
///
/// `message`, `path`, `actual` and `expected` are `None` for `pass` results. For `actual` and
/// `expected`, `Some(Value::Null)` means the element gave the value `null`, and `None` means the
/// element gave no value. This distinction is in memory only; how it appears in JSON is decided
/// by the report stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleResult {
    /// Rule ID.
    pub rule_id: String,
    /// `pass` or `fail`.
    pub status: ResultStatus,
    /// Severity of the rule.
    pub severity: Severity,
    /// The resource.
    pub resource: ResultResource,
    /// Description of the finding.
    pub message: Option<String>,
    /// JSON Pointer (RFC 6901) to the offending part of the resource.
    pub path: Option<String>,
    /// The actual value.
    pub actual: Option<Value>,
    /// The expected value.
    pub expected: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_as_snake_case_and_resource_uses_type_key() {
        assert_eq!(
            serde_json::to_value(ResultStatus::Fail).unwrap(),
            serde_json::json!("fail")
        );
        let resource = ResultResource {
            id: "i".to_owned(),
            resource_type: "aws.example.widget".to_owned(),
            account: "aws-123456789012".to_owned(),
            region: "us-east-1".to_owned(),
        };
        let v = serde_json::to_value(&resource).unwrap();
        assert_eq!(v["type"], serde_json::json!("aws.example.widget"));
    }
}
