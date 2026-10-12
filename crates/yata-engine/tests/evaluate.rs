//! Tests for evaluating rules and building outcomes (stage P2).
//!
//! The rules are in `tests/fixtures/policies/evaluate/`. Inventories are built here from dummy
//! values only; nothing connects to a cloud provider.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value, json};
use yata_core::{
    AccountInfo, CollectionError, CollectionErrorKind, Inventory, Resource, ResourceSet,
    ResourceTypeInfo, ResultStatus, RuleOutcome, RuleStatus, Scope, Severity, SkipReason,
};
use yata_engine::{
    LoadMode, LoadOptions, LoadedPolicies, ResourceLimits, Rule, YataVersion, evaluate,
    load_policies,
};

const ACCOUNT: &str = "aws-123456789012";
const OTHER_ACCOUNT: &str = "aws-111122223333";
const EAST: &str = "us-east-1";
const WEST: &str = "us-west-2";

const WIDGET: &str = "aws.example.widget";
const GADGET: &str = "aws.example.gadget";
const TRAIL: &str = "aws.example.trail";
const ACCOUNT_TYPE: &str = "aws.account.account";

fn policy_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policies/evaluate")
}

fn load(timeout: Duration) -> LoadedPolicies {
    let options = LoadOptions {
        yata_version: YataVersion::new(0, 1, 0),
        limits: ResourceLimits::default(),
        policy_timeout: timeout,
        mode: LoadMode::Rules,
    };
    let loaded = load_policies(&[policy_dir()], &options).unwrap();
    assert!(
        loaded.rule_failures.is_empty(),
        "{:?}",
        loaded.rule_failures
    );
    loaded
}

fn rule<'a>(loaded: &'a LoadedPolicies, name: &str) -> &'a Rule {
    let id = format!("eval_policies.{name}");
    loaded
        .rules()
        .find(|r| r.id() == id)
        .unwrap_or_else(|| panic!("rule {id} not found"))
}

/// Evaluates one rule against `inventory` with the default (long) time limit.
fn eval(name: &str, inventory: &Inventory) -> Vec<RuleOutcome> {
    let loaded = load(Duration::from_secs(30));
    evaluate(&[rule(&loaded, name)], inventory)
}

fn one(mut outcomes: Vec<RuleOutcome>) -> RuleOutcome {
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    outcomes.remove(0)
}

fn account(key: &str) -> AccountInfo {
    AccountInfo {
        account: key.to_owned(),
        id: key.trim_start_matches("aws-").to_owned(),
        provider: "aws".to_owned(),
    }
}

fn resource(
    account: &str,
    resource_type: &str,
    id: &str,
    region: &str,
    configuration: Value,
) -> Resource {
    Resource {
        id: id.to_owned(),
        resource_type: resource_type.to_owned(),
        account: account.to_owned(),
        region: region.to_owned(),
        name: None,
        tags: BTreeMap::new(),
        configuration: match configuration {
            Value::Object(map) => map,
            _ => Map::new(),
        },
    }
}

fn widget(id: &str, region: &str, owner: Option<&str>) -> Resource {
    let mut r = resource(ACCOUNT, WIDGET, id, region, json!({}));
    if let Some(owner) = owner {
        r.tags.insert("owner".to_owned(), owner.to_owned());
    }
    r
}

fn set(account: &str, region: &str, resource_type: &str, resources: Vec<Resource>) -> ResourceSet {
    ResourceSet {
        account: account.to_owned(),
        region: region.to_owned(),
        resource_type: resource_type.to_owned(),
        resources,
    }
}

fn failure(region: &str, resource_type: &str) -> CollectionError {
    CollectionError {
        account: ACCOUNT.to_owned(),
        region: region.to_owned(),
        resource_type: resource_type.to_owned(),
        kind: CollectionErrorKind::AccessDenied,
        message: "access denied".to_owned(),
    }
}

