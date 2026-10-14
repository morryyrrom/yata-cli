//! Fixture directories and fixture files: names, format validation and preparation.
//!
//! A fixture is a JSON object with a `data` (the Rego `data` the rule gets) and `expected.fail`
//! (the resources that must fail). The format is described in ADR-0001 "Policy and Fixture
//! Layout" and ADR-0030.
//! Every message names positions as JSON Pointers and never contains a value from the fixture
//! (keys are redacted when they look like account IDs, see `dummy`).

use std::collections::HashMap;
use std::ffi::OsString;

use serde_json::{Map, Value};
use yata_core::RuleMetadata;

use super::dummy::{pointer_token, redact};
use super::expected::{self, ExpectedEntry};
use crate::input::declared_types;
use crate::names::parse_resource_type;
use crate::walk::{FixtureEntry, FixtureEntryKind};

/// The suffix of a fixture file name.
const FIXTURE_EXTENSION: &str = ".json";

/// Whether a fixture must produce failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FixtureKind {
    /// `pass_<case>.json`: no resource fails.
    Pass,
    /// `fail_<case>.json`: at least one resource fails.
    Fail,
}

/// Classifies a file name: `pass_<case>.json` or `fail_<case>.json` with a case of at least one
/// character.
pub(super) fn fixture_kind(name: &str) -> Option<FixtureKind> {
    let (kind, rest) = if let Some(rest) = name.strip_prefix("pass_") {
        (FixtureKind::Pass, rest)
    } else {
        (FixtureKind::Fail, name.strip_prefix("fail_")?)
    };
    let case = rest.strip_suffix(FIXTURE_EXTENSION)?;
    (!case.is_empty()).then_some(kind)
}

/// A fixture file found in a fixture directory.
#[derive(Debug, Clone)]
pub(super) struct FixtureFile {
    /// File name (lossy).
    pub(super) name: String,
    /// File name as found on disk.
    pub(super) os_name: OsString,
    /// Pass or fail.
    pub(super) kind: FixtureKind,
}

/// What a fixture directory contains.
#[derive(Debug, Default)]
pub(super) struct DirContents {
    /// The fixture files, in ascending name order.
    pub(super) fixtures: Vec<FixtureFile>,
    /// Entries that are neither fixtures nor ignored, with the reason.
    pub(super) invalid: Vec<(String, String)>,
}

impl DirContents {
    /// Whether there is at least one fixture of this kind.
    pub(super) fn has(&self, kind: FixtureKind) -> bool {
        self.fixtures.iter().any(|f| f.kind == kind)
    }
}

/// Sorts the entries of a fixture directory into fixtures, ignored entries (names starting with
/// `.`) and invalid entries.
pub(super) fn check_dir(entries: &[FixtureEntry]) -> DirContents {
    let mut contents = DirContents::default();
    for entry in entries {
        if entry.name.starts_with('.') {
            continue;
        }
        let kind = fixture_kind(&entry.name);
        match (entry.kind, kind) {
            (FixtureEntryKind::File, Some(kind)) => contents.fixtures.push(FixtureFile {
                name: entry.name.clone(),
                os_name: entry.os_name.clone(),
                kind,
            }),
            (FixtureEntryKind::File, None) => contents.invalid.push((
                entry.name.clone(),
                "file is not named pass_<case>.json or fail_<case>.json".to_owned(),
            )),
            (FixtureEntryKind::Directory, _) => contents.invalid.push((
                entry.name.clone(),
                "directories are not allowed in a fixture directory".to_owned(),
            )),
            (FixtureEntryKind::Other, _) => contents
                .invalid
                .push((entry.name.clone(), "entry is not a regular file".to_owned())),
        }
    }
    contents.fixtures.sort_by(|a, b| a.name.cmp(&b.name));
    contents
}

/// A validated fixture, ready to be evaluated.
#[derive(Debug)]
pub(super) struct Fixture {
    /// The `data`, with the arrays of the declared types sorted by `(id, region)`.
    pub(super) data: Value,
    /// The elements of `expected.fail`.
    pub(super) expected: Vec<(usize, ExpectedEntry)>,
}

