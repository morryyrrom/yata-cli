//! The `Resource` type: one collected cloud resource.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A single collected resource with its common fields and a free-form configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Identifier unique within a `ResourceSet` (an ARN when one exists).
    pub id: String,
    /// Resource type, e.g. `aws.s3.bucket`.
    #[serde(rename = "type")]
    pub resource_type: String,
    /// Account key, e.g. `aws-123456789012`.
    pub account: String,
    /// Region of the resource, or `global`.
    pub region: String,
    /// Human-readable name, when the resource has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Resource tags. Always serialized, even when empty.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Type-specific configuration, keyed by API operation name.
    pub configuration: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_account_resource_shape() {
        let mut configuration = Map::new();
        configuration.insert(
            "GetCallerIdentity".to_owned(),
            json!({ "Account": "123456789012" }),
        );
        let resource = Resource {
            id: "aws.account.account:123456789012:global".to_owned(),
            resource_type: "aws.account.account".to_owned(),
            account: "aws-123456789012".to_owned(),
            region: "global".to_owned(),
            name: None,
            tags: BTreeMap::new(),
            configuration,
        };
        let value = serde_json::to_value(&resource).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "aws.account.account:123456789012:global",
                "type": "aws.account.account",
                "account": "aws-123456789012",
                "region": "global",
                "tags": {},
                "configuration": { "GetCallerIdentity": { "Account": "123456789012" } }
            })
        );
        let back: Resource = serde_json::from_value(value).unwrap();
        assert_eq!(back, resource);
    }

    #[test]
    fn serializes_name_when_present() {
        let resource = Resource {
            id: "id".to_owned(),
            resource_type: "aws.example.widget".to_owned(),
            account: "aws-123456789012".to_owned(),
            region: "us-east-1".to_owned(),
            name: Some("example".to_owned()),
            tags: BTreeMap::from([("k".to_owned(), "v".to_owned())]),
            configuration: Map::new(),
        };
        let value = serde_json::to_value(&resource).unwrap();
        assert_eq!(value["name"], json!("example"));
        assert_eq!(value["tags"], json!({ "k": "v" }));
    }
}