/// An inventory for one account, two regions and the example types, with every group collected
/// and empty. Tests replace the sets they care about.
fn base_inventory() -> Inventory {
    let mut resource_types = vec![
        ResourceTypeInfo {
            resource_type: ACCOUNT_TYPE.to_owned(),
            scope: Scope::Global,
        },
        ResourceTypeInfo {
            resource_type: GADGET.to_owned(),
            scope: Scope::Regional,
        },
        ResourceTypeInfo {
            resource_type: TRAIL.to_owned(),
            scope: Scope::Regional,
        },
        ResourceTypeInfo {
            resource_type: WIDGET.to_owned(),
            scope: Scope::Regional,
        },
    ];
    resource_types.sort_by(|a, b| a.resource_type.cmp(&b.resource_type));
    let mut resource_sets = vec![set(
        ACCOUNT,
        "global",
        ACCOUNT_TYPE,
        vec![resource(
            ACCOUNT,
            ACCOUNT_TYPE,
            "aws.account.account:123456789012:global",
            "global",
            json!({"GetCallerIdentity": {"Account": "123456789012"}}),
        )],
    )];
    for region in [EAST, WEST] {
        for t in [GADGET, TRAIL, WIDGET] {
            resource_sets.push(set(ACCOUNT, region, t, vec![]));
        }
    }
    Inventory {
        accounts: vec![account(ACCOUNT)],
        regions: vec![EAST.to_owned(), WEST.to_owned()],
        resource_types,
        resource_sets,
        collection_errors: vec![],
        started_at: SystemTime::UNIX_EPOCH,
        finished_at: SystemTime::UNIX_EPOCH,
    }
}

/// Replaces the set of one group.
fn put(inventory: &mut Inventory, region: &str, resource_type: &str, resources: Vec<Resource>) {
    let s = inventory
        .resource_sets
        .iter_mut()
        .find(|s| s.account == ACCOUNT && s.region == region && s.resource_type == resource_type)
        .unwrap();
    s.resources = resources;
}

/// Turns a collected group into a failed one.
fn fail_group(inventory: &mut Inventory, region: &str, resource_type: &str) {
    inventory
        .resource_sets
        .retain(|s| !(s.region == region && s.resource_type == resource_type));
    inventory
        .collection_errors
        .push(failure(region, resource_type));
}

/// Removes a group so that it is neither collected nor failed.
fn drop_group(inventory: &mut Inventory, region: &str, resource_type: &str) {
    inventory
        .resource_sets
        .retain(|s| !(s.region == region && s.resource_type == resource_type));
}

fn summary(outcome: &RuleOutcome) -> Vec<(String, String, ResultStatus)> {
    outcome
        .results
        .iter()
        .map(|r| (r.resource.id.clone(), r.resource.region.clone(), r.status))
        .collect()
}

fn triple(id: &str, region: &str, status: ResultStatus) -> (String, String, ResultStatus) {
    (id.to_owned(), region.to_owned(), status)
}

// ------------------------------------------------------------------ status order

#[test]
fn evaluated_with_pass_and_fail_results() {
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![
            widget("w-2", EAST, None),
            widget("w-1", EAST, Some("alice")),
        ],
    );
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(outcome.rule_id, "eval_policies.widget_owner");
    assert_eq!(outcome.account, ACCOUNT);
    assert_eq!(outcome.skip_reason, None);
    assert_eq!(outcome.error, None);
    assert!(outcome.incomplete_resources.is_empty());
    assert_eq!(outcome.metadata.title, "Widget must have an owner tag");
    assert_eq!(
        summary(&outcome),
        [
            triple("w-1", EAST, ResultStatus::Pass),
            triple("w-2", EAST, ResultStatus::Fail),
        ]
    );

    let pass = &outcome.results[0];
    assert_eq!(pass.message, None);
    assert_eq!(pass.path, None);
    assert_eq!(pass.actual, None);
    assert_eq!(pass.expected, None);
    assert_eq!(pass.severity, Severity::High);

    let fail = &outcome.results[1];
    assert_eq!(fail.rule_id, "eval_policies.widget_owner");
    assert_eq!(fail.message.as_deref(), Some("Widget w-2 has no owner tag"));
    assert_eq!(fail.path.as_deref(), Some("/tags"));
    assert_eq!(fail.actual, Some(json!({})));
    assert_eq!(fail.expected, Some(json!({"owner": "any"})));
    assert_eq!(fail.resource.resource_type, WIDGET);
    assert_eq!(fail.resource.account, ACCOUNT);
}

#[test]
fn zero_resources_is_evaluated_with_no_results() {
    let outcome = one(eval("widget_owner", &base_inventory()));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert!(outcome.results.is_empty());
}

