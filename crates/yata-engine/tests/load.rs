//! Tests for loading and validating policy directories (stage P1).
//!
//! Cases that need special file system entries (links, junctions, big files, limits, unreadable
//! files) are built in temporary directories; the committed sets are in
//! `tests/fixtures/policies/`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yata_engine::{
    DirectoryFailureKind as D, LoadMode, LoadOptions, LoadedPolicies, PolicyLoadError,
    ResourceLimits, RuleFailure, RuleFailureKind as R, YataVersion, load_policies,
};

fn policies(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/policies")
        .join(sub)
}

fn options(mode: LoadMode) -> LoadOptions {
    LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(30),
        mode,
    }
}

fn load(dirs: &[PathBuf], options: &LoadOptions) -> Result<LoadedPolicies, PolicyLoadError> {
    load_policies(dirs, options)
}

fn load_one(dir: &Path, options: &LoadOptions) -> Result<LoadedPolicies, PolicyLoadError> {
    load(&[dir.to_path_buf()], options)
}

/// The kinds of the directory-level failures of an `Err` result.
fn dir_kinds(result: Result<LoadedPolicies, PolicyLoadError>) -> Vec<D> {
    match result {
        Ok(loaded) => panic!("expected a directory-level failure, got {loaded:?}"),
        Err(e) => e.directory_failures.iter().map(|f| f.kind).collect(),
    }
}

fn dir_failure(result: Result<LoadedPolicies, PolicyLoadError>) -> yata_engine::DirectoryFailure {
    match result {
        Ok(loaded) => panic!("expected a directory-level failure, got {loaded:?}"),
        Err(mut e) => {
            assert_eq!(e.directory_failures.len(), 1, "{e:?}");
            e.directory_failures.remove(0)
        }
    }
}

fn rule_failure_pairs(failures: &[RuleFailure]) -> Vec<(String, R)> {
    failures.iter().map(|f| (f.path.clone(), f.kind)).collect()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

const MANIFEST: &str =
    r#"{"format_version": 1, "name": "tmp", "version": "0.1.0", "min_yata_version": "0.1.0"}"#;

/// A rule with valid metadata. `package` is the part after `yata.rules.tmp.`.
fn rule_source(package: &str) -> String {
    format!(
        r#"package yata.rules.tmp.{package}

import rego.v1

metadata := {{
	"title": "Temp rule",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}}

fail contains 1 if {{ false }}
"#
    )
}

/// A temporary policy directory with a manifest and one rule `tmp.r` in `r.rego`.
fn temp_policy_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "r.rego", &rule_source("r"));
    dir
}

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

    // The passing rule is loaded.
    let ids: Vec<&str> = loaded.rules().map(|r| r.id()).collect();
    assert_eq!(ids, ["test_policies.ok.passing_rule"]);

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

// ------------------------------------------------------------------ file sizes

#[test]
fn rule_and_library_over_the_size_limit() {
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_rego_bytes = 100;

    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "small.rego", "package yata.rules.tmp.small\n");
    write(dir.path(), "big.rego", &rule_source("big"));
    let loaded = load_one(dir.path(), &opts).unwrap();
    let pairs = rule_failure_pairs(&loaded.rule_failures);
    assert!(
        pairs.contains(&("big.rego".to_string(), R::FileTooLarge)),
        "{pairs:?}"
    );

    let dir = temp_policy_dir();
    write(
        dir.path(),
        "lib/big.rego",
        &format!("package yata.lib.tmp.big\n\n# {}\n", "x".repeat(200)),
    );
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_rego_bytes = 150;
    // The rule is below 150 bytes? It is not, but the library is read first.
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::LibraryInvalid]);
}

#[test]
fn non_utf8_rule_is_a_rego_error_and_non_utf8_library_is_invalid() {
    let dir = temp_policy_dir();
    fs::write(dir.path().join("bad.rego"), [0xff, 0xfe, 0xfd]).unwrap();
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("bad.rego".to_string(), R::RegoError)]
    );

    fs::create_dir(dir.path().join("lib")).unwrap();
    fs::write(dir.path().join("lib/bad.rego"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::LibraryInvalid]
    );
}

// ------------------------------------------------------------------ walk limits

#[test]
fn file_count_limit() {
    let dir = temp_policy_dir();
    write(dir.path(), "notes/a.txt", "a");
    write(dir.path(), "notes/b.txt", "b");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 3; // r.rego + a.txt + b.txt (the manifest is not counted)
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.limits.max_files = 2;
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::TooManyFiles]);
}

#[test]
fn directory_count_limit_ignores_dot_directories() {
    let dir = temp_policy_dir();
    write(dir.path(), "a/x.txt", "x");
    write(dir.path(), "b/x.txt", "x");
    write(dir.path(), ".git/x.txt", "x");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_directories = 2;
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.limits.max_directories = 1;
    assert_eq!(
        dir_kinds(load_one(dir.path(), &opts)),
        [D::TooManyDirectories]
    );
}

