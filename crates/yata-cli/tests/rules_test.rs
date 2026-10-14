//! Integration tests of `yata rules test`: the `yata` binary runs as a child process on the
//! policy sets in `tests/fixtures/policies/`. Nothing here connects to AWS; the environment of
//! the child is set up so that it could not read credentials or configuration of the user.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// The ESC character that starts ANSI escape sequences.
const ESC: char = '\u{1b}';
/// Placeholder for the package version in the golden files, so that a version bump does not
/// break them.
const VERSION_PLACEHOLDER: &str = "@VERSION@";

fn policies(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policies")
        .join(name)
}

/// A sandbox for child processes: a temporary directory that holds the (missing) AWS files.
struct Sandbox {
    tmp: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    /// `yata` with an environment that cannot reach AWS or pick up the user's settings.
    fn yata(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_yata"));
        // Remove every variable of the parent that could change the result or reach AWS.
        for (name, _) in std::env::vars_os() {
            let upper = name.to_string_lossy().to_uppercase();
            if upper.starts_with("AWS_") || upper.starts_with("YATA_") || upper == "NO_COLOR" {
                cmd.env_remove(&name);
            }
        }
        // Never read the credentials or the configuration of the user.
        cmd.env(
            "AWS_CONFIG_FILE",
            self.tmp.path().join("missing-aws-config"),
        );
        cmd.env(
            "AWS_SHARED_CREDENTIALS_FILE",
            self.tmp.path().join("missing-aws-credentials"),
        );
        cmd.env("AWS_EC2_METADATA_DISABLED", "true");
        cmd
    }

    /// `yata rules test --policy-dir <set> <extra args...>`.
    fn rules_test(&self, set: &str, extra: &[&str]) -> Command {
        let mut cmd = self.yata();
        cmd.args(["rules", "test", "--policy-dir"])
            .arg(policies(set))
            .args(extra);
        cmd
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(mut cmd: Command) -> Run {
    let output = cmd.output().unwrap();
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn json(run: &Run) -> Value {
    serde_json::from_str(&run.stdout).unwrap_or_else(|e| panic!("{e}: {}", run.stdout))
}

/// Compares `actual` with `tests/golden/<name>`. With `YATA_UPDATE_GOLDEN=1` the file is
/// rewritten instead. The package version is replaced by a placeholder on both sides.
fn assert_golden(name: &str, actual: &str) {
    let version_line = format!(
        "\"name\": \"yata\",\n    \"version\": \"{}\"",
        env!("CARGO_PKG_VERSION")
    );
    let placeholder_line =
        format!("\"name\": \"yata\",\n    \"version\": \"{VERSION_PLACEHOLDER}\"");
    assert_eq!(
        actual.matches(&version_line).count(),
        1,
        "the tool version is missing in the output:\n{actual}"
    );
    let normalized = actual.replacen(&version_line, &placeholder_line, 1);

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var("YATA_UPDATE_GOLDEN").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, normalized).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|e| {
            panic!(
                "cannot read {}: {e} (YATA_UPDATE_GOLDEN=1 creates it)",
                path.display()
            )
        })
        .replace("\r\n", "\n");
    assert_eq!(
        normalized, expected,
        "output differs from {name} (YATA_UPDATE_GOLDEN=1 updates the file)"
    );
}

fn policy_set_names(output: &Value) -> Vec<&str> {
    output["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------------------------

#[test]
fn passing_set_exits_with_0() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_ok", &[]));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(
        result
            .stdout
            .contains("cli_ok_policies 1.2.3 (format 1): 2 rules")
    );
    assert!(
        result
            .stdout
            .contains("ok      cli_ok_policies.account_flag (2 fixtures)")
    );
    assert!(
        result
            .stdout
            .contains("ok      cli_ok_policies.nested.account_env (3 fixtures)")
    );
    assert!(
        result
            .stdout
            .contains("test result: ok. 2 passed; 0 failed; 0 failures")
    );
    assert!(!result.stdout.contains("failures:"));
}

#[test]
fn rule_level_failures_exit_with_1_and_write_the_result() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_fail", &[]));
    assert_eq!(result.code, 1, "{}", result.stderr);
    assert!(
        result
            .stdout
            .contains("FAILED  cli_fail_policies.expected_mismatch (2 fixtures)")
    );
    assert!(
        result
            .stdout
            .contains("[fixture_mismatch] expected_mismatch.fixtures/fail_not_flagged.json")
    );
    assert!(
        result
            .stdout
            .contains("missing:    aws-123456789012 (global)")
    );
    assert!(
        result
            .stdout
            .contains("test result: FAILED. 0 passed; 2 failed; 2 failures")
    );
}

