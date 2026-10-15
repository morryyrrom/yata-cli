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
    ResourceTypeInfo, ResultStatus, RuleErrorKind, RuleOutcome, RuleStatus, Scope, Severity,
    SkipReason,
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
    install_print_log();
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

// ------------------------------------------------------------------ copies in several regions

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
fn copies_of_a_resource_in_several_regions_are_evaluated_separately() {
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
    // One result per copy, in `(id, region)` order.
    assert_eq!(
        summary(&outcome),
        [
            triple("t-1", EAST, ResultStatus::Fail),
            triple("t-1", WEST, ResultStatus::Pass),
            triple("t-2", EAST, ResultStatus::Fail),
            triple("t-2", WEST, ResultStatus::Fail),
            triple("t-3", EAST, ResultStatus::Pass),
            triple("t-3", WEST, ResultStatus::Pass),
        ]
    );
    // The rule tells the copies apart with `input.region`.
    assert_eq!(
        outcome.results[0].message.as_deref(),
        Some("Trail t-1 is not logging in us-east-1")
    );
    assert_eq!(
        outcome.results[3].message.as_deref(),
        Some("Trail t-2 is not logging in us-west-2")
    );
    assert_eq!(outcome.results[3].resource.region, WEST);
}

#[test]
fn results_are_one_per_target_resource_in_order() {
    let mut inv = base_inventory();
    put(
        &mut inv,
        WEST,
        WIDGET,
        vec![
            widget("w-b", WEST, Some("a")),
            widget("w-a", WEST, Some("a")),
        ],
    );
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-b", EAST, None), widget("w-a", EAST, Some("a"))],
    );
    let outcome = one(eval("widget_owner", &inv));
    assert_eq!(
        summary(&outcome),
        [
            triple("w-a", EAST, ResultStatus::Pass),
            triple("w-a", WEST, ResultStatus::Pass),
            triple("w-b", EAST, ResultStatus::Fail),
            triple("w-b", WEST, ResultStatus::Pass),
        ]
    );
}

// ------------------------------------------------------------------ errors

/// An inventory with a widget `w-1` and a widget `w-bad` named `bad`, both in the east region.
fn bad_inventory() -> Inventory {
    let mut inv = base_inventory();
    let mut bad = widget("w-bad", EAST, Some("alice"));
    bad.name = Some("bad".to_owned());
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-1", EAST, Some("alice")), bad],
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
fn rule_contract_violations_make_the_rule_status_error() {
    // (rule, text of the description, resource it names, kind)
    let cases = [
        (
            "undefined_at_runtime",
            "fail is undefined; add \"default fail := false\"",
            "w-1",
            RuleErrorKind::Type,
        ),
        (
            "fail_not_boolean",
            "fail must be a boolean, but was a string",
            "w-1",
            RuleErrorKind::Type,
        ),
        (
            "fail_conflict",
            "rule evaluation conflict",
            "w-1",
            RuleErrorKind::Conflict,
        ),
        (
            "message_not_string",
            "message must be a non-empty string, but was a number",
            "w-bad",
            RuleErrorKind::Type,
        ),
        (
            "message_empty",
            "message must be a non-empty string, but was an empty string",
            "w-bad",
            RuleErrorKind::Type,
        ),
        (
            "message_conflict",
            "rule evaluation conflict",
            "w-bad",
            RuleErrorKind::Conflict,
        ),
        (
            "reason_not_object",
            "reason must be an object, but was an array",
            "w-bad",
            RuleErrorKind::Type,
        ),
        (
            "reason_bad_path",
            "reason.path must be a JSON Pointer string",
            "w-bad",
            RuleErrorKind::Type,
        ),
        (
            "reason_path_null",
            "reason.path must be a JSON Pointer string",
            "w-bad",
            RuleErrorKind::Type,
        ),
    ];
    let loaded = load(Duration::from_secs(30));
    let inv = bad_inventory();
    for (name, text, resource, kind) in cases {
        let outcome = one(evaluate(&[rule(&loaded, &format!("bad.{name}"))], &inv));
        assert_error(&outcome, text);
        assert_eq!(outcome.error_kind, Some(kind), "{name}");
        let summary = outcome.error_summary.as_deref().unwrap();
        assert!(summary.contains(text), "{name}: {summary}");
        assert!(
            summary.ends_with(&format!("(resource {resource} in {EAST})")),
            "{name}: {summary}"
        );
        // The summary is what is left when the message of the engine is dropped.
        assert!(
            outcome.error.as_deref().unwrap().starts_with(summary),
            "{name}"
        );
    }
}

#[test]
fn the_message_of_the_engine_is_kept_apart_from_the_summary() {
    let outcome = one(eval("bad.fail_conflict", &bad_inventory()));
    assert_eq!(outcome.error_kind, Some(RuleErrorKind::Conflict));
    let error = outcome.error.as_deref().unwrap();
    let summary = outcome.error_summary.as_deref().unwrap();
    // The full description is the summary, then the engine's message.
    assert!(error.len() > summary.len(), "{error}");
    assert!(error.starts_with(&format!("{summary}: ")), "{error}");
    assert!(!summary.contains("multiple outputs"), "{summary}");
    assert!(!summary.contains("conflicts with"), "{summary}");
}

#[test]
fn error_descriptions_contain_no_values_from_the_rule() {
    // `fail_not_boolean` returns the string "yes", `message_not_string` returns 5.
    let outcome = one(eval("bad.fail_not_boolean", &bad_inventory()));
    assert!(!outcome.error.as_deref().unwrap().contains("yes"));
    let outcome = one(eval("bad.reason_bad_path", &bad_inventory()));
    assert!(!outcome.error.as_deref().unwrap().contains("no-slash"));
}

#[test]
fn message_and_reason_are_not_evaluated_for_resources_that_pass() {
    // Only `w-1` exists, and it is not named `bad`, so the broken `message` is never used.
    let mut inv = base_inventory();
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-1", EAST, Some("alice"))],
    );
    for name in ["bad.message_not_string", "bad.reason_not_object"] {
        let outcome = one(eval(name, &inv));
        assert_eq!(outcome.status, RuleStatus::Evaluated, "{name}");
        assert_eq!(summary(&outcome), [triple("w-1", EAST, ResultStatus::Pass)]);
        assert_eq!(outcome.error_kind, None);
        assert_eq!(outcome.error_summary, None);
    }
}