#[test]
fn null_and_absent_values_differ() {
    let mut inv = base_inventory();
    let mut nullish = widget("w-null", EAST, Some("alice"));
    nullish.name = Some("nullish".to_owned());
    put(&mut inv, EAST, WIDGET, vec![nullish]);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.results.len(), 1);
    let r = &outcome.results[0];
    assert_eq!(r.status, ResultStatus::Fail);
    assert_eq!(r.path, None);
    assert_eq!(r.actual, Some(Value::Null));
    assert_eq!(r.expected, Some(Value::Null));
}

#[test]
fn not_collected_group_skips_without_evaluating() {
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-2", EAST, None)]);
    // The related type is missing in one region.
    drop_group(&mut inv, WEST, GADGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Skipped);
    assert_eq!(outcome.skip_reason, Some(SkipReason::NotCollected));
    assert!(outcome.results.is_empty());
    assert!(outcome.incomplete_resources.is_empty());
    assert_eq!(outcome.error, None);
}

#[test]
fn type_that_was_not_targeted_is_not_collected() {
    let mut inv = base_inventory();
    inv.resource_types.retain(|t| t.resource_type != GADGET);
    inv.resource_sets.retain(|s| s.resource_type != GADGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Skipped);
    assert_eq!(outcome.skip_reason, Some(SkipReason::NotCollected));
}

#[test]
fn all_groups_failed_skips_with_collection_failed() {
    let mut inv = base_inventory();
    fail_group(&mut inv, EAST, GADGET);
    fail_group(&mut inv, WEST, GADGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Skipped);
    assert_eq!(outcome.skip_reason, Some(SkipReason::CollectionFailed));
    assert!(outcome.results.is_empty());
}

#[test]
fn not_collected_takes_precedence_over_collection_failed() {
    let mut inv = base_inventory();
    fail_group(&mut inv, EAST, GADGET);
    fail_group(&mut inv, WEST, GADGET);
    drop_group(&mut inv, EAST, WIDGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.skip_reason, Some(SkipReason::NotCollected));
}

#[test]
fn some_failed_groups_make_the_rule_incomplete_and_use_the_collected_ones() {
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-2", EAST, None)]);
    fail_group(&mut inv, WEST, WIDGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Incomplete);
    assert_eq!(summary(&outcome), [triple("w-2", EAST, ResultStatus::Fail)]);
    assert!(outcome.incomplete_resources.is_empty());
}

#[test]
fn a_failed_related_group_alone_makes_the_rule_incomplete() {
    let mut inv = base_inventory();
    fail_group(&mut inv, WEST, GADGET);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Incomplete);
}

