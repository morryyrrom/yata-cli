//! The `AccountInfo` type.

use serde::{Deserialize, Serialize};

/// An account that was collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Account key, e.g. `aws-123456789012`.
    pub account: String,
    /// Provider-specific account id, e.g. `123456789012`.
    pub id: String,
    /// Provider name, e.g. `aws`.
    pub provider: String,
}
