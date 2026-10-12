//! A loaded rule and its load-time validation.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use yata_core::RuleMetadata;

use crate::engine_config::{RegoEngine, RegoFailure, RegoValue};
use crate::error::RuleFailureKind;
use crate::files::{ReadError, read_text_limited};
use crate::metadata;

/// A rule that was loaded and validated.
///
/// It owns a Rego engine with the libraries of its policy set and its own module loaded.
/// Evaluation clones the engine, so a `Rule` is never modified after loading.
pub struct Rule {
    id: String,
    policy_set: String,
    path: String,
    metadata: RuleMetadata,
    engine: RegoEngine,
}

impl Rule {
    /// The rule ID, for example `example_policies.s3.bucket_public_access_block`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The name of the policy set this rule belongs to.
    pub fn policy_set(&self) -> &str {
        &self.policy_set
    }

    /// Path of the rule file relative to the policy directory, `/`-separated.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The validated metadata.
    pub fn metadata(&self) -> &RuleMetadata {
        &self.metadata
    }

    /// The Rego package of the rule, for example `data.yata.rules.<id>`.
    pub(crate) fn package(&self) -> String {
        package_of(&self.id)
    }

    /// The loaded engine. Clone it before evaluating.
    pub(crate) fn engine(&self) -> &RegoEngine {
        &self.engine
    }
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rule")
            .field("id", &self.id)
            .field("policy_set", &self.policy_set)
            .field("path", &self.path)
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

/// Why a rule failed to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleLoadFailure {
    pub(crate) kind: RuleFailureKind,
    pub(crate) message: String,
}

fn failed(kind: RuleFailureKind, message: impl Into<String>) -> RuleLoadFailure {
    RuleLoadFailure {
        kind,
        message: message.into(),
    }
}

/// The Rego package of a rule: `data.yata.rules.<rule ID>`.
pub(crate) fn package_of(rule_id: &str) -> String {
    format!("data.yata.rules.{rule_id}")
}

/// Loads and validates one rule.
pub(crate) fn load_rule(
    base: &RegoEngine,
    path: &Path,
    id: &str,
    rel_path: &str,
    policy_set: &str,
    max_rego_bytes: u64,
    policy_timeout: Duration,
) -> Result<Rule, RuleLoadFailure> {
    let source = match read_text_limited(path, max_rego_bytes) {
        Ok(s) => s,
        Err(ReadError::TooLarge) => {
            return Err(failed(
                RuleFailureKind::FileTooLarge,
                format!("rule file is larger than {max_rego_bytes} bytes"),
            ));
        }
        Err(ReadError::NotUtf8) => {
            return Err(failed(
                RuleFailureKind::RegoError,
                "rule file is not valid UTF-8",
            ));
        }
        Err(ReadError::Io(e)) => {
            return Err(failed(
                RuleFailureKind::Unreadable,
                format!("cannot read rule file: {e}"),
            ));
        }
    };

    let mut engine = base.clone();
    engine.set_label(id);
    let package = engine
        .add_policy(rel_path, &source)
        .map_err(|f| failed(RuleFailureKind::RegoError, f.message(policy_timeout)))?;
    let expected = package_of(id);
    if package != expected {
        return Err(failed(
            RuleFailureKind::PackageMismatch,
            format!(
                "package must be {}, but is {}",
                expected.trim_start_matches("data."),
                package.trim_start_matches("data.")
            ),
        ));
    }

    let metadata_value = match engine.eval_rule(&format!("{package}.metadata")) {
        Ok(RegoValue::Undefined) => {
            return Err(failed(
                RuleFailureKind::MetadataInvalid,
                "metadata is not defined (it must not depend on input)",
            ));
        }
        Ok(RegoValue::Set(_)) => {
            return Err(failed(
                RuleFailureKind::MetadataInvalid,
                "metadata must be an object",
            ));
        }
        Ok(RegoValue::Json(v)) => v,
        Err(f) => return Err(rego_error(&f, policy_timeout, "metadata")),
    };
    let metadata = metadata::validate(&metadata_value)
        .map_err(|m| failed(RuleFailureKind::MetadataInvalid, m))?;

    match engine.eval_rule(&format!("{package}.fail")) {
        Ok(RegoValue::Set(_)) => {}
        Ok(RegoValue::Undefined) => {
            return Err(failed(RuleFailureKind::FailMissing, "fail is not defined"));
        }
        Ok(RegoValue::Json(_)) => {
            return Err(failed(RuleFailureKind::FailMissing, "fail must be a set"));
        }
        Err(f) => return Err(rego_error(&f, policy_timeout, "fail")),
    }

    Ok(Rule {
        id: id.to_string(),
        policy_set: policy_set.to_string(),
        path: rel_path.to_string(),
        metadata,
        engine,
    })
}

fn rego_error(failure: &RegoFailure, timeout: Duration, what: &str) -> RuleLoadFailure {
    let detail = failure.message(timeout);
    failed(
        RuleFailureKind::RegoError,
        format!("cannot evaluate {what}: {detail}"),
    )
}
