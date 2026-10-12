//! Extraction and validation of rule metadata.

use serde_json::{Map, Value};
use yata_core::{Reference, RuleMetadata, Severity};

use crate::names::parse_resource_type;

const SEVERITIES: &str = "critical, high, medium, low, info";

/// Validates the value of a rule's `metadata` and converts it to [`RuleMetadata`].
///
/// All problems found are reported together in one message, separated by `; `.
pub(crate) fn validate(value: &Value) -> Result<RuleMetadata, String> {
    let Value::Object(object) = value else {
        return Err("metadata must be an object".to_string());
    };
    let mut errors = Vec::new();

    let title = non_empty_string(object, "title", &mut errors);
    if let Some(t) = &title
        && t.contains(['\n', '\r'])
    {
        errors.push("metadata.title must not contain line breaks".to_string());
    }
    let severity = match object.get("severity") {
        Some(Value::String(s)) => {
            let parsed = Severity::parse(s);
            if parsed.is_none() {
                errors.push(format!("metadata.severity must be one of {SEVERITIES}"));
            }
            parsed
        }
        Some(_) => {
            errors.push(format!("metadata.severity must be one of {SEVERITIES}"));
            None
        }
        None => {
            errors.push("metadata.severity is required".to_string());
            None
        }
    };
    let description = non_empty_string(object, "description", &mut errors);
    let remediation = non_empty_string(object, "remediation", &mut errors);

    let target_type = match object.get("target_type") {
        Some(Value::String(s)) if parse_resource_type(s).is_some() => Some(s.clone()),
        Some(Value::String(_)) => {
            errors.push(
                "metadata.target_type must be of the form <provider>.<service>.<resource>"
                    .to_string(),
            );
            None
        }
        Some(_) => {
            errors.push("metadata.target_type must be a string".to_string());
            None
        }
        None => {
            errors.push("metadata.target_type is required".to_string());
            None
        }
    };

    let related_types = related_types(object, target_type.as_deref(), &mut errors);
    let references = references(object, &mut errors);
    let tags = tags(object, &mut errors);

    match (title, severity, description, remediation, target_type) {
        (Some(title), Some(severity), Some(description), Some(remediation), Some(target_type))
            if errors.is_empty() =>
        {
            Ok(RuleMetadata {
                title,
                severity,
                description,
                remediation,
                target_type,
                related_types,
                references,
                tags,
            })
        }
        _ => Err(errors.join("; ")),
    }
}

fn non_empty_string(
    object: &Map<String, Value>,
    key: &str,
    errors: &mut Vec<String>,
) -> Option<String> {
    match object.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::String(_)) => {
            errors.push(format!("metadata.{key} must not be empty"));
            None
        }
        Some(_) => {
            errors.push(format!("metadata.{key} must be a string"));
            None
        }
        None => {
            errors.push(format!("metadata.{key} is required"));
            None
        }
    }
}

fn related_types(
    object: &Map<String, Value>,
    target_type: Option<&str>,
    errors: &mut Vec<String>,
) -> Vec<String> {
    let Some(value) = object.get("related_types") else {
        return Vec::new();
    };
    let Value::Array(items) = value else {
        errors.push("metadata.related_types must be an array of strings".to_string());
        return Vec::new();
    };
    let target_provider = target_type
        .and_then(parse_resource_type)
        .map(|(provider, _, _)| provider);
    let mut result: Vec<String> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Value::String(s) = item else {
            errors.push(format!("metadata.related_types[{i}] must be a string"));
            continue;
        };
        let Some((provider, _, _)) = parse_resource_type(s) else {
            errors.push(format!(
                "metadata.related_types[{i}] must be of the form <provider>.<service>.<resource>"
            ));
            continue;
        };
        if target_provider.is_some_and(|p| p != provider) {
            errors.push(format!(
                "metadata.related_types[{i}] must have the same provider as target_type"
            ));
        }
        if target_type == Some(s.as_str()) {
            errors.push(format!(
                "metadata.related_types[{i}] must not repeat target_type"
            ));
        }
        if result.contains(s) {
            errors.push(format!("metadata.related_types[{i}] is a duplicate"));
        }
        result.push(s.clone());
    }
    result
}