#[test]
fn account_resource_is_evaluated_like_any_other() {
    let mut inv = base_inventory();
    let outcome = one(eval("account_flag", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(
        summary(&outcome),
        [triple(
            "aws.account.account:123456789012:global",
            "global",
            ResultStatus::Pass
        )]
    );

    inv.resource_sets[0].resources[0]
        .configuration
        .insert("Flagged".to_owned(), json!(true));
    let outcome = one(eval("account_flag", &inv));
    assert_eq!(outcome.results[0].status, ResultStatus::Fail);
    assert_eq!(
        outcome.results[0].message.as_deref(),
        Some("Account is flagged")
    );
}

// ------------------------------------------------------------------ failure markers

#[test]
fn failure_markers_make_the_rule_incomplete_and_are_listed_in_order() {
    let mut inv = base_inventory();
    let mut w_west = widget("w-1", WEST, Some("alice"));
    w_west.configuration.insert(
        "DescribeB".to_owned(),
        json!({"yata_error": {"kind": "throttled"}}),
    );
    w_west
        .configuration
        .insert("DescribeA".to_owned(), json!({"yata_error": {"kind": 5}}));
    // Not markers: another key next to `yata_error`, and a plain value.
    w_west.configuration.insert(
        "Other".to_owned(),
        json!({"yata_error": {"kind": "x"}, "more": 1}),
    );
    w_west
        .configuration
        .insert("Plain".to_owned(), json!({"a": 1}));
    let mut w_east = widget("w-1", EAST, Some("alice"));
    w_east.configuration.insert(
        "Describe".to_owned(),
        json!({"yata_error": {"kind": "access_denied"}}),
    );
    put(&mut inv, EAST, WIDGET, vec![w_east]);
    put(&mut inv, WEST, WIDGET, vec![w_west]);
    let gadget = resource(
        ACCOUNT,
        GADGET,
        "g-1",
        EAST,
        json!({"List": {"yata_error": {"kind": "timeout"}}}),
    );
    put(&mut inv, EAST, GADGET, vec![gadget]);

    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Incomplete);
    // The marked resources are still evaluated.
    assert_eq!(
        summary(&outcome),
        [
            triple("w-1", EAST, ResultStatus::Pass),
            triple("w-1", WEST, ResultStatus::Pass),
        ]
    );
    let listed: Vec<(&str, &str, &str, &str, &str)> = outcome
        .incomplete_resources
        .iter()
        .map(|i| {
            (
                i.resource_type.as_str(),
                i.resource_id.as_str(),
                i.region.as_str(),
                i.operation.as_str(),
                i.kind.as_str(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (GADGET, "g-1", EAST, "List", "timeout"),
            (WIDGET, "w-1", EAST, "Describe", "access_denied"),
            (WIDGET, "w-1", WEST, "DescribeA", "unknown"),
            (WIDGET, "w-1", WEST, "DescribeB", "throttled"),
        ]
    );
}

#[test]
fn markers_of_types_the_rule_does_not_declare_are_ignored() {
    let mut inv = base_inventory();
    let trail = resource(
        ACCOUNT,
        TRAIL,
        "t-1",
        EAST,
        json!({"Describe": {"yata_error": {"kind": "timeout"}}}),
    );
    put(&mut inv, EAST, TRAIL, vec![trail]);
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert!(outcome.incomplete_resources.is_empty());
}

// ------------------------------------------------------------------ mapping with regions

fn trail(id: &str, region: &str, logging: &str, scope: &str) -> Resource {
    resource(
        ACCOUNT,
        TRAIL,
        id,
        region,
        json!({"Logging": logging, "Scope": scope}),
    )
}

#[test]
fn resource_region_maps_to_one_resource_and_its_absence_to_all_copies() {
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        TRAIL,
        vec![
            trail("t-1", EAST, "off", "region"),
            trail("t-2", EAST, "off", "all"),
            trail("t-3", EAST, "on", "all"),
        ],
    );
    put(
        &mut inv,
        WEST,
        TRAIL,
        vec![
            // The same trail id seen from another region: only the east copy is not logging.
            trail("t-1", WEST, "on", "region"),
            trail("t-2", WEST, "off", "all"),
            trail("t-3", WEST, "on", "all"),
        ],
    );
    let outcome = one(eval("trail_region", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(
        summary(&outcome),
        [
            triple("t-1", EAST, ResultStatus::Fail),
            triple("t-1", WEST, ResultStatus::Pass),
            // `t-2` has no `resource_region`: every copy fails, whichever one matched the body.
            triple("t-2", EAST, ResultStatus::Fail),
            triple("t-2", WEST, ResultStatus::Fail),
            triple("t-3", EAST, ResultStatus::Pass),
            triple("t-3", WEST, ResultStatus::Pass),
        ]
    );
    assert_eq!(
        outcome.results[0].message.as_deref(),
        Some("Trail t-1 is not logging in us-east-1")
    );
    assert_eq!(
        outcome.results[2].message.as_deref(),
        Some("Trail t-2 is not logging anywhere")
    );
    assert_eq!(outcome.results[3].resource.region, WEST);
}

// ------------------------------------------------------------------ errors

fn bad_inventory() -> Inventory {
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-1", EAST, Some("alice"))],
    );
    inv
}

fn assert_error(outcome: &RuleOutcome, needle: &str) {
    assert_eq!(outcome.status, RuleStatus::Error, "{outcome:?}");
    let message = outcome.error.as_deref().unwrap();
    assert!(message.contains(needle), "{message}");
    assert!(!message.contains('\n'), "{message}");
    assert!(outcome.results.is_empty());
    assert!(outcome.incomplete_resources.is_empty());
    assert_eq!(outcome.skip_reason, None);
}

#[test]
fn malformed_fail_elements_make_the_rule_status_error() {
    let cases = [
        ("bad.no_resource_id", "`resource_id` is missing"),
        ("bad.no_message", "`message` is missing"),
        ("bad.region_not_string", "`resource_region` is not a string"),
        ("bad.region_null", "`resource_region` is not a string"),
        ("bad.bad_path", "JSON Pointer"),
        ("bad.not_object", "not an object"),
        ("bad.unknown_id", "unknown resource \"no-such-widget\""),
        ("bad.unknown_id_region", "in region \"eu-west-3\""),
    ];
    let loaded = load(Duration::from_secs(30));
    let inv = bad_inventory();
    for (name, needle) in cases {
        let outcome = one(evaluate(&[rule(&loaded, name)], &inv));
        assert_error(&outcome, needle);
    }
}

#[test]
fn fail_that_is_not_a_set_makes_the_rule_status_error() {
    // With no widget the rule's `fail` is an (empty) set; with a widget it is undefined.
    let outcome = one(eval("fail_not_set", &base_inventory()));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    let outcome = one(eval("fail_not_set", &bad_inventory()));
    assert_error(&outcome, "`fail` is not a set");
}

#[test]
fn duplicate_id_and_region_in_the_target_type_is_an_error() {
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![
            widget("w-1", EAST, Some("a")),
            widget("w-1", EAST, Some("b")),
        ],
    );
    assert_error(
        &one(eval("widget_owner", &inv)),
        "duplicate resource \"w-1\" in region \"us-east-1\"",
    );
}

#[test]
fn a_rule_that_does_not_finish_in_time_is_an_error() {
    let loaded = load(Duration::from_secs(1));
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-1", EAST, Some("alice"))],
    );
    let started = Instant::now();
    let outcome = one(evaluate(&[rule(&loaded, "slow")], &inv));
    assert_error(&outcome, "timed out");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );

    // Without a widget the same rule ends at once.
    let outcome = one(evaluate(&[rule(&loaded, "slow")], &base_inventory()));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
}