#[test]
fn the_first_error_stops_the_rule_and_hides_the_other_results() {
    // `w-1` passes, `w-bad` makes `message` fail: no result is kept.
    let outcome = one(eval("bad.message_not_string", &bad_inventory()));
    assert_eq!(outcome.status, RuleStatus::Error);
    assert!(outcome.results.is_empty());
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
    let outcome = one(eval("widget_owner", &inv));
    assert_error(&outcome, "duplicate resource w-1 in us-east-1");
    assert_eq!(outcome.error_kind, Some(RuleErrorKind::Internal));
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
    assert_eq!(outcome.error_kind, Some(RuleErrorKind::Timeout));
    assert!(
        outcome
            .error_summary
            .as_deref()
            .unwrap()
            .contains("(resource w-1 in us-east-1)")
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );

    // Without a widget the rule is not evaluated at all.
    let outcome = one(evaluate(&[rule(&loaded, "slow")], &base_inventory()));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
}

/// A widget whose evaluation by `slow_total` takes longer when `spin` is larger.
fn spinning_widget(id: &str, spin: u64) -> Resource {
    resource(ACCOUNT, WIDGET, id, EAST, json!({"Spin": spin}))
}

fn inventory_of_spinning_widgets(count: usize, spin: u64) -> Inventory {
    let mut inv = base_inventory();
    let widgets = (0..count)
        .map(|i| spinning_widget(&format!("w-{i:04}"), spin))
        .collect();
    put(&mut inv, EAST, WIDGET, widgets);
    inv
}

