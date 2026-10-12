//! The `RuleMetadata` and `Reference` types.

use serde::{Deserialize, Serialize};

use crate::severity::Severity;

/// A reference from a rule to a control of a standard (for example a CIS benchmark control).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    /// Standard identifier, e.g. `cis-aws-foundations`.
    pub standard: String,
    /// Version of the standard, e.g. `7.1.0`.
    pub version: String,
    /// Control identifier within the standard.
    pub control: String,
    /// Optional URL of the control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Validated metadata of a rule, taken from the `metadata` object of the Rego package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// One-line title.
    pub title: String,
    /// Severity of a failing result.
    pub severity: Severity,
    /// What the rule checks and why it matters.
    pub description: String,
    /// How to fix a failing result.
    pub remediation: String,
    /// Resource type the rule produces results for, e.g. `aws.s3.bucket`.
    pub target_type: String,
    /// Resource types the rule reads but does not produce results for.
    #[serde(default)]
    pub related_types: Vec<String>,
    /// References to controls of standards.
    pub references: Vec<Reference>,
    /// Free-form tags used for filtering.
    #[serde(default)]
    pub tags: Vec<String>,
}
