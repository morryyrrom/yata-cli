//! `run_tests`: loads policy directories and checks every rule against its fixtures.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use yata_core::{PolicySetInfo, ResultStatus};

use super::dummy::find_non_dummy;
use super::expected::{compare, expected_set, target_keys};
use super::fixture::{
    DirContents, Fixture, FixtureFile, FixtureKind, check_dir, join_errors, validate,
};
use super::report::{
    FixtureResult, FixtureStatus, RuleTestResult, TestReport, belongs_to_rule, fixtures_dir_of,
};
use crate::error::{
    EvaluationError, FailureDetails, PolicyLoadError, RuleFailure, RuleFailureKind,
};
use crate::evaluate::eval_fail;
use crate::files::{ReadError, read_limited};
use crate::findings::build_results;
use crate::input::resources_of;
use crate::loader::{LoadMode, LoadOptions, LoadedPolicies, PolicySet, load_policies};
use crate::rule::Rule;
use crate::walk::FixtureDir;

/// Loads the policy directories with their fixture directories and tests every rule.
///
/// The directories are always loaded with [`LoadMode::RulesWithFixtures`], whatever
/// `options.mode` says. A directory-level failure makes the whole call fail, as in
/// [`load_policies`]. Everything else, including every rule-level failure, is in the returned
/// [`TestReport`]. Rules that failed to load are not tested and their fixtures are `not_run`;
/// for the other rules, a problem in one fixture does not stop the tests of the others.
pub fn run_tests(dirs: &[PathBuf], options: &LoadOptions) -> Result<TestReport, PolicyLoadError> {
    let mut options = options.clone();
    options.mode = LoadMode::RulesWithFixtures;
    let loaded = load_policies(dirs, &options)?;
    Ok(test_loaded(&loaded, &options))
}

fn test_loaded(loaded: &LoadedPolicies, options: &LoadOptions) -> TestReport {
    let policies: Vec<PolicySetInfo> = loaded
        .policy_sets
        .iter()
        .map(|set| set.info().clone())
        .collect();
    let mut report = TestReport::new(policies);
    let mut fixture_failures = Vec::new();

    for set in &loaded.policy_sets {
        test_set(set, loaded, options, &mut report, &mut fixture_failures);
    }

    // The failures of the loading step come first, so that within a rule they precede the
    // failures of its fixtures.
    for failure in &loaded.rule_failures {
        report.add_failure(failure.clone());
    }
    for failure in fixture_failures {
        report.add_failure(failure);
    }
    report
}

fn test_set(
    set: &PolicySet,
    loaded: &LoadedPolicies,
    options: &LoadOptions,
    report: &mut TestReport,
    failures: &mut Vec<RuleFailure>,
) {
    let policy_set = set.info().name.as_str();
    let rules: HashMap<&str, &Rule> = set.rules().iter().map(|r| (r.path(), r)).collect();
    let dirs: HashMap<&str, &FixtureDir> = set
        .fixture_dirs()
        .iter()
        .map(|d| (d.rule_path.as_str(), d))
        .collect();

    for file in set.rule_files() {
        let rule = rules.get(file.rel_path.as_str()).copied();
        let dir = dirs.get(file.rel_path.as_str()).copied();
        let contents = dir.map(|d| check_dir(&d.entries)).unwrap_or_default();
        // A rule that failed anywhere in the loading step (also in its fixture directory) is
        // not tested.
        let load_failed = loaded
            .rule_failures
            .iter()
            .any(|f| f.policy_set == policy_set && belongs_to_rule(&file.rel_path, &f.path));

        let status = if rule.is_some() && !load_failed {
            FixtureStatus::Passed
        } else {
            FixtureStatus::NotRun
        };
        let fixtures = contents
            .fixtures
            .iter()
            .map(|f| FixtureResult {
                file: f.name.clone(),
                status,
            })
            .collect();
        let types = rule.map(|r| {
            let metadata = r.metadata();
            (metadata.target_type.clone(), metadata.related_types.clone())
        });
        report.push_rule(RuleTestResult::new(
            rule.map(|r| r.id().to_owned()).or_else(|| file.id.clone()),
            policy_set.to_owned(),
            file.rel_path.clone(),
            types,
            fixtures,
        ));

        if let (Some(rule), false) = (rule, load_failed) {
            failures.extend(test_rule(rule, policy_set, dir, &contents, options));
        }
    }
}

/// Checks the fixture directory and every fixture of one rule. Returns the failures found.
fn test_rule(
    rule: &Rule,
    policy_set: &str,
    dir: Option<&FixtureDir>,
    contents: &DirContents,
    options: &LoadOptions,
) -> Vec<RuleFailure> {
    let started = Instant::now();
    let rule_dir = fixtures_dir_of(rule.path());
    let failure = |path: String, kind, message: String, details| RuleFailure {
        policy_set: policy_set.to_owned(),
        rule_id: Some(rule.id().to_owned()),
        path,
        kind,
        message,
        details,
    };
    let mut failures = Vec::new();

    // 1. Entries of the fixture directory that are not fixtures.
    for (name, reason) in &contents.invalid {
        failures.push(failure(
            format!("{rule_dir}/{name}"),
            RuleFailureKind::FixtureInvalid,
            reason.clone(),
            None,
        ));
    }

    // 2. At least one `pass_` and one `fail_` fixture.
    let missing_pass = !contents.has(FixtureKind::Pass);
    let missing_fail = !contents.has(FixtureKind::Fail);
    if missing_pass || missing_fail {
        let what = match (missing_pass, missing_fail) {
            (true, true) => "neither a pass_ nor a fail_ fixture",
            (true, false) => "no pass_ fixture",
            _ => "no fail_ fixture",
        };
        let message = if dir.is_some() {
            format!("the fixture directory has {what}")
        } else {
            "the fixture directory does not exist; the rule needs at least one pass_ and one \
             fail_ fixture"
                .to_owned()
        };
        failures.push(failure(
            rule_dir.clone(),
            RuleFailureKind::FixturesMissing,
            message,
            None,
        ));
    }

    // 3. Every fixture, in name order.
    if let Some(dir) = dir {
        for file in &contents.fixtures {
            let path = format!("{rule_dir}/{}", file.name);
            for (kind, message, details) in test_fixture(rule, dir, file, options) {
                failures.push(failure(path.clone(), kind, message, details));
            }
        }
    }

    tracing::debug!(
        rule = rule.id(),
        fixtures = contents.fixtures.len(),
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "tested the fixtures of a rule"
    );
    failures
}

