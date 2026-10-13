//! Loading: file size limits (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode, RuleFailureKind as R};

// ------------------------------------------------------------------ file sizes

#[test]
fn rule_and_library_over_the_size_limit() {
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_rego_bytes = 100;

    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "small.rego", "package yata.rules.tmp.small\n");
    write(dir.path(), "big.rego", &rule_source("big"));
    let loaded = load_one(dir.path(), &opts).unwrap();
    let pairs = rule_failure_pairs(&loaded.rule_failures);
    assert!(
        pairs.contains(&("big.rego".to_string(), R::FileTooLarge)),
        "{pairs:?}"
    );

    let dir = temp_policy_dir();
    write(
        dir.path(),
        "lib/big.rego",
        &format!("package yata.lib.tmp.big\n\n# {}\n", "x".repeat(200)),
    );
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_rego_bytes = 150;
    // The rule is below 150 bytes? It is not, but the library is read first.
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::LibraryInvalid]);
}

#[test]
fn non_utf8_rule_is_a_rego_error_and_non_utf8_library_is_invalid() {
    let dir = temp_policy_dir();
    fs::write(dir.path().join("bad.rego"), [0xff, 0xfe, 0xfd]).unwrap();
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("bad.rego".to_string(), R::RegoError)]
    );

    fs::create_dir(dir.path().join("lib")).unwrap();
    fs::write(dir.path().join("lib/bad.rego"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::LibraryInvalid]
    );
}