#[test]
fn unsupported_resource_type_exits_with_1() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("unsupported_type", &["--format", "json"]));
    assert_eq!(result.code, 1, "{}", result.stderr);
    let output = json(&result);
    assert_eq!(output["result"], "failed");
    let failures = output["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["kind"], "unsupported_resource_type");
    assert_eq!(failures[0]["path"], "unsupported_widget.rego");
    assert_eq!(
        failures[0]["details"]["types"],
        serde_json::json!(["aws.example.unsupported"])
    );
    // The fixtures themselves pass; only the rule is marked as failed.
    assert_eq!(output["rules"][0]["status"], "failed");
    assert_eq!(output["summary"]["fixtures"]["passed"], 2);
    assert_golden("unsupported_type.json", &result.stdout);
}

#[test]
fn unsupported_resource_type_is_shown_in_text() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("unsupported_type", &[]));
    assert_eq!(result.code, 1);
    assert!(
        result
            .stdout
            .contains("[unsupported_resource_type] unsupported_widget.rego")
    );
    assert!(
        result
            .stdout
            .contains("unsupported types: aws.example.unsupported")
    );
}

#[test]
fn directory_level_failure_exits_with_2_and_writes_nothing_to_stdout() {
    let sandbox = Sandbox::new();
    for format in ["text", "json"] {
        let result = run(sandbox.rules_test("manifest_missing", &["--format", format]));
        assert_eq!(result.code, 2, "{format}");
        assert_eq!(result.stdout, "", "{format}");
        assert!(
            result.stderr.contains("[manifest_missing]"),
            "{}",
            result.stderr
        );
    }
}

#[test]
fn missing_policy_directory_exits_with_2() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--policy-dir"])
        .arg(sandbox.tmp.path().join("does-not-exist"));
    let result = run(cmd);
    assert_eq!(result.code, 2);
    assert_eq!(result.stdout, "");
    assert!(result.stderr.contains("[not_found]"), "{}", result.stderr);
}

#[test]
fn no_policy_directory_exits_with_2() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test"]);
    let result = run(cmd);
    assert_eq!(result.code, 2);
    assert_eq!(result.stdout, "");
    assert!(
        result.stderr.contains("YATA_POLICY_DIR"),
        "{}",
        result.stderr
    );
}

#[test]
fn argument_errors_exit_with_2_and_write_nothing_to_stdout() {
    let sandbox = Sandbox::new();
    let cases: [&[&str]; 6] = [
        &["--format", "xml"],
        &["--format", "table"],
        &["--policy-timeout", "0"],
        &["--policy-timeout", "3601"],
        &["--policy-timeout", "soon"],
        &["--color", "sometimes"],
    ];
    for extra in cases {
        let result = run(sandbox.rules_test("rules_test_ok", extra));
        assert_eq!(result.code, 2, "{extra:?}: {}", result.stderr);
        assert_eq!(result.stdout, "", "{extra:?}");
        assert!(!result.stderr.is_empty(), "{extra:?}");
    }
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "-v", "-q"]);
    assert_eq!(run(cmd).code, 2);
}

#[test]
fn the_longest_policy_timeout_is_accepted() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_ok", &["--policy-timeout", "3600"]));
    assert_eq!(result.code, 0, "{}", result.stderr);
}

#[test]
fn help_and_version_exit_with_0() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--help"]);
    let help = run(cmd);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("--policy-dir"));
    let mut cmd = sandbox.yata();
    cmd.arg("--version");
    let version = run(cmd);
    assert_eq!(version.code, 0);
    assert!(version.stdout.contains(env!("CARGO_PKG_VERSION")));
}

// ---------------------------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------------------------

#[test]
fn json_of_a_passing_set_matches_the_golden_file() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_ok", &["--format", "json"]));
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_golden("rules_test_ok.json", &result.stdout);
}

#[test]
fn json_of_a_failing_set_matches_the_golden_file() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_fail", &["--format", "json"]));
    assert_eq!(result.code, 1, "{}", result.stderr);
    assert_golden("rules_test_fail.json", &result.stdout);
}

