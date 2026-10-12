//! Interpretation of the elements of `fail` and their mapping to resources.
//!
//! This module works on JSON values only (the elements of `fail` and the `target_type` array of
//! the `input`), so it does not depend on [`yata_core::Inventory`] and can be shared with the
//! rule tests.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use yata_core::{ResultResource, ResultStatus, RuleResult, Severity};

use crate::error::EvaluationError;

/// A validated element of `fail`.
#[derive(Debug, Clone, PartialEq)]
struct Finding {
    resource_id: String,
    resource_region: Option<String>,
    message: String,
    path: Option<String>,
    actual: Option<Value>,
    expected: Option<Value>,
}

/// Maps the elements of `fail` to the resources of `target` and builds the results.
///
/// `target` is the `target_type` array of the `input` (resource objects with at least `id`,
/// `type`, `account` and `region`). For every resource, in `(id, region)` order, it returns one
/// `fail` result per mapped element, or one `pass` result when nothing maps to it.
pub(crate) fn build_results(
    rule_id: &str,
    severity: Severity,
    elements: &[Value],
    target: &[Value],
) -> Result<Vec<RuleResult>, EvaluationError> {
    let resources = target
        .iter()
        .map(Target::from_value)
        .collect::<Result<Vec<_>, _>>()?;
    let findings = elements
        .iter()
        .enumerate()
        .map(|(index, element)| parse_element(index, element))
        .collect::<Result<Vec<_>, _>>()?;

    let mut seen = BTreeSet::new();
    for r in &resources {
        if !seen.insert((r.id.as_str(), r.region.as_str())) {
            return Err(EvaluationError::DuplicateResource {
                resource_id: r.id.clone(),
                region: r.region.clone(),
            });
        }
    }

    // Every element must map to at least one resource.
    for f in &findings {
        if !resources.iter().any(|r| r.matches(f)) {
            return Err(EvaluationError::UnknownResource {
                resource_id: f.resource_id.clone(),
                region: f.resource_region.clone(),
            });
        }
    }

    let mut ordered: Vec<&Target> = resources.iter().collect();
    ordered.sort_by(|a, b| (&a.id, &a.region).cmp(&(&b.id, &b.region)));

    let mut results = Vec::new();
    for resource in ordered {
        let mapped: Vec<&Finding> = findings.iter().filter(|f| resource.matches(f)).collect();
        if mapped.is_empty() {
            results.push(RuleResult {
                rule_id: rule_id.to_owned(),
                status: ResultStatus::Pass,
                severity,
                resource: resource.to_result_resource(),
                message: None,
                path: None,
                actual: None,
                expected: None,
            });
        }
        for f in mapped {
            results.push(RuleResult {
                rule_id: rule_id.to_owned(),
                status: ResultStatus::Fail,
                severity,
                resource: resource.to_result_resource(),
                message: Some(f.message.clone()),
                path: f.path.clone(),
                actual: f.actual.clone(),
                expected: f.expected.clone(),
            });
        }
    }
    Ok(results)
}

struct Target {
    id: String,
    resource_type: String,
    account: String,
    region: String,
}

impl Target {
    fn from_value(value: &Value) -> Result<Self, EvaluationError> {
        let field = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| EvaluationError::Engine {
                    message: format!("input resource has no string `{name}`"),
                })
        };
        Ok(Self {
            id: field("id")?,
            resource_type: field("type")?,
            account: field("account")?,
            region: field("region")?,
        })
    }

    fn matches(&self, finding: &Finding) -> bool {
        self.id == finding.resource_id
            && finding
                .resource_region
                .as_ref()
                .is_none_or(|region| *region == self.region)
    }

    fn to_result_resource(&self) -> ResultResource {
        ResultResource {
            id: self.id.clone(),
            resource_type: self.resource_type.clone(),
            account: self.account.clone(),
            region: self.region.clone(),
        }
    }
}

fn invalid(index: usize, reason: impl Into<String>) -> EvaluationError {
    EvaluationError::InvalidFailElement {
        index,
        reason: reason.into(),
    }
}

