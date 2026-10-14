//! Evaluation of rules against an [`Inventory`]: rule status and results.

use serde_json::Value;
use yata_core::{
    AccountInfo, IncompleteResource, Inventory, RuleOutcome, RuleResult, RuleStatus, Scope,
    SkipReason,
};

use crate::error::EvaluationError;
use crate::input::{build_data, declared_types, resources_of};
use crate::per_resource::evaluate_resources;
use crate::rule::Rule;

/// Evaluates `rules` against every account in `inventory`.
///
/// Returns one outcome per rule x account, ordered by rule ID then account key. Rules are
/// evaluated one at a time on a clone of their engine, so `rules` is never modified.
pub fn evaluate(rules: &[&Rule], inventory: &Inventory) -> Vec<RuleOutcome> {
    let mut rules: Vec<&Rule> = rules.to_vec();
    rules.sort_by(|a, b| a.id().cmp(b.id()));
    let mut accounts: Vec<&AccountInfo> = inventory.accounts.iter().collect();
    accounts.sort_by(|a, b| a.account.cmp(&b.account));

    let mut outcomes = Vec::with_capacity(rules.len() * accounts.len());
    for rule in rules {
        for account in &accounts {
            outcomes.push(evaluate_rule(rule, inventory, account));
        }
    }
    outcomes
}

/// How many (account, region) groups of one resource type are collected, failed or missing.
#[derive(Debug, Default, Clone, Copy)]
struct Coverage {
    collected: usize,
    failed: usize,
    not_collected: usize,
}

impl Coverage {
    fn total(self) -> usize {
        self.collected + self.failed + self.not_collected
    }
}

/// Classifies the target groups of one type for one account.
fn coverage(inventory: &Inventory, account: &str, type_name: &str) -> Coverage {
    let mut coverage = Coverage::default();
    // The account type is made from the caller identity, not collected: it is always there.
    if type_name == "aws.account.account" {
        coverage.collected = 1;
        return coverage;
    }
    let Some(info) = inventory
        .resource_types
        .iter()
        .find(|t| t.resource_type == type_name)
    else {
        // The type was not targeted at all: every group is missing.
        coverage.not_collected = 1;
        return coverage;
    };
    let global = [String::from("global")];
    let regions: &[String] = match info.scope {
        Scope::Global => &global,
        Scope::Regional => &inventory.regions,
    };
    for region in regions {
        let is_group = |a: &str, r: &str, t: &str| a == account && r == region && t == type_name;
        if inventory
            .resource_sets
            .iter()
            .any(|s| is_group(&s.account, &s.region, &s.resource_type))
        {
            coverage.collected += 1;
        } else if inventory
            .collection_errors
            .iter()
            .any(|e| is_group(&e.account, &e.region, &e.resource_type))
        {
            coverage.failed += 1;
        } else {
            coverage.not_collected += 1;
        }
    }
    coverage
}

fn evaluate_rule(rule: &Rule, inventory: &Inventory, account: &AccountInfo) -> RuleOutcome {
    let metadata = rule.metadata();
    let declared = declared_types(metadata);
    let coverages: Vec<Coverage> = declared
        .iter()
        .map(|t| coverage(inventory, &account.account, t))
        .collect();

    let outcome = |status: RuleStatus| RuleOutcome {
        rule_id: rule.id().to_owned(),
        metadata: metadata.clone(),
        account: account.account.clone(),
        status,
        skip_reason: None,
        error: None,
        error_kind: None,
        error_summary: None,
        results: Vec::new(),
        incomplete_resources: Vec::new(),
    };

    // Step 1: any group that was never collected.
    if coverages.iter().any(|c| c.not_collected > 0) {
        return RuleOutcome {
            skip_reason: Some(SkipReason::NotCollected),
            ..outcome(RuleStatus::Skipped)
        };
    }
    // Step 2: every group of some type failed.
    if coverages
        .iter()
        .any(|c| c.total() > 0 && c.failed == c.total())
    {
        return RuleOutcome {
            skip_reason: Some(SkipReason::CollectionFailed),
            ..outcome(RuleStatus::Skipped)
        };
    }

    // Step 3: evaluate.
    let (results, incomplete_resources) = match run(rule, inventory, account, &declared) {
        Ok(v) => v,
        Err(e) => {
            return RuleOutcome {
                error: Some(one_line(&e.to_string())),
                error_kind: Some(e.kind()),
                error_summary: Some(one_line(&e.summary())),
                ..outcome(RuleStatus::Error)
            };
        }
    };

    // Steps 4 and 5: partial failures and failure markers.
    let partial = coverages.iter().any(|c| c.failed > 0);
    let status = if partial || !incomplete_resources.is_empty() {
        RuleStatus::Incomplete
    } else {
        RuleStatus::Evaluated
    };
    RuleOutcome {
        results,
        incomplete_resources,
        ..outcome(status)
    }
}

/// Joins the lines of a message with single spaces, so the description is one line.
fn one_line(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Evaluates one rule for one account. Returns the results and the incomplete resources.
fn run(
    rule: &Rule,
    inventory: &Inventory,
    account: &AccountInfo,
    declared: &[&str],
) -> Result<(Vec<RuleResult>, Vec<IncompleteResource>), EvaluationError> {
    let data = build_data(inventory, account, declared)?;
    let results = evaluate_resources(rule, &data)?;
    Ok((results, incomplete_resources(&data, declared)))
}

/// Finds resources whose configuration has an operation value that is a failure marker: an
/// object whose only key is `yata_error`. Sorted by (type, id, region, operation).
fn incomplete_resources(data: &Value, declared: &[&str]) -> Vec<IncompleteResource> {
    let mut found = Vec::new();
    for type_name in declared {
        for resource in resources_of(data, type_name) {
            let (Some(id), Some(region)) = (
                resource.get("id").and_then(Value::as_str),
                resource.get("region").and_then(Value::as_str),
            ) else {
                continue;
            };
            let Some(configuration) = resource.get("configuration").and_then(Value::as_object)
            else {
                continue;
            };
            for (operation, value) in configuration {
                let Some(marker) = value
                    .as_object()
                    .filter(|o| o.len() == 1)
                    .and_then(|o| o.get("yata_error"))
                else {
                    continue;
                };
                let kind = marker
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                found.push(IncompleteResource {
                    resource_id: id.to_owned(),
                    resource_type: (*type_name).to_owned(),
                    region: region.to_owned(),
                    operation: operation.clone(),
                    kind: kind.to_owned(),
                });
            }
        }
    }
    found.sort_by(|a, b| {
        (&a.resource_type, &a.resource_id, &a.region, &a.operation).cmp(&(
            &b.resource_type,
            &b.resource_id,
            &b.region,
            &b.operation,
        ))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn one_line_joins_lines_with_single_spaces() {
        assert_eq!(one_line("a\n  b\r\nc"), "a b c");
        assert_eq!(one_line("single"), "single");
    }
}
