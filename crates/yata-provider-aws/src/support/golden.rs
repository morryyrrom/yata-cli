//! Comparison of collector output with expected files (golden files).
//!
//! Expected files live in `tests/golden/<service>/<resource>.json`. They are compared as
//! parsed JSON values, so key order does not matter. Running the tests with
//! `YATA_UPDATE_GOLDEN=1` rewrites them with recursively sorted keys.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Environment variable that makes [`assert_golden`] rewrite the expected file.
pub(crate) const UPDATE_ENV: &str = "YATA_UPDATE_GOLDEN";

/// Returns the path of the expected file for `service` and `resource`.
pub(crate) fn golden_path(service: &str, resource: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(service)
        .join(format!("{resource}.json"))
}

/// Returns a copy of `value` with the keys of every object in ascending order.
pub(crate) fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                if let Some(inner) = map.get(key) {
                    sorted.insert(key.clone(), canonicalize(inner));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

/// Writes `actual` to `path` in the canonical form (sorted keys, pretty printed).
pub(crate) fn write_golden(path: &Path, actual: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&canonicalize(actual))
        .map_err(|e| format!("serialize: {e}"))?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Compares `actual` with the file at `path`, or rewrites the file when `update` is true.
pub(crate) fn check_golden(path: &Path, actual: &Value, update: bool) -> Result<(), String> {
    if update {
        return write_golden(path, actual);
    }
    let text = fs::read_to_string(path).map_err(|e| {
        format!(
            "read {}: {e} (run with {UPDATE_ENV}=1 to create it)",
            path.display()
        )
    })?;
    let expected: Value =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if expected == *actual {
        Ok(())
    } else {
        Err(format!(
            "{} differs from the actual output; run with {UPDATE_ENV}=1 to update it",
            path.display()
        ))
    }
}

/// Asserts that `actual` matches the expected file at `path`.
///
/// The file is rewritten only when `YATA_UPDATE_GOLDEN=1` is set.
pub(crate) fn assert_golden(path: &Path, actual: &Value) {
    let update = std::env::var(UPDATE_ENV).is_ok_and(|v| v == "1");
    if let Err(message) = check_golden(path, actual, update) {
        panic!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_matter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        fs::write(
            &path,
            r#"{"b": 1, "a": {"y": [{"q": 1, "p": 2}], "x": null}}"#,
        )
        .unwrap();
        let actual = json!({"a": {"x": null, "y": [{"p": 2, "q": 1}]}, "b": 1});
        check_golden(&path, &actual, false).unwrap();
        let different = json!({"a": {"x": null, "y": [{"p": 3, "q": 1}]}, "b": 1});
        assert!(check_golden(&path, &different, false).is_err());
    }

    #[test]
    fn written_file_has_recursively_sorted_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("out.json");
        let actual = json!({"z": 1, "m": {"b": [{"d": 1, "c": 2}], "a": 0}, "a": true});
        check_golden(&path, &actual, true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let positions: Vec<usize> = ["\"a\": true", "\"m\"", "\"z\""]
            .iter()
            .map(|needle| text.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(text.find("\"a\": 0").unwrap() < text.find("\"b\"").unwrap());
        assert!(text.find("\"c\"").unwrap() < text.find("\"d\"").unwrap());
        assert!(text.ends_with('\n'));
        check_golden(&path, &actual, false).unwrap();
    }

    #[test]
    fn missing_file_is_an_error_without_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        assert!(check_golden(&path, &json!({}), false).is_err());
    }
}
