//! Loading and validation of the libraries under `lib/`.

use std::time::Duration;

use crate::engine_config::{RegoEngine, RegoValue};
use crate::error::DirectoryFailureKind;
use crate::files::{ReadError, read_text_limited};
use crate::walk::WalkedFile;

/// A library problem. Always a directory-level failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryFailure {
    pub(crate) kind: DirectoryFailureKind,
    pub(crate) path: Option<String>,
    pub(crate) message: String,
}

fn invalid(path: &str, message: impl Into<String>) -> LibraryFailure {
    LibraryFailure {
        kind: DirectoryFailureKind::LibraryInvalid,
        path: Some(path.to_string()),
        message: message.into(),
    }
}

/// Creates the base engine and loads every library into it.
///
/// `libraries` are in ascending order of their relative paths; they are read from their real paths. On success the returned engine
/// is cloned for each rule.
pub(crate) fn load_libraries(
    libraries: &[WalkedFile],
    policy_set: &str,
    max_rego_bytes: u64,
    policy_timeout: Duration,
) -> Result<RegoEngine, LibraryFailure> {
    let mut engine = RegoEngine::new(policy_timeout);
    engine.set_label(&format!("lib:{policy_set}"));
    let expected = format!("data.yata.lib.{policy_set}");

    let mut packages = Vec::new();
    for library in libraries {
        let rel = &library.rel_path;
        let source = match read_text_limited(&library.path, max_rego_bytes) {
            Ok(s) => s,
            Err(ReadError::TooLarge) => {
                return Err(invalid(
                    rel,
                    format!("library is larger than {max_rego_bytes} bytes"),
                ));
            }
            Err(ReadError::NotUtf8) => return Err(invalid(rel, "library is not valid UTF-8")),
            Err(ReadError::Io(e)) => {
                return Err(LibraryFailure {
                    kind: DirectoryFailureKind::Unreadable,
                    path: Some(rel.clone()),
                    message: format!("cannot read library: {e}"),
                });
            }
        };
        let package = engine
            .add_policy(rel, &source)
            .map_err(|f| invalid(rel, f.message(policy_timeout)))?;
        if package != expected && !package.starts_with(&format!("{expected}.")) {
            return Err(invalid(
                rel,
                format!("package must be yata.lib.{policy_set} or below, but is {package}"),
            ));
        }
        packages.push((rel, package));
    }

    // A library must not define `metadata` or `fail`. This finds rules put in `lib/` by mistake.
    for (rel, package) in &packages {
        for name in ["metadata", "fail"] {
            match engine.eval_rule(&format!("{package}.{name}")) {
                Ok(RegoValue::NoRule | RegoValue::Undefined) => {}
                Ok(_) => {
                    return Err(invalid(
                        rel,
                        format!("a library must not define `{name}` (package {package})"),
                    ));
                }
                Err(f) => return Err(invalid(rel, f.message(policy_timeout))),
            }
        }
    }

    if !packages.is_empty() {
        engine.eval_all().map_err(|f| LibraryFailure {
            kind: DirectoryFailureKind::LibraryInvalid,
            path: None,
            message: f.message(policy_timeout),
        })?;
    }
    Ok(engine)
}