#[test]
fn the_time_limit_covers_all_resources_of_a_rule_together() {
    // Find a `spin` for which one widget takes roughly 100 ms (with a long limit).
    let long = load(Duration::from_secs(60));
    let mut spin = 100;
    let per_widget = loop {
        let inv = inventory_of_spinning_widgets(1, spin);
        let started = Instant::now();
        let outcome = one(evaluate(&[rule(&long, "slow_total")], &inv));
        let elapsed = started.elapsed();
        assert_eq!(outcome.status, RuleStatus::Evaluated);
        if elapsed >= Duration::from_millis(100) {
            break elapsed;
        }
        spin *= 2;
        assert!(spin < 100_000_000, "the rule is too fast to calibrate");
    };

    // Each widget is far below the limit, but all of them together are well above it.
    let limit = Duration::from_secs(2);
    let count = usize::try_from(limit.as_millis() * 4 / per_widget.as_millis().max(1)).unwrap() + 4;
    assert!(per_widget < limit / 2, "{per_widget:?}");
    let short = load(limit);
    let inv = inventory_of_spinning_widgets(count, spin);
    let started = Instant::now();
    let outcome = one(evaluate(&[rule(&short, "slow_total")], &inv));
    assert_error(&outcome, "timed out");
    assert_eq!(outcome.error_kind, Some(RuleErrorKind::Timeout));
    assert!(
        started.elapsed() < limit * 5,
        "{:?} for {count} widgets",
        started.elapsed()
    );

    // The same widgets in a smaller number fit in the limit.
    let inv = inventory_of_spinning_widgets(2, spin);
    let outcome = one(evaluate(&[rule(&short, "slow_total")], &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(outcome.results.len(), 2);
}

#[test]
fn an_error_in_one_rule_does_not_affect_the_others() {
    let loaded = load(Duration::from_secs(30));
    let inv = bad_inventory();
    let outcomes = evaluate(
        &[
            rule(&loaded, "widget_owner"),
            rule(&loaded, "bad.fail_conflict"),
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
            ("eval_policies.bad.fail_conflict", RuleStatus::Error),
            ("eval_policies.widget_owner", RuleStatus::Evaluated),
        ]
    );
}

// ------------------------------------------------------------------ input and data

#[test]
fn input_is_the_resource_and_data_has_context_and_sorted_declared_types() {
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
    // `input` is the resource itself, and `data` is the same for every resource.
    let probe = |index: usize| outcome.results[index].actual.clone().unwrap();
    assert_eq!(probe(0)["input_id"], json!("w-a"));
    assert_eq!(probe(2)["input_id"], json!("w-b"));
    assert_eq!(
        probe(0)["input_keys"],
        json!(["account", "configuration", "id", "region", "tags", "type"])
    );
    let mut shared = probe(2);
    shared.as_object_mut().unwrap().remove("input_id");
    assert_eq!(
        shared,
        json!({
            "input_keys": ["account", "configuration", "id", "region", "tags", "type"],
            "account_id": "123456789012",
            "regions": [EAST, WEST],
            "context_keys": ["account", "regions"],
            "example_keys": ["gadget", "widget"],
            "service_keys": ["example"],
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
                RuleStatus::Evaluated
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

/// A `(level, rule, message)` record of one event of the `print` target.
type PrintEvent = (String, String, String);

thread_local! {
    /// The `print` events of this thread, recorded only while [`capture_prints`] runs.
    static CAPTURED: std::cell::RefCell<Option<Vec<PrintEvent>>> =
        const { std::cell::RefCell::new(None) };
}

/// A minimal `tracing` subscriber that records the events of the `print` target.
///
/// It is the global default of this test binary rather than a scoped one: tracing caches each
/// callsite's interest for the whole process, and while a single scoped subscriber is
/// registered, a callsite registered on another thread (every test runs `print` while loading
/// the policies) can be cached as `never`, so the event is lost. As the global default it is
/// installed by [`load`] before any policy runs, so every callsite is enabled on every thread.
struct PrintLog;

/// Installs [`PrintLog`] as the global default subscriber, once per process.
fn install_print_log() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| tracing::subscriber::set_global_default(PrintLog).unwrap());
}

/// Runs `f` and returns the `print` events it logged on this thread.
fn capture_prints<T>(f: impl FnOnce() -> T) -> (T, Vec<PrintEvent>) {
    CAPTURED.set(Some(Vec::new()));
    let value = f();
    (value, CAPTURED.take().unwrap())
}

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
            CAPTURED.with_borrow_mut(|captured| {
                if let Some(events) = captured {
                    events.push((level, collector.rule, collector.message));
                }
            });
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

    let (outcomes, events) = capture_prints(|| evaluate(&[rule(&loaded, "printing")], &inv));
    let outcome = one(outcomes);

    assert_eq!(outcome.status, RuleStatus::Evaluated);
    assert_eq!(summary(&outcome), [triple("w-1", EAST, ResultStatus::Fail)]);
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
fn evaluating_twice_gives_the_same_outcomes() {
    let loaded = load(Duration::from_secs(30));
    let mut inv = base_inventory();
    put(&mut inv, EAST, WIDGET, vec![widget("w-1", EAST, None)]);
    let rules = [rule(&loaded, "widget_owner")];
    assert_eq!(evaluate(&rules, &inv), evaluate(&rules, &inv));
}

#[test]
fn the_title_is_the_message_when_the_rule_has_no_message() {
    let mut inv = base_inventory();
    let mut flagged = widget("w-2", EAST, None);
    flagged.tags.insert("flag".to_owned(), "bad".to_owned());
    put(
        &mut inv,
        EAST,
        WIDGET,
        vec![widget("w-1", EAST, None), flagged],
    );
    let outcome = one(eval("title_only", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated);
    let fail = &outcome.results[1];
    assert_eq!(fail.status, ResultStatus::Fail);
    assert_eq!(fail.message.as_deref(), Some("Widget must not be flagged"));
    // Without `reason` there is no path, actual or expected.
    assert_eq!(
        (&fail.path, &fail.actual, &fail.expected),
        (&None, &None, &None)
    );
    assert_eq!(outcome.results[0].message, None);
}

#[test]
fn the_account_type_is_always_collected_and_is_in_data_and_as_the_target() {
    // The inventory declares no collection error and `account_flag` targets
    // `aws.account.account`: one result for the single account resource.
    let mut inv = base_inventory();
    // Even without a resource type entry the account type is treated as collected.
    inv.resource_types
        .retain(|t| t.resource_type != ACCOUNT_TYPE);
    let outcome = one(eval("account_flag", &inv));
    assert_eq!(outcome.status, RuleStatus::Evaluated, "{outcome:?}");
    assert_eq!(
        summary(&outcome),
        [triple(
            "aws.account.account:123456789012:global",
            "global",
            ResultStatus::Pass
        )]
    );
}
