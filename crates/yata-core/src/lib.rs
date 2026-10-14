#![forbid(unsafe_code)]
//! Core types and abstractions shared by all Yata Cloud crates.

pub mod account;
pub mod collection_error;
pub mod inventory;
pub mod policy_set_info;
pub mod resource;
pub mod resource_set;
pub mod resource_type_info;
pub mod rule_metadata;
pub mod rule_outcome;
pub mod rule_result;
pub mod scope;
pub mod severity;

pub use account::AccountInfo;
pub use collection_error::{CollectionError, CollectionErrorKind};
pub use inventory::Inventory;
pub use policy_set_info::PolicySetInfo;
pub use resource::Resource;
pub use resource_set::ResourceSet;
pub use resource_type_info::ResourceTypeInfo;
pub use rule_metadata::{Reference, RuleMetadata};
pub use rule_outcome::{IncompleteResource, RuleErrorKind, RuleOutcome, RuleStatus, SkipReason};
pub use rule_result::{ResultResource, ResultStatus, RuleResult};
pub use scope::Scope;
pub use severity::Severity;
