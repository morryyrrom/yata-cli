//! Resource limits applied while loading a policy directory.

use std::time::Duration;

/// Default for `--policy-timeout`: the time limit of one evaluation.
pub const DEFAULT_POLICY_TIMEOUT: Duration = Duration::from_secs(30);

/// Resource limits applied while loading a policy directory.
///
/// The defaults are fixed for users; tests pass smaller values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum size of the manifest in bytes.
    pub max_manifest_bytes: u64,
    /// Maximum size of a rule or library `.rego` file in bytes.
    pub max_rego_bytes: u64,
    /// Maximum size of a fixture file in bytes.
    pub max_fixture_bytes: u64,
    /// Maximum number of files per policy directory.
    pub max_files: usize,
    /// Maximum number of directories (excluding the root) per policy directory.
    pub max_directories: usize,
    /// Maximum depth of the directory tree (entries directly under the root have depth 1).
    pub max_depth: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_manifest_bytes: 64 * 1024,
            max_rego_bytes: 1024 * 1024,
            max_fixture_bytes: 4 * 1024 * 1024,
            max_files: 20_000,
            max_directories: 5_000,
            max_depth: 10,
        }
    }
}
