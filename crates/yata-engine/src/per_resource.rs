//! Per-resource evaluation of a rule: `fail`, `message` and `reason`, and the results.
//!
//! This module works on the JSON `data` document only, so it does not depend on
//! [`yata_core::Inventory`] and is shared by the evaluation of an inventory and the fixtures of
//! the rule tests. See ADR-0030.

use std::collections::BTreeSet;
use std::time::Instant;

use serde_json::{Map, Value};
use yata_core::{ResultResource, ResultStatus, RuleResult};

use crate::engine_config::{RegoEngine, RegoFailure, RegoValue};
use crate::error::{EvaluationError, ResourceKey, article};
use crate::input::resources_of;
use crate::rule::Rule;

/// Evaluates `rule` for every resource of its `target_type` in `data` and returns one result per
/// resource, in the order of the array (which is `(id, region)` order).
///
/// `data` is added to a clone of the rule's engine once; then `input` is replaced for each
/// resource. The time limit of the rule's engine applies to the whole call, not to each
/// resource. The first error stops the evaluation.
pub(crate) fn evaluate_resources(
    rule: &Rule,
    data: &Value,
) -> Result<Vec<RuleResult>, EvaluationError> {
    let started = Instant::now();
    let metadata = rule.metadata();
    let target_type = metadata.target_type.as_str();
    let target = resources_of(data, target_type);
    if target.is_empty() {
        return Ok(Vec::new());
    }
    let targets = target
        .iter()
        .enumerate()
        .map(|(index, value)| Target::from_value(index, value))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    for t in &targets {
        if !seen.insert((t.key.id.as_str(), t.key.region.as_str())) {
            return Err(EvaluationError::DuplicateResource {
                resource_id: t.key.id.clone(),
                region: t.key.region.clone(),
            });
        }
    }

    let mut engine = rule.engine().clone();
    let limit = engine.timeout();
    engine.set_data(data).map_err(|failure| match failure {
        RegoFailure::Timeout => EvaluationError::Timeout {
            limit,
            resource: None,
        },
        RegoFailure::Other(message) => EvaluationError::Engine {
            message,
            resource: None,
        },
    })?;

    let package = rule.package();
    let mut results = Vec::with_capacity(targets.len());
    for (index, t) in targets.iter().enumerate() {
        if !engine.set_input_from_data(target_type, index) {
            return Err(EvaluationError::InvalidResource { index });
        }
        engine.set_resource(&t.key.id);

        let failed = match eval(&mut engine, &package, "fail", started, &t.key)? {
            RegoValue::Bool(b) => b,
            RegoValue::NoRule | RegoValue::Undefined => {
                return Err(EvaluationError::FailUndefined {
                    resource: t.key.clone(),
                });
            }
            other => {
                return Err(EvaluationError::FailNotBoolean {
                    found: other.type_name(),
                    resource: t.key.clone(),
                });
            }
        };

        let mut result = RuleResult {
            rule_id: rule.id().to_owned(),
            status: if failed {
                ResultStatus::Fail
            } else {
                ResultStatus::Pass
            },
            severity: metadata.severity,
            resource: t.resource.clone(),
            message: None,
            path: None,
            actual: None,
            expected: None,
        };
        if failed {
            let message = match eval(&mut engine, &package, "message", started, &t.key)? {
                RegoValue::NoRule | RegoValue::Undefined => metadata.title.clone(),
                value => message_of(value, &t.key)?,
            };
            result.message = Some(message);
            match eval(&mut engine, &package, "reason", started, &t.key)? {
                RegoValue::NoRule | RegoValue::Undefined => {}
                value => apply_reason(value, &mut result, &t.key)?,
            }
        }
        results.push(result);
    }
    Ok(results)
}

/// A resource of the target type: its key and the part of the result that describes it.
struct Target {
    key: ResourceKey,
    resource: ResultResource,
}

