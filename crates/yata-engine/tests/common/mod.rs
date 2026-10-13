//! Helpers shared by the policy loading tests (`tests/load_*.rs`).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yata_engine::{
    DirectoryFailureKind as D, LoadMode, LoadOptions, LoadedPolicies, PolicyLoadError,
    ResourceLimits, RuleFailure, RuleFailureKind as R, YataVersion, load_policies,
};
pub fn policies(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policies")
        .join(sub)
}

pub fn options(mode: LoadMode) -> LoadOptions {
    LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(30),
        mode,
    }
}

pub fn load(dirs: &[PathBuf], options: &LoadOptions) -> Result<LoadedPolicies, PolicyLoadError> {
    load_policies(dirs, options)
}

pub fn load_one(dir: &Path, options: &LoadOptions) -> Result<LoadedPolicies, PolicyLoadError> {
    load(&[dir.to_path_buf()], options)
}

/// The kinds of the directory-level failures of an `Err` result.
pub fn dir_kinds(result: Result<LoadedPolicies, PolicyLoadError>) -> Vec<D> {
    match result {
        Ok(loaded) => panic!("expected a directory-level failure, got {loaded:?}"),
        Err(e) => e.directory_failures.iter().map(|f| f.kind).collect(),
    }
}

pub fn dir_failure(
    result: Result<LoadedPolicies, PolicyLoadError>,
) -> yata_engine::DirectoryFailure {
    match result {
        Ok(loaded) => panic!("expected a directory-level failure, got {loaded:?}"),
        Err(mut e) => {
            assert_eq!(e.directory_failures.len(), 1, "{e:?}");
            e.directory_failures.remove(0)
        }
    }
}

pub fn rule_failure_pairs(failures: &[RuleFailure]) -> Vec<(String, R)> {
    failures.iter().map(|f| (f.path.clone(), f.kind)).collect()
}

pub fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

pub const MANIFEST: &str =
    r#"{"format_version": 1, "name": "tmp", "version": "0.1.0", "min_yata_version": "0.1.0"}"#;

/// A rule with valid metadata. `package` is the part after `yata.rules.tmp.`.
pub fn rule_source(package: &str) -> String {
    format!(
        r#"package yata.rules.tmp.{package}

import rego.v1

metadata := {{
	"title": "Temp rule",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}}

fail contains 1 if {{ false }}
"#
    )
}

/// A temporary policy directory with a manifest and one rule `tmp.r` in `r.rego`.
pub fn temp_policy_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "r.rego", &rule_source("r"));
    dir
}

/// Creates a symbolic link to a file. Returns false (after printing the reason to stderr) when it
/// cannot be made.
///
/// On Windows, creating symbolic links needs Developer Mode or an elevated prompt (otherwise
/// os error 1314); run the tests that way to exercise the link tests. They are skipped
/// otherwise, and the junction tests still run.
pub fn symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(target, link);
    report_skip(result)
}

/// Creates a symbolic link to a directory. See [`symlink_file`].
pub fn symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    report_skip(result)
}

pub fn report_skip(result: std::io::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping: cannot create a symbolic link here: {e}");
            false
        }
    }
}
