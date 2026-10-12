//! The `CollectionError` type: the record of one failed collection group.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Kind of a collection failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CollectionErrorKind {
    /// The caller is not allowed to call the API.
    AccessDenied,
    /// The API throttled the request and retries were exhausted.
    Throttled,
    /// The region could not be reached or is not enabled.
    RegionUnavailable,
    /// The request timed out.
    Timeout,
    /// Any other service-side or client-side failure.
    Service,
    /// The response could not be interpreted, or a collector returned invalid data.
    InvalidResponse,
}

impl CollectionErrorKind {
    /// Returns the serialized name of the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            CollectionErrorKind::AccessDenied => "access_denied",
            CollectionErrorKind::Throttled => "throttled",
            CollectionErrorKind::RegionUnavailable => "region_unavailable",
            CollectionErrorKind::Timeout => "timeout",
            CollectionErrorKind::Service => "service",
            CollectionErrorKind::InvalidResponse => "invalid_response",
        }
    }
}

impl fmt::Display for CollectionErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A failed collection group (account x region x resource type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionError {
    /// Account key, e.g. `aws-123456789012`.
    pub account: String,
    /// Region of the group, or `global`.
    pub region: String,
    /// Resource type that failed to be collected.
    pub resource_type: String,
    /// Kind of the failure.
    pub kind: CollectionErrorKind,
    /// Human-readable description. Contains no credentials.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_serializes_as_snake_case() {
        let value = serde_json::to_value(CollectionErrorKind::RegionUnavailable).unwrap();
        assert_eq!(value, serde_json::json!("region_unavailable"));
        for kind in [
            CollectionErrorKind::AccessDenied,
            CollectionErrorKind::Throttled,
            CollectionErrorKind::RegionUnavailable,
            CollectionErrorKind::Timeout,
            CollectionErrorKind::Service,
            CollectionErrorKind::InvalidResponse,
        ] {
            let value = serde_json::to_value(kind).unwrap();
            assert_eq!(value, serde_json::json!(kind.as_str()));
        }
    }
}
