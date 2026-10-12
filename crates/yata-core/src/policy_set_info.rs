//! The `PolicySetInfo` type.

use serde::{Deserialize, Serialize};

/// Identity of a loaded policy set, taken from its manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySetInfo {
    /// Name of the policy set (the first part of every rule ID).
    pub name: String,
    /// Version of the policy set, `x.y.z`.
    pub version: String,
    /// Policy format version.
    pub format_version: u32,
}