fn references(object: &Map<String, Value>, errors: &mut Vec<String>) -> Vec<Reference> {
    let items = match object.get("references") {
        Some(Value::Array(items)) => items,
        Some(_) => {
            errors.push("metadata.references must be an array".to_string());
            return Vec::new();
        }
        None => {
            errors.push("metadata.references is required".to_string());
            return Vec::new();
        }
    };
    let mut result = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Value::Object(reference) = item else {
            errors.push(format!("metadata.references[{i}] must be an object"));
            continue;
        };
        let mut field = |key: &str| match reference.get(key) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::String(_)) => {
                errors.push(format!("metadata.references[{i}].{key} must not be empty"));
                None
            }
            Some(_) => {
                errors.push(format!("metadata.references[{i}].{key} must be a string"));
                None
            }
            None => {
                errors.push(format!("metadata.references[{i}].{key} is required"));
                None
            }
        };
        let standard = field("standard");
        let version = field("version");
        let control = field("control");
        let url = match reference.get("url") {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => {
                errors.push(format!("metadata.references[{i}].url must be a string"));
                None
            }
        };
        if let (Some(standard), Some(version), Some(control)) = (standard, version, control) {
            result.push(Reference {
                standard,
                version,
                control,
                url,
            });
        }
    }
    result
}

fn tags(object: &Map<String, Value>, errors: &mut Vec<String>) -> Vec<String> {
    let Some(value) = object.get("tags") else {
        return Vec::new();
    };
    let Value::Array(items) = value else {
        errors.push("metadata.tags must be an array of strings".to_string());
        return Vec::new();
    };
    let mut result = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::String(s) => result.push(s.clone()),
            _ => errors.push(format!("metadata.tags[{i}] must be a string")),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid() -> Value {
        json!({
            "title": "A title",
            "severity": "high",
            "description": "d",
            "remediation": "r",
            "target_type": "aws.example.widget",
            "related_types": ["aws.example.gadget"],
            "references": [{"standard": "s", "version": "1", "control": "1.1", "url": "https://example.com"}],
            "tags": ["t"],
            "unknown": 1
        })
    }

    #[test]
    fn accepts_valid_metadata() {
        let m = validate(&valid()).unwrap();
        assert_eq!(m.severity, Severity::High);
        assert_eq!(m.related_types, vec!["aws.example.gadget"]);
        assert_eq!(m.references[0].url.as_deref(), Some("https://example.com"));
        assert_eq!(m.tags, vec!["t"]);
    }

    #[test]
    fn optional_keys_may_be_missing() {
        let mut v = valid();
        v.as_object_mut().unwrap().remove("related_types");
        v.as_object_mut().unwrap().remove("tags");
        v["references"] = json!([]);
        let m = validate(&v).unwrap();
        assert!(m.related_types.is_empty() && m.tags.is_empty() && m.references.is_empty());
    }

    fn error_of(mutate: impl FnOnce(&mut Value)) -> String {
        let mut v = valid();
        mutate(&mut v);
        validate(&v).unwrap_err()
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(error_of(|v| v["severity"] = json!("urgent")).contains("must be one of critical"));
        assert!(error_of(|v| v["title"] = json!("a\nb")).contains("line breaks"));
        assert!(error_of(|v| v["title"] = json!("")).contains("must not be empty"));
        assert!(
            error_of(|v| {
                v.as_object_mut().unwrap().remove("description");
            })
            .contains("description is required")
        );
        assert!(error_of(|v| v["target_type"] = json!("aws.s3")).contains("target_type"));
        assert!(error_of(|v| v["related_types"] = json!(["gcp.x.y"])).contains("same provider"));
        assert!(
            error_of(|v| v["related_types"] = json!(["aws.example.widget"]))
                .contains("repeat target_type")
        );
        assert!(
            error_of(|v| v["related_types"] = json!(["aws.example.gadget", "aws.example.gadget"]))
                .contains("duplicate")
        );
        assert!(
            error_of(|v| v["references"] = json!([{"standard": "s"}]))
                .contains("references[0].version is required")
        );
        assert!(error_of(|v| v["tags"] = json!([1])).contains("tags[0]"));
        assert!(
            validate(&json!([]))
                .unwrap_err()
                .contains("must be an object")
        );
    }

    #[test]
    fn reports_several_errors_in_one_message() {
        let e = error_of(|v| {
            v["severity"] = json!("x");
            v["tags"] = json!("not-an-array");
        });
        assert!(e.contains("severity") && e.contains("tags") && e.contains("; "));
    }
}
