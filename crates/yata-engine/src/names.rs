//! Naming rules for policy sets, directories, rules and resource types.

/// Rego keywords that cannot be used as a name.
const RESERVED_WORDS: &[&str] = &[
    "as", "contains", "default", "else", "every", "false", "if", "import", "in", "not", "null",
    "package", "some", "true", "with",
];

/// Returns whether `name` follows the name rules: `[a-z][a-z0-9_]*` and not a Rego keyword.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !RESERVED_WORDS.contains(&name)
}

/// Splits a resource type of the form `<provider>.<service>.<resource>` into its three parts.
///
/// Returns `None` unless there are exactly three parts and each one follows the name rules.
pub fn parse_resource_type(resource_type: &str) -> Option<(&str, &str, &str)> {
    let mut parts = resource_type.split('.');
    let provider = parts.next()?;
    let service = parts.next()?;
    let resource = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    [provider, service, resource]
        .iter()
        .all(|p| is_valid_name(p))
        .then_some((provider, service, resource))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for n in [
            "a",
            "s3",
            "bucket_public_access_block",
            "a_1",
            "test_policies",
        ] {
            assert!(is_valid_name(n), "{n}");
        }
    }

    #[test]
    fn invalid_names() {
        for n in [
            "", "1digit", "_x", "Upper", "bad-dir", "a.b", "a b", "é", "x\n",
        ] {
            assert!(!is_valid_name(n), "{n:?}");
        }
    }

    #[test]
    fn reserved_words_are_rejected() {
        for n in RESERVED_WORDS {
            assert!(!is_valid_name(n), "{n}");
        }
        assert!(is_valid_name("as_"));
        assert!(is_valid_name("input"));
    }

    #[test]
    fn resource_types() {
        assert_eq!(
            parse_resource_type("aws.s3.bucket"),
            Some(("aws", "s3", "bucket"))
        );
        assert_eq!(parse_resource_type("aws.s3"), None);
        assert_eq!(parse_resource_type("aws.s3.bucket.x"), None);
        assert_eq!(parse_resource_type("aws.s3.1bucket"), None);
        assert_eq!(parse_resource_type("aws.s3.in"), None);
        assert_eq!(parse_resource_type("aws..bucket"), None);
    }
}
