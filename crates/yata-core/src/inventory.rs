//! The `Inventory` type: the result of one collection run.

use std::time::SystemTime;

use crate::account::AccountInfo;
use crate::collection_error::CollectionError;
use crate::resource_set::ResourceSet;
use crate::resource_type_info::ResourceTypeInfo;

/// The whole result of one collection run.
///
/// Every targeted group (account x region x resource type) appears in exactly one of
/// `resource_sets` or `collection_errors`. This type is not serialized directly.
#[derive(Debug, Clone, PartialEq)]
pub struct Inventory {
    /// Collected accounts.
    pub accounts: Vec<AccountInfo>,
    /// Targeted regions, ascending.
    pub regions: Vec<String>,
    /// Targeted resource types with their scopes, ascending by type name.
    pub resource_types: Vec<ResourceTypeInfo>,
    /// Successful groups, ascending by (account, region, resource type).
    pub resource_sets: Vec<ResourceSet>,
    /// Failed groups, ascending by (account, region, resource type).
    pub collection_errors: Vec<CollectionError>,
    /// When the collection started.
    pub started_at: SystemTime,
    /// When the collection finished.
    pub finished_at: SystemTime,
}
