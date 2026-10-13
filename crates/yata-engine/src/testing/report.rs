//! The result of `run_tests`: [`TestReport`] and the types it is made of.
//!
//! [`TestReport`] serializes to the `result`, `summary`, `policies`, `rules` and `failures`
//! members of the JSON output of `yata rules test`, in this order. `yata-cli` adds
//! `schema_version` and `tool` in front of them.

use serde::{Serialize, Serializer};
use yata_core::PolicySetInfo;

use crate::error::{FailureDetails, RuleFailure, RuleFailureKind};

/// Suffix of a rule file name.
const REGO_EXTENSION: &str = ".rego";
/// Suffix of the name of a fixture directory.
const FIXTURES_SUFFIX: &str = ".fixtures";

/// Status of one rule in a [`TestReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleTestStatus {
    /// The rule has no failure.
    Passed,
    /// The rule has at least one failure.
    Failed,
}

/// Status of one fixture in a [`TestReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureStatus {
    /// The fixture is valid and the rule produced the expected result.
    Passed,
    /// The fixture has a format error, a non-dummy account ID, a wrong result or an evaluation
    /// error.
    Failed,
    /// The rule failed to load, so the fixture was not checked.
    NotRun,
}

/// The result of one fixture file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixtureResult {
    /// File name, for example `pass_all_blocked.json`.
    pub file: String,
    /// Status.
    pub status: FixtureStatus,
}

/// The result of one rule file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleTestResult {
    id: Option<String>,
    policy_set: String,
    path: String,
    target_type: Option<String>,
    related_types: Vec<String>,
    status: RuleTestStatus,
    fixtures: Vec<FixtureResult>,
}

impl RuleTestResult {
    /// Creates a passed result. Failures are added through [`TestReport::add_failure`].
    pub(super) fn new(
        id: Option<String>,
        policy_set: String,
        path: String,
        types: Option<(String, Vec<String>)>,
        fixtures: Vec<FixtureResult>,
    ) -> Self {
        let (target_type, related_types) = match types {
            Some((target, related)) => (Some(target), related),
            None => (None, Vec::new()),
        };
        Self {
            id,
            policy_set,
            path,
            target_type,
            related_types,
            status: RuleTestStatus::Passed,
            fixtures,
        }
    }

    /// The rule ID, or `None` when the names do not allow one.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The name of the policy set.
    pub fn policy_set(&self) -> &str {
        &self.policy_set
    }

    /// Path of the rule file relative to the policy directory, `/`-separated.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `target_type` from the metadata, or `None` when the rule failed to load.
    pub fn target_type(&self) -> Option<&str> {
        self.target_type.as_deref()
    }

    /// `related_types` from the metadata, empty when the rule failed to load.
    pub fn related_types(&self) -> &[String] {
        &self.related_types
    }

    /// `passed` when the rule has no failure.
    pub fn status(&self) -> RuleTestStatus {
        self.status
    }

    /// The fixtures of the rule, in name order.
    pub fn fixtures(&self) -> &[FixtureResult] {
        &self.fixtures
    }

    /// Path of the fixture directory of this rule (which may not exist).
    fn fixtures_dir(&self) -> String {
        fixtures_dir_of(&self.path)
    }

    /// Whether a failure with this `path` belongs to this rule.
    fn owns(&self, path: &str) -> bool {
        belongs_to_rule(&self.path, path)
    }
}

/// Path of the fixture directory of the rule file `rule_path` (which may not exist).
pub(super) fn fixtures_dir_of(rule_path: &str) -> String {
    let stem = rule_path.strip_suffix(REGO_EXTENSION).unwrap_or(rule_path);
    format!("{stem}{FIXTURES_SUFFIX}")
}

