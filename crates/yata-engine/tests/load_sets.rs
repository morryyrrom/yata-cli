//! Loading the committed policy sets and their failures (stage P1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use yata_engine::{DirectoryFailureKind as D, LoadMode, RuleFailure, RuleFailureKind as R};

// ------------------------------------------------------------------ committed sets

#[test]
fn valid_set_loads_with_rules_in_id_order() {
    let loaded = load_one(&policies("valid"), &options(LoadMode::Rules)).unwrap();
    assert!(
        loaded.rule_failures.is_empty(),
        "{:?}",
        loaded.rule_failures
    );
    assert_eq!(loaded.policy_sets.len(), 1);
    let set = &loaded.policy_sets[0];
    assert_eq!(set.info().name, "test_policies");
    assert_eq!(set.info().version, "0.1.0");
    assert_eq!(set.info().format_version, 1);
    assert_eq!(set.min_yata_version(), "0.1.0");
    let ids: Vec<&str> = loaded.rules().map(|r| r.id()).collect();
    assert_eq!(
        ids,
        [
            "test_policies.account_rule",
            "test_policies.example.nested.deep.deep_rule",
            "test_policies.example.widget_tagged",
            "test_policies.regional.trail_region",
        ]
    );
    // `README.md` and `.hidden/broken.rego` were ignored, and the library rule loaded.
    let widget = loaded
        .rules()
        .find(|r| r.id() == "test_policies.example.widget_tagged")
        .unwrap();
    assert_eq!(widget.path(), "example/widget_tagged.rego");
    assert_eq!(widget.policy_set(), "test_policies");
    assert_eq!(widget.metadata().target_type, "aws.example.widget");
    assert_eq!(widget.metadata().related_types, ["aws.example.gadget"]);
    assert_eq!(widget.metadata().references.len(), 1);
}

#[test]
fn directory_level_failures_have_the_designed_kinds() {
    let cases: &[(&str, D)] = &[
        ("manifest_missing", D::ManifestMissing),
        ("manifest_invalid_json", D::ManifestInvalid),
        ("manifest_missing_key", D::ManifestInvalid),
        ("manifest_name_reserved", D::ManifestInvalid),
        ("manifest_name_digit", D::ManifestInvalid),
        ("manifest_version_format", D::ManifestInvalid),
        ("manifest_format_version", D::UnsupportedFormatVersion),
        ("manifest_min_version_too_new", D::YataTooOld),
        ("library_syntax", D::LibraryInvalid),
        ("library_package", D::LibraryInvalid),
        ("library_has_metadata", D::LibraryInvalid),
        ("library_has_fail", D::LibraryInvalid),
        ("library_undefined_function", D::LibraryInvalid),
        ("no_rules", D::NoRules),
    ];
    for (case, kind) in cases {
        let dir = policies(&format!("dir_failures/{case}"));
        for mode in [LoadMode::Rules, LoadMode::RulesWithFixtures] {
            let result = load_one(&dir, &options(mode));
            assert_eq!(dir_kinds(result), [*kind], "case {case}");
        }
    }
}

#[test]
fn library_failure_message_names_the_file() {
    let f = dir_failure(load_one(
        &policies("dir_failures/library_undefined_function"),
        &options(LoadMode::Rules),
    ));
    assert_eq!(f.path.as_deref(), None);
    assert!(f.message.contains("lib/bad.rego"), "{}", f.message);
    let f = dir_failure(load_one(
        &policies("dir_failures/library_has_metadata"),
        &options(LoadMode::Rules),
    ));
    assert_eq!(f.path.as_deref(), Some("lib/bad.rego"));
}

#[test]
fn duplicate_names_fail_the_later_directory() {
    let a = policies("dir_failures/duplicate_name_a");
    let b = policies("dir_failures/duplicate_name_b");
    let err = load(&[a.clone(), b.clone()], &options(LoadMode::Rules)).unwrap_err();
    assert_eq!(err.directory_failures.len(), 1);
    assert_eq!(err.directory_failures[0].kind, D::DuplicateName);
    assert_eq!(err.directory_failures[0].policy_dir, b);
    // Each directory alone is fine.
    assert!(load_one(&a, &options(LoadMode::Rules)).is_ok());
    assert!(load_one(&b, &options(LoadMode::Rules)).is_ok());
}

#[test]
fn a_failing_directory_does_not_stop_the_others() {
    // A directory that loads but has a failing rule (named `tmp`, so no name clash).
    let with_failure = temp_policy_dir();
    write(with_failure.path(), "Upper/r.rego", &rule_source("r"));
    let dirs = [
        policies("dir_failures/manifest_missing"),
        policies("valid"),
        policies("dir_failures/library_syntax"),
        with_failure.path().to_path_buf(),
        PathBuf::from("this/does/not/exist"),
    ];
    let err = load(&dirs, &options(LoadMode::Rules)).unwrap_err();
    let kinds: Vec<D> = err.directory_failures.iter().map(|f| f.kind).collect();
    // library_syntax also uses the name test_policies, so it clashes with `valid` (now counted).
    assert_eq!(
        kinds,
        [
            D::ManifestMissing,
            D::LibraryInvalid,
            D::NotFound,
            D::DuplicateName
        ]
    );
    // Rule-level failures of the directory that loaded are still reported.
    assert!(err.rule_failures.iter().any(|f| f.kind == R::InvalidName));
    // The error message counts the directory-level failures.
    assert!(err.to_string().contains('4'));
}

