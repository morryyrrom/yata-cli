//! Tests of `yata_engine::testing::run_tests` (stage P3).
//!
//! The committed sets are in `tests/fixtures/policies/`. Cases that need special entries (a
//! number that is not a dummy account ID, links, big or unreadable files, two policy
//! directories) are built in temporary directories.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{policies, symlink_dir, write};
use serde_json::{Value, json};
use yata_engine::testing::{
    DUMMY_ACCOUNT_IDS, FixtureStatus, RuleTestResult, RuleTestStatus, TestReport, run_tests,
};
use yata_engine::{
    DirectoryFailureKind, FailureDetails, LoadMode, LoadOptions, PolicyLoadError, ResourceKey,
    ResourceLimits, RuleFailure, RuleFailureKind as K, UnexpectedResource, YataVersion,
};

fn options(timeout_seconds: u64) -> LoadOptions {
    LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(timeout_seconds),
        // `run_tests` loads the fixtures whatever the mode is.
        mode: LoadMode::Rules,
    }
}

fn run(dirs: &[PathBuf], timeout_seconds: u64) -> TestReport {
    run_tests(dirs, &options(timeout_seconds)).unwrap()
}

fn run_one(dir: &Path, timeout_seconds: u64) -> TestReport {
    run(&[dir.to_path_buf()], timeout_seconds)
}

fn rule<'a>(report: &'a TestReport, path: &str) -> &'a RuleTestResult {
    report
        .rules()
        .iter()
        .find(|r| r.path() == path)
        .unwrap_or_else(|| panic!("no rule {path}"))
}

/// The failures at `path` (the rule file, a fixture or a fixture directory).
fn failures_at<'a>(report: &'a TestReport, path: &str) -> Vec<&'a RuleFailure> {
    report
        .failures()
        .into_iter()
        .filter(|f| f.path == path)
        .collect()
}

fn only_failure<'a>(report: &'a TestReport, path: &str) -> &'a RuleFailure {
    let found = failures_at(report, path);
    assert_eq!(found.len(), 1, "{path}: {found:?}");
    found[0]
}

fn fixture_statuses(rule: &RuleTestResult) -> Vec<(&str, FixtureStatus)> {
    rule.fixtures()
        .iter()
        .map(|f| (f.file.as_str(), f.status))
        .collect()
}

// ------------------------------------------------------------------ helpers for temporary sets

const MANIFEST_TEMPLATE: &str =
    r#"{"format_version": 1, "name": "NAME", "version": "0.1.0", "min_yata_version": "0.1.0"}"#;

/// A temporary policy directory with a manifest of this name.
fn temp_set(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "yata-policies.json",
        &MANIFEST_TEMPLATE.replace("NAME", name),
    );
    dir
}

/// A rule `<set>.<rel>` in the root of the set: it fails the widgets whose flag is `bad`.
fn rule_source(set: &str, name: &str, metadata_severity: &str) -> String {
    format!(
        r#"package yata.rules.{set}.{name}

import rego.v1

metadata := {{
	"title": "Test rule",
	"severity": "{metadata_severity}",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}}

fail contains finding if {{
	some r in input.aws.example.widget
	r.tags.flag == "bad"
	finding := {{"resource_id": r.id, "message": sprintf("Resource %s is flagged", [r.id])}}
}}
"#
    )
}

fn widget(id: &str, region: &str, flag: &str) -> Value {
    json!({
        "id": id, "type": "aws.example.widget", "account": "aws-123456789012",
        "region": region, "name": "example", "tags": {"flag": flag}, "configuration": {}
    })
}

fn fixture_json(widgets: Vec<Value>, fail: Value) -> String {
    serde_json::to_string_pretty(&json!({
        "description": "test",
        "input": {
            "account": {"account": "aws-123456789012", "id": "123456789012", "provider": "aws"},
            "regions": ["us-east-1", "eu-west-1"],
            "aws": {"example": {"widget": widgets}}
        },
        "expected": {"fail": fail}
    }))
    .unwrap()
}