#[test]
fn files_in_dot_directories_are_not_counted() {
    let dir = temp_policy_dir();
    write(dir.path(), ".git/a", "x");
    write(dir.path(), ".git/b", "x");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 1;
    assert!(load_one(dir.path(), &opts).is_ok());
}

#[test]
fn depth_limit() {
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_depth = 3;

    // A file at depth 3 is fine.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    assert!(load_one(dir.path(), &opts).is_ok());

    // A directory at depth 3 with something inside is too deep.
    write(dir.path(), "a/b/c/x.txt", "x");
    let f = dir_failure(load_one(dir.path(), &opts));
    assert_eq!(f.kind, D::DepthExceeded);
    assert_eq!(f.path.as_deref(), Some("a/b/c"));

    // A name starting with a dot counts as something inside.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    write(dir.path(), "a/b/c/.keep", "");
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::DepthExceeded]);

    // An empty directory at depth 3 is not a failure.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    assert!(load_one(dir.path(), &opts).is_ok());

    // A dot directory at depth 3 is skipped without looking inside.
    write(dir.path(), "a/b/.cache/x", "x");
    assert!(load_one(dir.path(), &opts).is_ok());
}

#[test]
fn fixtures_at_the_depth_limit_depend_on_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    write(dir.path(), "a/b/r.rego", &rule_source("a.b.r"));
    write(dir.path(), "a/b/r.fixtures/pass_x.json", "{}");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_depth = 3;
    // The fixture directory is at depth 3 and its content at depth 4.
    let loaded = load_one(dir.path(), &opts).unwrap();
    assert_eq!(loaded.rules().count(), 1);
    opts.mode = LoadMode::RulesWithFixtures;
    let f = dir_failure(load_one(dir.path(), &opts));
    assert_eq!(f.kind, D::DepthExceeded);
    assert_eq!(f.path.as_deref(), Some("a/b/r.fixtures"));
}

#[test]
fn fixtures_directory_contents_are_counted_only_with_fixtures() {
    let dir = temp_policy_dir();
    write(dir.path(), "r.fixtures/pass_a.json", "{}");
    write(dir.path(), "r.fixtures/fail_a.json", "{}");
    let mut opts = options(LoadMode::Rules);
    opts.limits.max_files = 1;
    assert!(load_one(dir.path(), &opts).is_ok());
    opts.mode = LoadMode::RulesWithFixtures;
    assert_eq!(dir_kinds(load_one(dir.path(), &opts)), [D::TooManyFiles]);
}

#[test]
fn non_rego_files_and_dot_directories_are_ignored() {
    let dir = temp_policy_dir();
    write(dir.path(), "notes.txt", "x");
    write(dir.path(), ".git/broken.rego", "package (");
    write(dir.path(), "docs/readme.md", "x");
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    assert_eq!(loaded.rules().count(), 1);
}

#[test]
fn rules_in_lib_are_libraries_and_need_no_valid_names() {
    let dir = temp_policy_dir();
    // Names below `lib/` are not checked.
    write(
        dir.path(),
        "lib/Odd-Name/1.rego",
        "package yata.lib.tmp.odd\n\nf(x) := x\n",
    );
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rules().count(), 1);
    assert!(loaded.rule_failures.is_empty());
}

// ------------------------------------------------------------------ links

/// Creates a symbolic link to a file. Returns false (after printing the reason to stderr) when it
/// cannot be made.
///
/// On Windows, creating symbolic links needs Developer Mode or an elevated prompt (otherwise
/// os error 1314); run the tests that way to exercise the link tests. They are skipped
/// otherwise, and the junction tests still run.
fn symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(target, link);
    report_skip(result)
}

/// Creates a symbolic link to a directory. See [`symlink_file`].
fn symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    report_skip(result)
}

fn report_skip(result: std::io::Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping: cannot create a symbolic link here: {e}");
            false
        }
    }
}

#[test]
fn symlinked_rule_file_is_a_rule_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "x.rego", &rule_source("linked"));
    if !symlink_file(
        &outside.path().join("x.rego"),
        &dir.path().join("linked.rego"),
    ) {
        return;
    }
    for mode in [LoadMode::Rules, LoadMode::RulesWithFixtures] {
        let loaded = load_one(dir.path(), &options(mode)).unwrap();
        assert_eq!(
            rule_failure_pairs(&loaded.rule_failures),
            [("linked.rego".to_string(), R::Symlink)]
        );
        assert_eq!(
            loaded.rule_failures[0].rule_id.as_deref(),
            Some("tmp.linked")
        );
        assert_eq!(loaded.rules().count(), 1);
    }
}

#[test]
fn symlinked_rule_file_alone_is_not_no_rules() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "yata-policies.json", MANIFEST);
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "x.rego", &rule_source("linked"));
    if !symlink_file(
        &outside.path().join("x.rego"),
        &dir.path().join("linked.rego"),
    ) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rule_failures.len(), 1);
}

