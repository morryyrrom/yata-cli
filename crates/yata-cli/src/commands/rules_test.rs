//! `yata rules test`: checks the rules of policy directories with their fixtures and against the
//! resource types the provider supports.

mod json;
mod text;

use std::collections::BTreeSet;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;

use yata_engine::testing::{TestReport, run_tests};
use yata_engine::{
    FailureDetails, LoadMode, LoadOptions, PolicyLoadError, ResourceLimits, RuleFailure,
    RuleFailureKind, YataVersion,
};

use super::CommandContext;
use crate::cli::{RulesTestArgs, RulesTestFormat};
use crate::exit_code::{self, CliError, Interrupted};
use crate::policy_dir;
use crate::sanitize::escape_controls;

/// Runs `yata rules test` and returns the exit code (0 or 1). Errors are mapped to exit codes
/// by the caller; Ctrl-C ends the process with 130 from here.
pub async fn run(args: &RulesTestArgs, context: &CommandContext) -> Result<i32, CliError> {
    let dirs = policy_dir::resolve(&args.policy.policy_dir, context.policy_dir_env.clone())?;
    let yata_version = YataVersion::from_package_version(env!("CARGO_PKG_VERSION"))
        .map_err(|err| CliError::Runtime(format!("invalid package version: {err}")))?;
    let options = LoadOptions {
        yata_version,
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(args.policy_timeout),
        mode: LoadMode::RulesWithFixtures,
    };

    let outcome = load_and_test(dirs, options).await?;
    let mut report = outcome.map_err(|err| CliError::Usage(format_load_error(&err)))?;

    let supported = supported_types();
    add_unsupported_type_failures(&mut report, &supported);
    let summary = report.summary();
    tracing::info!(
        rules = summary.rules.total,
        fixtures = summary.fixtures.total,
        failures = summary.failures,
        "tested the rules"
    );

    write_report(&report, args.format, context.stdout_color)?;
    Ok(if report.passed() {
        exit_code::SUCCESS
    } else {
        exit_code::RULE_FAILURES
    })
}

/// Runs `run_tests` on a blocking thread while waiting for Ctrl-C. If Ctrl-C comes first, writes
/// to standard error and ends the process with 130 (nothing is written to standard output).
async fn load_and_test(
    dirs: Vec<PathBuf>,
    options: LoadOptions,
) -> Result<Result<TestReport, PolicyLoadError>, CliError> {
    let work = tokio::task::spawn_blocking(move || run_tests(&dirs, &options));
    match exit_code::run_or_interrupt(work, exit_code::ctrl_c()).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(err)) => Err(CliError::Runtime(format!("the test task failed: {err}"))),
        // The runtime is not dropped: dropping it would wait for the blocking task.
        Err(Interrupted) => exit_code::exit_interrupted(),
    }
}

/// The resource types the AWS provider can collect.
fn supported_types() -> BTreeSet<String> {
    yata_provider_aws::supported_resource_types()
        .into_iter()
        .map(|spec| spec.resource_type)
        .collect()
}

/// The declared types (the target type first, then the related types, without duplicates) that
/// the provider does not support. A rule whose `target_type` does not start with `aws` is
/// treated as supporting none.
fn unsupported_types(
    target_type: &str,
    related_types: &[String],
    supported: &BTreeSet<String>,
) -> Vec<String> {
    let aws_rule = target_type.split('.').next() == Some("aws");
    let mut unsupported: Vec<String> = Vec::new();
    for declared in std::iter::once(target_type).chain(related_types.iter().map(String::as_str)) {
        let is_supported = aws_rule && supported.contains(declared);
        if !is_supported && !unsupported.iter().any(|t| t == declared) {
            unsupported.push(declared.to_owned());
        }
    }
    unsupported
}

/// Adds an `unsupported_resource_type` failure for every loaded rule that declares a type the
/// provider does not support.
fn add_unsupported_type_failures(report: &mut TestReport, supported: &BTreeSet<String>) {
    let failures: Vec<RuleFailure> = report
        .rules()
        .iter()
        .filter_map(|rule| {
            let target = rule.target_type()?;
            let types = unsupported_types(target, rule.related_types(), supported);
            if types.is_empty() {
                return None;
            }
            Some(RuleFailure {
                policy_set: rule.policy_set().to_owned(),
                rule_id: rule.id().map(str::to_owned),
                path: rule.path().to_owned(),
                kind: RuleFailureKind::UnsupportedResourceType,
                message: "the rule declares a resource type that the provider does not support"
                    .to_owned(),
                details: Some(FailureDetails::Types { types }),
            })
        })
        .collect();
    for failure in failures {
        report.add_failure(failure);
    }
}

/// Formats a directory-level load failure (and the rule-level failures found with it) for
/// standard error. Text that comes from the policy directory is escaped.
fn format_load_error(err: &PolicyLoadError) -> String {
    let mut out = err.to_string();
    for failure in &err.directory_failures {
        let dir = failure.policy_dir.display().to_string();
        let place = match &failure.path {
            Some(path) => format!("{dir} ({path})"),
            None => dir,
        };
        out.push_str(&format!(
            "\n  [{}] {}\n    {}",
            failure.kind,
            escape_controls(&place),
            escape_controls(&failure.message)
        ));
    }
    if !err.rule_failures.is_empty() {
        out.push_str("\nrule-level failures found before the directory-level failure:");
        for failure in &err.rule_failures {
            out.push_str(&format!(
                "\n  [{}] {}: {}\n    {}",
                failure.kind,
                escape_controls(&failure.policy_set),
                escape_controls(&failure.path),
                escape_controls(&failure.message)
            ));
        }
    }
    out
}

/// Writes the result to standard output.
fn write_report(report: &TestReport, format: RulesTestFormat, color: bool) -> Result<(), CliError> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let written = match format {
        RulesTestFormat::Text => text::write(&mut out, report, color),
        RulesTestFormat::Json => json::write(&mut out, report),
    };
    written.and_then(|()| out.flush()).map_err(CliError::Output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported() -> BTreeSet<String> {
        ["aws.account.account", "aws.s3.bucket"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn supported_types_pass() {
        let related = strings(&["aws.s3.bucket"]);
        assert!(unsupported_types("aws.account.account", &related, &supported()).is_empty());
    }

    #[test]
    fn unsupported_types_are_listed_in_declaration_order_without_duplicates() {
        let related = strings(&[
            "aws.s3.bucket",
            "aws.example.b",
            "aws.example.a",
            "aws.example.b",
        ]);
        assert_eq!(
            unsupported_types("aws.example.a", &related, &supported()),
            strings(&["aws.example.a", "aws.example.b"])
        );
    }

    #[test]
    fn a_rule_of_another_provider_supports_nothing() {
        let related = strings(&["aws.s3.bucket"]);
        assert_eq!(
            unsupported_types("gcp.storage.bucket", &related, &supported()),
            strings(&["gcp.storage.bucket", "aws.s3.bucket"])
        );
    }

    #[test]
    fn the_real_provider_supports_the_account_type() {
        assert!(supported_types().contains("aws.account.account"));
    }
}