#[test]
fn an_error_in_one_rule_does_not_affect_the_others() {
    let loaded = load(Duration::from_secs(30));
    let inv = bad_inventory();
    let outcomes = evaluate(
        &[
            rule(&loaded, "widget_owner"),
            rule(&loaded, "bad.unknown_id"),
        ],
        &inv,
    );
    let statuses: Vec<(&str, RuleStatus)> = outcomes
        .iter()
        .map(|o| (o.rule_id.as_str(), o.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("eval_policies.bad.unknown_id", RuleStatus::Error),
            ("eval_policies.widget_owner", RuleStatus::Evaluated),
        ]
    );
}

// ------------------------------------------------------------------ input and ordering

#[test]
fn input_has_sorted_arrays_and_only_declared_types() {
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-a", EAST, None)]);
    put(
        &mut inv,
        WEST,
        WIDGET,
        vec![widget("w-b", WEST, None), widget("w-a", WEST, None)],
    );
    put(
        &mut inv,
        EAST,
        GADGET,
        vec![resource(ACCOUNT, GADGET, "g-1", EAST, json!({}))],
    );
    put(
        &mut inv,
        WEST,
        GADGET,
        vec![resource(ACCOUNT, GADGET, "g-2", WEST, json!({}))],
    );
    // A trail exists in the inventory, but the probe rule does not declare the type.
    put(&mut inv, EAST, TRAIL, vec![trail("t-1", EAST, "on", "all")]);

    let outcome = one(eval("input_probe", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(
        summary(&outcome),
        [
            triple("w-a", EAST, ResultStatus::Fail),
            triple("w-a", WEST, ResultStatus::Fail),
            triple("w-b", WEST, ResultStatus::Fail),
        ]
    );
    let actual = outcome.results[0].actual.clone().unwrap();
    assert_eq!(
        actual,
        json!({
            "account_id": "123456789012",
            "regions": [EAST, WEST],
            "example_keys": ["gadget", "widget"],
            "top_keys": ["account", "aws", "regions"],
            "widget_order": ["w-a@us-east-1", "w-a@us-west-2", "w-b@us-west-2"],
            "gadget_count": 2,
        })
    );
}

#[test]
fn outcomes_are_ordered_by_rule_id_then_account_and_use_each_accounts_data() {
    let loaded = load(Duration::from_secs(30));
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-1", EAST, None)]);
    // A second account whose groups were never collected.
    inv.accounts.insert(0, account(OTHER_ACCOUNT));
    // And a third one with its own collected data, listed after the others.
    inv.accounts.push(account("aws-444455556666"));
    let third = "aws-444455556666";
    inv.resource_sets.push(set(
        third,
        "global",
        ACCOUNT_TYPE,
        vec![resource(third, ACCOUNT_TYPE, "x", "global", json!({}))],
    ));
    for region in [EAST, WEST] {
        for t in [GADGET, WIDGET] {
            let resources = if region == EAST && t == WIDGET {
                vec![resource(third, WIDGET, "w-3", EAST, json!({}))]
            } else {
                vec![]
            };
            inv.resource_sets.push(set(third, region, t, resources));
        }
    }

    // Rules are passed in reverse order.
    let outcomes = evaluate(
        &[rule(&loaded, "widget_owner"), rule(&loaded, "account_flag")],
        &inv,
    );
    let keys: Vec<(&str, &str, RuleStatus)> = outcomes
        .iter()
        .map(|o| (o.rule_id.as_str(), o.account.as_str(), o.status))
        .collect();
    assert_eq!(
        keys,
        [
            (
                "eval_policies.account_flag",
                OTHER_ACCOUNT,
                RuleStatus::Skipped
            ),
            ("eval_policies.account_flag", ACCOUNT, RuleStatus::Evaluated),
            ("eval_policies.account_flag", third, RuleStatus::Evaluated),
            (
                "eval_policies.widget_owner",
                OTHER_ACCOUNT,
                RuleStatus::Skipped
            ),
            ("eval_policies.widget_owner", ACCOUNT, RuleStatus::Evaluated),
            ("eval_policies.widget_owner", third, RuleStatus::Evaluated),
        ]
    );
    // Each account sees only its own resources.
    assert_eq!(outcomes[4].results.len(), 1);
    assert_eq!(outcomes[4].results[0].resource.id, "w-1");
    assert_eq!(outcomes[5].results.len(), 1);
    assert_eq!(outcomes[5].results[0].resource.id, "w-3");
    assert_eq!(outcomes[5].results[0].resource.account, third);
}

/// A minimal `tracing` subscriber that records the events of the `print` target.
#[derive(Clone, Default)]
struct PrintLog(std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>);

struct FieldCollector {
    rule: String,
    message: String,
}

impl tracing::field::Visit for FieldCollector {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "rule" => self.rule = format!("{value:?}"),
            "message" => self.message = format!("{value:?}"),
            _ => {}
        }
    }
}

