//! Timing of the per-resource evaluation with many resources (ADR-0030).
//!
//! These tests are ignored by default because they measure time and take a few seconds. Run
//! them in release mode, which is how `yata` runs:
//!
//! ```text
//! cargo test -p yata-engine --release --test evaluate_perf -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, json};
use yata_core::{
    AccountInfo, Inventory, Resource, ResourceSet, ResourceTypeInfo, ResultStatus, RuleStatus,
    Scope,
};
use yata_engine::{
    LoadMode, LoadOptions, LoadedPolicies, ResourceLimits, YataVersion, evaluate, load_policies,
};

const ACCOUNT: &str = "aws-123456789012";
const REGION: &str = "us-east-1";
const WIDGET: &str = "aws.example.widget";
const RESOURCES: usize = 10_000;

fn load() -> LoadedPolicies {
    let dir: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policies/evaluate");
    let options = LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: Duration::from_secs(300),
        mode: LoadMode::Rules,
    };
    let loaded = load_policies(&[dir], &options).unwrap();
    assert!(loaded.rule_failures.is_empty());
    loaded
}

/// An inventory with `RESOURCES` widgets. When `flagged` is true every widget has the tag
/// `flag=bad`, so the rule `title_only` fails all of them (and evaluates `message` and `reason`).
fn inventory(flagged: bool) -> Inventory {
    let widgets = (0..RESOURCES)
        .map(|i| Resource {
            id: format!("w-{i:05}"),
            resource_type: WIDGET.to_owned(),
            account: ACCOUNT.to_owned(),
            region: REGION.to_owned(),
            name: Some(format!("widget-{i}")),
            tags: BTreeMap::from([(
                "flag".to_owned(),
                if flagged { "bad" } else { "good" }.to_owned(),
            )]),
            configuration: match json!({
                "Describe": {"Owner": "alice", "Items": [1, 2, 3, 4, 5], "Enabled": true},
                "GetPolicy": {"Policy": {"Version": "2012-10-17", "Statement": []}},
            }) {
                serde_json::Value::Object(map) => map,
                _ => Map::new(),
            },
        })
        .collect();
    Inventory {
        accounts: vec![AccountInfo {
            account: ACCOUNT.to_owned(),
            id: "123456789012".to_owned(),
            provider: "aws".to_owned(),
        }],
        regions: vec![REGION.to_owned()],
        resource_types: vec![ResourceTypeInfo {
            resource_type: WIDGET.to_owned(),
            scope: Scope::Regional,
        }],
        resource_sets: vec![ResourceSet {
            account: ACCOUNT.to_owned(),
            region: REGION.to_owned(),
            resource_type: WIDGET.to_owned(),
            resources: widgets,
        }],
        collection_errors: vec![],
        started_at: SystemTime::UNIX_EPOCH,
        finished_at: SystemTime::UNIX_EPOCH,
    }
}

fn measure(rule_name: &str, flagged: bool, expected: ResultStatus) -> Duration {
    let loaded = load();
    let id = format!("eval_policies.{rule_name}");
    let rule = loaded.rules().find(|r| r.id() == id).unwrap();
    let inv = inventory(flagged);
    let started = Instant::now();
    let outcomes = evaluate(&[rule], &inv);
    let elapsed = started.elapsed();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, RuleStatus::Evaluated, "{outcomes:?}");
    assert_eq!(outcomes[0].results.len(), RESOURCES);
    assert!(outcomes[0].results.iter().all(|r| r.status == expected));
    elapsed
}

#[test]
#[ignore = "measures time; run with --release -- --ignored --nocapture"]
fn ten_thousand_passing_resources() {
    let elapsed = measure("title_only", false, ResultStatus::Pass);
    println!(
        "{RESOURCES} passing resources: {elapsed:?} ({:?} per resource)",
        elapsed / u32::try_from(RESOURCES).unwrap()
    );
}

#[test]
#[ignore = "measures time; run with --release -- --ignored --nocapture"]
fn ten_thousand_failing_resources_with_message_and_reason() {
    let elapsed = measure("widget_owner_all_fail", true, ResultStatus::Fail);
    println!(
        "{RESOURCES} failing resources: {elapsed:?} ({:?} per resource)",
        elapsed / u32::try_from(RESOURCES).unwrap()
    );
}