/// Whether a failure at `path` belongs to the rule file `rule_path`: the rule file, its fixture
/// directory or something inside the directory.
pub(super) fn belongs_to_rule(rule_path: &str, path: &str) -> bool {
    let dir = fixtures_dir_of(rule_path);
    path == rule_path
        || path == dir
        || path
            .strip_prefix(&dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Counts of rules by status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuleCounts {
    /// Number of rule files.
    pub total: usize,
    /// Rules without a failure.
    pub passed: usize,
    /// Rules with at least one failure.
    pub failed: usize,
}

/// Counts of fixtures by status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FixtureCounts {
    /// Number of fixture files.
    pub total: usize,
    /// Fixtures that passed.
    pub passed: usize,
    /// Fixtures that failed.
    pub failed: usize,
    /// Fixtures that were not checked because their rule failed to load.
    pub not_run: usize,
}

/// The `summary` of a [`TestReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TestSummary {
    /// Rules by status.
    pub rules: RuleCounts,
    /// Fixtures by status.
    pub fixtures: FixtureCounts,
    /// Number of failures, including the ones that belong to no rule.
    pub failures: usize,
}

/// The result of testing the rules of one or more policy directories.
///
/// It holds the policy sets in the order of the given directories, the result of every rule
/// file (policy set order, then path) and the failures in the order in which they were found.
/// [`TestReport::failures`] returns them in the order of the JSON output.
#[derive(Debug)]
pub struct TestReport {
    policies: Vec<PolicySetInfo>,
    rules: Vec<RuleTestResult>,
    failures: Vec<RuleFailure>,
}

impl TestReport {
    /// Creates a report for `policies` without rules and failures.
    pub(super) fn new(policies: Vec<PolicySetInfo>) -> Self {
        Self {
            policies,
            rules: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Adds the result of a rule. The caller adds rules in policy set order, then path order.
    pub(super) fn push_rule(&mut self, rule: RuleTestResult) {
        self.rules.push(rule);
    }

    /// The policy sets (manifest values), in the order of the given directories.
    pub fn policies(&self) -> &[PolicySetInfo] {
        &self.policies
    }

    /// The result of every rule file, in policy directory order, then path order.
    pub fn rules(&self) -> &[RuleTestResult] {
        &self.rules
    }

    /// Adds a failure found outside the engine (for example `unsupported_resource_type`).
    ///
    /// The rule the failure belongs to (the same policy set and a `path` that is the rule file,
    /// its fixture directory or a fixture file) becomes `failed`, and so does the fixture when
    /// the `path` names one.
    pub fn add_failure(&mut self, failure: RuleFailure) {
        let owner = self
            .rules
            .iter_mut()
            .find(|r| r.policy_set == failure.policy_set && r.owns(&failure.path));
        if let Some(rule) = owner {
            rule.status = RuleTestStatus::Failed;
            let dir = rule.fixtures_dir();
            let file = failure
                .path
                .strip_prefix(&dir)
                .and_then(|rest| rest.strip_prefix('/'));
            if let Some(file) = file
                && let Some(fixture) = rule.fixtures.iter_mut().find(|f| f.file == file)
                && fixture.status == FixtureStatus::Passed
            {
                fixture.status = FixtureStatus::Failed;
            }
        }
        self.failures.push(failure);
    }

    /// Whether there is no failure (exit code 0 of `yata rules test`).
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// The failures in the order of the JSON output.
    ///
    /// For each policy set, in order: first the failures of its rules (in the order of
    /// [`TestReport::rules`], and within a rule in the order in which they were found), then the
    /// failures that belong to no rule (such as `orphan_fixtures`), by path. Failures of an
    /// unknown policy set come last.
    pub fn failures(&self) -> Vec<&RuleFailure> {
        let mut used = vec![false; self.failures.len()];
        let mut ordered = Vec::with_capacity(self.failures.len());
        for policy in &self.policies {
            for rule in self.rules.iter().filter(|r| r.policy_set == policy.name) {
                for (i, failure) in self.failures.iter().enumerate() {
                    if !used[i] && failure.policy_set == policy.name && rule.owns(&failure.path) {
                        used[i] = true;
                        ordered.push(failure);
                    }
                }
            }
            let mut rest: Vec<(usize, &RuleFailure)> = self
                .failures
                .iter()
                .enumerate()
                .filter(|(i, f)| !used[*i] && f.policy_set == policy.name)
                .collect();
            rest.sort_by(|a, b| a.1.path.cmp(&b.1.path));
            for (i, failure) in rest {
                used[i] = true;
                ordered.push(failure);
            }
        }
        for (i, failure) in self.failures.iter().enumerate() {
            if !used[i] {
                ordered.push(failure);
            }
        }
        ordered
    }

    /// Counts of rules, fixtures and failures.
    pub fn summary(&self) -> TestSummary {
        let failed_rules = self
            .rules
            .iter()
            .filter(|r| r.status == RuleTestStatus::Failed)
            .count();
        let count = |status: FixtureStatus| {
            self.rules
                .iter()
                .flat_map(|r| r.fixtures.iter())
                .filter(|f| f.status == status)
                .count()
        };
        let (passed, failed, not_run) = (
            count(FixtureStatus::Passed),
            count(FixtureStatus::Failed),
            count(FixtureStatus::NotRun),
        );
        TestSummary {
            rules: RuleCounts {
                total: self.rules.len(),
                passed: self.rules.len() - failed_rules,
                failed: failed_rules,
            },
            fixtures: FixtureCounts {
                total: passed + failed + not_run,
                passed,
                failed,
                not_run,
            },
            failures: self.failures.len(),
        }
    }
}

/// One entry of `failures[]` in the JSON output.
#[derive(Serialize)]
struct FailureView<'a> {
    kind: RuleFailureKind,
    policy_set: &'a str,
    rule_id: Option<&'a str>,
    path: &'a str,
    message: &'a str,
    details: Option<&'a FailureDetails>,
}

/// The members of the JSON output that the engine knows.
#[derive(Serialize)]
struct ReportView<'a> {
    result: &'static str,
    summary: TestSummary,
    policies: &'a [PolicySetInfo],
    rules: &'a [RuleTestResult],
    failures: Vec<FailureView<'a>>,
}