/// Writes `<name>.rego` and a `pass_ok.json` and a `fail_bad.json` that the rule passes.
fn write_good_rule(root: &Path, set: &str, name: &str) {
    write(
        root,
        &format!("{name}.rego"),
        &rule_source(set, name, "low"),
    );
    write(
        root,
        &format!("{name}.fixtures/pass_ok.json"),
        &fixture_json(vec![widget("w1", "us-east-1", "good")], json!([])),
    );
    write(
        root,
        &format!("{name}.fixtures/fail_bad.json"),
        &fixture_json(vec![widget("w1", "us-east-1", "bad")], json!(["w1"])),
    );
}

// ------------------------------------------------------------------ the committed sets

#[test]
fn valid_set_passes_with_string_and_object_expectations() {
    let report = run_one(&policies("valid"), 10);
    assert!(report.passed(), "{:?}", report.failures());
    assert!(report.failures().is_empty());
    let paths: Vec<&str> = report.rules().iter().map(|r| r.path()).collect();
    assert_eq!(
        paths,
        [
            "account_rule.rego",
            "example/nested/deep/deep_rule.rego",
            "example/widget_tagged.rego",
            "regional/trail_region.rego",
        ]
    );
    assert!(report.rules().iter().all(|r| {
        r.status() == RuleTestStatus::Passed
            && r.fixtures()
                .iter()
                .all(|f| f.status == FixtureStatus::Passed)
    }));

    // `expected.fail` as an id string (every region) and as an object (one region), for a
    // rule whose findings name the region.
    let trail = rule(&report, "regional/trail_region.rego");
    assert_eq!(
        fixture_statuses(trail),
        [
            ("fail_all_regions_string.json", FixtureStatus::Passed),
            ("fail_one_region_object.json", FixtureStatus::Passed),
            ("pass_all_on.json", FixtureStatus::Passed),
        ]
    );
    assert_eq!(trail.id(), Some("test_policies.regional.trail_region"));
    assert_eq!(trail.target_type(), Some("aws.example.trail"));
    let widget = rule(&report, "example/widget_tagged.rego");
    assert_eq!(widget.related_types(), ["aws.example.gadget"]);

    let summary = report.summary();
    assert_eq!((summary.rules.total, summary.rules.passed), (4, 4));
    assert_eq!((summary.fixtures.total, summary.fixtures.passed), (11, 11));
    assert_eq!(summary.failures, 0);
    assert_eq!(report.policies()[0].name, "test_policies");
    assert_eq!(
        serde_json::to_value(&report).unwrap()["result"],
        json!("passed")
    );
}

