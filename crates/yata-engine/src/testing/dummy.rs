//! Dummy account IDs and the check for 12-digit numbers in fixtures.
//!
//! Any 12-digit number can be a real AWS account ID. Fixtures are committed to public
//! repositories, so a fixture may contain only the numbers listed in [`DUMMY_ACCOUNT_IDS`].
//! Changing the list changes which fixtures pass, so it is changed in a pull request of its own.

use serde_json::Value;

/// The 12-digit numbers that fixtures may contain.
///
/// AWS does not state that these values are reserved for examples. They are the values that
/// the AWS documentation uses in its examples, plus the numbers made of one repeated digit, so
/// that a real account ID is not committed by mistake.
pub const DUMMY_ACCOUNT_IDS: &[&str] = &[
    // The value used most often in the AWS documentation.
    "123456789012",
    // Further values from AWS documentation examples (several accounts in one example).
    "111122223333",
    "222233334444",
    "333344445555",
    "444455556666",
    "777788889999",
    "000011112222",
    // One repeated digit.
    "000000000000",
    "111111111111",
    "222222222222",
    "333333333333",
    "444444444444",
    "555555555555",
    "666666666666",
    "777777777777",
    "888888888888",
    "999999999999",
];

/// Number of digits in an AWS account ID.
const ACCOUNT_ID_LEN: usize = 12;

/// Whether `run` (a maximal run of ASCII digits) is a 12-digit number that is not a dummy ID.
fn is_non_dummy(run: &str) -> bool {
    run.len() == ACCOUNT_ID_LEN && !DUMMY_ACCOUNT_IDS.contains(&run)
}

/// The maximal runs of ASCII digits in `text`, as byte ranges.
fn digit_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Whether `text` contains a 12-digit number that is not a dummy ID.
fn contains_non_dummy(text: &str) -> bool {
    digit_runs(text)
        .into_iter()
        .any(|(s, e)| is_non_dummy(&text[s..e]))
}

/// Replaces every 12-digit number that is not a dummy ID in `text` with `#` characters.
///
/// Used for object keys that are printed as part of a JSON Pointer or a message.
pub(super) fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (s, e) in digit_runs(text) {
        if is_non_dummy(&text[s..e]) {
            out.push_str(&text[last..s]);
            out.push_str(&"#".repeat(ACCOUNT_ID_LEN));
            last = e;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Escapes a key as one reference token of a JSON Pointer (RFC 6901), after redacting it.
pub(super) fn pointer_token(key: &str) -> String {
    redact(key).replace('~', "~0").replace('/', "~1")
}

/// Finds the 12-digit numbers that are not dummy account IDs in the keys and the string and
/// integer values of `value`.
///
/// Returns the JSON Pointers of the places, sorted and without duplicates. A number in an
/// object key is reported at the object that has the key. The numbers themselves are never part
/// of the result, also not through a key in a pointer (such keys are redacted).
pub(super) fn find_non_dummy(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    visit(value, &mut String::new(), &mut found);
    found.sort();
    found.dedup();
    found
}

fn visit(value: &Value, pointer: &mut String, found: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if contains_non_dummy(s) {
                found.push(pointer.clone());
            }
        }
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                let text = n.to_string();
                if is_non_dummy(text.trim_start_matches('-')) {
                    found.push(pointer.clone());
                }
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&i.to_string());
                visit(item, pointer, found);
                pointer.truncate(len);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if contains_non_dummy(key) {
                    found.push(pointer.clone());
                }
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&pointer_token(key));
                visit(item, pointer, found);
                pointer.truncate(len);
            }
        }
        Value::Bool(_) | Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_list_has_only_distinct_12_digit_numbers() {
        for id in DUMMY_ACCOUNT_IDS {
            assert_eq!(id.len(), 12, "{id}");
            assert!(id.bytes().all(|b| b.is_ascii_digit()), "{id}");
        }
        let mut sorted = DUMMY_ACCOUNT_IDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), DUMMY_ACCOUNT_IDS.len());
    }

    #[test]
    fn finds_only_maximal_runs_of_exactly_12_digits() {
        assert!(contains_non_dummy("arn:aws:iam::123456789013:user/x"));
        assert!(contains_non_dummy("aws-123456789013"));
        assert!(contains_non_dummy("example-123456789013-logs"));
        // Dummy values, and runs that are shorter or longer than 12 digits (a space ends a run).
        assert!(!contains_non_dummy("arn:aws:iam::123456789012:user/x"));
        assert!(!contains_non_dummy("12345678901"));
        assert!(!contains_non_dummy("1234567890123"));
        assert!(contains_non_dummy("123456789013 4"));
        assert!(contains_non_dummy("1234567890134 123456789013"));
        assert!(!contains_non_dummy(""));
    }

    #[test]
    fn redact_replaces_only_non_dummy_numbers() {
        assert_eq!(redact("a-123456789013-b"), "a-############-b");
        assert_eq!(redact("a-123456789012-b"), "a-123456789012-b");
        assert_eq!(redact("no digits"), "no digits");
    }

    #[test]
    fn locations_are_json_pointers_and_never_contain_the_number() {
        let value = json!({
            "a": { "id": "123456789013", "ok": "123456789012" },
            "list": [1, "x", { "n": 123456789013u64 }, 123456789012u64, -123456789013i64],
            "b/c~d": "arn:aws:iam::123456789013:root",
            "key": { "123456789013": 1 },
            "short": 12345678901u64,
            "float": 123456789013.5,
        });
        let found = find_non_dummy(&value);
        assert_eq!(found, ["/a/id", "/b~1c~0d", "/key", "/list/2/n", "/list/4"]);
        for location in &found {
            assert!(!location.contains("123456789013"), "{location}");
        }
    }

    #[test]
    fn descendants_of_a_key_with_a_number_are_located_without_the_number() {
        let value = json!({ "x-123456789013": { "id": "123456789013" } });
        let found = find_non_dummy(&value);
        assert_eq!(found, ["", "/x-############/id"]);
    }
}
