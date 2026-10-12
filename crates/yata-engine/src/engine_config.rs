//! The configured Rego engine used for libraries and rules.
//!
//! This module is the only place that names `regorus` types. Everything else sees
//! [`RegoEngine`], [`RegoFailure`] and [`RegoValue`], so the engine can be replaced later.

use std::num::NonZeroU32;
use std::time::Duration;

use regorus::utils::limits::ExecutionTimerConfig;
use regorus::{Engine, LimitError, Value};

/// Number of statements between two clock reads of the evaluation timer.
const TIMER_CHECK_INTERVAL: u32 = 64;

/// Message of regorus when a rule path does not exist.
const UNKNOWN_RULE_PATH: &str = "not a valid rule path";

/// Why a Rego operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegoFailure {
    /// The evaluation exceeded the time limit.
    Timeout,
    /// Any other error, with the engine's message.
    Other(String),
}

impl RegoFailure {
    /// The message to show to the user.
    pub(crate) fn message(&self, limit: Duration) -> String {
        match self {
            Self::Timeout => format!(
                "evaluation exceeded the time limit of {} s",
                limit.as_secs_f64()
            ),
            Self::Other(m) => m.clone(),
        }
    }
}

/// Builds a [`RegoFailure`] from a regorus error without naming the error type.
macro_rules! failure {
    ($err:expr) => {{
        let err = $err;
        match err.downcast_ref::<LimitError>() {
            Some(LimitError::TimeLimitExceeded { .. }) => RegoFailure::Timeout,
            _ => RegoFailure::Other(format!("{err:#}")),
        }
    }};
}

/// The result of evaluating a rule path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RegoValue {
    /// The path is not defined.
    Undefined,
    /// A set, with its elements in the engine's order.
    Set(Vec<serde_json::Value>),
    /// Any other value.
    Json(serde_json::Value),
}

/// A Rego engine with the Yata configuration. Cloning copies the loaded modules and settings.
#[derive(Clone)]
pub(crate) struct RegoEngine {
    inner: Engine,
    label: String,
    timeout: Duration,
}

impl RegoEngine {
    /// Creates the base engine: Rego v1, an evaluation time limit, `print` output gathered, and
    /// built-in function errors treated as undefined.
    pub(crate) fn new(policy_timeout: Duration) -> Self {
        let mut inner = Engine::new();
        let check_interval = NonZeroU32::new(TIMER_CHECK_INTERVAL).unwrap_or(NonZeroU32::MIN);
        inner.set_execution_timer_config(ExecutionTimerConfig {
            limit: policy_timeout,
            check_interval,
        });
        inner.set_gather_prints(true);
        inner.set_strict_builtin_errors(false);
        Self {
            inner,
            label: String::new(),
            timeout: policy_timeout,
        }
    }

    /// The evaluation time limit this engine was created with.
    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Sets the `input` document for the following evaluations.
    pub(crate) fn set_input(&mut self, input: serde_json::Value) {
        self.inner.set_input(Value::from(input));
    }

    /// Sets the label (for example a rule ID) attached to `print` output in the logs.
    pub(crate) fn set_label(&mut self, label: &str) {
        self.label = label.to_string();
    }

    /// Parses a module. Returns its package, for example `data.yata.rules.p.r`.
    pub(crate) fn add_policy(&mut self, path: &str, source: &str) -> Result<String, RegoFailure> {
        self.inner
            .add_policy(path.to_string(), source.to_string())
            .map_err(|e| failure!(e))
    }

    /// Evaluates a rule path (for example `data.yata.rules.p.r.fail`) with the current input.
    pub(crate) fn eval_rule(&mut self, path: &str) -> Result<RegoValue, RegoFailure> {
        let result = self.inner.eval_rule(path.to_string());
        self.log_prints();
        let value = match result {
            Ok(v) => v,
            // regorus reports a path that is not a rule of any loaded module this way.
            Err(e) if e.to_string() == UNKNOWN_RULE_PATH => return Ok(RegoValue::Undefined),
            Err(e) => return Err(failure!(e)),
        };
        Ok(match value {
            Value::Undefined => RegoValue::Undefined,
            Value::Set(set) => {
                let mut elements = Vec::with_capacity(set.len());
                for v in set.iter() {
                    elements.push(to_json(v)?);
                }
                RegoValue::Set(elements)
            }
            other => RegoValue::Json(to_json(&other)?),
        })
    }

    /// Evaluates every rule of all loaded modules with no input (the query `data`).
    ///
    /// This finds module-level errors such as references to undefined functions or unsafe
    /// variables. The query `true` does not: regorus analyzes rule bodies lazily.
    pub(crate) fn eval_all(&mut self) -> Result<(), RegoFailure> {
        let result = self.inner.eval_query("data".to_string(), false);
        self.log_prints();
        result.map(|_| ()).map_err(|e| failure!(e))
    }

