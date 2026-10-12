#![forbid(unsafe_code)]
//! Core types and abstractions shared by all Yata Cloud crates.

pub mod account;
pub mod collection_error;
pub mod inventory;
pub mod resource;
pub mod resource_set;
pub mod resource_type_info;
pub mod scope;

pub use account::AccountInfo;
pub use collection_error::{CollectionError, CollectionErrorKind};
pub use inventory::Inventory;
pub use resource::Resource;
pub use resource_set::ResourceSet;
pub use resource_type_info::ResourceTypeInfo;
pub use scope::Scope;
