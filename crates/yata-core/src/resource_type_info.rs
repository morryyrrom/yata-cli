//! The `ResourceTypeInfo` type.

use serde::{Deserialize, Serialize};

use crate::scope::Scope;

/// A resource type that was targeted by a collection, with its scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceTypeInfo {
    /// Resource type, e.g. `aws.s3.bucket`.
    #[serde(rename = "type")]
    pub resource_type: String,
    /// Whether the type is global or regional.
    pub scope: Scope,
}
