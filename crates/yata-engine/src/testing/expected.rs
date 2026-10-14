//! `expected.fail` of a fixture: parsing, validation and comparison with the actual result.
//!
//! An element is an `id` string (every resource with that `id`, whatever its region) or an
//! object `{"id": ..., "region": ...}` (one resource). The messages never contain the `id` or
//! `region` values, only positions, because fixture values may not be shown (see `dummy`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

use crate::error::{FailureDetails, ResourceKey, UnexpectedResource};

/// A resource key: `(id, region)`.
pub(super) type Key = (String, String);

/// One element of `expected.fail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExpectedEntry {
    /// Every resource with this `id`.
    Id(String),
    /// The resource with this `id` and `region`.
    Key {
        /// Resource ID.
        id: String,
        /// Region.
        region: String,
    },
}

/// Parses the array `expected.fail`.
///
/// Returns the well-formed elements with their position in the array. Malformed and repeated
/// elements are reported in `errors`; a malformed element is not part of the result.
pub(super) fn parse(elements: &[Value], errors: &mut Vec<String>) -> Vec<(usize, ExpectedEntry)> {
    let at = |i: usize| format!("/expected/fail/{i}");
    let mut entries = Vec::new();
    let mut string_ids: HashMap<&str, usize> = HashMap::new();
    let mut object_ids: HashMap<&str, usize> = HashMap::new();
    let mut object_keys: HashMap<(&str, &str), usize> = HashMap::new();

    for (i, element) in elements.iter().enumerate() {
        match element {
            Value::String(id) => {
                if let Some(first) = string_ids.get(id.as_str()) {
                    errors.push(format!("{} repeats {}", at(i), at(*first)));
                } else if let Some(first) = object_ids.get(id.as_str()) {
                    errors.push(format!(
                        "{} names an id as a string that {} names as an object",
                        at(i),
                        at(*first)
                    ));
                } else {
                    string_ids.insert(id, i);
                }
                entries.push((i, ExpectedEntry::Id(id.clone())));
            }
            Value::Object(map) => {
                let id = map.get("id").and_then(Value::as_str);
                let region = map.get("region").and_then(Value::as_str);
                let (Some(id), Some(region)) = (id, region) else {
                    errors.push(format!(
                        "{} is an object without a string `id` and a string `region`",
                        at(i)
                    ));
                    continue;
                };
                if let Some(first) = object_keys.get(&(id, region)) {
                    errors.push(format!("{} repeats {}", at(i), at(*first)));
                } else if let Some(first) = string_ids.get(id) {
                    errors.push(format!(
                        "{} names an id as an object that {} names as a string",
                        at(i),
                        at(*first)
                    ));
                } else {
                    object_keys.insert((id, region), i);
                    object_ids.entry(id).or_insert(i);
                }
                entries.push((
                    i,
                    ExpectedEntry::Key {
                        id: id.to_owned(),
                        region: region.to_owned(),
                    },
                ));
            }
            _ => errors.push(format!("{} is neither a string nor an object", at(i))),
        }
    }
    entries
}