/// The failures of the `rule_failures` set that come from fixtures, as `(path, kind)`.
fn fixture_failure_pairs(report: &TestReport) -> Vec<(String, K)> {
    let mut pairs: Vec<(String, K)> = report
        .failures()
        .iter()
        .filter(|f| f.path.contains(".fixtures"))
        .map(|f| (f.path.clone(), f.kind))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

#[test]
fn rule_failures_set_reports_every_fixture_failure_kind() {
    // A short limit makes `timeout/slow` stop after about a second.
    let report = run_one(&policies("rule_failures"), 1);

    let mut expected: Vec<(String, K)> = [
        // fail_element: the rule returns a malformed element of `fail`.
        (
            "fail_element/bad_path.fixtures/fail_bad.json",
            K::FixtureEvalError,
        ),
        (
            "fail_element/missing_message.fixtures/fail_bad.json",
            K::FixtureEvalError,
        ),
        (
            "fail_element/region_not_string.fixtures/fail_bad.json",
            K::FixtureEvalError,
        ),
        (
            "fail_element/unknown_id.fixtures/fail_bad.json",
            K::FixtureEvalError,
        ),
        (
            "fail_element/unknown_id_region.fixtures/fail_bad.json",
            K::FixtureEvalError,
        ),
        // expected: a malformed `expected.fail`.
        (
            "expected/dup_object.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/dup_string.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/object_missing_region.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/region_not_string.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/string_and_object.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/unknown_id.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "expected/unknown_id_region.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        // fixture_format
        (
            "fixture_format/bad_resource.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/fail_without_fail.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/input_dup_id_region.fixtures/fail_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/missing_declared_type.fixtures/pass_missing_type.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/pass_with_fail.fixtures/pass_invalid.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/undeclared_type.fixtures/pass_undeclared.json",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/unknown_file.fixtures/extra_dir",
            K::FixtureInvalid,
        ),
        (
            "fixture_format/unknown_file.fixtures/pass_extra.jsn",
            K::FixtureInvalid,
        ),
        // fixtures_missing
        ("fixtures_missing/no_dir.fixtures", K::FixturesMissing),
        (
            "fixtures_missing/no_fail_fixture.fixtures",
            K::FixturesMissing,
        ),
        (
            "fixtures_missing/no_pass_fixture.fixtures",
            K::FixturesMissing,
        ),
        // mismatch, timeout and the fixture directory without a rule
        (
            "mismatch/wrong_expectation.fixtures/fail_wrong_expectation.json",
            K::FixtureMismatch,
        ),
        ("timeout/slow.fixtures/fail_slow.json", K::FixtureTimeout),
        ("orphan/gone.fixtures", K::OrphanFixtures),
    ]
    .into_iter()
    .map(|(p, k)| (p.to_owned(), k))
    .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(fixture_failure_pairs(&report), expected);

    // The rule that has nothing wrong passes although the others fail.
    let ok = rule(&report, "ok/passing_rule.rego");
    assert_eq!(ok.status(), RuleTestStatus::Passed);
    assert_eq!(
        fixture_statuses(ok),
        [
            ("fail_bad.json", FixtureStatus::Passed),
            ("pass_good.json", FixtureStatus::Passed),
        ]
    );
    assert!(failures_at(&report, "ok/passing_rule.rego").is_empty());
    assert!(!report.passed());
}

#[test]
fn fixture_eval_errors_name_the_fail_element_problem() {
    let report = run_one(&policies("rule_failures"), 1);
    let cases = [
        ("bad_path", "JSON Pointer"),
        ("missing_message", "`message` is missing"),
        ("region_not_string", "`resource_region` is not a string"),
        ("unknown_id", "unknown resource"),
        ("unknown_id_region", "unknown resource"),
    ];
    for (name, text) in cases {
        let f = only_failure(
            &report,
            &format!("fail_element/{name}.fixtures/fail_bad.json"),
        );
        assert_eq!(f.kind, K::FixtureEvalError, "{name}");
        assert!(f.message.contains(text), "{name}: {}", f.message);
        assert_eq!(f.details, None);
        assert_eq!(
            f.rule_id.as_deref(),
            Some(format!("test_policies.fail_element.{name}").as_str())
        );
        // The fixture that does not trigger the malformed element passes.
        let r = rule(&report, &format!("fail_element/{name}.rego"));
        assert_eq!(
            fixture_statuses(r),
            [
                ("fail_bad.json", FixtureStatus::Failed),
                ("pass_good.json", FixtureStatus::Passed)
            ]
        );
    }
}

#[test]
fn malformed_expected_fail_is_a_fixture_invalid_and_the_other_fixtures_still_run() {
    let report = run_one(&policies("rule_failures"), 1);
    let cases = [
        ("dup_string", "repeats"),
        ("dup_object", "repeats"),
        ("string_and_object", "as a string"),
        (
            "object_missing_region",
            "without a string `id` and a string `region`",
        ),
        (
            "region_not_string",
            "without a string `id` and a string `region`",
        ),
        ("unknown_id", "matches no resource"),
        ("unknown_id_region", "matches no resource"),
    ];
    for (name, text) in cases {
        let f = only_failure(
            &report,
            &format!("expected/{name}.fixtures/fail_invalid.json"),
        );
        assert_eq!(f.kind, K::FixtureInvalid, "{name}");
        assert!(f.message.contains(text), "{name}: {}", f.message);
        let r = rule(&report, &format!("expected/{name}.rego"));
        assert_eq!(
            fixture_statuses(r),
            [
                ("fail_bad.json", FixtureStatus::Passed),
                ("fail_invalid.json", FixtureStatus::Failed),
                ("pass_good.json", FixtureStatus::Passed),
            ],
            "{name}"
        );
    }
}

#[test]
fn fixture_format_errors_and_directory_checks() {
    let report = run_one(&policies("rule_failures"), 1);
    let cases = [
        (
            "input_dup_id_region",
            "fail_invalid.json",
            "same id and region",
        ),
        (
            "missing_declared_type",
            "pass_missing_type.json",
            "gadget is missing",
        ),
        (
            "undeclared_type",
            "pass_undeclared.json",
            "does not declare",
        ),
        (
            "bad_resource",
            "fail_invalid.json",
            "region must be a string",
        ),
        (
            "pass_with_fail",
            "pass_invalid.json",
            "must be empty in a pass_ fixture",
        ),
        (
            "fail_without_fail",
            "fail_invalid.json",
            "must not be empty in a fail_ fixture",
        ),
    ];
    for (name, file, text) in cases {
        let f = only_failure(&report, &format!("fixture_format/{name}.fixtures/{file}"));
        assert_eq!(f.kind, K::FixtureInvalid, "{name}");
        assert!(f.message.contains(text), "{name}: {}", f.message);
    }

    // Entries that are not fixtures: the other fixtures are still checked and pass.
    let dir_entry = only_failure(&report, "fixture_format/unknown_file.fixtures/extra_dir");
    assert!(
        dir_entry.message.contains("directories"),
        "{}",
        dir_entry.message
    );
    let file_entry = only_failure(
        &report,
        "fixture_format/unknown_file.fixtures/pass_extra.jsn",
    );
    assert!(file_entry.message.contains("pass_<case>.json"));
    let r = rule(&report, "fixture_format/unknown_file.rego");
    assert_eq!(r.status(), RuleTestStatus::Failed);
    assert_eq!(
        fixture_statuses(r),
        [
            ("fail_bad.json", FixtureStatus::Passed),
            ("pass_good.json", FixtureStatus::Passed)
        ]
    );

    // fixtures_missing: the path is the fixture directory, also when it does not exist.
    let f = only_failure(&report, "fixtures_missing/no_dir.fixtures");
    assert!(f.message.contains("does not exist"), "{}", f.message);
    assert!(
        rule(&report, "fixtures_missing/no_dir.rego")
            .fixtures()
            .is_empty()
    );
    let f = only_failure(&report, "fixtures_missing/no_fail_fixture.fixtures");
    assert!(f.message.contains("no fail_ fixture"), "{}", f.message);
    let f = only_failure(&report, "fixtures_missing/no_pass_fixture.fixtures");
    assert!(f.message.contains("no pass_ fixture"), "{}", f.message);
    // The fixture that exists is still checked.
    assert_eq!(
        fixture_statuses(rule(&report, "fixtures_missing/no_fail_fixture.rego")),
        [("pass_good.json", FixtureStatus::Passed)]
    );
}

#[test]
fn mismatch_lists_missing_and_unexpected_resources_in_order() {
    let report = run_one(&policies("rule_failures"), 1);
    let f = only_failure(
        &report,
        "mismatch/wrong_expectation.fixtures/fail_wrong_expectation.json",
    );
    assert_eq!(f.kind, K::FixtureMismatch);
    assert_eq!(f.message, "fixture result does not match expected.fail");
    assert_eq!(
        f.details,
        Some(FailureDetails::Mismatch {
            missing: vec![ResourceKey {
                id: "widget-1".into(),
                region: "us-east-1".into()
            }],
            unexpected: vec![
                UnexpectedResource {
                    id: "widget-2".into(),
                    region: "eu-west-1".into(),
                    messages: vec!["Resource widget-2 is flagged".into()],
                },
                UnexpectedResource {
                    id: "widget-2".into(),
                    region: "us-east-1".into(),
                    messages: vec!["Resource widget-2 is flagged".into()],
                },
            ],
        })
    );
}

#[test]
fn timeout_reports_the_limit_and_does_not_stop_the_other_fixtures() {
    let report = run_one(&policies("rule_failures"), 1);
    let f = only_failure(&report, "timeout/slow.fixtures/fail_slow.json");
    assert_eq!(f.kind, K::FixtureTimeout);
    assert_eq!(
        f.details,
        Some(FailureDetails::Timeout { timeout_seconds: 1 })
    );
    assert!(f.message.contains("time limit"), "{}", f.message);
    let r = rule(&report, "timeout/slow.rego");
    assert_eq!(
        fixture_statuses(r),
        [
            ("fail_slow.json", FixtureStatus::Failed),
            ("pass_empty.json", FixtureStatus::Passed)
        ]
    );
}

#[test]
fn rules_that_failed_to_load_are_reported_and_their_fixtures_are_not_run() {
    let report = run_one(&policies("rule_failures"), 1);
    // A rule without a fixture directory that fails to load has only the loading failure.
    let f = only_failure(&report, "metadata/bad_severity.rego");
    assert_eq!(f.kind, K::MetadataInvalid);
    let r = rule(&report, "metadata/bad_severity.rego");
    assert_eq!(r.status(), RuleTestStatus::Failed);
    assert!(r.fixtures().is_empty());
    assert_eq!(r.target_type(), None);
    assert!(r.related_types().is_empty());
    // A rule name that is not valid has no ID.
    let r = rule(&report, "invalid_name/1digit.rego");
    assert_eq!(r.id(), None);
    assert_eq!(r.status(), RuleTestStatus::Failed);

    // With fixtures: they are not run, and nothing else is reported for them.
    let dir = temp_set("tmp");
    write(
        dir.path(),
        "bad.rego",
        &rule_source("tmp", "bad", "not-a-severity"),
    );
    write(dir.path(), "bad.fixtures/pass_a.json", "this is not json");
    write(dir.path(), "bad.fixtures/fail_b.json", "{}");
    write(dir.path(), "bad.fixtures/unknown.txt", "x");
    write_good_rule(dir.path(), "tmp", "good");
    let report = run_one(dir.path(), 10);
    let bad = rule(&report, "bad.rego");
    assert_eq!(
        fixture_statuses(bad),
        [
            ("fail_b.json", FixtureStatus::NotRun),
            ("pass_a.json", FixtureStatus::NotRun)
        ]
    );
    let failures: Vec<(&str, K)> = report
        .failures()
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert_eq!(failures, [("bad.rego", K::MetadataInvalid)]);
    assert_eq!(rule(&report, "good.rego").status(), RuleTestStatus::Passed);
    let summary = report.summary();
    assert_eq!((summary.rules.total, summary.rules.failed), (2, 1));
    assert_eq!(
        (
            summary.fixtures.total,
            summary.fixtures.passed,
            summary.fixtures.failed,
            summary.fixtures.not_run
        ),
        (4, 2, 0, 2)
    );
}

// ------------------------------------------------------------------ non-dummy account IDs

/// `123456789012` with the last digit changed. This number is not in the list of dummy IDs
/// (the test checks that), and it is made here from a dummy one, so no such number is committed.
fn non_dummy_number() -> String {
    let mut number = String::from("123456789012");
    number.pop();
    number.push('3');
    number
}

#[test]
fn non_dummy_account_ids_are_reported_by_location_without_the_value() {
    let non_dummy = non_dummy_number();
    assert_eq!(non_dummy.len(), 12);
    assert!(!DUMMY_ACCOUNT_IDS.contains(&non_dummy.as_str()));

    // Copy the passing rule of the committed set and replace the dummy account ID in its
    // fixtures with the number above. The rule has no other problem, so the evaluation passes.
    let source = policies("rule_failures");
    let dir = temp_set("test_policies");
    fs::create_dir_all(dir.path().join("ok/passing_rule.fixtures")).unwrap();
    let copy = |rel: &str, edit: &dyn Fn(String) -> String| {
        let text = fs::read_to_string(source.join(rel)).unwrap();
        write(dir.path(), rel, &edit(text));
    };
    copy("ok/passing_rule.rego", &|t| t);
    let replace = |t: String| t.replace("123456789012", &non_dummy);
    copy("ok/passing_rule.fixtures/fail_bad.json", &replace);
    copy("ok/passing_rule.fixtures/pass_good.json", &replace);

    // A fixture with a format error and the number in a key: both kinds are reported once.
    let both = {
        let text =
            fs::read_to_string(source.join("ok/passing_rule.fixtures/pass_good.json")).unwrap();
        let mut value: Value = serde_json::from_str(&text).unwrap();
        value.as_object_mut().unwrap().remove("expected");
        value[format!("note-{non_dummy}")] = json!(1);
        serde_json::to_string_pretty(&value).unwrap()
    };
    write(dir.path(), "ok/passing_rule.fixtures/pass_both.json", &both);

    let report = run_one(dir.path(), 10);
    let rule = rule(&report, "ok/passing_rule.rego");
    assert_eq!(
        fixture_statuses(rule),
        [
            ("fail_bad.json", FixtureStatus::Failed),
            ("pass_both.json", FixtureStatus::Failed),
            ("pass_good.json", FixtureStatus::Failed),
        ]
    );

    // The fixtures with the number are still evaluated (and match), so it is the only failure.
    let locations = vec![
        "/input/account/account".to_owned(),
        "/input/account/id".to_owned(),
        "/input/aws/example/widget/0/account".to_owned(),
    ];
    for file in ["fail_bad.json", "pass_good.json"] {
        let f = only_failure(&report, &format!("ok/passing_rule.fixtures/{file}"));
        assert_eq!(f.kind, K::NonDummyAccountId);
        assert_eq!(
            f.details,
            Some(FailureDetails::Locations {
                locations: locations.clone()
            }),
            "{file}"
        );
    }

    // A format error does not hide the number, and both are reported once each.
    let found = failures_at(&report, "ok/passing_rule.fixtures/pass_both.json");
    let kinds: Vec<K> = found.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [K::FixtureInvalid, K::NonDummyAccountId]);
    let Some(FailureDetails::Locations { locations }) = &found[1].details else {
        panic!("expected locations");
    };
    // The key is reported at the object that has it (the root, `""`).
    assert!(locations.contains(&String::new()), "{locations:?}");

    // The number is nowhere in the report: not in messages, details or the JSON.
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(&non_dummy), "{json}");
    assert!(!format!("{:?}", report.failures()).contains(&non_dummy));
    assert!(json.contains("non_dummy_account_id"));
}

