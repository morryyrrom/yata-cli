//! Loading: symbolic links, junctions and fixture directories (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode, RuleFailureKind as R};

// ------------------------------------------------------------------ links

#[test]
fn symlinked_rule_file_is_a_rule_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "x.rego", &rule_source("linked"));
    if !symlink_file(
        &outside.path().join("x.rego"),
        &dir.path().join("linked.rego"),
    ) {
        return;
    }
    for mode in [LoadMode::Rules, LoadMode::RulesWithFixtures] {
        let loaded = load_one(dir.path(), &options(mode)).unwrap();
        assert_eq!(
            rule_failure_pairs(&loaded.rule_failures),
            [("linked.rego".to_string(), R::Symlink)]
        );
        assert_eq!(
            loaded.rule_failures[0].rule_id.as_deref(),
            Some("tmp.linked")
        );
        assert_eq!(loaded.rules().count(), 1);
    }
}

#[test]
fn symlinked_rule_file_alone_is_not_no_rules() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "x.rego", &rule_source("linked"));
    if !symlink_file(
        &outside.path().join("x.rego"),
        &dir.path().join("linked.rego"),
    ) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rule_failures.len(), 1);
}

#[test]
fn symlinked_directory_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "inner/x.rego", &rule_source("inner.x"));
    if !symlink_dir(&outside.path().join("inner"), &dir.path().join("linked")) {
        return;
    }
    let f = dir_failure(load_one(dir.path(), &options(LoadMode::Rules)));
    assert_eq!(f.kind, D::Symlink);
    assert_eq!(f.path.as_deref(), Some("linked"));
}

#[test]
fn symlinked_non_rego_file_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "notes.txt", "x");
    if !symlink_file(
        &outside.path().join("notes.txt"),
        &dir.path().join("notes.txt"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::Symlink]
    );
}

#[test]
fn symlinked_library_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "h.rego", "package yata.lib.tmp.h\n");
    fs::create_dir(dir.path().join("lib")).unwrap();
    if !symlink_file(
        &outside.path().join("h.rego"),
        &dir.path().join("lib/h.rego"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::Symlink]
    );
}

#[test]
fn symlinked_manifest_counts_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "r.rego", &rule_source("r"));
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "m.json", MANIFEST);
    if !symlink_file(
        &outside.path().join("m.json"),
        &dir.path().join("yata-policies.json"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::ManifestMissing]
    );
}

#[test]
fn symlinked_root_is_followed() {
    let real = temp_policy_dir();
    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("policies");
    if !symlink_dir(real.path(), &link) {
        return;
    }
    let loaded = load_one(&link, &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rules().count(), 1);
}

#[test]
fn symlink_inside_a_fixture_directory_depends_on_the_mode() {
    let dir = temp_policy_dir();
    write(dir.path(), "r.fixtures/pass_a.json", "{}");
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "f.json", "{}");
    if !symlink_file(
        &outside.path().join("f.json"),
        &dir.path().join("r.fixtures/fail_b.json"),
    ) {
        return;
    }
    // Not read, so not a failure.
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    // With fixtures it is a failure of the rule.
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("r.fixtures/fail_b.json".to_string(), R::Symlink)]
    );
    assert_eq!(loaded.rule_failures[0].rule_id.as_deref(), Some("tmp.r"));
}

#[test]
fn symlinked_fixture_directory_depends_on_the_mode() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "fx/pass_a.json", "{}");
    if !symlink_dir(&outside.path().join("fx"), &dir.path().join("r.fixtures")) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("r.fixtures".to_string(), R::Symlink)]
    );
}

#[test]
fn symlinked_fixture_directory_without_a_rule_is_an_orphan() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "fx/pass_a.json", "{}");
    if !symlink_dir(
        &outside.path().join("fx"),
        &dir.path().join("gone.fixtures"),
    ) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("gone.fixtures".to_string(), R::OrphanFixtures)]
    );
}

#[cfg(windows)]
mod junction {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    /// Creates a junction with `cmd /C mklink /J`.
    fn make(target: &Path, link: &Path) -> bool {
        let output = Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output();
        match output {
            Ok(o) if o.status.success() => true,
            other => {
                eprintln!("skipping: cannot create a junction: {other:?}");
                false
            }
        }
    }

    #[test]
    fn junction_directory_is_a_directory_level_failure() {
        let dir = temp_policy_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "inner/x.rego", &rule_source("inner.x"));
        if !make(&outside.path().join("inner"), &dir.path().join("linked")) {
            return;
        }
        let f = dir_failure(load_one(dir.path(), &options(LoadMode::Rules)));
        assert_eq!(f.kind, D::Symlink);
        assert_eq!(f.path.as_deref(), Some("linked"));
    }

    #[test]
    fn junction_as_fixture_directory_depends_on_the_mode() {
        let dir = temp_policy_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "fx/pass_a.json", "{}");
        if !make(&outside.path().join("fx"), &dir.path().join("r.fixtures")) {
            return;
        }
        let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
        assert!(loaded.rule_failures.is_empty());
        let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
        assert_eq!(
            rule_failure_pairs(&loaded.rule_failures),
            [("r.fixtures".to_string(), R::Symlink)]
        );
    }

    #[test]
    fn junction_root_is_followed() {
        let real = temp_policy_dir();
        let parent = tempfile::tempdir().unwrap();
        let link = parent.path().join("policies");
        if !make(real.path(), &link) {
            return;
        }
        let loaded = load_one(&link, &options(LoadMode::Rules)).unwrap();
        assert_eq!(loaded.rules().count(), 1);
    }
}
