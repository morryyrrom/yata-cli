//! Loading: the policy root and the manifest (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode, YataVersion};

// ------------------------------------------------------------------ root and manifest

#[test]
fn missing_and_non_directory_roots() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    assert_eq!(
        dir_kinds(load_one(&missing, &options(LoadMode::Rules))),
        [D::NotFound]
    );
    let file = dir.path().join("file");
    fs::write(&file, "x").unwrap();
    assert_eq!(
        dir_kinds(load_one(&file, &options(LoadMode::Rules))),
        [D::NotADirectory]
    );
}

#[test]
fn manifest_over_the_size_limit_is_rejected() {
    let dir = temp_policy_dir();
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_manifest_bytes = 10;
    assert_eq!(
        dir_kinds(load_one(dir.path(), &opts)),
        [D::ManifestTooLarge]
    );
}

#[test]
fn yata_version_is_compared_with_min_yata_version() {
    let dir = temp_policy_dir();
    let mut opts = options(LoadMode::Rules);
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.yata_version = YataVersion::new(0, 0, 9);
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::YataTooOld]);
    opts.yata_version = YataVersion::from_package_version("0.1.0-alpha.1+build").unwrap();
    assert!(load_one(dir.path(), &opts).is_ok());
}

#[test]
fn manifest_that_is_a_directory_counts_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("yata-policies.json")).unwrap();
    write(dir.path(), "r.rego", &rule_source("r"));
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::ManifestMissing]
    );
}