// ------------------------------------------------------------------ other fixture problems

#[test]
fn unparsable_oversized_and_unreadable_fixtures() {
    let dir = temp_set("tmp");
    write_good_rule(dir.path(), "tmp", "r");
    write(dir.path(), "r.fixtures/pass_notjson.json", "{");
    fs::write(
        dir.path().join("r.fixtures/pass_binary.json"),
        [0xff, 0xfe, 0x00],
    )
    .unwrap();
    write(dir.path(), "r.fixtures/pass_array.json", "[]");
    write(
        dir.path(),
        "r.fixtures/pass_big.json",
        &format!("{{\"description\": \"{}\"}}", "x".repeat(2500)),
    );
    let mut opts = options(10);
    opts.limits.max_fixture_bytes = 2000;
    let report = run_tests(&[dir.path().to_path_buf()], &opts).unwrap();
    let kinds: Vec<(&str, K)> = report
        .failures()
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("r.fixtures/pass_array.json", K::FixtureInvalid),
            ("r.fixtures/pass_big.json", K::FileTooLarge),
            ("r.fixtures/pass_binary.json", K::FixtureInvalid),
            ("r.fixtures/pass_notjson.json", K::FixtureInvalid),
        ]
    );
    assert!(report.failures()[2].message.contains("not valid JSON"));
}

