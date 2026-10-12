//! Loading policy directories: `load_policies` and its option and result types.

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yata_core::PolicySetInfo;

use crate::error::{DirectoryFailure, DirectoryFailureKind, PolicyLoadError, RuleFailure};
use crate::library::load_libraries;
use crate::limits::ResourceLimits;
use crate::manifest;
use crate::rule::{Rule, load_rule};
use crate::version::YataVersion;
use crate::walk::{self, FixtureDir};

/// What to load from a policy directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// Rules and libraries only (`scan`, `collect`, `rules list`, `permissions`).
    Rules,
    /// Rules, libraries and fixture directories (`rules test`).
    RulesWithFixtures,
}

/// Options for [`load_policies`].
#[derive(Debug, Clone)]
pub struct LoadOptions {
    /// Version of the running `yata`, checked against `min_yata_version` of each manifest.
    pub yata_version: YataVersion,
    /// Resource limits for the walk and for file reads.
    pub limits: ResourceLimits,
    /// Time limit of one evaluation, also used for the load-time check of `metadata` and `fail`.
    pub policy_timeout: Duration,
    /// What to load.
    pub mode: LoadMode,
}

/// The result of a successful [`load_policies`] call.
#[derive(Debug)]
pub struct LoadedPolicies {
    /// Loaded policy sets in the order of the given directories.
    pub policy_sets: Vec<PolicySet>,
    /// Rule-level failures. Exit code 2 outside `rules test`, 1 in `rules test`.
    pub rule_failures: Vec<RuleFailure>,
}

impl LoadedPolicies {
    /// Successfully loaded rules, in directory order then rule ID order.
    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.policy_sets.iter().flat_map(|set| set.rules.iter())
    }
}

/// A loaded policy directory.
#[derive(Debug)]
pub struct PolicySet {
    root: PathBuf,
    info: PolicySetInfo,
    min_yata_version: String,
    rules: Vec<Rule>,
    fixture_dirs: Vec<FixtureDir>,
}

impl PolicySet {
    /// The policy directory as given by the caller.
    #[allow(dead_code)] // Used by the rule tests (stage P3).
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Name, version and format version from the manifest.
    pub fn info(&self) -> &PolicySetInfo {
        &self.info
    }

    /// `min_yata_version` from the manifest, as written.
    pub fn min_yata_version(&self) -> &str {
        &self.min_yata_version
    }

    /// The rules that loaded successfully, in rule ID order.
    #[allow(dead_code)] // Used by the rule tests (stage P3).
    pub(crate) fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Fixture directories found next to rule files. Empty unless the mode was
    /// [`LoadMode::RulesWithFixtures`].
    #[allow(dead_code)] // Used by the rule tests (stage P3).
    pub(crate) fn fixture_dirs(&self) -> &[FixtureDir] {
        &self.fixture_dirs
    }
}

/// What loading one directory produced.
struct DirOutcome {
    /// Manifest name, when the manifest could be read (even if the directory later failed).
    name: Option<String>,
    set: Option<PolicySet>,
    directory_failures: Vec<DirectoryFailure>,
    rule_failures: Vec<RuleFailure>,
}

/// Loads and validates the policy directories, in the given order.
///
/// A directory-level failure stops the processing of that directory only; the other directories
/// are still processed so that one run reports as many problems as possible. If any directory
/// has a directory-level failure, the result is `Err` and carries every failure found. Otherwise
/// the result is `Ok` and rule-level failures are in [`LoadedPolicies::rule_failures`].
pub fn load_policies(
    dirs: &[PathBuf],
    options: &LoadOptions,
) -> Result<LoadedPolicies, PolicyLoadError> {
    let mut sets: Vec<PolicySet> = Vec::new();
    let mut directory_failures = Vec::new();
    let mut rule_failures = Vec::new();
    let mut duplicate_failures = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    for dir in dirs {
        let outcome = load_dir(dir, options);
        directory_failures.extend(outcome.directory_failures);
        rule_failures.extend(outcome.rule_failures);
        // The name counts even when the directory failed, as long as the manifest was read.
        let duplicate = match &outcome.name {
            Some(name) => !seen_names.insert(name.clone()),
            None => false,
        };
        if duplicate {
            duplicate_failures.push(DirectoryFailure {
                policy_dir: dir.clone(),
                path: None,
                kind: DirectoryFailureKind::DuplicateName,
                message: format!(
                    "manifest name {:?} is already used by an earlier policy directory",
                    outcome.name.as_deref().unwrap_or_default()
                ),
            });
        } else if let Some(set) = outcome.set {
            sets.push(set);
        }
    }
    directory_failures.extend(duplicate_failures);

    if directory_failures.is_empty() {
        Ok(LoadedPolicies {
            policy_sets: sets,
            rule_failures,
        })
    } else {
        Err(PolicyLoadError {
            directory_failures,
            rule_failures,
        })
    }
}