#[test]
fn symlinked_directory_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "inner/x.rego", &rule_source("inner.x"));
    if !symlink_dir(&outside.path().join("inner"), &dir.path().join("linked")) {
        return;
    }
    let f = dir_failure(load_one(dir.path(), &options(LoadMode::Rules)));
    assert_eq!(f.kind, D::Symlink);
    assert_eq!(f.path.as_deref(), Some("linked"));
}

#[test]
fn symlinked_non_rego_file_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "notes.txt", "x");
    if !symlink_file(
        &outside.path().join("notes.txt"),
        &dir.path().join("notes.txt"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::Symlink]
    );
}

#[test]
fn symlinked_library_is_a_directory_level_failure() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "h.rego", "package yata.lib.tmp.h\n");
    fs::create_dir(dir.path().join("lib")).unwrap();
    if !symlink_file(
        &outside.path().join("h.rego"),
        &dir.path().join("lib/h.rego"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::Symlink]
    );
}

#[test]
fn symlinked_manifest_counts_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "r.rego", &rule_source("r"));
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "m.json", MANIFEST);
    if !symlink_file(
        &outside.path().join("m.json"),
        &dir.path().join("yata-policies.json"),
    ) {
        return;
    }
    assert_eq!(
        dir_kinds(load_one(dir.path(), &options(LoadMode::Rules))),
        [D::ManifestMissing]
    );
}

#[test]
fn symlinked_root_is_followed() {
    let real = temp_policy_dir();
    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("policies");
    if !symlink_dir(real.path(), &link) {
        return;
    }
    let loaded = load_one(&link, &options(LoadMode::Rules)).unwrap();
    assert_eq!(loaded.rules().count(), 1);
}

#[test]
fn symlink_inside_a_fixture_directory_depends_on_the_mode() {
    let dir = temp_policy_dir();
    write(dir.path(), "r.fixtures/pass_a.json", "{}");
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "f.json", "{}");
    if !symlink_file(
        &outside.path().join("f.json"),
        &dir.path().join("r.fixtures/fail_b.json"),
    ) {
        return;
    }
    // Not read, so not a failure.
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    // With fixtures it is a failure of the rule.
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("r.fixtures/fail_b.json".to_string(), R::Symlink)]
    );
    assert_eq!(loaded.rule_failures[0].rule_id.as_deref(), Some("tmp.r"));
}

#[test]
fn symlinked_fixture_directory_depends_on_the_mode() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "fx/pass_a.json", "{}");
    if !symlink_dir(&outside.path().join("fx"), &dir.path().join("r.fixtures")) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
    assert!(loaded.rule_failures.is_empty());
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("r.fixtures".to_string(), R::Symlink)]
    );
}

#[test]
fn symlinked_fixture_directory_without_a_rule_is_an_orphan() {
    let dir = temp_policy_dir();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), "fx/pass_a.json", "{}");
    if !symlink_dir(
        &outside.path().join("fx"),
        &dir.path().join("gone.fixtures"),
    ) {
        return;
    }
    let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
    assert_eq!(
        rule_failure_pairs(&loaded.rule_failures),
        [("gone.fixtures".to_string(), R::OrphanFixtures)]
    );
}

#[cfg(windows)]
mod junction {
    use super::*;
    use std::process::Command;

    /// Creates a junction with `cmd /C mklink /J`.
    fn make(target: &Path, link: &Path) -> bool {
        let output = Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output();
        match output {
            Ok(o) if o.status.success() => true,
            other => {
                eprintln!("skipping: cannot create a junction: {other:?}");
                false
            }
        }
    }

    #[test]
    fn junction_directory_is_a_directory_level_failure() {
        let dir = temp_policy_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "inner/x.rego", &rule_source("inner.x"));
        if !make(&outside.path().join("inner"), &dir.path().join("linked")) {
            return;
        }
        let f = dir_failure(load_one(dir.path(), &options(LoadMode::Rules)));
        assert_eq!(f.kind, D::Symlink);
        assert_eq!(f.path.as_deref(), Some("linked"));
    }

    #[test]
    fn junction_as_fixture_directory_depends_on_the_mode() {
        let dir = temp_policy_dir();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "fx/pass_a.json", "{}");
        if !make(&outside.path().join("fx"), &dir.path().join("r.fixtures")) {
            return;
        }
        let loaded = load_one(dir.path(), &options(LoadMode::Rules)).unwrap();
        assert!(loaded.rule_failures.is_empty());
        let loaded = load_one(dir.path(), &options(LoadMode::RulesWithFixtures)).unwrap();
        assert_eq!(
            rule_failure_pairs(&loaded.rule_failures),
            [("r.fixtures".to_string(), R::Symlink)]
        );
    }

    #[test]
    fn junction_root_is_followed() {
        let real = temp_policy_dir();
        let parent = tempfile::tempdir().unwrap();
        let link = parent.path().join("policies");
        if !make(real.path(), &link) {
            return;
        }
        let loaded = load_one(&link, &options(LoadMode::Rules)).unwrap();
        assert_eq!(loaded.rules().count(), 1);
    }
}

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