#[cfg(unix)]
#[test]
fn unreadable_fixture() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_set("tmp");
    write_good_rule(dir.path(), "tmp", "r");
    let path = dir.path().join("r.fixtures/pass_ok.json");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        eprintln!("skipping: permissions are not enforced (running as root?)");
        return;
    }
    let report = run_one(dir.path(), 10);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let f = only_failure(&report, "r.fixtures/pass_ok.json");
    assert_eq!(f.kind, K::Unreadable);
}

#[test]
fn symlinked_fixture_directory_fails_the_load_and_the_rule_is_not_tested() {
    let dir = temp_set("tmp");
    write(dir.path(), "r.rego", &rule_source("tmp", "r", "low"));
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "pass_a.json", "{}");
    if !symlink_dir(outside.path(), &dir.path().join("r.fixtures")) {
        return;
    }
    let report = run_one(dir.path(), 10);
    let kinds: Vec<(&str, K)> = report
        .failures()
        .iter()
        .map(|f| (f.path.as_str(), f.kind))
        .collect();
    // No `fixtures_missing`: the loading failure already stands for the fixture directory.
    assert_eq!(kinds, [("r.fixtures", K::Symlink)]);
    assert_eq!(rule(&report, "r.rego").status(), RuleTestStatus::Failed);
}