impl Target {
    fn from_value(index: usize, value: &Value) -> Result<Self, EvaluationError> {
        let string = |name: &str| value.get(name).and_then(Value::as_str);
        let (Some(id), Some(region)) = (string("id"), string("region")) else {
            return Err(EvaluationError::InvalidResource { index });
        };
        Ok(Self {
            key: ResourceKey {
                id: id.to_owned(),
                region: region.to_owned(),
            },
            resource: ResultResource {
                id: id.to_owned(),
                resource_type: string("type").unwrap_or_default().to_owned(),
                account: string("account").unwrap_or_default().to_owned(),
                region: region.to_owned(),
            },
        })
    }
}

/// Evaluates `<package>.<name>` with the time that is left of the limit of the whole call.
fn eval(
    engine: &mut RegoEngine,
    package: &str,
    name: &str,
    started: Instant,
    key: &ResourceKey,
) -> Result<RegoValue, EvaluationError> {
    let limit = engine.timeout();
    let timeout = || EvaluationError::Timeout {
        limit,
        resource: Some(key.clone()),
    };
    let left = limit
        .checked_sub(started.elapsed())
        .filter(|d| !d.is_zero());
    let Some(left) = left else {
        return Err(timeout());
    };
    engine.set_time_limit(left);
    match engine.eval_rule(&format!("{package}.{name}")) {
        Ok(value) => Ok(value),
        Err(RegoFailure::Timeout) => Err(timeout()),
        Err(RegoFailure::Other(message)) => Err(EvaluationError::Engine {
            message,
            resource: Some(key.clone()),
        }),
    }
}

fn message_of(value: RegoValue, key: &ResourceKey) -> Result<String, EvaluationError> {
    let invalid = |reason: String| EvaluationError::InvalidMessage {
        reason,
        resource: key.clone(),
    };
    match value {
        RegoValue::Json(Value::String(s)) if s.is_empty() => Err(invalid(
            "message must be a non-empty string, but was an empty string".to_owned(),
        )),
        RegoValue::Json(Value::String(s)) => Ok(s),
        other => Err(invalid(format!(
            "message must be a non-empty string, but was {}",
            article(other.type_name())
        ))),
    }
}

fn apply_reason(
    value: RegoValue,
    result: &mut RuleResult,
    key: &ResourceKey,
) -> Result<(), EvaluationError> {
    let invalid = |reason: String| EvaluationError::InvalidReason {
        reason,
        resource: key.clone(),
    };
    let RegoValue::Json(Value::Object(reason)) = &value else {
        return Err(invalid(format!(
            "reason must be an object, but was {}",
            article(value.type_name())
        )));
    };
    result.path = path_of(reason).map_err(invalid)?;
    result.actual = reason.get("actual").cloned();
    result.expected = reason.get("expected").cloned();
    Ok(())
}

/// The `path` of a `reason`: absent, or a JSON Pointer string (RFC 6901).
fn path_of(reason: &Map<String, Value>) -> Result<Option<String>, String> {
    match reason.get("path") {
        None => Ok(None),
        Some(Value::String(path)) if is_json_pointer(path) => Ok(Some(path.clone())),
        Some(_) => Err("reason.path must be a JSON Pointer string".to_owned()),
    }
}

/// Whether `s` is a JSON Pointer: empty, or starting with `/`, with `~` only before `0` or `1`.
fn is_json_pointer(s: &str) -> bool {
    if !(s.is_empty() || s.starts_with('/')) {
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
    use super::is_json_pointer;

    #[test]
    fn json_pointers() {
        for ok in ["", "/", "/a/b", "/a~0b", "/a~1b", "/configuration/0/x"] {
            assert!(is_json_pointer(ok), "{ok}");
        }
        for bad in ["a", "a/b", "/a~", "/a~2", "/a~b", "~0"] {
            assert!(!is_json_pointer(bad), "{bad}");
        }
    }
}