/// Validates the fixture `value` for the rule with `metadata`.
///
/// Returns every format error found (as English messages with JSON Pointers), or the fixture.
pub(super) fn validate(
    value: &Value,
    metadata: &RuleMetadata,
    kind: FixtureKind,
) -> Result<Fixture, Vec<String>> {
    let mut errors = Vec::new();
    let Some(root) = value.as_object() else {
        return Err(vec!["the fixture must be a JSON object".to_owned()]);
    };
    if root.get("description").is_some_and(|d| !d.is_string()) {
        errors.push("/description must be a string".to_owned());
    }

    let declared = declared_types(metadata);
    let provider = metadata.target_type.split('.').next().unwrap_or_default();
    if root.contains_key("input") {
        errors.push(
            "fixture uses the old \"input\" key; rename it to \"data\" and move account and regions under data.yata.context"
                .to_owned(),
        );
    }
    let mut target_keys = None;
    let data = match root.get("data") {
        Some(Value::Object(data)) => {
            target_keys = validate_data(data, provider, metadata, &declared, &mut errors);
            Some(data)
        }
        Some(_) => {
            errors.push("/data must be an object".to_owned());
            None
        }
        // The old key is reported above; do not add a second error for the same mistake.
        None if root.contains_key("input") => None,
        None => {
            errors.push("/data is missing".to_owned());
            None
        }
    };

    let entries = validate_expected(root, kind, target_keys.as_deref(), &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }
    let (Some(data), Some(entries)) = (data, entries) else {
        return Err(vec!["the fixture is invalid".to_owned()]);
    };
    let mut data = Value::Object(data.clone());
    sort_declared(&mut data, &declared);
    Ok(Fixture {
        data,
        expected: entries,
    })
}

/// Validates `data`. Returns the `(id, region)` pairs of the target type array, if it is an
/// array.
fn validate_data(
    data: &Map<String, Value>,
    provider: &str,
    metadata: &RuleMetadata,
    declared: &[&str],
    errors: &mut Vec<String>,
) -> Option<Vec<expected::Key>> {
    let account_key = validate_yata(data, errors);

    for key in data.keys() {
        if key != "yata" && key != provider {
            errors.push(format!(
                "/data/{} is not allowed; the keys are yata and {provider}",
                pointer_token(key)
            ));
        }
    }

    // Resource arrays that the rule did not declare are an error: the engine passes only the
    // declared types, so a rule that forgot to declare one would pass the fixtures only.
    if let Some(tree) = data.get(provider) {
        match tree.as_object() {
            Some(services) => {
                for (service, resources) in services {
                    let Some(resources) = resources.as_object() else {
                        errors.push(format!(
                            "/data/{provider}/{} must be an object",
                            pointer_token(service)
                        ));
                        continue;
                    };
                    for resource in resources.keys() {
                        let name = format!("{provider}.{service}.{resource}");
                        if !declared.contains(&name.as_str()) {
                            errors.push(format!(
                                "/data/{provider}/{}/{} is a resource type that the rule does not declare in target_type or related_types",
                                pointer_token(service),
                                pointer_token(resource)
                            ));
                        }
                    }
                }
            }
            None => errors.push(format!("/data/{provider} must be an object")),
        }
    }

    let mut target = None;
    for type_name in declared {
        let Some((p, service, resource)) = parse_resource_type(type_name) else {
            continue;
        };
        let pointer = format!("/data/{p}/{service}/{resource}");
        let array = data
            .get(p)
            .and_then(|v| v.get(service))
            .and_then(|v| v.get(resource));
        let Some(array) = array else {
            errors.push(format!(
                "{pointer} is missing; the rule declares the resource type {type_name}"
            ));
            continue;
        };
        let Some(elements) = array.as_array() else {
            errors.push(format!("{pointer} must be an array"));
            continue;
        };
        validate_resources(elements, type_name, &pointer, account_key, errors);
        if *type_name == metadata.target_type {
            target = Some(expected::target_keys(elements));
        }
    }
    target
}