#[test]
fn fixtures_are_loaded_whatever_the_mode_in_the_options() {
    for mode in [LoadMode::Rules, LoadMode::RulesWithFixtures] {
        let mut opts = options(10);
        opts.mode = mode;
        let report = run_tests(&[policies("valid")], &opts).unwrap();
        assert_eq!(report.summary().fixtures.total, 11);
    }
}

// ------------------------------------------------------------------ directory-level failures

#[test]
fn a_directory_level_failure_fails_the_call() {
    let err: PolicyLoadError = run_tests(
        &[policies("valid"), policies("dir_failures/manifest_missing")],
        &options(10),
    )
    .unwrap_err();
    assert_eq!(err.directory_failures.len(), 1);
    assert_eq!(
        err.directory_failures[0].kind,
        DirectoryFailureKind::ManifestMissing
    );
}

// ------------------------------------------------------------------ the report

#[test]
fn add_failure_marks_the_rule_and_appears_in_the_json() {
    let mut report = run_one(&policies("valid"), 10);
    assert!(report.passed());
    report.add_failure(RuleFailure {
        policy_set: "test_policies".to_owned(),
        rule_id: Some("test_policies.account_rule".to_owned()),
        path: "account_rule.rego".to_owned(),
        kind: K::UnsupportedResourceType,
        message: "unsupported".to_owned(),
        details: Some(FailureDetails::Types {
            types: vec!["aws.example.unsupported".to_owned()],
        }),
    });
    assert!(!report.passed());
    assert_eq!(
        rule(&report, "account_rule.rego").status(),
        RuleTestStatus::Failed
    );
    assert_eq!(
        rule(&report, "example/widget_tagged.rego").status(),
        RuleTestStatus::Passed
    );
    let summary = report.summary();
    assert_eq!(
        (summary.rules.passed, summary.rules.failed, summary.failures),
        (3, 1, 1)
    );
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["result"], json!("failed"));
    assert_eq!(
        value["failures"][0]["details"],
        json!({"types": ["aws.example.unsupported"]})
    );
}

