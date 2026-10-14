//! Tests of the fixture directories collected by the walk (`LoadMode::RulesWithFixtures`).
//!
//! These use crate-internal accessors, so they live here instead of in `tests/load.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::walk::FixtureEntryKind;
use crate::{LoadMode, LoadOptions, LoadedPolicies, ResourceLimits, YataVersion, load_policies};

fn options(mode: LoadMode) -> LoadOptions {
    LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(1),
        mode,
    }
}

fn policies(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policies")
        .join(sub)
}

fn load(dir: &Path, mode: LoadMode) -> LoadedPolicies {
    load_policies(&[dir.to_path_buf()], &options(mode)).unwrap()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn fixture_directories_are_not_collected_in_rules_mode() {
    let loaded = load(&policies("valid"), LoadMode::Rules);
    assert!(loaded.policy_sets[0].fixture_dirs().is_empty());
}

#[test]
fn valid_set_lists_fixture_directories_when_asked() {
    let loaded = load(&policies("valid"), LoadMode::RulesWithFixtures);
    assert!(loaded.rule_failures.is_empty());
    let dirs = loaded.policy_sets[0].fixture_dirs();
    let mut paths: Vec<&str> = dirs.iter().map(|d| d.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        [
            "account_rule.fixtures",
            "example/nested/deep/deep_rule.fixtures",
            "example/widget_tagged.fixtures",
            "regional/trail_region.fixtures",
        ]
    );
    let widget = dirs
        .iter()
        .find(|d| d.path == "example/widget_tagged.fixtures")
        .unwrap();
    assert_eq!(widget.rule_path, "example/widget_tagged.rego");
    let names: Vec<&str> = widget.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "fail_no_owner.json",
            "pass_error_marker.json",
            "pass_no_widgets.json",
            "pass_owner.json",
        ]
    );
    assert!(
        widget
            .entries
            .iter()
            .all(|e| e.kind == FixtureEntryKind::File)
    );
    assert_eq!(loaded.policy_sets[0].rules().len(), 4);
}

#[test]
fn orphan_fixture_directory_is_not_listed() {
    let loaded = load(&policies("rule_failures"), LoadMode::RulesWithFixtures);
    let dirs = loaded.policy_sets[0].fixture_dirs();
    assert!(dirs.iter().any(|d| d.path == "ok/passing_rule.fixtures"));
    assert!(dirs.iter().all(|d| d.path != "orphan/gone.fixtures"));
    assert_eq!(
        loaded.policy_sets[0].root(),
        policies("rule_failures").as_path()
    );
}

#[test]
fn fixture_directory_entries_are_listed_with_their_kinds() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "yata-policies.json",
        r#"{"format_version": 1, "name": "tmp", "version": "0.1.0", "min_yata_version": "0.1.0"}"#,
    );
    write(
        dir.path(),
        "r.rego",
        "package yata.rules.tmp.r\n\nimport rego.v1\n\nmetadata := {\"title\": \"t\", \"severity\": \"low\", \"description\": \"d\", \"remediation\": \"r\", \"target_type\": \"aws.example.widget\", \"references\": []}\n\ndefault fail := false\n",
    );
    write(dir.path(), "r.fixtures/pass_a.json", "{}");
    write(dir.path(), "r.fixtures/.DS_Store", "");
    write(dir.path(), "r.fixtures/extra/inner.txt", "x");
    write(dir.path(), "r.fixtures/.hidden_dir/inner.txt", "x");
    let loaded = load(dir.path(), LoadMode::RulesWithFixtures);
    let fixture_dir = &loaded.policy_sets[0].fixture_dirs()[0];
    let entries: Vec<(&str, FixtureEntryKind)> = fixture_dir
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.kind))
        .collect();
    assert_eq!(
        entries,
        [
            (".DS_Store", FixtureEntryKind::File),
            ("extra", FixtureEntryKind::Directory),
            ("pass_a.json", FixtureEntryKind::File),
        ]
    );
}