impl tracing::Subscriber for PrintLog {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut collector = FieldCollector {
            rule: String::new(),
            message: String::new(),
        };
        event.record(&mut collector);
        let level = event.metadata().level().to_string();
        if event.metadata().target() == "yata_engine::rego_print" {
            self.0
                .lock()
                .unwrap()
                .push((level, collector.rule, collector.message));
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// That nothing is written to standard output is guaranteed in the code: `yata-engine` has no
// `print!` / `println!` call, and `RegoEngine` gathers `print` output and only logs it.
#[test]
fn print_output_goes_to_the_debug_log_with_the_rule_id() {
    let loaded = load(Duration::from_secs(30));
    let mut inv = base_inventory();
    let mut printed = widget("w-1", EAST, None);
    printed.name = Some("printed".to_owned());
    put(&mut inv, EAST, WIDGET, vec![printed]);

    let log = PrintLog::default();
    let guard = tracing::subscriber::set_default(log.clone());
    let outcome = one(evaluate(&[rule(&loaded, "printing")], &inv));
    drop(guard);

    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(summary(&outcome), [triple("w-1", EAST, ResultStatus::Fail)]);
    let events = log.0.lock().unwrap().clone();
    assert_eq!(events.len(), 1, "{events:?}");
    let (level, rule_id, message) = &events[0];
    assert_eq!(level, "DEBUG");
    assert_eq!(rule_id, "eval_policies.printing");
    assert!(
        message.contains("checking") && message.contains("w-1"),
        "{message}"
    );
}

#[test]
fn a_fail_value_that_is_not_a_set_is_an_error() {
    // Each rule's `fail` is a set without widgets and a string / an array with one.
    let loaded = load(Duration::from_secs(30));
    for name in ["fail_string", "fail_array"] {
        let ok = one(evaluate(&[rule(&loaded, name)], &base_inventory()));
        assert_eq!(ok.status, RuleStatus::Evaluated, "{name}");
        let outcome = one(evaluate(&[rule(&loaded, name)], &bad_inventory()));
        assert_error(&outcome, "`fail` is not a set");
    }
}
#[test]
fn evaluating_twice_gives_the_same_outcomes() {
    let loaded = load(Duration::from_secs(30));
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-1", EAST, None)]);
    let rules = [rule(&loaded, "widget_owner")];
    assert_eq!(evaluate(&rules, &inv), evaluate(&rules, &inv));
}