#[test]
fn json_has_the_designed_shape_and_order() {
    let dir = temp_set("shape");
    write_good_rule(dir.path(), "shape", "a");
    write(dir.path(), "b.rego", &rule_source("shape", "b", "high"));
    write(
        dir.path(),
        "b.fixtures/pass_ok.json",
        &fixture_json(vec![widget("w1", "us-east-1", "good")], json!([])),
    );
    // w1 does not fail but is expected to; w2 fails but is not expected to.
    write(
        dir.path(),
        "b.fixtures/fail_wrong.json",
        &fixture_json(
            vec![
                widget("w1", "us-east-1", "good"),
                widget("w2", "us-east-1", "bad"),
            ],
            json!(["w1"]),
        ),
    );
    write(dir.path(), "old.fixtures/pass_x.json", "{}");
    let report = run_one(dir.path(), 10);

    let expected = concat!(
        r#"{"result":"failed","#,
        r#""summary":{"rules":{"total":2,"passed":1,"failed":1},"#,
        r#""fixtures":{"total":4,"passed":3,"failed":1,"not_run":0},"failures":2},"#,
        r#""policies":[{"name":"shape","version":"0.1.0","format_version":1}],"#,
        r#""rules":["#,
        r#"{"id":"shape.a","policy_set":"shape","path":"a.rego","target_type":"aws.example.widget","#,
        r#""related_types":[],"status":"passed","fixtures":["#,
        r#"{"file":"fail_bad.json","status":"passed"},{"file":"pass_ok.json","status":"passed"}]},"#,
        r#"{"id":"shape.b","policy_set":"shape","path":"b.rego","target_type":"aws.example.widget","#,
        r#""related_types":[],"status":"failed","fixtures":["#,
        r#"{"file":"fail_wrong.json","status":"failed"},{"file":"pass_ok.json","status":"passed"}]}],"#,
        r#""failures":["#,
        r#"{"kind":"fixture_mismatch","policy_set":"shape","rule_id":"shape.b","#,
        r#""path":"b.fixtures/fail_wrong.json","message":"fixture result does not match expected.fail","#,
        r#""details":{"missing":[{"id":"w1","region":"us-east-1"}],"#,
        r#""unexpected":[{"id":"w2","region":"us-east-1","messages":["Resource w2 is flagged"]}]}},"#,
        r#"{"kind":"orphan_fixtures","policy_set":"shape","rule_id":null,"path":"old.fixtures","#,
        r#""message":"no rule file old.rego for this fixture directory","details":null}]}"#,
    );
    assert_eq!(serde_json::to_string(&report).unwrap(), expected);
}

