//! Source checks: SDK clients are created only through `support/clients.rs`, and
//! collectors do not touch `aws_config`.
//!
//! These are string checks. They can be bypassed with a type alias (for example
//! `use aws_sdk_s3::Client as S3;` followed by `S3::from_conf(`), so code review has to
//! cover that case.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("read source directory");
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn source_files() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&src)
                .expect("path is under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&path).expect("read source file");
            (relative, text)
        })
        .collect()
}

#[test]
fn clients_are_created_only_in_clients_rs() {
    let files = source_files();
    assert!(files.iter().any(|(name, _)| name == "support/clients.rs"));
    for (name, text) in &files {
        if name == "support/clients.rs" {
            continue;
        }
        assert!(
            !text.contains("Client::new("),
            "{name}: create clients through CollectContext::client"
        );
        assert!(
            !text.contains("Client::from_conf("),
            "{name}: create clients through CollectContext::client"
        );
    }
}

#[test]
fn collectors_do_not_use_aws_config() {
    for (name, text) in source_files() {
        if name.starts_with("collectors/") {
            assert!(
                !text.contains("aws_config::"),
                "{name}: collectors must not use aws_config"
            );
        }
    }
}
