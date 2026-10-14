//! Small conversion helpers shared by collectors.
//!
//! The functions do not depend on a particular AWS service: every SDK crate re-exports the
//! same `DateTime` type.

// Used by the service collectors that are added in later changes.
#![allow(dead_code)]

use std::collections::HashMap;

use aws_sdk_sts::primitives::DateTime;
use aws_sdk_sts::primitives::DateTimeFormat;
use serde_json::{Map, Value};

/// Formats a timestamp as an RFC 3339 string in UTC with a `Z` suffix.
///
/// Fractional seconds are kept when present (`2024-01-02T03:04:05.5Z`). Returns `None` if
/// the timestamp cannot be formatted.
pub(crate) fn datetime_to_rfc3339(value: &DateTime) -> Option<String> {
    value.fmt(DateTimeFormat::DateTime).ok()
}

/// Converts an optional timestamp to a JSON string, or `null` when absent or unformattable.
pub(crate) fn opt_datetime(value: Option<&DateTime>) -> Value {
    value
        .and_then(datetime_to_rfc3339)
        .map_or(Value::Null, Value::String)
}

/// Converts an optional string to a JSON string, or `null` when absent.
pub(crate) fn opt_string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |s| Value::String(s.to_owned()))
}

/// Converts a map to a JSON object whose keys are in ascending order.
pub(crate) fn map_to_sorted_object<V>(
    map: &HashMap<String, V>,
    convert: impl Fn(&V) -> Value,
) -> Value {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut object = Map::new();
    for key in keys {
        if let Some(value) = map.get(key) {
            object.insert(key.clone(), convert(value));
        }
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn datetime_is_rfc3339_with_z() {
        let whole = DateTime::from_secs(1_704_164_645);
        assert_eq!(
            datetime_to_rfc3339(&whole).as_deref(),
            Some("2024-01-02T03:04:05Z")
        );
        let fractional = DateTime::from_millis(1_704_164_645_500);
        assert_eq!(
            datetime_to_rfc3339(&fractional).as_deref(),
            Some("2024-01-02T03:04:05.5Z")
        );
        assert_eq!(opt_datetime(Some(&whole)), json!("2024-01-02T03:04:05Z"));
        assert_eq!(opt_datetime(None), Value::Null);
    }

    #[test]
    fn option_string_is_null_when_absent() {
        assert_eq!(opt_string(Some("a")), json!("a"));
        assert_eq!(opt_string(None), Value::Null);
    }

    #[test]
    fn map_keys_are_ascending() {
        let map: HashMap<String, i64> = ["delta", "alpha", "charlie", "bravo"]
            .iter()
            .enumerate()
            .map(|(i, k)| ((*k).to_owned(), i as i64))
            .collect();
        let value = map_to_sorted_object(&map, |v| json!(v));
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["alpha", "bravo", "charlie", "delta"]);
        assert_eq!(value["delta"], json!(0));
    }
}