#[test]
fn rule_level_failures_are_all_collected_and_other_rules_load() {
    // A short time limit makes `slow_metadata` time out while loading.
    let mut opts = options(LoadMode::Rules);
    opts.policy_timeout = Duration::from_secs(1);
    let loaded = load_one(&policies("rule_failures"), &opts).unwrap();

    let mut pairs = rule_failure_pairs(&loaded.rule_failures);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected: Vec<(String, R)> = [
        ("Bad-Dir/rule.rego", R::InvalidName),
        ("fail_missing/complete_rule.rego", R::FailMissing),
        ("fail_missing/no_fail.rego", R::FailMissing),
        ("invalid_name/1digit.rego", R::InvalidName),
        ("invalid_name/as.rego", R::InvalidName),
        ("metadata/bad_severity.rego", R::MetadataInvalid),
        ("metadata/bad_type.rego", R::MetadataInvalid),
        ("metadata/input_dependent.rego", R::MetadataInvalid),
        ("metadata/missing_key.rego", R::MetadataInvalid),
        ("metadata/related_has_target.rego", R::MetadataInvalid),
        ("metadata/related_other_provider.rego", R::MetadataInvalid),
        ("package_mismatch/wrong_package.rego", R::PackageMismatch),
        ("rego_error/slow_metadata.rego", R::RegoError),
        ("rego_error/syntax.rego", R::RegoError),
    ]
    .into_iter()
    .map(|(p, k)| (p.to_string(), k))
    .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(pairs, expected);

    // The passing rule is loaded, and so are the rules that fail only when their fixtures are
    // tested (the cases of stage P3), in rule ID order.
    let ids: Vec<&str> = loaded.rules().map(|r| r.id()).collect();
    assert_eq!(
        ids,
        [
            "test_policies.expected.dup_object",
            "test_policies.expected.dup_string",
            "test_policies.expected.object_missing_region",
            "test_policies.expected.region_not_string",
            "test_policies.expected.string_and_object",
            "test_policies.expected.unknown_id",
            "test_policies.expected.unknown_id_region",
            "test_policies.fail_element.bad_path",
            "test_policies.fail_element.missing_message",
            "test_policies.fail_element.region_not_string",
            "test_policies.fail_element.unknown_id",
            "test_policies.fail_element.unknown_id_region",
            "test_policies.fixture_format.bad_resource",
            "test_policies.fixture_format.fail_without_fail",
            "test_policies.fixture_format.input_dup_id_region",
            "test_policies.fixture_format.missing_declared_type",
            "test_policies.fixture_format.pass_with_fail",
            "test_policies.fixture_format.undeclared_type",
            "test_policies.fixture_format.unknown_file",
            "test_policies.fixtures_missing.no_dir",
            "test_policies.fixtures_missing.no_fail_fixture",
            "test_policies.fixtures_missing.no_pass_fixture",
            "test_policies.mismatch.wrong_expectation",
            "test_policies.ok.passing_rule",
            "test_policies.timeout.slow",
        ]
    );

    // Failures carry the set name, the rule ID when it can be built, and a message.
    let slow = loaded
        .rule_failures
        .iter()
        .find(|f| f.path == "rego_error/slow_metadata.rego")
        .unwrap();
    assert_eq!(slow.policy_set, "test_policies");
    assert_eq!(
        slow.rule_id.as_deref(),
        Some("test_policies.rego_error.slow_metadata")
    );
    assert!(slow.message.contains("time limit"), "{}", slow.message);
    let invalid = loaded
        .rule_failures
        .iter()
        .find(|f| f.path == "invalid_name/1digit.rego")
        .unwrap();
    assert_eq!(invalid.rule_id, None);
    let severity = loaded
        .rule_failures
        .iter()
        .find(|f| f.path == "metadata/bad_severity.rego")
        .unwrap();
    assert!(
        severity
            .message
            .contains("metadata.severity must be one of critical, high, medium, low, info"),
        "{}",
        severity.message
    );
}

#[test]
fn orphan_fixtures_are_reported_only_with_fixtures() {
    let mut opts = options(LoadMode::RulesWithFixtures);
    opts.policy_timeout = Duration::from_secs(1);
    let loaded = load_one(&policies("rule_failures"), &opts).unwrap();
    let orphan: Vec<&RuleFailure> = loaded
        .rule_failures
        .iter()
        .filter(|f| f.kind == R::OrphanFixtures)
        .collect();
    assert_eq!(orphan.len(), 1);
    assert_eq!(orphan[0].path, "orphan/gone.fixtures");
    assert_eq!(orphan[0].rule_id, None);
    assert!(orphan[0].message.contains("orphan/gone.rego"));
}

#[test]
fn rule_with_invalid_name_directory_still_counts_as_a_rule_file() {
    // A directory whose only rule file has an invalid name is not `no_rules`.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "Upper/r.rego", &rule_source("r"));
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("Upper/r.rego".to_string(), R::InvalidName)]
    );
    assert_eq!(loaded.rules().count(), 0);
}

#[test]
fn duplicate_name_counts_a_directory_that_failed_after_its_manifest() {
    // A: name `dup`, fails because of a library. B: name `dup`, otherwise fine.
    let a = policies("dir_failures/library_syntax"); // name test_policies
    let b = policies("dir_failures/duplicate_name_a"); // name test_policies
    let err = load(&[a.clone(), b.clone()], &options(LoadMode::Rules)).unwrap_err();
    let failures: Vec<(D, PathBuf)> = err
        .directory_failures
        .iter()
        .map(|f| (f.kind, f.policy_dir.clone()))
        .collect();
    assert_eq!(failures, [(D::LibraryInvalid, a), (D::DuplicateName, b)]);
}
