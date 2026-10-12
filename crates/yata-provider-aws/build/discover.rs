//! Collector discovery and code generation.
//!
//! This file uses only the standard library. It is included by `build.rs` and by
//! `tests/discover.rs` through `#[path]`, so the rules are tested without running Cargo.
//!
//! Collectors live in `src/collectors/<service>/<resource>.rs` and are the only place the
//! layout is derived from; no list of collectors is kept anywhere else.

use std::fs;
use std::path::{Path, PathBuf};

/// Rego reserved words that must not be used as a service or resource name.
const RESERVED_WORDS: &[&str] = &[
    "as", "contains", "default", "else", "every", "false", "if", "import", "in", "not", "null",
    "package", "some", "true", "with",
];

/// One discovered collector file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Service name (the directory name), e.g. `s3`.
    pub service: String,
    /// Resource name (the file stem), e.g. `bucket`.
    pub resource: String,
    /// Path of the collector file.
    pub path: PathBuf,
}

impl Entry {
    /// Returns the resource type derived from the path, e.g. `aws.s3.bucket`.
    pub fn resource_type(&self) -> String {
        format!("aws.{}.{}", self.service, self.resource)
    }

    /// Returns the module name used in the generated code, e.g. `s3_bucket`.
    pub fn module_name(&self) -> String {
        format!("{}_{}", self.service, self.resource)
    }
}

/// Returns true if `name` matches the service rule `[a-z][a-z0-9]*` and is not reserved.
pub fn is_valid_service_name(name: &str) -> bool {
    let mut chars = name.chars();
    let starts_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    starts_ok
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !RESERVED_WORDS.contains(&name)
}

/// Returns true if `name` matches the resource rule `[a-z][a-z0-9_]*` and is not reserved.
pub fn is_valid_resource_name(name: &str) -> bool {
    let mut chars = name.chars();
    let starts_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    starts_ok
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !RESERVED_WORDS.contains(&name)
}

fn name_of(path: &Path) -> Result<&str, String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: the name is not valid UTF-8", path.display()))
}

fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut children = Vec::new();
    let read = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for child in read {
        let child = child.map_err(|e| format!("{}: {e}", dir.display()))?;
        children.push(child.path());
    }
    children.sort();
    Ok(children)
}

fn kind_of(path: &Path) -> Result<fs::FileType, String> {
    fs::symlink_metadata(path)
        .map(|m| m.file_type())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn is_rs(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs")
}

/// Scans `collectors_dir` and returns the discovered collectors in path order.
///
/// A missing directory yields an empty list. Files that are not `.rs` are ignored
/// wherever they are. Everything else that breaks the layout rules is an error.
pub fn discover(collectors_dir: &Path) -> Result<Vec<Entry>, String> {
    if !collectors_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for service_path in sorted_children(collectors_dir)? {
        let kind = kind_of(&service_path)?;
        if kind.is_symlink() {
            return Err(format!(
                "{}: symbolic links are not allowed in the collectors directory",
                service_path.display()
            ));
        }
        if kind.is_file() {
            if is_rs(&service_path) {
                return Err(format!(
                    "{}: a .rs file directly under the collectors directory is not allowed; \
                     use <service>/<resource>.rs",
                    service_path.display()
                ));
            }
            continue;
        }
        let service = name_of(&service_path)?.to_owned();
        if !is_valid_service_name(&service) {
            return Err(format!(
                "{}: invalid service directory name '{service}' (expected [a-z][a-z0-9]* and not a Rego reserved word)",
                service_path.display()
            ));
        }
        for file_path in sorted_children(&service_path)? {
            let kind = kind_of(&file_path)?;
            if kind.is_symlink() {
                return Err(format!(
                    "{}: symbolic links are not allowed in the collectors directory",
                    file_path.display()
                ));
            }
            if kind.is_dir() {
                return Err(format!(
                    "{}: nested directories are not allowed under <service>/",
                    file_path.display()
                ));
            }
            if !is_rs(&file_path) {
                continue;
            }
            let file_name = name_of(&file_path)?;
            if file_name == "mod.rs" {
                return Err(format!(
                    "{}: mod.rs is not allowed; use <resource>.rs",
                    file_path.display()
                ));
            }
            let resource = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("{}: the name is not valid UTF-8", file_path.display()))?
                .to_owned();
            if !is_valid_resource_name(&resource) {
                return Err(format!(
                    "{}: invalid resource file name '{resource}' (expected [a-z][a-z0-9_]* and not a Rego reserved word)",
                    file_path.display()
                ));
            }
            entries.push(Entry {
                service: service.clone(),
                resource,
                path: file_path,
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Generates the Rust source included by `src/collectors.rs`.
pub fn generate(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("// @generated by build.rs. Do not edit.\n\n");
    for entry in entries {
        let path = entry.path.to_string_lossy();
        out.push_str(&format!(
            "#[path = {path:?}]\nmod {};\n",
            entry.module_name()
        ));
    }
    out.push_str(
        "\n/// Returns every discovered collector.\npub(crate) fn all_collectors() -> Vec<Box<dyn crate::collector::Collector>> {\n",
    );
    if entries.is_empty() {
        out.push_str("    Vec::new()\n");
    } else {
        out.push_str("    vec![\n");
        for entry in entries {
            out.push_str(&format!("        {}::collector(),\n", entry.module_name()));
        }
        out.push_str("    ]\n");
    }
    out.push_str("}\n\n");
    out.push_str(
        "/// Resource types derived from the collector file paths, in the order of `all_collectors()`.\n#[allow(dead_code)]\npub(crate) const COLLECTOR_PATH_TYPES: &[&str] = &[\n",
    );
    for entry in entries {
        out.push_str(&format!("    {:?},\n", entry.resource_type()));
    }
    out.push_str("];\n");
    out
}
