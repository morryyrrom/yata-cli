//! The text output of `yata rules test` (for people; the format is not stable).

use std::io::{self, Write};

use yata_engine::testing::{FixtureStatus, RuleTestResult, TestReport};
use yata_engine::{FailureDetails, RuleFailure};

use crate::sanitize::escape_controls;

const GREEN: &str = "\u{1b}[32m";
const RED: &str = "\u{1b}[31m";
const RESET: &str = "\u{1b}[0m";
/// Width of the `ok` / `FAILED` column, including the gap.
const STATUS_WIDTH: usize = 8;

/// Writes the report. Text that comes from the policy directories is escaped with
/// [`escape_controls`]; paths are relative to the policy directories.
pub fn write(out: &mut dyn Write, report: &TestReport, color: bool) -> io::Result<()> {
    write_rules(out, report, color)?;
    write_failures(out, report)?;
    write_summary(out, report, color)
}

fn paint(text: &str, code: &str, color: bool) -> String {
    if color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_owned()
    }
}

fn status_cell(passed: bool, color: bool) -> String {
    let (word, code) = if passed {
        ("ok", GREEN)
    } else {
        ("FAILED", RED)
    };
    let padding = " ".repeat(STATUS_WIDTH.saturating_sub(word.len()));
    format!("{}{padding}", paint(word, code, color))
}

/// A rule is "not run" when it failed to load or all its fixtures were skipped.
fn is_not_run(rule: &RuleTestResult) -> bool {
    rule.target_type().is_none()
        || (!rule.fixtures().is_empty()
            && rule
                .fixtures()
                .iter()
                .all(|f| f.status == FixtureStatus::NotRun))
}

fn rule_label(rule: &RuleTestResult) -> String {
    escape_controls(rule.id().unwrap_or_else(|| rule.path())).into_owned()
}

fn write_rules(out: &mut dyn Write, report: &TestReport, color: bool) -> io::Result<()> {
    for (index, policy) in report.policies().iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        let rules: Vec<&RuleTestResult> = report
            .rules()
            .iter()
            .filter(|rule| rule.policy_set() == policy.name)
            .collect();
        let noun = if rules.len() == 1 { "rule" } else { "rules" };
        writeln!(
            out,
            "{} {} (format {}): {} {noun}\n",
            escape_controls(&policy.name),
            escape_controls(&policy.version),
            policy.format_version,
            rules.len()
        )?;
        for rule in rules {
            let passed = rule.status() == yata_engine::testing::RuleTestStatus::Passed;
            let detail = if is_not_run(rule) {
                "not run".to_owned()
            } else if rule.fixtures().len() == 1 {
                "1 fixture".to_owned()
            } else {
                format!("{} fixtures", rule.fixtures().len())
            };
            writeln!(
                out,
                "{}{} ({detail})",
                status_cell(passed, color),
                rule_label(rule)
            )?;
        }
    }
    Ok(())
}

/// The rule a failure belongs to: the same policy set and a path that is the rule file, its
/// fixture directory or something inside it.
fn owner(report: &TestReport, failure: &RuleFailure) -> Option<usize> {
    report.rules().iter().position(|rule| {
        if rule.policy_set() != failure.policy_set {
            return false;
        }
        let stem = rule.path().strip_suffix(".rego").unwrap_or(rule.path());
        let dir = format!("{stem}.fixtures");
        failure.path == rule.path()
            || failure.path == dir
            || failure
                .path
                .strip_prefix(&dir)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn detail_lines(details: &FailureDetails) -> Vec<String> {
    let mut lines = Vec::new();
    match details {
        FailureDetails::Mismatch {
            missing,
            unexpected,
        } => {
            for key in missing {
                lines.push(format!(
                    "missing:    {} ({})",
                    escape_controls(&key.id),
                    escape_controls(&key.region)
                ));
            }
            for item in unexpected {
                let who = format!(
                    "{} ({})",
                    escape_controls(&item.id),
                    escape_controls(&item.region)
                );
                if item.messages.is_empty() {
                    lines.push(format!("unexpected: {who}"));
                }
                for message in &item.messages {
                    lines.push(format!("unexpected: {who}: {}", escape_controls(message)));
                }
            }
        }
        FailureDetails::Locations { locations } => {
            for location in locations {
                lines.push(format!("location: {}", escape_controls(location)));
            }
        }
        FailureDetails::Types { types } => {
            let escaped: Vec<_> = types.iter().map(|t| escape_controls(t)).collect();
            lines.push(format!("unsupported types: {}", escaped.join(", ")));
        }
        FailureDetails::Timeout { timeout_seconds } => {
            lines.push(format!("time limit: {timeout_seconds} s"));
        }
        // New kinds of details: nothing is printed beyond the message.
        _ => {}
    }
    lines
}

fn write_failures(out: &mut dyn Write, report: &TestReport) -> io::Result<()> {
    let failures = report.failures();
    if failures.is_empty() {
        return Ok(());
    }
    let several_sets = report.policies().len() > 1;
    writeln!(out, "\nfailures:")?;
    // The failures arrive grouped by rule; a new heading starts when the owner changes.
    let mut current: Option<(String, Option<usize>)> = None;
    for failure in failures {
        let owner = owner(report, failure);
        let key = (failure.policy_set.clone(), owner);
        if current.as_ref() != Some(&key) {
            let heading = match owner.and_then(|i| report.rules().get(i)) {
                Some(rule) => rule_label(rule),
                None if several_sets => {
                    format!("(no rule: {})", escape_controls(&failure.policy_set))
                }
                None => "(no rule)".to_owned(),
            };
            writeln!(out, "\n{heading}")?;
            current = Some(key);
        }
        writeln!(
            out,
            "  [{}] {}\n    {}",
            failure.kind,
            escape_controls(&failure.path),
            escape_controls(&failure.message)
        )?;
        if let Some(details) = &failure.details {
            for line in detail_lines(details) {
                writeln!(out, "    {line}")?;
            }
        }
    }
    Ok(())
}

fn write_summary(out: &mut dyn Write, report: &TestReport, color: bool) -> io::Result<()> {
    let s = report.summary();
    let verdict = if report.passed() {
        paint("ok", GREEN, color)
    } else {
        paint("FAILED", RED, color)
    };
    writeln!(
        out,
        "\ntest result: {verdict}. {} passed; {} failed; {} failures (fixtures: {} passed, {} failed, {} not run)",
        s.rules.passed,
        s.rules.failed,
        s.failures,
        s.fixtures.passed,
        s.fixtures.failed,
        s.fixtures.not_run
    )
}