#[test]
fn json_has_the_schema_version_and_the_tool() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_ok", &["--format", "json"]));
    let output = json(&result);
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["tool"]["name"], "yata");
    assert_eq!(output["tool"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(output["result"], "passed");
    // `schema_version` and `tool` come first (checked on the text; a parsed object has no order).
    let position = |key: &str| result.stdout.find(&format!("\"{key}\"")).unwrap();
    assert!(position("schema_version") < position("tool"));
    assert!(position("tool") < position("result"));
}

#[test]
fn json_keeps_control_characters_escaped() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_fail", &["--format", "json"]));
    assert!(!result.stdout.contains(ESC));
    let output = json(&result);
    let failures = output["failures"].as_array().unwrap();
    let messages: Vec<&str> = failures
        .iter()
        .flat_map(|f| f["details"]["unexpected"].as_array().into_iter().flatten())
        .flat_map(|u| u["messages"].as_array().unwrap())
        .map(|m| m.as_str().unwrap())
        .collect();
    assert_eq!(messages, ["red \u{1b}[31mtext\u{1b}[0m\nsecond line"]);
}

// ---------------------------------------------------------------------------------------------
// Text output
// ---------------------------------------------------------------------------------------------

#[test]
fn text_escapes_control_characters_of_the_rule_message() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_fail", &["--color", "never"]));
    assert_eq!(result.code, 1);
    assert!(!result.stdout.contains(ESC), "{:?}", result.stdout);
    assert!(
        result.stdout.contains(
            "unexpected: aws-123456789012 (global): red \\u{1b}[31mtext\\u{1b}[0m\\u{a}second line"
        ),
        "{}",
        result.stdout
    );
    // One value per line: no line was broken by the line feed of the message.
    assert!(!result.stdout.contains("\nsecond line"));
}

#[test]
fn text_groups_failures_by_rule() {
    let sandbox = Sandbox::new();
    let result = run(sandbox.rules_test("rules_test_fail", &["--color", "never"]));
    let failures = result.stdout.split("failures:\n").nth(1).unwrap();
    let control = failures
        .find("\ncli_fail_policies.control_message\n")
        .unwrap();
    let mismatch = failures
        .find("\ncli_fail_policies.expected_mismatch\n")
        .unwrap();
    assert!(control < mismatch, "rules are in path order");
}

#[test]
fn colors_follow_the_color_option_and_no_color() {
    let sandbox = Sandbox::new();
    let colored = format!("{ESC}[31mFAILED{ESC}[0m");
    // Not a terminal: no colors by default.
    let result = run(sandbox.rules_test("rules_test_fail", &[]));
    assert!(!result.stdout.contains(ESC));
    // The option turns them on, also when NO_COLOR is set.
    let result = run(sandbox.rules_test("rules_test_fail", &["--color", "always"]));
    assert!(result.stdout.contains(&colored), "{:?}", result.stdout);
    let mut cmd = sandbox.rules_test("rules_test_fail", &["--color", "always"]);
    cmd.env("NO_COLOR", "1");
    assert!(run(cmd).stdout.contains(&colored));
    // `never` and NO_COLOR turn them off.
    let result = run(sandbox.rules_test("rules_test_fail", &["--color", "never"]));
    assert!(!result.stdout.contains(ESC));
    let result = run(sandbox.rules_test("rules_test_ok", &["--color", "always"]));
    assert!(result.stdout.contains(&format!("{ESC}[32mok{ESC}[0m")));
    // Colors never reach the JSON output.
    let result = run(sandbox.rules_test(
        "rules_test_fail",
        &["--color", "always", "--format", "json"],
    ));
    assert!(!result.stdout.contains(ESC));
}

// ---------------------------------------------------------------------------------------------
// Policy directories: --policy-dir and YATA_POLICY_DIR
// ---------------------------------------------------------------------------------------------

#[test]
fn policy_dir_option_wins_over_the_environment_and_is_not_mixed_with_it() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.rules_test("rules_test_ok", &["--format", "json"]);
    cmd.env("YATA_POLICY_DIR", policies("rules_test_fail"));
    let result = run(cmd);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(policy_set_names(&json(&result)), ["cli_ok_policies"]);
}

#[test]
fn environment_variable_is_used_without_the_option() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--format", "json"])
        .env("YATA_POLICY_DIR", policies("rules_test_fail"));
    let result = run(cmd);
    assert_eq!(result.code, 1, "{}", result.stderr);
    assert_eq!(policy_set_names(&json(&result)), ["cli_fail_policies"]);
}