/// One problem found in a fixture: kind, message and details.
type Problem = (RuleFailureKind, String, Option<FailureDetails>);

/// Reads, validates and evaluates one fixture. A fixture can have a problem of several kinds
/// (for example a format error and a non-dummy account ID); each kind is reported once.
fn test_fixture(
    rule: &Rule,
    dir: &FixtureDir,
    file: &FixtureFile,
    options: &LoadOptions,
) -> Vec<Problem> {
    let limit = options.limits.max_fixture_bytes;
    let bytes = match read_limited(&dir.os_path.join(&file.os_name), limit) {
        Ok(bytes) => bytes,
        Err(ReadError::TooLarge) => {
            return vec![(
                RuleFailureKind::FileTooLarge,
                format!("fixture is larger than {limit} bytes"),
                None,
            )];
        }
        // `read_limited` does not check UTF-8; non-UTF-8 bytes fail the JSON parse below. Kept
        // for exhaustiveness and classified the same way.
        Err(ReadError::NotUtf8) => {
            return vec![(
                RuleFailureKind::FixtureInvalid,
                "fixture is not valid JSON (not UTF-8)".to_owned(),
                None,
            )];
        }
        Err(ReadError::Io(e)) => {
            return vec![(
                RuleFailureKind::Unreadable,
                format!("cannot read fixture: {e}"),
                None,
            )];
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            return vec![(
                RuleFailureKind::FixtureInvalid,
                format!("fixture is not valid JSON: {e}"),
                None,
            )];
        }
    };

    let mut problems = Vec::new();
    let fixture = validate(&value, rule.metadata(), file.kind);
    if let Err(errors) = &fixture {
        problems.push((
            RuleFailureKind::FixtureInvalid,
            format!("fixture is invalid: {}", join_errors(errors)),
            None,
        ));
    }

    // The check for account IDs runs on every fixture that parsed: a real account ID in a
    // fixture with another problem must still be found.
    let locations = find_non_dummy(&value);
    if !locations.is_empty() {
        problems.push((
            RuleFailureKind::NonDummyAccountId,
            format!(
                "fixture contains a 12-digit number that is not a dummy account ID in {} place(s); \
                 see the locations",
                locations.len()
            ),
            Some(FailureDetails::Locations { locations }),
        ));
    }

    // A fixture with a format error is not evaluated; one with a non-dummy ID is.
    if let Ok(fixture) = &fixture
        && let Some(problem) = evaluate_fixture(rule, fixture)
    {
        problems.push(problem);
    }
    problems
}

/// Evaluates a valid fixture and compares the result with `expected.fail`.
fn evaluate_fixture(rule: &Rule, fixture: &Fixture) -> Option<Problem> {
    match compare_fixture(rule, fixture) {
        Ok(None) => None,
        Ok(Some(details)) => Some((
            RuleFailureKind::FixtureMismatch,
            "fixture result does not match expected.fail".to_owned(),
            Some(details),
        )),
        Err(EvaluationError::Timeout { limit }) => {
            let seconds = limit.as_secs() + u64::from(limit.subsec_nanos() > 0);
            Some((
                RuleFailureKind::FixtureTimeout,
                timeout_message(limit),
                Some(FailureDetails::Timeout {
                    timeout_seconds: seconds,
                }),
            ))
        }
        Err(e) => Some((RuleFailureKind::FixtureEvalError, e.to_string(), None)),
    }
}

fn timeout_message(limit: Duration) -> String {
    format!(
        "evaluation exceeded the time limit of {} s",
        limit.as_secs_f64()
    )
}

/// Evaluates `fail` on the fixture's input exactly as a real evaluation would, and compares the
/// resources that failed with the expected ones.
fn compare_fixture(
    rule: &Rule,
    fixture: &Fixture,
) -> Result<Option<FailureDetails>, EvaluationError> {
    let metadata = rule.metadata();
    let elements = eval_fail(rule, &fixture.input)?;
    let target = resources_of(&fixture.input, &metadata.target_type);
    let results = build_results(rule.id(), metadata.severity, &elements, target)?;

    let mut actual: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for result in results
        .into_iter()
        .filter(|r| r.status == ResultStatus::Fail)
    {
        actual
            .entry((result.resource.id, result.resource.region))
            .or_default()
            .extend(result.message);
    }
    let expected = expected_set(&fixture.expected, &target_keys(target));
    Ok(compare(&expected, &actual))
}
