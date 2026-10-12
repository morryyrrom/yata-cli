//! Tests of the collector discovery rules in `build/discover.rs`.
#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../build/discover.rs"]
mod discover;

use std::fs;
use std::path::Path;

use discover::{discover, generate};

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("has a parent")).expect("create directories");
    fs::write(path, "").expect("write file");
}

fn mkdir(root: &Path, relative: &str) {
    fs::create_dir_all(root.join(relative)).expect("create directory");
}

fn types(root: &Path) -> Vec<String> {
    discover(root)
        .expect("discovery succeeds")
        .iter()
        .map(|e| e.resource_type())
        .collect()
}

#[test]
fn missing_directory_gives_an_empty_list() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("collectors");
    assert!(discover(&missing).expect("ok").is_empty());
    let code = generate(&[]);
    assert!(code.contains("Vec::new()"));
    assert!(code.contains("COLLECTOR_PATH_TYPES: &[&str] = &[\n];"));
}

#[test]
fn valid_layout_is_sorted_by_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, "s3/bucket.rs");
    touch(root, "iam/user.rs");
    touch(root, "iam/access_key.rs");
    touch(root, "ec2/security_group.rs");
    assert_eq!(
        types(root),
        [
            "aws.ec2.security_group",
            "aws.iam.access_key",
            "aws.iam.user",
            "aws.s3.bucket"
        ]
    );
}

#[test]
fn generated_code_lists_modules_collectors_and_types_in_order() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, "iam/user.rs");
    touch(root, "s3/bucket_policy.rs");
    let entries = discover(root).expect("ok");
    let code = generate(&entries);

    let iam_path = format!("{:?}", entries[0].path.to_string_lossy());
    assert!(code.contains(&format!("#[path = {iam_path}]\nmod iam_user;")));
    assert!(code.contains("mod s3_bucket_policy;"));
    let iam_call = code.find("iam_user::collector(),").expect("iam call");
    let s3_call = code
        .find("s3_bucket_policy::collector(),")
        .expect("s3 call");
    assert!(iam_call < s3_call);
    assert!(code.contains("&[\n    \"aws.iam.user\",\n    \"aws.s3.bucket_policy\",\n];"));
}

#[test]
fn non_rs_files_are_ignored_anywhere() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, ".gitkeep");
    touch(root, "README.md");
    touch(root, "Weird-Name.txt");
    touch(root, "s3/.gitkeep");
    touch(root, "s3/NOTES.md");
    touch(root, "s3/Bad-Name.json");
    touch(root, "s3/bucket.rs");
    assert_eq!(types(root), ["aws.s3.bucket"]);
}

#[test]
fn invalid_layouts_are_errors() {
    let cases: &[(&str, &str)] = &[
        ("direct .rs file", "bucket.rs"),
        ("mod.rs", "s3/mod.rs"),
        ("third level directory", "s3/nested/deep.rs"),
        ("empty third level directory", "s3/nested/.gitkeep"),
        ("upper-case resource", "s3/Bucket.rs"),
        ("hyphen in resource", "s3/my-bucket.rs"),
        ("dot in resource", "s3/my.bucket.rs"),
        ("digit first", "s3/1bucket.rs"),
        ("underscore in service", "s3_control/bucket.rs"),
        ("upper-case service", "S3/bucket.rs"),
        ("hyphen in service", "s3-control/bucket.rs"),
        ("reserved service", "default/bucket.rs"),
        ("reserved resource", "s3/default.rs"),
        ("reserved resource in", "s3/in.rs"),
    ];
    for (label, relative) in cases {
        let dir = tempfile::tempdir().expect("temp dir");
        touch(dir.path(), relative);
        assert!(discover(dir.path()).is_err(), "{label}: {relative}");
    }
}

#[test]
fn empty_directory_with_invalid_name_is_an_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    mkdir(dir.path(), "Bad_Service");
    assert!(discover(dir.path()).is_err());
}

#[test]
fn empty_directory_with_valid_name_is_fine() {
    let dir = tempfile::tempdir().expect("temp dir");
    mkdir(dir.path(), "s3");
    assert!(discover(dir.path()).expect("ok").is_empty());
}

#[test]
fn module_names_do_not_collide() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, "s3/bucket_policy.rs");
    touch(root, "s3/bucket.rs");
    let entries = discover(root).expect("ok");
    let mut names: Vec<String> = entries.iter().map(|e| e.module_name()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 2);
}

/// Creates a symbolic link; returns false (after printing why) if the platform or the
/// privileges do not allow it, so the test can be skipped.
fn try_symlink(target: &Path, link: &Path, dir: bool) -> bool {
    #[cfg(unix)]
    let result = {
        let _ = dir;
        std::os::unix::fs::symlink(target, link)
    };
    #[cfg(windows)]
    let result = if dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    #[cfg(not(any(unix, windows)))]
    let result: std::io::Result<()> = {
        let _ = (target, link, dir);
        Err(std::io::Error::other("symbolic links are not supported"))
    };
    match result {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping: cannot create a symbolic link here: {e}");
            false
        }
    }
}

#[test]
fn file_symbolic_links_are_errors() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, "s3/bucket.rs");
    if try_symlink(&root.join("s3/bucket.rs"), &root.join("s3/link.rs"), false) {
        assert!(discover(root).is_err());
    }
}

#[test]
fn directory_symbolic_links_are_errors() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    touch(root, "s3/bucket.rs");
    if try_symlink(&root.join("s3"), &root.join("iam"), true) {
        assert!(discover(root).is_err());
    }
    let other = tempfile::tempdir().expect("temp dir");
    mkdir(other.path(), "target");
    touch(other.path(), "s3/bucket.rs");
    if try_symlink(
        &other.path().join("target"),
        &other.path().join("s3/nested"),
        true,
    ) {
        assert!(discover(other.path()).is_err());
    }
}