fn load_dir(dir: &Path, options: &LoadOptions) -> DirOutcome {
    let dir_failure = |path: Option<String>, kind, message: String| DirOutcome {
        name: None,
        set: None,
        directory_failures: vec![DirectoryFailure {
            policy_dir: dir.to_path_buf(),
            path,
            kind,
            message,
        }],
        rule_failures: Vec::new(),
    };

    // 1. The root: it may be a link, which is followed.
    match fs::metadata(dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return dir_failure(
                None,
                DirectoryFailureKind::NotADirectory,
                "policy directory is not a directory".to_string(),
            );
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return dir_failure(
                None,
                DirectoryFailureKind::NotFound,
                "policy directory does not exist".to_string(),
            );
        }
        Err(e) => {
            return dir_failure(
                None,
                DirectoryFailureKind::Unreadable,
                format!("cannot read policy directory: {e}"),
            );
        }
    }

    // 2. The manifest.
    let manifest =
        match manifest::load(dir, options.yata_version, options.limits.max_manifest_bytes) {
            Ok(m) => m,
            Err((kind, message)) => {
                return dir_failure(
                    Some(manifest::MANIFEST_FILE_NAME.to_string()),
                    kind,
                    message,
                );
            }
        };
    let policy_set = manifest.name.as_str();
    tracing::debug!(dir = %dir.display(), policy_set, "loading policy directory");

    let mut out = DirOutcome {
        name: Some(manifest.name.clone()),
        set: None,
        directory_failures: Vec::new(),
        rule_failures: Vec::new(),
    };
    let rule_failure = |kind, rule_id, path: String, message: String| RuleFailure {
        policy_set: policy_set.to_string(),
        rule_id,
        path,
        kind,
        message,
        details: None,
    };
    let fail_dir = |out: &mut DirOutcome, path: Option<String>, kind, message: String| {
        out.directory_failures.push(DirectoryFailure {
            policy_dir: dir.to_path_buf(),
            path: path.filter(|p| !p.is_empty()),
            kind,
            message,
        });
    };

    // 3. The walk.
    let walked = walk::walk(
        dir,
        policy_set,
        &options.limits,
        options.mode == LoadMode::RulesWithFixtures,
    );
    out.rule_failures.extend(
        walked
            .rule_failures
            .iter()
            .map(|f| rule_failure(f.kind, f.rule_id.clone(), f.path.clone(), f.message.clone())),
    );
    if let Some(f) = walked.failure {
        fail_dir(&mut out, f.path, f.kind, f.message);
        finish_rule_failures(&mut out);
        return out;
    }

    // 4. The libraries.
    let base = match load_libraries(
        &walked.libraries,
        policy_set,
        options.limits.max_rego_bytes,
        options.policy_timeout,
    ) {
        Ok(engine) => engine,
        Err(f) => {
            fail_dir(&mut out, f.path, f.kind, f.message);
            finish_rule_failures(&mut out);
            return out;
        }
    };

    // 5. The rules.
    let mut rules = Vec::new();
    for walked_rule in &walked.rules {
        match load_rule(
            &base,
            &walked_rule.path,
            &walked_rule.id,
            &walked_rule.rel_path,
            policy_set,
            options.limits.max_rego_bytes,
            options.policy_timeout,
        ) {
            Ok(rule) => rules.push(rule),
            Err(f) => out.rule_failures.push(rule_failure(
                f.kind,
                Some(walked_rule.id.clone()),
                walked_rule.rel_path.clone(),
                f.message,
            )),
        }
    }

    // 6. The count.
    if walked.rule_file_count == 0 {
        fail_dir(
            &mut out,
            None,
            DirectoryFailureKind::NoRules,
            "no rule files (.rego outside lib/) found".to_string(),
        );
        finish_rule_failures(&mut out);
        return out;
    }

    finish_rule_failures(&mut out);
    out.set = Some(PolicySet {
        root: dir.to_path_buf(),
        info: PolicySetInfo {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            format_version: manifest.format_version,
        },
        min_yata_version: manifest.min_yata_version.clone(),
        rules,
        fixture_dirs: walked.fixture_dirs,
    });
    out
}

/// Orders the rule-level failures of one directory by path (stable, so a rule's failures keep
/// the order in which they were found).
fn finish_rule_failures(out: &mut DirOutcome) {
    out.rule_failures.sort_by(|a, b| a.path.cmp(&b.path));
}
