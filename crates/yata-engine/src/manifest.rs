//! The policy manifest `yata-policies.json`.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;

use crate::error::DirectoryFailureKind;
use crate::files::{self, ReadError};
use crate::names::is_valid_name;
use crate::version::YataVersion;

/// File name of the manifest in the root of a policy directory.
pub(crate) const MANIFEST_FILE_NAME: &str = "yata-policies.json";

/// Policy format versions this `yata` supports.
pub const SUPPORTED_FORMAT_VERSIONS: &[u32] = &[1];

/// A validated manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub(crate) format_version: u32,
    pub(crate) name: String,
    pub(crate) version: String,
    /// `min_yata_version` exactly as written in the manifest.
    pub(crate) min_yata_version: String,
}

/// A directory-level failure found while reading the manifest.
pub(crate) type ManifestFailure = (DirectoryFailureKind, String);

fn fail(kind: DirectoryFailureKind, message: impl Into<String>) -> ManifestFailure {
    (kind, message.into())
}

/// Reads and validates the manifest in `root`.
pub(crate) fn load(
    root: &Path,
    yata_version: YataVersion,
    max_bytes: u64,
) -> Result<Manifest, ManifestFailure> {
    let path = root.join(MANIFEST_FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(fail(
                DirectoryFailureKind::ManifestMissing,
                format!("{MANIFEST_FILE_NAME} not found"),
            ));
        }
        Err(e) => {
            return Err(fail(
                DirectoryFailureKind::Unreadable,
                format!("cannot read {MANIFEST_FILE_NAME}: {e}"),
            ));
        }
    };
    if files::is_link(&metadata.file_type()) || !metadata.file_type().is_file() {
        return Err(fail(
            DirectoryFailureKind::ManifestMissing,
            format!("{MANIFEST_FILE_NAME} is not a regular file (links are not followed)"),
        ));
    }
    let bytes = match files::read_limited(&path, max_bytes) {
        Ok(b) => b,
        Err(ReadError::TooLarge) => {
            return Err(fail(
                DirectoryFailureKind::ManifestTooLarge,
                format!("{MANIFEST_FILE_NAME} is larger than {max_bytes} bytes"),
            ));
        }
        Err(ReadError::Io(e)) => {
            return Err(fail(
                DirectoryFailureKind::Unreadable,
                format!("cannot read {MANIFEST_FILE_NAME}: {e}"),
            ));
        }
        Err(ReadError::NotUtf8) => {
            return Err(fail(
                DirectoryFailureKind::ManifestInvalid,
                format!("{MANIFEST_FILE_NAME} is not valid UTF-8"),
            ));
        }
    };
    parse(&bytes, yata_version)
}

fn invalid(message: impl Into<String>) -> ManifestFailure {
    fail(DirectoryFailureKind::ManifestInvalid, message)
}

/// Validates manifest content.
fn parse(bytes: &[u8], yata_version: YataVersion) -> Result<Manifest, ManifestFailure> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| invalid(format!("{MANIFEST_FILE_NAME} is not valid JSON: {e}")))?;
    let Value::Object(object) = value else {
        return Err(invalid(format!(
            "{MANIFEST_FILE_NAME} must be a JSON object"
        )));
    };

    let get_string = |key: &str| -> Result<&str, ManifestFailure> {
        match object.get(key) {
            None => Err(invalid(format!("missing required key `{key}`"))),
            Some(Value::String(s)) => Ok(s.as_str()),
            Some(_) => Err(invalid(format!("`{key}` must be a string"))),
        }
    };

    let format_version = match object.get("format_version") {
        None => return Err(invalid("missing required key `format_version`")),
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| invalid("`format_version` must be a non-negative integer"))?,
        Some(_) => return Err(invalid("`format_version` must be a non-negative integer")),
    };
    let name = get_string("name")?;
    let version = get_string("version")?;
    let min_yata_version = get_string("min_yata_version")?;

    // Checks in the order of the design: required keys and types first, then values.
    let supported = u32::try_from(format_version)
        .ok()
        .filter(|v| SUPPORTED_FORMAT_VERSIONS.contains(v));
    let Some(format_version) = supported else {
        return Err(fail(
            DirectoryFailureKind::UnsupportedFormatVersion,
            format!(
                "format_version {format_version} is not supported (supported: {})",
                join_u32(SUPPORTED_FORMAT_VERSIONS)
            ),
        ));
    };
    if !is_valid_name(name) {
        return Err(invalid(format!(
            "`name` {name:?} must match [a-z][a-z0-9_]* and must not be a Rego keyword"
        )));
    }
    YataVersion::parse(version)
        .map_err(|_| invalid(format!("`version` {version:?} must be of the form x.y.z")))?;
    let min = YataVersion::parse(min_yata_version).map_err(|_| {
        invalid(format!(
            "`min_yata_version` {min_yata_version:?} must be of the form x.y.z"
        ))
    })?;
    if min > yata_version {
        return Err(fail(
            DirectoryFailureKind::YataTooOld,
            format!("policies require yata {min} or newer, but this is yata {yata_version}"),
        ));
    }

    Ok(Manifest {
        format_version,
        name: name.to_string(),
        version: version.to_string(),
        min_yata_version: min_yata_version.to_string(),
    })
}

fn join_u32(values: &[u32]) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(json: &str) -> Result<Manifest, DirectoryFailureKind> {
        parse(json.as_bytes(), YataVersion::new(0, 1, 0)).map_err(|(k, _)| k)
    }

    const OK: &str =
        r#"{"format_version":1,"name":"p","version":"1.2.3","min_yata_version":"0.1.0","x":1}"#;

    #[test]
    fn accepts_valid_manifest_and_ignores_unknown_keys() {
        let m = check(OK).unwrap();
        assert_eq!(m.name, "p");
        assert_eq!(m.format_version, 1);
        assert_eq!(m.min_yata_version, "0.1.0");
    }

    #[test]
    fn classifies_errors() {
        use DirectoryFailureKind::*;
        assert_eq!(check("{"), Err(ManifestInvalid));
        assert_eq!(check("[]"), Err(ManifestInvalid));
        assert_eq!(check(r#"{"format_version":1}"#), Err(ManifestInvalid));
        assert_eq!(
            check(
                r#"{"format_version":"1","name":"p","version":"1.0.0","min_yata_version":"0.1.0"}"#
            ),
            Err(ManifestInvalid)
        );
        assert_eq!(
            check(
                r#"{"format_version":1.5,"name":"p","version":"1.0.0","min_yata_version":"0.1.0"}"#
            ),
            Err(ManifestInvalid)
        );
        assert_eq!(
            check(
                r#"{"format_version":2,"name":"p","version":"1.0.0","min_yata_version":"0.1.0"}"#
            ),
            Err(UnsupportedFormatVersion)
        );
        assert_eq!(
            check(
                r#"{"format_version":1,"name":"in","version":"1.0.0","min_yata_version":"0.1.0"}"#
            ),
            Err(ManifestInvalid)
        );
        assert_eq!(
            check(r#"{"format_version":1,"name":"p","version":"1.0","min_yata_version":"0.1.0"}"#),
            Err(ManifestInvalid)
        );
        assert_eq!(
            check(
                r#"{"format_version":1,"name":"p","version":"1.0.0","min_yata_version":"0.2.0"}"#
            ),
            Err(YataTooOld)
        );
    }
}
