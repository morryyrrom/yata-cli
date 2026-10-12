#![forbid(unsafe_code)]
//! Policy engine: loads Rego policies and evaluates them against collected resources.
//!
//! This crate implements loading and validation of policy directories
//! ([`load_policies`]) and the evaluation of rules ([`evaluate`]). The rule tests are added later.

mod engine_config;
pub mod error;
mod evaluate;
mod files;
mod findings;
mod input;
mod library;
pub mod limits;
mod loader;
mod manifest;
mod metadata;
pub mod names;
mod rule;
pub mod version;
mod walk;

pub use error::{
    DirectoryFailure, DirectoryFailureKind, EvaluationError, FailureDetails, PolicyLoadError,
    ResourceKey, RuleFailure, RuleFailureKind, UnexpectedResource,
};
pub use evaluate::evaluate;
pub use limits::{DEFAULT_POLICY_TIMEOUT, ResourceLimits};
pub use loader::{LoadMode, LoadOptions, LoadedPolicies, PolicySet, load_policies};
pub use manifest::SUPPORTED_FORMAT_VERSIONS;
pub use rule::Rule;
pub use version::{VersionError, YataVersion};
#[cfg(test)]
mod fixture_dir_tests;
