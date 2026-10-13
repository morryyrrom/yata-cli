//! Loading: unreadable files and non-UTF-8 names (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

mod common;

use std::fs;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode, RuleFailureKind as R};

// ------------------------------------------------------------------ unreadable (Unix)

#[cfg(unix)]
#[test]
fn unreadable_rule_file_and_directory() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_policy_dir();
    write(dir.path(), "secret.rego", &rule_source("secret"));
    write(dir.path(), "closed/inner.txt", "x");
    let rule = dir.path().join("secret.rego");
    let closed = dir.path().join("closed");
    fs::set_permissions(&rule, fs::Permissions::from_mode(0o000)).unwrap();
    let readable_anyway = fs::read(&rule).is_ok();
    if readable_anyway {
        eprintln!("skipping: permissions are not enforced (running as root?)");
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("secret.rego".to_string(), R::Unreadable)]
    );
    fs::set_permissions(&rule, fs::Permissions::from_mode(0o644)).unwrap();

    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let result = load_one(dir.path(), &options(LoadMode::Rules));
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    let f = dir_failure(result);
    assert_eq!(f.kind, D::Unreadable);
    assert_eq!(f.path.as_deref(), Some("closed"));
}

#[cfg(unix)]
#[test]
fn non_utf8_names() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = temp_policy_dir();
    // A directory whose name is not UTF-8: each `.rego` inside is `invalid_name`.
    let bad_dir = dir.path().join(OsStr::from_bytes(b"bad\xff"));
    fs::create_dir(&bad_dir).unwrap();
    fs::write(bad_dir.join("x.rego"), rule_source("x")).unwrap();
    // A file whose name is not UTF-8 is ignored.
    fs::write(dir.path().join(OsStr::from_bytes(b"odd\xff.rego")), "x").unwrap();
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rule_failures.len(), 1);
    assert_eq!(loaded.rule_failures[0].kind, R::InvalidName);
    assert_eq!(loaded.rules().count(), 1);
}
#[cfg(unix)]
fn unreadable_file_test(rel: &str, with_library: bool) -> Vec<D> {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_policy_dir();
    if with_library {
        write(
            dir.path(),
            "lib/h.rego",
            "package yata.lib.tmp.h\n\nf(x) := x\n",
        );
    }
    let path = dir.path().join(rel);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        eprintln!("skipping: permissions are not enforced (running as root?)");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        return vec![D::Unreadable];
    }
    let result = load_one(dir.path(), &options(LoadMode::Rules));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    dir_kinds(result)
}

#[cfg(unix)]
#[test]
fn unreadable_manifest_is_a_directory_level_failure() {
    assert_eq!(
        unreadable_file_test("yata-policies.json", false),
        [D::Unreadable]
    );
}

#[cfg(unix)]
#[test]
fn unreadable_library_is_a_directory_level_failure() {
    assert_eq!(unreadable_file_test("lib/h.rego", true), [D::Unreadable]);
}