fn parse_element(index: usize, element: &Value) -> Result<Finding, EvaluationError> {
    let Value::Object(map) = element else {
        return Err(invalid(index, "element is not an object"));
    };
    let resource_id = required_string(index, map, "resource_id")?;
    let message = required_string(index, map, "message")?;
    let resource_region = optional_string(index, map, "resource_region")?;
    let path = optional_string(index, map, "path")?;
    if let Some(p) = &path
        && !is_json_pointer(p)
    {
        return Err(invalid(index, "`path` is not a JSON Pointer (RFC 6901)"));
    }
    Ok(Finding {
        resource_id,
        resource_region,
        message,
        path,
        actual: map.get("actual").cloned(),
        expected: map.get("expected").cloned(),
    })
}

fn required_string(
    index: usize,
    map: &Map<String, Value>,
    key: &str,
) -> Result<String, EvaluationError> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(invalid(index, format!("`{key}` is not a string"))),
        None => Err(invalid(index, format!("`{key}` is missing"))),
    }
}

fn optional_string(
    index: usize,
    map: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, EvaluationError> {
    match map.get(key) {
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(invalid(index, format!("`{key}` is not a string"))),
        None => Ok(None),
    }
}

/// Whether `s` is a JSON Pointer (RFC 6901): empty, or `/`-prefixed with `~` only before `0`/`1`.
fn is_json_pointer(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if !s.starts_with('/') {
        return false;
    }
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn res(id: &str, region: &str) -> Value {
        json!({"id": id, "type": "aws.example.widget", "account": "aws-123456789012", "region": region})
    }

    #[test]
    fn json_pointer_rules() {
        assert!(is_json_pointer(""));
        assert!(is_json_pointer("/"));
        assert!(is_json_pointer("/a/b~0c~1d"));
        assert!(!is_json_pointer("a"));
        assert!(!is_json_pointer("/a~"));
        assert!(!is_json_pointer("/a~2"));
    }

    #[test]
    fn maps_with_and_without_region_and_orders_by_id_region() {
        let target = vec![
            res("b", "us-east-1"),
            res("a", "us-west-2"),
            res("a", "us-east-1"),
        ];
        let elements = vec![
            json!({"resource_id": "a", "message": "m1"}),
            json!({"resource_id": "a", "resource_region": "us-west-2", "message": "m2", "actual": null}),
        ];
        let results = build_results("r", Severity::High, &elements, &target).unwrap();
        let summary: Vec<(String, String, ResultStatus)> = results
            .iter()
            .map(|r| (r.resource.id.clone(), r.resource.region.clone(), r.status))
            .collect();
        assert_eq!(
            summary,
            [
                ("a".into(), "us-east-1".into(), ResultStatus::Fail),
                ("a".into(), "us-west-2".into(), ResultStatus::Fail),
                ("a".into(), "us-west-2".into(), ResultStatus::Fail),
                ("b".into(), "us-east-1".into(), ResultStatus::Pass),
            ]
        );
        assert_eq!(results[2].actual, Some(Value::Null));
        assert_eq!(results[1].actual, None);
        assert_eq!(results[3].message, None);
    }

    #[test]
    fn errors() {
        let target = vec![res("a", "us-east-1")];
        let run = |e: Value| build_results("r", Severity::Low, &[e], &target).unwrap_err();
        assert!(matches!(
            run(json!({"resource_id": "zz", "message": "m"})),
            EvaluationError::UnknownResource { region: None, .. }
        ));
        assert!(matches!(
            run(json!({"resource_id": "a", "resource_region": "eu-west-1", "message": "m"})),
            EvaluationError::UnknownResource {
                region: Some(_),
                ..
            }
        ));
        assert!(matches!(
            run(json!({"resource_id": "a", "resource_region": null, "message": "m"})),
            EvaluationError::InvalidFailElement { index: 0, .. }
        ));
        assert!(matches!(
            build_results("r", Severity::Low, &[], &[res("a", "x"), res("a", "x")]),
            Err(EvaluationError::DuplicateResource { .. })
        ));
    }
}
