//! Scope of a resource type.

use serde::{Deserialize, Serialize};

/// Whether a resource type exists once per account or once per region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The resource type is collected once per account (region `global`).
    Global,
    /// The resource type is collected once per region.
    Regional,
}

impl Scope {
    /// Returns the serialized name of the scope (`global` or `regional`).
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Regional => "regional",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_lowercase() {
        assert_eq!(serde_json::to_string(&Scope::Global).unwrap(), "\"global\"");
        assert_eq!(
            serde_json::to_string(&Scope::Regional).unwrap(),
            "\"regional\""
        );
        let back: Scope = serde_json::from_str("\"regional\"").unwrap();
        assert_eq!(back, Scope::Regional);
        assert_eq!(Scope::Regional.as_str(), "regional");
    }
}
