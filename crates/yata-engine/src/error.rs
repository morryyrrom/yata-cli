//! Error and failure types of the policy engine.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

/// Policy loading failed with at least one directory-level failure (exit code 2).
///
/// Rule-level failures found before or after the directory-level failure are included, so one
/// run reports as many problems as possible.
#[derive(Debug, thiserror::Error)]
#[error("failed to load policies ({} directory-level failure(s))", .directory_failures.len())]
#[non_exhaustive]
pub struct PolicyLoadError {
    /// Failures that make a whole policy directory unusable.
    pub directory_failures: Vec<DirectoryFailure>,
    /// Failures of single rules found while loading.
    pub rule_failures: Vec<RuleFailure>,
}

/// A failure that makes a whole policy directory unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryFailure {
    /// Policy directory as given by the caller.
    pub policy_dir: PathBuf,
    /// Location relative to the policy directory (`/`-separated), if any.
    pub path: Option<String>,
    /// Kind of the failure.
    pub kind: DirectoryFailureKind,
    /// English description.
    pub message: String,
}

/// A failure of a single rule (or of a fixture directory without a rule).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleFailure {
    /// Name of the policy set (manifest `name`).
    pub policy_set: String,
    /// Rule ID, or `None` when it cannot be built or the failure belongs to no rule.
    pub rule_id: Option<String>,
    /// Location relative to the policy directory (`/`-separated).
    pub path: String,
    /// Kind of the failure.
    pub kind: RuleFailureKind,
    /// English description.
    pub message: String,
    /// Kind-specific details.
    pub details: Option<FailureDetails>,
}

macro_rules! kind_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        #[serde(rename_all = "snake_case")]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Returns the snake_case name used in JSON output.
            pub fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $text ),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

kind_enum! {
    /// Kind of a directory-level failure.
    DirectoryFailureKind {
        /// The policy directory does not exist.
        NotFound => "not_found",
        /// The policy directory path is not a directory.
        NotADirectory => "not_a_directory",
        /// A directory, the manifest or a library cannot be read.
        Unreadable => "unreadable",
        /// The manifest is missing, is a link, or is not a regular file.
        ManifestMissing => "manifest_missing",
        /// The manifest exceeds the size limit.
        ManifestTooLarge => "manifest_too_large",
        /// The manifest has a format error.
        ManifestInvalid => "manifest_invalid",
        /// `format_version` is not supported.
        UnsupportedFormatVersion => "unsupported_format_version",
        /// `min_yata_version` is newer than the running `yata`.
        YataTooOld => "yata_too_old",
        /// The manifest `name` is used by an earlier policy directory.
        DuplicateName => "duplicate_name",
        /// A library is invalid.
        LibraryInvalid => "library_invalid",
        /// A symbolic link or reparse point was found where it is not a rule-level failure.
        Symlink => "symlink",
        /// The file count limit was exceeded.
        TooManyFiles => "too_many_files",
        /// The directory count limit was exceeded.
        TooManyDirectories => "too_many_directories",
        /// Something exists deeper than the depth limit.
        DepthExceeded => "depth_exceeded",
        /// The directory contains no rule file.
        NoRules => "no_rules",
    }
}

kind_enum! {
    /// Kind of a rule-level failure.
    RuleFailureKind {
        /// The rule name or a directory name above it breaks the name rules.
        InvalidName => "invalid_name",
        /// The rule file or its fixture directory is a symbolic link or reparse point.
        Symlink => "symlink",
        /// The rule file or a fixture exceeds the size limit.
        FileTooLarge => "file_too_large",
        /// The rule file or a fixture cannot be read.
        Unreadable => "unreadable",
        /// Syntax or analysis error, non-UTF-8 content, or a timeout while checking.
        RegoError => "rego_error",
        /// The package name does not match the path.
        PackageMismatch => "package_mismatch",
        /// The metadata is invalid.
        MetadataInvalid => "metadata_invalid",
        /// `fail` is missing or is not a set.
        FailMissing => "fail_missing",
        /// The provider does not support a declared resource type.
        UnsupportedResourceType => "unsupported_resource_type",
        /// A fixture directory has no matching rule.
        OrphanFixtures => "orphan_fixtures",
        /// A `pass_` or `fail_` fixture is missing.
        FixturesMissing => "fixtures_missing",
        /// A fixture has a format error.
        FixtureInvalid => "fixture_invalid",
        /// A fixture contains a 12-digit number that is not an accepted dummy account ID.
        NonDummyAccountId => "non_dummy_account_id",
        /// The fixture result does not match `expected.fail`.
        FixtureMismatch => "fixture_mismatch",
        /// Evaluating a fixture failed.
        FixtureEvalError => "fixture_eval_error",
        /// Evaluating a fixture timed out.
        FixtureTimeout => "fixture_timeout",
    }
}

