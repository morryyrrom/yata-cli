//! Error and failure types of the policy engine.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use yata_core::RuleErrorKind;

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
        /// `fail` is missing, undefined without input, or is not a boolean.
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
    /// The `message` of the result for this resource.
    pub message: String,
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
///
/// Evaluation stops at the first error of a rule and account. The descriptions made by this
/// crate name the resource (`id` and region) and a type or a rule, never a value returned by the
/// rule. The message of the Rego engine is kept apart in [`EvaluationError::Engine`], because it
/// can contain values; [`EvaluationError::summary`] leaves it out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EvaluationError {
    /// The evaluation exceeded the time limit.
    #[error("{}", self.summary())]
    Timeout {
        /// The limit that was exceeded.
        limit: Duration,
        /// The resource being evaluated when the limit was hit, if any.
        resource: Option<ResourceKey>,
    },
    /// The Rego engine reported an error (a conflict, for example).
    #[error("{}: {message}", self.summary())]
    Engine {
        /// Message of the engine. It can contain values from the rule.
        message: String,
        /// The resource being evaluated, if the error belongs to one.
        resource: Option<ResourceKey>,
    },
    /// `fail` is undefined for a resource.
    #[error("{}", self.summary())]
    FailUndefined {
        /// The resource being evaluated.
        resource: ResourceKey,
    },
    /// `fail` is not a boolean.
    #[error("{}", self.summary())]
    FailNotBoolean {
        /// Name of the type that `fail` has.
        found: &'static str,
        /// The resource being evaluated.
        resource: ResourceKey,
    },
    /// `message` is not a non-empty string.
    #[error("{}", self.summary())]
    InvalidMessage {
        /// What is wrong, with the name of a type but no value.
        reason: String,
        /// The resource being evaluated.
        resource: ResourceKey,
    },
    /// `reason` is not an object, or `reason.path` is not a JSON Pointer string.
    #[error("{}", self.summary())]
    InvalidReason {
        /// What is wrong, with the name of a type but no value.
        reason: String,
        /// The resource being evaluated.
        resource: ResourceKey,
    },
    /// Two resources of the target type share the same `id` and region.
    #[error("{}", self.summary())]
    DuplicateResource {
        /// Resource ID.
        resource_id: String,
        /// Region.
        region: String,
    },
    /// A resource of the target type has no string `id` or `region`.
    #[error("{}", self.summary())]
    InvalidResource {
        /// Position of the resource in the array of the target type.
        index: usize,
    },
}

impl EvaluationError {
    /// The kind of the error, safe to log at any level.
    pub fn kind(&self) -> RuleErrorKind {
        match self {
            Self::Timeout { .. } => RuleErrorKind::Timeout,
            Self::Engine { message, .. } if is_conflict(message) => RuleErrorKind::Conflict,
            Self::Engine { .. } => RuleErrorKind::Engine,
            Self::FailUndefined { .. }
            | Self::FailNotBoolean { .. }
            | Self::InvalidMessage { .. }
            | Self::InvalidReason { .. } => RuleErrorKind::Type,
            Self::DuplicateResource { .. } | Self::InvalidResource { .. } => {
                RuleErrorKind::Internal
            }
        }
    }

    /// A one-line description without the message of the Rego engine.
    ///
    /// It contains the kind of the error and the `id` and region of the resource being
    /// evaluated, but no value returned by the rule. `Display` is this description followed by
    /// the message of the Rego engine, if there is one.
    pub fn summary(&self) -> String {
        let at =
            |resource: &ResourceKey| format!(" (resource {} in {})", resource.id, resource.region);
        let at_opt = |resource: &Option<ResourceKey>| resource.as_ref().map(at).unwrap_or_default();
        match self {
            Self::Timeout { limit, resource } => format!(
                "evaluation timed out after {} s{}",
                limit.as_secs_f64(),
                at_opt(resource)
            ),
            Self::Engine { resource, .. } => {
                let what = if self.kind() == RuleErrorKind::Conflict {
                    "rule evaluation conflict"
                } else {
                    "rule evaluation error"
                };
                format!("{what}{}", at_opt(resource))
            }
            Self::FailUndefined { resource } => {
                format!(
                    "fail is undefined; add \"default fail := false\"{}",
                    at(resource)
                )
            }
            Self::FailNotBoolean { found, resource } => {
                format!(
                    "fail must be a boolean, but was {}{}",
                    article(found),
                    at(resource)
                )
            }
            Self::InvalidMessage { reason, resource }
            | Self::InvalidReason { reason, resource } => {
                format!("{reason}{}", at(resource))
            }
            Self::DuplicateResource {
                resource_id,
                region,
            } => format!("duplicate resource {resource_id} in {region}"),
            Self::InvalidResource { index } => {
                format!("resource {index} of the target type has no string id or region")
            }
        }
    }
}

/// Whether a message of the Rego engine reports a conflict: a complete rule or a function that
/// produced two different values.
fn is_conflict(message: &str) -> bool {
    // regorus words conflicts in several ways: `rules should not produce multiple outputs`,
    // `function produced multiple outputs ...` and, for two definitions of a complete rule that
    // give different values, `rule conflicts with the following rule`.
    message.contains("multiple outputs") || message.contains("rule conflicts with")
}

/// A type name with its indefinite article, for example `a string` or `an array`.
pub(crate) fn article(type_name: &str) -> String {
    if type_name == "null" {
        return type_name.to_owned();
    }
    let vowel = type_name.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {type_name}", if vowel { "an" } else { "a" })
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