#[test]
fn environment_variable_accepts_several_directories() {
    let sandbox = Sandbox::new();
    let joined =
        std::env::join_paths([policies("unsupported_type"), policies("rules_test_ok")]).unwrap();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--format", "json"])
        .env("YATA_POLICY_DIR", joined);
    let result = run(cmd);
    assert_eq!(result.code, 1, "{}", result.stderr);
    assert_eq!(
        policy_set_names(&json(&result)),
        ["cli_unsupported_policies", "cli_ok_policies"]
    );
}

#[test]
fn repeated_policy_dir_options_are_used_in_the_given_order() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--format", "json", "--policy-dir"])
        .arg(policies("rules_test_ok"))
        .arg("--policy-dir")
        .arg(policies("rules_test_fail"));
    let result = run(cmd);
    assert_eq!(result.code, 1, "{}", result.stderr);
    let output = json(&result);
    assert_eq!(
        policy_set_names(&output),
        ["cli_ok_policies", "cli_fail_policies"]
    );
    assert_eq!(output["summary"]["rules"]["total"], 4);
}

#[test]
fn text_of_several_policy_directories_has_a_heading_for_each() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--policy-dir"])
        .arg(policies("rules_test_ok"))
        .arg("--policy-dir")
        .arg(policies("unsupported_type"));
    let result = run(cmd);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stdout
            .contains("cli_ok_policies 1.2.3 (format 1): 2 rules")
    );
    assert!(
        result
            .stdout
            .contains("cli_unsupported_policies 1.2.3 (format 1): 1 rule\n")
    );
}

#[test]
fn one_broken_directory_fails_the_whole_run_with_2() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.yata();
    cmd.args(["rules", "test", "--policy-dir"])
        .arg(policies("rules_test_ok"))
        .arg("--policy-dir")
        .arg(policies("manifest_missing"));
    let result = run(cmd);
    assert_eq!(result.code, 2);
    assert_eq!(result.stdout, "");
}

// ---------------------------------------------------------------------------------------------
// Logging: -v / -q / YATA_LOG
// ---------------------------------------------------------------------------------------------

#[test]
fn logs_go_to_stderr_and_follow_the_verbosity_options() {
    let sandbox = Sandbox::new();
    let quiet = run(sandbox.rules_test("rules_test_ok", &[]));
    assert_eq!(quiet.stderr, "");

    let verbose = run(sandbox.rules_test("rules_test_ok", &["-vv"]));
    assert_eq!(verbose.code, 0);
    assert!(
        verbose.stderr.contains("loading policy directory"),
        "{}",
        verbose.stderr
    );
    assert!(!verbose.stdout.contains("loading policy directory"));

    let info = run(sandbox.rules_test("rules_test_ok", &["-v"]));
    assert!(info.stderr.contains("tested the rules"), "{}", info.stderr);
    assert!(!info.stderr.contains("loading policy directory"));
}

#[test]
fn verbosity_options_win_over_yata_log() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.rules_test("rules_test_ok", &["-q"]);
    cmd.env("YATA_LOG", "debug");
    assert_eq!(run(cmd).stderr, "");

    let mut cmd = sandbox.rules_test("rules_test_ok", &[]);
    cmd.env("YATA_LOG", "debug");
    assert!(run(cmd).stderr.contains("loading policy directory"));
}

#[test]
fn invalid_yata_log_exits_with_2() {
    let sandbox = Sandbox::new();
    let mut cmd = sandbox.rules_test("rules_test_ok", &[]);
    cmd.env("YATA_LOG", "yata=nonsense=");
    let result = run(cmd);
    assert_eq!(result.code, 2);
    assert_eq!(result.stdout, "");
    // `-v` makes `YATA_LOG` unused, so its value does not matter.
    let mut cmd = sandbox.rules_test("rules_test_ok", &["-v"]);
    cmd.env("YATA_LOG", "yata=nonsense=");
    assert_eq!(run(cmd).code, 0);
}

#[test]
fn directory_level_failure_also_lists_the_rule_level_failures_found_so_far() {
    let sandbox = Sandbox::new();
    for format in ["text", "json"] {
        let mut cmd = sandbox.yata();
        cmd.args(["rules", "test", "--format", format, "--policy-dir"])
            .arg(policies("rule_load_failure"))
            .arg("--policy-dir")
            .arg(policies("manifest_missing"));
        let result = run(cmd);
        assert_eq!(result.code, 2, "{}", result.stderr);
        assert_eq!(result.stdout, "", "{format}");
        assert!(
            result.stderr.contains("[manifest_missing]"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("[metadata_invalid]"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("bad_metadata.rego"),
            "{}",
            result.stderr
        );
    }
}
