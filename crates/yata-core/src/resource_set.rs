//! The `ResourceSet` type: the result of one successful collection group.

use serde::{Deserialize, Serialize};

use crate::resource::Resource;

/// Resources collected for one account, one region and one resource type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSet {
    /// Account key, e.g. `aws-123456789012`.
    pub account: String,
    /// Region of the group, or `global`.
    pub region: String,
    /// Resource type of every resource in the set.
    pub resource_type: String,
    /// Collected resources, sorted by `id` ascending.
    pub resources: Vec<Resource>,
}