/// Validates `data.yata`, which holds only `context` with `account` and `regions`. Returns the
/// account key (`data.yata.context.account.account`) if it is valid.
fn validate_yata<'a>(data: &'a Map<String, Value>, errors: &mut Vec<String>) -> Option<&'a str> {
    let Some(yata) = data.get("yata") else {
        errors.push("/data/yata is missing; it holds the context (account and regions)".to_owned());
        return None;
    };
    let Some(yata) = yata.as_object() else {
        errors.push("/data/yata must be an object".to_owned());
        return None;
    };
    for key in yata.keys() {
        if key != "context" {
            errors.push(format!(
                "/data/yata/{} is not allowed; the only key is context",
                pointer_token(key)
            ));
        }
    }
    let Some(context) = yata.get("context") else {
        errors.push("/data/yata/context is missing".to_owned());
        return None;
    };
    let Some(context) = context.as_object() else {
        errors.push("/data/yata/context must be an object".to_owned());
        return None;
    };
    for key in context.keys() {
        if !matches!(key.as_str(), "account" | "regions") {
            errors.push(format!(
                "/data/yata/context/{} is not allowed; the keys are account and regions",
                pointer_token(key)
            ));
        }
    }
    match context.get("regions") {
        Some(Value::Array(regions)) if regions.iter().all(Value::is_string) => {}
        Some(_) => errors.push("/data/yata/context/regions must be an array of strings".to_owned()),
        None => errors.push("/data/yata/context/regions is missing".to_owned()),
    }
    validate_account(context, errors)
}

/// Validates `context.account`. Returns the account key if it is valid.
fn validate_account<'a>(
    context: &'a Map<String, Value>,
    errors: &mut Vec<String>,
) -> Option<&'a str> {
    let Some(account) = context.get("account") else {
        errors.push("/data/yata/context/account is missing".to_owned());
        return None;
    };
    let Some(account) = account.as_object() else {
        errors.push("/data/yata/context/account must be an object".to_owned());
        return None;
    };
    for key in ["account", "id", "provider"] {
        if !account.get(key).is_some_and(Value::is_string) {
            errors.push(format!("/data/yata/context/account/{key} must be a string"));
        }
    }
    account.get("account").and_then(Value::as_str)
}

/// Validates the elements of one resource array.
fn validate_resources(
    elements: &[Value],
    type_name: &str,
    pointer: &str,
    account_key: Option<&str>,
    errors: &mut Vec<String>,
) {
    let mut first_seen: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, element) in elements.iter().enumerate() {
        let at = format!("{pointer}/{i}");
        let Some(resource) = element.as_object() else {
            errors.push(format!("{at} must be an object"));
            continue;
        };
        let mut strings = [None; 4];
        for (slot, key) in strings.iter_mut().zip(["id", "type", "account", "region"]) {
            match resource.get(key) {
                Some(Value::String(s)) => *slot = Some(s.as_str()),
                _ => errors.push(format!("{at}/{key} must be a string")),
            }
        }
        let [id, resource_type, account, region] = strings;
        if resource_type.is_some_and(|t| t != type_name) {
            errors.push(format!("{at}/type must be {type_name}"));
        }
        if let (Some(account), Some(expected)) = (account, account_key)
            && account != expected
        {
            errors.push(format!(
                "{at}/account must equal /data/yata/context/account/account"
            ));
        }
        if !resource.get("configuration").is_some_and(Value::is_object) {
            errors.push(format!("{at}/configuration must be an object"));
        }
        if resource.get("name").is_some_and(|n| !n.is_string()) {
            errors.push(format!("{at}/name must be a string when present"));
        }
        if let Some(tags) = resource.get("tags")
            && !tags
                .as_object()
                .is_some_and(|t| t.values().all(Value::is_string))
        {
            errors.push(format!(
                "{at}/tags must be an object of strings when present"
            ));
        }
        if let (Some(id), Some(region)) = (id, region) {
            if let Some(first) = first_seen.get(&(id, region)) {
                errors.push(format!(
                    "{at} has the same id and region as {pointer}/{first}"
                ));
            } else {
                first_seen.insert((id, region), i);
            }
        }
    }
}

