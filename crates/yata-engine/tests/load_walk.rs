//! Loading: walk limits and ignored entries (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode};

// ------------------------------------------------------------------ walk limits

#[test]
fn file_count_limit() {
    let dir = temp_policy_dir();
    write(dir.path(), "notes/a.txt", "a");
    write(dir.path(), "notes/b.txt", "b");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 3; // r.rego + a.txt + b.txt (the manifest is not counted)
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.limits.max_files = 2;
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::TooManyFiles]);
}

#[test]
fn directory_count_limit_ignores_dot_directories() {
    let dir = temp_policy_dir();
    write(dir.path(), "a/x.txt", "x");
    write(dir.path(), "b/x.txt", "x");
    write(dir.path(), ".git/x.txt", "x");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_directories = 2;
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.limits.max_directories = 1;
    assert_eq!(
        dir_kinds(load_one(dir.path(), &opts)),
        [D::TooManyDirectories]
    );
}

#[test]
fn files_in_dot_directories_are_not_counted() {
    let dir = temp_policy_dir();
    write(dir.path(), ".git/a", "x");
    write(dir.path(), ".git/b", "x");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 1;
    assert!(load_one(dir.path(), &opts).is_ok());
}

#[test]
fn depth_limit() {
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_depth = 3;

    // A file at depth 3 is fine.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    assert!(load_one(dir.path(), &opts).is_ok());

    // A directory at depth 3 with something inside is too deep.
    write(dir.path(), "a/b/c/x.txt", "x");
    let f = dir_failure(load_one(dir.path(), &opts));
    assert_eq!(f.kind, D::DepthExceeded);
    assert_eq!(f.path.as_deref(), Some("a/b/c"));

    // A name starting with a dot counts as something inside.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    write(dir.path(), "a/b/c/.keep", "");
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::DepthExceeded]);

    // An empty directory at depth 3 is not a failure.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    assert!(load_one(dir.path(), &opts).is_ok());

    // A dot directory at depth 3 is skipped without looking inside.
    write(dir.path(), "a/b/.cache/x", "x");
    assert!(load_one(dir.path(), &opts).is_ok());
}

#[test]
fn fixtures_at_the_depth_limit_depend_on_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    write(dir.path(), "a/b/r.fixtures/pass_x.json", "{}");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_depth = 3;
    // The fixture directory is at depth 3 and its content at depth 4.
    let loaded = load_one(dir.path(), &opts).unwrap();
    assert_eq!(loaded.rules().count(), 1);
    opts.mode = LoadMode::RulesWithFixtures;
    let f = dir_failure(load_one(dir.path(), &opts));
    assert_eq!(f.kind, D::DepthExceeded);
    assert_eq!(f.path.as_deref(), Some("a/b/r.fixtures"));
}

#[test]
fn fixtures_directory_contents_are_counted_only_with_fixtures() {
    let dir = temp_policy_dir();
    write(dir.path(), "r.fixtures/pass_a.json", "{}");
    write(dir.path(), "r.fixtures/fail_a.json", "{}");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 1;
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.mode = LoadMode::RulesWithFixtures;
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::TooManyFiles]);
}

#[test]
fn non_rego_files_and_dot_directories_are_ignored() {
    let dir = temp_policy_dir();
    write(dir.path(), "notes.txt", "x");
    write(dir.path(), ".git/broken.rego", "package (");
    write(dir.path(), "docs/readme.md", "x");
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    assert_eq!(loaded.rules().count(), 1);
}

#[test]
fn rules_in_lib_are_libraries_and_need_no_valid_names() {
    let dir = temp_policy_dir();
    // Names below `lib/` are not checked.
    write(
        dir.path(),
        "lib/Odd-Name/1.rego",
        "package yata.lib.tmp.odd\n\nf(x) := x\n",
    );
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rules().count(), 1);
    assert!(loaded.rule_failures.is_empty());
}