#[test]
fn rules_and_failures_follow_the_order_of_the_policy_directories() {
    // The directories are given in the order bbb, aaa: that order is kept (not sorted by name).
    let b = temp_set("bbb");
    let a = temp_set("aaa");
    for (dir, set) in [(&b, "bbb"), (&a, "aaa")] {
        write(
            dir.path(),
            "z.rego",
            &rule_source(set, "z", "not-a-severity"),
        );
        write_good_rule(dir.path(), set, "m");
        write(
            dir.path(),
            "a.rego",
            &rule_source(set, "a", "not-a-severity"),
        );
        write(dir.path(), "gone.fixtures/pass_x.json", "{}");
    }
    let report = run(&[b.path().to_path_buf(), a.path().to_path_buf()], 10);
    let names: Vec<&str> = report.policies().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["bbb", "aaa"]);
    let rules: Vec<(&str, &str)> = report
        .rules()
        .iter()
        .map(|r| (r.policy_set(), r.path()))
        .collect();
    assert_eq!(
        rules,
        [
            ("bbb", "a.rego"),
            ("bbb", "m.rego"),
            ("bbb", "z.rego"),
            ("aaa", "a.rego"),
            ("aaa", "m.rego"),
            ("aaa", "z.rego"),
        ]
    );
    // Per policy directory: failures of the rules in rule order, then those of no rule.
    let failures: Vec<(&str, &str)> = report
        .failures()
        .iter()
        .map(|f| (f.policy_set.as_str(), f.path.as_str()))
        .collect();
    assert_eq!(
        failures,
        [
            ("bbb", "a.rego"),
            ("bbb", "z.rego"),
            ("bbb", "gone.fixtures"),
            ("aaa", "a.rego"),
            ("aaa", "z.rego"),
            ("aaa", "gone.fixtures"),
        ]
    );
}