/// A resource identified by its `id` and `region`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceKey {
    /// Resource ID.
    pub id: String,
    /// Region of the resource.
    pub region: String,
}

/// A resource that failed although the fixture did not expect it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnexpectedResource {
    /// Resource ID.
    pub id: String,
    /// Region of the resource.
    pub region: String,
    /// Messages of the findings for this resource.
    pub messages: Vec<String>,
}

/// Kind-specific details of a [`RuleFailure`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum FailureDetails {
    /// `fixture_mismatch`.
    Mismatch {
        /// Expected to fail but did not.
        missing: Vec<ResourceKey>,
        /// Failed but was not expected to.
        unexpected: Vec<UnexpectedResource>,
    },
    /// `non_dummy_account_id`: JSON Pointers of the offending values (the values are not shown).
    Locations {
        /// JSON Pointers.
        locations: Vec<String>,
    },
    /// `unsupported_resource_type`.
    Types {
        /// Resource types the provider does not support.
        types: Vec<String>,
    },
    /// `fixture_timeout`.
    Timeout {
        /// The limit in seconds.
        timeout_seconds: u64,
    },
}

/// Error while evaluating one rule (rule status `error`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EvaluationError {
    /// The evaluation exceeded the time limit.
    #[error("evaluation timed out after {} s", .limit.as_secs_f64())]
    Timeout {
        /// The limit that was exceeded.
        limit: Duration,
    },
    /// The Rego engine reported an error.
    #[error("rego engine error: {message}")]
    Engine {
        /// Message of the engine.
        message: String,
    },
    /// `fail` is undefined or not a set.
    #[error("`fail` is not a set")]
    FailNotSet,
    /// An element of `fail` is malformed.
    #[error("`fail` element {index} is invalid: {reason}")]
    InvalidFailElement {
        /// Position of the element.
        index: usize,
        /// Why it is invalid.
        reason: String,
    },
    /// An element of `fail` refers to a resource that is not in the input.
    #[error("`fail` refers to an unknown resource {resource_id:?}{}", region_suffix(.region))]
    UnknownResource {
        /// The `resource_id` of the element.
        resource_id: String,
        /// The `resource_region` of the element, if given.
        region: Option<String>,
    },
    /// Two resources of the target type share the same `id` and `region`.
    #[error("duplicate resource {resource_id:?} in region {region:?}")]
    DuplicateResource {
        /// Resource ID.
        resource_id: String,
        /// Region.
        region: String,
    },
}

fn region_suffix(region: &Option<String>) -> String {
    region
        .as_ref()
        .map(|r| format!(" in region {r:?}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_serialize_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&DirectoryFailureKind::UnsupportedFormatVersion).unwrap(),
            "\"unsupported_format_version\""
        );
        assert_eq!(
            serde_json::to_string(&RuleFailureKind::NonDummyAccountId).unwrap(),
            "\"non_dummy_account_id\""
        );
        assert_eq!(
            RuleFailureKind::OrphanFixtures.to_string(),
            "orphan_fixtures"
        );
    }

    #[test]
    fn details_serialize_untagged() {
        let d = FailureDetails::Locations {
            locations: vec!["/a".into()],
        };
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"locations":["/a"]}"#
        );
    }
}