/// Validates `expected`. Returns the well-formed elements of `expected.fail`, if it is an array.
fn validate_expected(
    root: &Map<String, Value>,
    kind: FixtureKind,
    target: Option<&[expected::Key]>,
    errors: &mut Vec<String>,
) -> Option<Vec<(usize, ExpectedEntry)>> {
    let Some(expected) = root.get("expected") else {
        errors.push("/expected is missing".to_owned());
        return None;
    };
    let Some(expected) = expected.as_object() else {
        errors.push("/expected must be an object".to_owned());
        return None;
    };
    let Some(fail) = expected.get("fail") else {
        errors.push("/expected/fail is missing".to_owned());
        return None;
    };
    let Some(fail) = fail.as_array() else {
        errors.push("/expected/fail must be an array".to_owned());
        return None;
    };

    let entries = expected::parse(fail, errors);
    if let Some(target) = target {
        expected::check_targets(&entries, target, errors);
    }
    match kind {
        FixtureKind::Pass if !fail.is_empty() => {
            errors.push("/expected/fail must be empty in a pass_ fixture".to_owned());
        }
        FixtureKind::Fail if fail.is_empty() => {
            errors.push("/expected/fail must not be empty in a fail_ fixture".to_owned());
        }
        _ => {}
    }
    Some(entries)
}