impl Serialize for TestReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ReportView {
            result: if self.passed() { "passed" } else { "failed" },
            summary: self.summary(),
            policies: &self.policies,
            rules: &self.rules,
            failures: self
                .failures()
                .into_iter()
                .map(|f| FailureView {
                    kind: f.kind,
                    policy_set: &f.policy_set,
                    rule_id: f.rule_id.as_deref(),
                    path: &f.path,
                    message: &f.message,
                    details: f.details.as_ref(),
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(set: &str, path: &str, kind: RuleFailureKind) -> RuleFailure {
        RuleFailure {
            policy_set: set.to_owned(),
            rule_id: None,
            path: path.to_owned(),
            kind,
            message: "m".to_owned(),
            details: None,
        }
    }

    fn rule(set: &str, path: &str, files: &[&str]) -> RuleTestResult {
        RuleTestResult::new(
            Some("id".to_owned()),
            set.to_owned(),
            path.to_owned(),
            Some(("aws.example.widget".to_owned(), vec![])),
            files
                .iter()
                .map(|f| FixtureResult {
                    file: (*f).to_owned(),
                    status: FixtureStatus::Passed,
                })
                .collect(),
        )
    }

    fn info(name: &str) -> PolicySetInfo {
        PolicySetInfo {
            name: name.to_owned(),
            version: "0.1.0".to_owned(),
            format_version: 1,
        }
    }

    #[test]
    fn ownership_by_path() {
        let r = rule("s", "a/b.rego", &[]);
        assert!(r.owns("a/b.rego"));
        assert!(r.owns("a/b.fixtures"));
        assert!(r.owns("a/b.fixtures/pass_x.json"));
        assert!(!r.owns("a/b.fixtures2/pass_x.json"));
        assert!(!r.owns("a/c.fixtures"));
        assert!(!r.owns("b.rego"));
    }

    #[test]
    fn add_failure_marks_the_rule_and_the_fixture() {
        let mut report = TestReport::new(vec![info("s")]);
        report.push_rule(rule("s", "a.rego", &["fail_x.json", "pass_y.json"]));
        assert!(report.passed());
        report.add_failure(failure(
            "s",
            "a.fixtures/pass_y.json",
            RuleFailureKind::FixtureMismatch,
        ));
        assert!(!report.passed());
        let rule = &report.rules()[0];
        assert_eq!(rule.status(), RuleTestStatus::Failed);
        assert_eq!(rule.fixtures()[0].status, FixtureStatus::Passed);
        assert_eq!(rule.fixtures()[1].status, FixtureStatus::Failed);
        // A failure of another policy set with the same path does not touch the rule.
        let mut report = TestReport::new(vec![info("s"), info("t")]);
        report.push_rule(rule_for_test());
        report.add_failure(failure("t", "a.rego", RuleFailureKind::Symlink));
        assert_eq!(report.rules()[0].status(), RuleTestStatus::Passed);
    }

    fn rule_for_test() -> RuleTestResult {
        rule("s", "a.rego", &[])
    }

    #[test]
    fn failures_are_ordered_by_policy_set_then_rule_then_orphans() {
        let mut report = TestReport::new(vec![info("s"), info("t")]);
        report.push_rule(rule("s", "a.rego", &[]));
        report.push_rule(rule("s", "b.rego", &[]));
        report.push_rule(rule("t", "a.rego", &[]));
        // Added in a scrambled order.
        report.add_failure(failure("t", "a.rego", RuleFailureKind::Symlink));
        report.add_failure(failure("s", "z.fixtures", RuleFailureKind::OrphanFixtures));
        report.add_failure(failure("s", "b.rego", RuleFailureKind::MetadataInvalid));
        report.add_failure(failure("s", "m.fixtures", RuleFailureKind::OrphanFixtures));
        report.add_failure(failure(
            "s",
            "a.fixtures/fail_1.json",
            RuleFailureKind::FixtureInvalid,
        ));
        report.add_failure(failure(
            "s",
            "a.rego",
            RuleFailureKind::UnsupportedResourceType,
        ));
        report.add_failure(failure("u", "x.rego", RuleFailureKind::Symlink));
        let order: Vec<(&str, &str)> = report
            .failures()
            .iter()
            .map(|f| (f.policy_set.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("s", "a.fixtures/fail_1.json"),
                ("s", "a.rego"),
                ("s", "b.rego"),
                ("s", "m.fixtures"),
                ("s", "z.fixtures"),
                ("t", "a.rego"),
                ("u", "x.rego"),
            ]
        );
    }

    #[test]
    fn summary_counts() {
        let mut report = TestReport::new(vec![info("s")]);
        report.push_rule(rule("s", "a.rego", &["fail_x.json", "pass_y.json"]));
        let mut not_run = rule("s", "b.rego", &["fail_x.json"]);
        not_run.fixtures[0].status = FixtureStatus::NotRun;
        report.push_rule(not_run);
        report.push_rule(rule("s", "c.rego", &["pass_a.json"]));
        report.add_failure(failure(
            "s",
            "a.fixtures/fail_x.json",
            RuleFailureKind::FixtureInvalid,
        ));
        report.add_failure(failure("s", "b.rego", RuleFailureKind::MetadataInvalid));
        report.add_failure(failure(
            "s",
            "gone.fixtures",
            RuleFailureKind::OrphanFixtures,
        ));
        let s = report.summary();
        assert_eq!(
            s.rules,
            RuleCounts {
                total: 3,
                passed: 1,
                failed: 2
            }
        );
        assert_eq!(
            s.fixtures,
            FixtureCounts {
                total: 4,
                passed: 2,
                failed: 1,
                not_run: 1
            }
        );
        assert_eq!(s.failures, 3);
    }
}