    fn log_prints(&mut self) {
        if let Ok(prints) = self.inner.take_prints() {
            for line in prints {
                tracing::debug!(target: "yata_engine::rego_print", rule = %self.label, "{line}");
            }
        }
    }
}

fn to_json(value: &Value) -> Result<serde_json::Value, RegoFailure> {
    serde_json::to_value(value)
        .map_err(|e| RegoFailure::Other(format!("cannot convert a Rego value to JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    const LIMIT: Duration = Duration::from_millis(300);

    fn engine_with(source: &str) -> RegoEngine {
        let mut e = RegoEngine::new(LIMIT);
        e.add_policy("t.rego", source).unwrap();
        e
    }

    #[test]
    fn clone_keeps_the_timer_and_print_settings() {
        let base = engine_with(
            "package t\nimport rego.v1\nslow := {1 | some i in numbers.range(1, 5000); some j in numbers.range(1, 5000); some k in numbers.range(1, 5000); i + j + k < 0}\nhello := 1 if { print(\"hi\") }\n",
        );
        let mut clone = base.clone();
        let start = Instant::now();
        assert_eq!(clone.eval_rule("data.t.slow"), Err(RegoFailure::Timeout));
        assert!(start.elapsed() < Duration::from_secs(20));
        // `print` output is gathered by the clone too (read it from the inner engine, because the
        // wrapper drains it into the log).
        let mut clone = base.clone();
        clone.inner.eval_rule("data.t.hello".to_string()).unwrap();
        assert_eq!(clone.inner.take_prints().unwrap().len(), 1);
    }

    #[test]
    fn time_limit_stops_comprehensions_every_and_walk() {
        let cases = [
            "x := count({1 | some i in numbers.range(1, 5000); some j in numbers.range(1, 5000); some k in numbers.range(1, 5000); i + j + k < 0})",
            "x if { every i in numbers.range(1, 5000) { every j in numbers.range(1, 5000) { every k in numbers.range(1, 5000) { i + j + k > 0 } } } }",
            "x if { some i in numbers.range(1, 5000); some j in numbers.range(1, 5000); some k in numbers.range(1, 5000); i + j + k < 0 }",
            "x := count([p | walk({\"a\": numbers.range(1, 3000), \"b\": numbers.range(1, 3000)}, [p, _]); some i in numbers.range(1, 3000); some j in numbers.range(1, 3000); i + j < 0])",
        ];
        for body in cases {
            let mut e = engine_with(&format!("package t\nimport rego.v1\n{body}\n"));
            let start = Instant::now();
            assert_eq!(e.eval_rule("data.t.x"), Err(RegoFailure::Timeout), "{body}");
            assert!(start.elapsed() < Duration::from_secs(20), "{body}");
        }
    }

    #[test]
    fn unknown_rule_paths_are_undefined_and_empty_partial_sets_are_empty_sets() {
        let mut e = engine_with(
            "package t\nimport rego.v1\nfail contains 1 if { input.never }\nf(x) := x\nneeds_input := input.x\n",
        );
        assert_eq!(e.eval_rule("data.t.fail"), Ok(RegoValue::Set(vec![])));
        assert_eq!(e.eval_rule("data.t.metadata"), Ok(RegoValue::Undefined));
        assert_eq!(
            e.eval_rule("data.nothing.metadata"),
            Ok(RegoValue::Undefined)
        );
        assert_eq!(e.eval_rule("data.t.needs_input"), Ok(RegoValue::Undefined));
    }

    #[test]
    fn evaluating_all_modules_finds_what_the_query_true_does_not() {
        let mut e = engine_with("package t\nbad := missing_function(1)\n");
        // regorus analyzes rule bodies lazily, so `true` alone does not find the error.
        assert!(e.inner.eval_query("true".to_string(), false).is_ok());
        let failure = e.eval_all().unwrap_err();
        let RegoFailure::Other(message) = failure else {
            panic!("expected an engine error");
        };
        assert!(message.contains("missing_function"), "{message}");
    }

    #[test]
    fn built_in_errors_are_undefined_not_errors() {
        let mut e = engine_with(
            "package t\nimport rego.v1\nx := time.parse_rfc3339_ns(\"N/A\")\ny := 1 if { not x }\n",
        );
        assert_eq!(e.eval_rule("data.t.x"), Ok(RegoValue::Undefined));
        assert_eq!(
            e.eval_rule("data.t.y"),
            Ok(RegoValue::Json(serde_json::json!(1)))
        );
    }

    #[test]
    fn syntax_errors_are_reported_by_add_policy() {
        let mut e = RegoEngine::new(LIMIT);
        assert!(matches!(
            e.add_policy("t.rego", "package t\nx := (\n"),
            Err(RegoFailure::Other(_))
        ));
    }
}
