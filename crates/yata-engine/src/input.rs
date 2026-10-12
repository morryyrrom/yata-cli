//! Building the Rego `input` document from an [`Inventory`].

use serde_json::{Map, Value};
use yata_core::{AccountInfo, Inventory, Resource, RuleMetadata};

use crate::error::EvaluationError;

/// The resource types a rule declares: `target_type` first, then `related_types` without
/// duplicates.
pub(crate) fn declared_types(metadata: &RuleMetadata) -> Vec<&str> {
    let mut types: Vec<&str> = vec![metadata.target_type.as_str()];
    for related in &metadata.related_types {
        if !types.contains(&related.as_str()) {
            types.push(related.as_str());
        }
    }
    types
}

/// Builds the `input` of one rule for one account.
///
/// It has `account`, `regions` and, for every declared type, the account's resources in one array
/// sorted by `(id, region)`, nested by `<provider>.<service>.<resource>`. A type without
/// collected resources gets an empty array.
pub(crate) fn build_input(
    inventory: &Inventory,
    account: &AccountInfo,
    declared: &[&str],
) -> Result<Value, EvaluationError> {
    let mut root = Map::new();
    root.insert("account".to_owned(), to_json(account)?);
    root.insert("regions".to_owned(), to_json(&inventory.regions)?);
    for type_name in declared {
        let mut resources: Vec<&Resource> = inventory
            .resource_sets
            .iter()
            .filter(|s| s.account == account.account && s.resource_type == *type_name)
            .flat_map(|s| s.resources.iter())
            .collect();
        resources.sort_by(|a, b| (&a.id, &a.region).cmp(&(&b.id, &b.region)));
        let array = resources
            .into_iter()
            .map(to_json)
            .collect::<Result<Vec<_>, _>>()?;
        insert_nested(&mut root, type_name, Value::Array(array));
    }
    Ok(Value::Object(root))
}

/// The array of resources of `type_name` in `input`. Empty when the type is not in the input.
pub(crate) fn resources_of<'a>(input: &'a Value, type_name: &str) -> &'a [Value] {
    let mut current = input;
    for part in type_name.split('.') {
        match current.get(part) {
            Some(next) => current = next,
            None => return &[],
        }
    }
    current.as_array().map_or(&[], Vec::as_slice)
}

/// Puts `value` at the nested position of a dotted type name, e.g. `aws.s3.bucket`.
fn insert_nested(root: &mut Map<String, Value>, type_name: &str, value: Value) {
    let parts: Vec<&str> = type_name.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        return;
    };
    let mut current = root;
    for part in parents {
        let entry = current
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = match entry {
            Value::Object(map) => map,
            // Unreachable: the entry was made an object just above.
            _ => return,
        };
    }
    current.insert((*last).to_owned(), value);
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, EvaluationError> {
    serde_json::to_value(value).map_err(|e| EvaluationError::Engine {
        message: format!("cannot build the input: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nested_insert_and_lookup() {
        let mut root = Map::new();
        insert_nested(&mut root, "aws.s3.bucket", json!([1]));
        insert_nested(&mut root, "aws.s3.other", json!([2]));
        let input = Value::Object(root);
        assert_eq!(resources_of(&input, "aws.s3.bucket"), [json!(1)]);
        assert_eq!(resources_of(&input, "aws.s3.other"), [json!(2)]);
        assert!(resources_of(&input, "aws.s3.none").is_empty());
        assert!(resources_of(&input, "aws.iam.user").is_empty());
    }
}