/// The `(id, region)` pairs of the resources in a `target_type` array of the `data`.
///
/// Resources without a string `id` or `region` are left out (the format check reports them).
pub(super) fn target_keys(target: &[Value]) -> Vec<Key> {
    target
        .iter()
        .filter_map(|r| {
            Some((
                r.get("id")?.as_str()?.to_owned(),
                r.get("region")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// Reports every element that points to no resource of the target type.
///
/// This catches a misspelled `id` in a `fail_` fixture, which would otherwise pass silently
/// with a rule that finds nothing. The elements are given with their position in
/// `expected.fail`, so that the report names the right element even after malformed elements
/// were left out.
pub(super) fn check_targets(
    entries: &[(usize, ExpectedEntry)],
    target: &[Key],
    errors: &mut Vec<String>,
) {
    for (i, entry) in entries {
        let found = target.iter().any(|(id, region)| match entry {
            ExpectedEntry::Id(want) => id == want,
            ExpectedEntry::Key {
                id: want_id,
                region: want_region,
            } => id == want_id && region == want_region,
        });
        if !found {
            errors.push(format!(
                "/expected/fail/{i} matches no resource of the target type in the data"
            ));
        }
    }
}

/// The set of resources the fixture expects to fail.
pub(super) fn expected_set(entries: &[(usize, ExpectedEntry)], target: &[Key]) -> BTreeSet<Key> {
    let mut set = BTreeSet::new();
    for (_, entry) in entries {
        match entry {
            ExpectedEntry::Id(want) => {
                set.extend(target.iter().filter(|(id, _)| id == want).cloned());
            }
            ExpectedEntry::Key { id, region } => {
                set.insert((id.clone(), region.clone()));
            }
        }
    }
    set
}

/// Compares the expected and the actual failing resources.
///
/// `actual` maps every resource that failed to the `message` of its result. Returns `None`
/// when the sets are equal, otherwise the details of a `fixture_mismatch`.
pub(super) fn compare(
    expected: &BTreeSet<Key>,
    actual: &BTreeMap<Key, String>,
) -> Option<FailureDetails> {
    let missing: Vec<ResourceKey> = expected
        .iter()
        .filter(|key| !actual.contains_key(*key))
        .map(|(id, region)| ResourceKey {
            id: id.clone(),
            region: region.clone(),
        })
        .collect();
    let unexpected: Vec<UnexpectedResource> = actual
        .iter()
        .filter(|(key, _)| !expected.contains(*key))
        .map(|((id, region), message)| UnexpectedResource {
            id: id.clone(),
            region: region.clone(),
            message: message.clone(),
        })
        .collect();
    if missing.is_empty() && unexpected.is_empty() {
        None
    } else {
        Some(FailureDetails::Mismatch {
            missing,
            unexpected,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse_all(value: Value) -> (Vec<ExpectedEntry>, Vec<String>) {
        let mut errors = Vec::new();
        let entries = parse(value.as_array().unwrap(), &mut errors);
        (entries.into_iter().map(|(_, e)| e).collect(), errors)
    }

    #[test]
    fn parses_strings_and_objects() {
        let (entries, errors) = parse_all(json!(["a", {"id": "b", "region": "us-east-1"}]));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            entries,
            [
                ExpectedEntry::Id("a".into()),
                ExpectedEntry::Key {
                    id: "b".into(),
                    region: "us-east-1".into()
                }
            ]
        );
    }

    #[test]
    fn reports_repeated_and_mixed_elements_by_position() {
        let (_, errors) = parse_all(json!(["a", "a"]));
        assert_eq!(errors, ["/expected/fail/1 repeats /expected/fail/0"]);

        let object = json!({"id": "a", "region": "r"});
        let (_, errors) = parse_all(json!([object, object]));
        assert_eq!(errors, ["/expected/fail/1 repeats /expected/fail/0"]);

        let (_, errors) = parse_all(json!(["a", {"id": "a", "region": "r"}]));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("/expected/fail/1 "), "{errors:?}");
        let (_, errors) = parse_all(json!([{"id": "a", "region": "r"}, "a"]));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("/expected/fail/1 "), "{errors:?}");

        // The same id in two regions, and an id once as a string only, are fine.
        let (_, errors) = parse_all(json!([
            {"id": "a", "region": "r"},
            {"id": "a", "region": "s"},
            "b"
        ]));
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn reports_malformed_elements() {
        for bad in [
            json!([{"id": "a"}]),
            json!([{"region": "r"}]),
            json!([{"id": "a", "region": 5}]),
            json!([{"id": 5, "region": "r"}]),
            json!([5]),
            json!([null]),
            json!([["a"]]),
        ] {
            let (entries, errors) = parse_all(bad.clone());
            assert_eq!(errors.len(), 1, "{bad}");
            assert!(
                entries.is_empty() || matches!(bad[0], Value::String(_)),
                "{bad}"
            );
        }
    }

    #[test]
    fn expected_set_expands_strings_to_every_region() {
        let target = vec![
            ("a".to_owned(), "eu-west-1".to_owned()),
            ("a".to_owned(), "us-east-1".to_owned()),
            ("b".to_owned(), "us-east-1".to_owned()),
        ];
        let entries = vec![
            (0, ExpectedEntry::Id("a".into())),
            (
                1,
                ExpectedEntry::Key {
                    id: "b".into(),
                    region: "us-east-1".into(),
                },
            ),
        ];
        assert_eq!(
            expected_set(&entries, &target)
                .into_iter()
                .collect::<Vec<_>>(),
            target
        );
        let one = [(
            0,
            ExpectedEntry::Key {
                id: "a".into(),
                region: "eu-west-1".into(),
            },
        )];
        assert_eq!(expected_set(&one, &target).len(), 1);
    }

    #[test]
    fn check_targets_names_the_positions_that_match_nothing() {
        let target = vec![("a".to_owned(), "r".to_owned())];
        let numbered = vec![
            (0, ExpectedEntry::Id("a".into())),
            (2, ExpectedEntry::Id("zz".into())),
            (
                3,
                ExpectedEntry::Key {
                    id: "a".into(),
                    region: "other".into(),
                },
            ),
        ];
        let mut errors = Vec::new();
        check_targets(&numbered, &target, &mut errors);
        assert_eq!(
            errors,
            [
                "/expected/fail/2 matches no resource of the target type in the data",
                "/expected/fail/3 matches no resource of the target type in the data"
            ]
        );
    }

    #[test]
    fn compare_lists_missing_and_unexpected_in_ascending_order() {
        let key = |id: &str, region: &str| (id.to_owned(), region.to_owned());
        let expected: BTreeSet<Key> = [key("a", "us-east-1"), key("c", "us-east-1")].into();
        let actual: BTreeMap<Key, String> = [
            (key("c", "us-east-1"), "m".to_owned()),
            (key("b", "us-west-2"), "x".to_owned()),
            (key("b", "eu-west-1"), "z".to_owned()),
        ]
        .into();
        let Some(FailureDetails::Mismatch {
            missing,
            unexpected,
        }) = compare(&expected, &actual)
        else {
            panic!("expected a mismatch");
        };
        assert_eq!(
            missing,
            [ResourceKey {
                id: "a".into(),
                region: "us-east-1".into()
            }]
        );
        let order: Vec<(&str, &str)> = unexpected
            .iter()
            .map(|u| (u.id.as_str(), u.region.as_str()))
            .collect();
        assert_eq!(order, [("b", "eu-west-1"), ("b", "us-west-2")]);
        assert_eq!(unexpected[1].message, "x");

        let same: BTreeMap<Key, String> = [
            (key("a", "us-east-1"), "1".to_owned()),
            (key("c", "us-east-1"), "2".to_owned()),
        ]
        .into();
        assert_eq!(compare(&expected, &same), None);
    }
}