/// Sorts the arrays of the declared types by `(id, region)`, as the real evaluation does.
fn sort_declared(data: &mut Value, declared: &[&str]) {
    for type_name in declared {
        let Some((provider, service, resource)) = parse_resource_type(type_name) else {
            continue;
        };
        let array = data
            .get_mut(provider)
            .and_then(|v| v.get_mut(service))
            .and_then(|v| v.get_mut(resource))
            .and_then(Value::as_array_mut);
        if let Some(array) = array {
            let key = |v: &Value| {
                let field = |name: &str| {
                    v.get(name)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                (field("id"), field("region"))
            };
            array.sort_by_cached_key(key);
        }
    }
}

/// Joins format errors into one message, at most `LIMIT` of them.
pub(super) fn join_errors(errors: &[String]) -> String {
    const LIMIT: usize = 10;
    let mut message = errors
        .iter()
        .take(LIMIT)
        .map(|e| redact(e))
        .collect::<Vec<_>>()
        .join("; ");
    if errors.len() > LIMIT {
        message.push_str(&format!("; and {} more", errors.len() - LIMIT));
    }
    message
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use yata_core::Severity;

    use super::*;

    fn metadata() -> RuleMetadata {
        RuleMetadata {
            title: "t".into(),
            severity: Severity::Low,
            description: "d".into(),
            remediation: "r".into(),
            target_type: "aws.example.widget".into(),
            related_types: vec!["aws.example.gadget".into()],
            references: vec![],
            tags: vec![],
        }
    }

    fn resource(id: &str, region: &str) -> Value {
        json!({
            "id": id, "type": "aws.example.widget", "account": "aws-123456789012",
            "region": region, "tags": {}, "configuration": {}
        })
    }

    fn fixture(widgets: Vec<Value>, fail: Value) -> Value {
        json!({
            "description": "d",
            "data": {
                "yata": {"context": {
                    "account": {"account": "aws-123456789012", "id": "123456789012", "provider": "aws"},
                    "regions": ["us-east-1"]
                }},
                "aws": {"example": {"widget": widgets, "gadget": []}}
            },
            "expected": {"fail": fail}
        })
    }

    fn errors_of(value: &Value, kind: FixtureKind) -> Vec<String> {
        validate(value, &metadata(), kind).unwrap_err()
    }

    #[test]
    fn names() {
        assert_eq!(fixture_kind("pass_a.json"), Some(FixtureKind::Pass));
        assert_eq!(fixture_kind("fail_a_b.json"), Some(FixtureKind::Fail));
        for bad in [
            "pass_.json",
            "pass_a.jsn",
            "pass_a.json.bak",
            "pass.json",
            "Pass_a.json",
            "xfail_a.json",
            "a.json",
        ] {
            assert_eq!(fixture_kind(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_valid_fixture_is_returned_with_sorted_arrays() {
        let value = fixture(
            vec![
                resource("b", "us-east-1"),
                resource("a", "us-west-2"),
                resource("a", "us-east-1"),
            ],
            json!(["a"]),
        );
        let fx = validate(&value, &metadata(), FixtureKind::Fail).unwrap();
        let ids: Vec<(&str, &str)> = fx.data["aws"]["example"]["widget"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["id"].as_str().unwrap(), r["region"].as_str().unwrap()))
            .collect();
        assert_eq!(
            ids,
            [("a", "us-east-1"), ("a", "us-west-2"), ("b", "us-east-1")]
        );
        assert_eq!(fx.expected.len(), 1);
    }

    #[test]
    fn top_level_errors() {
        assert_eq!(errors_of(&json!([]), FixtureKind::Pass).len(), 1);
        let mut v = fixture(vec![], json!([]));
        v["description"] = json!(5);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/description must be a string"]
        );
        let mut v = fixture(vec![], json!([]));
        v.as_object_mut().unwrap().remove("data");
        assert!(errors_of(&v, FixtureKind::Pass).contains(&"/data is missing".to_owned()));
        v["data"] = json!([]);
        assert!(errors_of(&v, FixtureKind::Pass).contains(&"/data must be an object".to_owned()));
        let mut v = fixture(vec![], json!([]));
        v.as_object_mut().unwrap().remove("expected");
        assert_eq!(errors_of(&v, FixtureKind::Pass), ["/expected is missing"]);
        v["expected"] = json!([]);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/expected must be an object"]
        );
        v["expected"] = json!({});
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/expected/fail is missing"]
        );
        v["expected"] = json!({"fail": "a"});
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/expected/fail must be an array"]
        );
    }

    #[test]
    fn unknown_top_level_and_expected_keys_are_ignored() {
        let mut v = fixture(vec![], json!([]));
        v["future"] = json!(1);
        v["expected"]["future"] = json!(1);
        assert!(validate(&v, &metadata(), FixtureKind::Pass).is_ok());
    }

    #[test]
    fn data_errors() {
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"]["account"] = json!({"account": "x", "id": 5});
        let errors = errors_of(&v, FixtureKind::Pass);
        assert_eq!(
            errors,
            [
                "/data/yata/context/account/id must be a string",
                "/data/yata/context/account/provider must be a string"
            ]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"]["account"] = json!("x");
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context/account must be an object"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"]["regions"] = json!(["a", 1]);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context/regions must be an array of strings"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"]
            .as_object_mut()
            .unwrap()
            .remove("regions");
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context/regions is missing"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["gcp"] = json!({});
        let errors = errors_of(&v, FixtureKind::Pass);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].starts_with("/data/gcp is not allowed"),
            "{errors:?}"
        );
    }

    #[test]
    fn the_old_input_key_is_reported_with_the_way_to_fix_it() {
        let mut v = fixture(vec![], json!([]));
        let data = v.as_object_mut().unwrap().remove("data").unwrap();
        v["input"] = data;
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            [
                "fixture uses the old \"input\" key; rename it to \"data\" and move account and regions under data.yata.context"
            ]
        );
    }

    #[test]
    fn context_and_yata_errors() {
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["extra"] = json!({});
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/extra is not allowed; the only key is context"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"]["extra"] = json!(1);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context/extra is not allowed; the keys are account and regions"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"] = json!([]);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata must be an object"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"] = json!({});
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context is missing"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"].as_object_mut().unwrap().remove("yata");
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata is missing; it holds the context (account and regions)"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["yata"]["context"] = json!([]);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/yata/context must be an object"]
        );
    }

    #[test]
    fn declared_and_undeclared_types() {
        let mut v = fixture(vec![], json!([]));
        v["data"]["aws"]["example"]
            .as_object_mut()
            .unwrap()
            .remove("gadget");
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            [
                "/data/aws/example/gadget is missing; the rule declares the resource type aws.example.gadget"
            ]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["aws"]["example"]["gadget"] = json!({});
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/aws/example/gadget must be an array"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["aws"]["s3"] = json!({"bucket": []});
        let errors = errors_of(&v, FixtureKind::Pass);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].starts_with(
                "/data/aws/s3/bucket is a resource type that the rule does not declare"
            )
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["aws"]["s3"] = json!([]);
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/aws/s3 must be an object"]
        );
        let mut v = fixture(vec![], json!([]));
        v["data"]["aws"] = json!([]);
        let errors = errors_of(&v, FixtureKind::Pass);
        assert!(
            errors.contains(&"/data/aws must be an object".to_owned()),
            "{errors:?}"
        );
    }

    #[test]
    fn resource_errors() {
        let mut bad = resource("a", "us-east-1");
        bad["type"] = json!("aws.example.gadget");
        bad["account"] = json!("aws-111122223333");
        bad["configuration"] = json!([]);
        bad["name"] = json!(1);
        bad["tags"] = json!({"k": 1});
        bad.as_object_mut().unwrap().remove("region");
        let v = fixture(vec![bad, json!(5)], json!([]));
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            [
                "/data/aws/example/widget/0/region must be a string",
                "/data/aws/example/widget/0/type must be aws.example.widget",
                "/data/aws/example/widget/0/account must equal /data/yata/context/account/account",
                "/data/aws/example/widget/0/configuration must be an object",
                "/data/aws/example/widget/0/name must be a string when present",
                "/data/aws/example/widget/0/tags must be an object of strings when present",
                "/data/aws/example/widget/1 must be an object",
            ]
        );
        // `name` and `tags` may be absent.
        let mut ok = resource("a", "us-east-1");
        ok.as_object_mut().unwrap().remove("tags");
        assert!(
            validate(
                &fixture(vec![ok], json!([])),
                &metadata(),
                FixtureKind::Pass
            )
            .is_ok()
        );
    }

    #[test]
    fn duplicate_id_and_region_in_one_array() {
        let v = fixture(
            vec![
                resource("a", "us-east-1"),
                resource("a", "us-west-2"),
                resource("a", "us-east-1"),
            ],
            json!([]),
        );
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/data/aws/example/widget/2 has the same id and region as /data/aws/example/widget/0"]
        );
    }

    #[test]
    fn expected_errors_depend_on_the_kind() {
        let widgets = vec![resource("a", "us-east-1")];
        let v = fixture(widgets.clone(), json!(["a"]));
        assert_eq!(
            errors_of(&v, FixtureKind::Pass),
            ["/expected/fail must be empty in a pass_ fixture"]
        );
        let v = fixture(widgets.clone(), json!([]));
        assert_eq!(
            errors_of(&v, FixtureKind::Fail),
            ["/expected/fail must not be empty in a fail_ fixture"]
        );
        let v = fixture(widgets.clone(), json!(["zz"]));
        assert_eq!(
            errors_of(&v, FixtureKind::Fail),
            ["/expected/fail/0 matches no resource of the target type in the data"]
        );
        let v = fixture(widgets, json!([{"id": "a", "region": "eu-west-1"}]));
        assert_eq!(
            errors_of(&v, FixtureKind::Fail),
            ["/expected/fail/0 matches no resource of the target type in the data"]
        );
    }

    #[test]
    fn several_errors_are_collected_and_joined() {
        let mut v = fixture(vec![resource("a", "us-east-1")], json!(["a", "a"]));
        v["description"] = json!(1);
        let errors = errors_of(&v, FixtureKind::Pass);
        assert_eq!(errors.len(), 3, "{errors:?}");
        let joined = join_errors(&errors);
        assert_eq!(joined.matches("; ").count(), 2);
        let many: Vec<String> = (0..12).map(|i| format!("e{i}")).collect();
        assert!(join_errors(&many).ends_with("; and 2 more"));
    }

    #[test]
    fn directory_entries_are_classified() {
        let entry = |name: &str, kind| FixtureEntry {
            name: name.to_owned(),
            os_name: OsString::from(name),
            kind,
        };
        let entries = vec![
            entry(".DS_Store", FixtureEntryKind::File),
            entry("extra", FixtureEntryKind::Directory),
            entry("fail_b.json", FixtureEntryKind::File),
            entry("pass_a.json", FixtureEntryKind::File),
            entry("pass_a.jsn", FixtureEntryKind::File),
            entry("pass_socket.json", FixtureEntryKind::Other),
        ];
        let contents = check_dir(&entries);
        let names: Vec<&str> = contents.fixtures.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["fail_b.json", "pass_a.json"]);
        let invalid: Vec<&str> = contents.invalid.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(invalid, ["extra", "pass_a.jsn", "pass_socket.json"]);
        assert!(contents.has(FixtureKind::Pass) && contents.has(FixtureKind::Fail));
    }
}
