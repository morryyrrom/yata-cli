//! Directory walk: finds libraries, rule files and fixture directories under a policy root.
//!
//! The walk is depth-first, visits the entries of each directory in byte order of their names
//! and never follows links, so it reports the same failures in the same order for the same
//! input. It enforces the resource limits (file count, directory count, depth).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{DirectoryFailureKind, RuleFailureKind};
use crate::files::is_link;
use crate::limits::ResourceLimits;
use crate::manifest::MANIFEST_FILE_NAME;
use crate::names::is_valid_name;

const REGO_EXTENSION: &str = ".rego";
const FIXTURES_SUFFIX: &str = ".fixtures";
const LIBRARY_DIR: &str = "lib";

/// A directory-level failure found by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkDirFailure {
    pub(crate) kind: DirectoryFailureKind,
    pub(crate) path: Option<String>,
    pub(crate) message: String,
}

/// A rule-level failure found by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkRuleFailure {
    pub(crate) kind: RuleFailureKind,
    pub(crate) rule_id: Option<String>,
    pub(crate) path: String,
    pub(crate) message: String,
}

/// A file found by the walk: its display path and its real path on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkedFile {
    /// Path relative to the policy directory, `/`-separated, lossy. For display only.
    pub(crate) rel_path: String,
    /// Real path, built from the names as found on disk. Use this to read the file.
    pub(crate) path: PathBuf,
}

/// Any rule file found outside `lib/`, including the ones with invalid names and links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkedRuleFile {
    /// Path relative to the policy directory, `/`-separated.
    pub(crate) rel_path: String,
    /// Rule ID, when the names are valid.
    pub(crate) id: Option<String>,
}

/// A rule file with valid names, found outside `lib/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WalkedRule {
    /// Rule ID: manifest name, directory names and file name joined by `.`.
    pub(crate) id: String,
    /// Path relative to the policy directory, `/`-separated.
    pub(crate) rel_path: String,
    /// Real path on disk. Use this to read the file.
    pub(crate) path: PathBuf,
}

/// What a fixture directory contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FixtureEntryKind {
    /// A regular file.
    File,
    /// A directory (not entered).
    Directory,
    /// Something else (a socket, a device, ...).
    Other,
}

/// An entry of a fixture directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixtureEntry {
    /// File name (lossy for names that are not UTF-8).
    pub name: String,
    /// File name as found on disk. Use this to read the file.
    pub os_name: OsString,
    /// Kind of the entry. Links are never listed; they are reported as failures.
    pub kind: FixtureEntryKind,
}

/// A `<rule>.fixtures` directory found next to a rule file (`LoadMode::RulesWithFixtures` only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixtureDir {
    /// Path of the directory relative to the policy directory, `/`-separated.
    pub path: String,
    /// Real path of the directory on disk. Use this to read the fixtures.
    pub os_path: PathBuf,
    /// Path of the rule file this directory belongs to, relative to the policy directory.
    pub rule_path: String,
    /// Entries in byte order of their names. Directories whose name starts with `.` are skipped.
    pub entries: Vec<FixtureEntry>,
}

/// The result of walking one policy directory.
#[derive(Debug, Default)]
pub(crate) struct WalkOutput {
    /// Library files, relative paths in ascending order.
    pub(crate) libraries: Vec<WalkedFile>,
    /// Rules with valid names, in ascending rule ID order.
    pub(crate) rules: Vec<WalkedRule>,
    /// Number of rule files found outside `lib/`, including the ones that failed.
    pub(crate) rule_file_count: usize,
    /// Every rule file found outside `lib/`, in ascending path order.
    pub(crate) rule_files: Vec<WalkedRuleFile>,
    /// Fixture directories that belong to a rule file.
    pub(crate) fixture_dirs: Vec<FixtureDir>,
    /// Rule-level failures, in walk order.
    pub(crate) rule_failures: Vec<WalkRuleFailure>,
    /// The directory-level failure that stopped the walk, if any.
    pub(crate) failure: Option<WalkDirFailure>,
}

/// Kind of a directory entry, without following links.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Link,
    Other,
}

struct Entry {
    name: OsString,
    kind: Kind,
}

impl Entry {
    fn name_str(&self) -> Option<&str> {
        self.name.to_str()
    }

    fn starts_with_dot(&self) -> bool {
        self.name.as_encoded_bytes().first() == Some(&b'.')
    }

    fn display_name(&self) -> String {
        self.name.to_string_lossy().into_owned()
    }
}

/// Where in the tree a directory is.
#[derive(Clone)]
struct Place {
    /// Names of the directories from the root, lossy.
    components: Vec<String>,
    /// The same names as actually found on disk. Used only to access the file system, because
    /// two different names can have the same lossy form.
    os_components: Vec<OsString>,
    /// Rule ID parts (valid directory names), or the first offending directory name.
    invalid_component: Option<String>,
    in_lib: bool,
}

impl Place {
    fn depth(&self) -> usize {
        self.components.len()
    }

    fn rel(&self, name: &str) -> String {
        let mut parts = self.components.clone();
        parts.push(name.to_string());
        parts.join("/")
    }
}

struct Walker<'a> {
    root: &'a Path,
    manifest_name: &'a str,
    limits: &'a ResourceLimits,
    collect_fixtures: bool,
    files: usize,
    directories: usize,
    out: WalkOutput,
}

/// Walks a policy directory.
///
/// `collect_fixtures` is true for `LoadMode::RulesWithFixtures`: fixture directories are then
/// entered, checked for links and listed; otherwise they are counted but not entered.
pub(crate) fn walk(
    root: &Path,
    manifest_name: &str,
    limits: &ResourceLimits,
    collect_fixtures: bool,
) -> WalkOutput {
    let mut walker = Walker {
        root,
        manifest_name,
        limits,
        collect_fixtures,
        files: 0,
        directories: 0,
        out: WalkOutput::default(),
    };
    let place = Place {
        components: Vec::new(),
        os_components: Vec::new(),
        invalid_component: None,
        in_lib: false,
    };
    if let Err(failure) = walker.walk_dir(&place) {
        walker.out.failure = Some(failure);
    }
    let mut out = walker.out;
    out.libraries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out.rules.sort_by(|a, b| a.id.cmp(&b.id));
    out.rule_files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

type Stop = WalkDirFailure;

impl Walker<'_> {
    fn dir_path(&self, place: &Place) -> PathBuf {
        let mut p = self.root.to_path_buf();
        for c in &place.os_components {
            p.push(c);
        }
        p
    }

    /// Reads the entries of a directory in byte order of their names.
    fn read_entries(&self, path: &Path, rel: &str) -> Result<Vec<Entry>, Stop> {
        let unreadable = |what: String| Stop {
            kind: DirectoryFailureKind::Unreadable,
            path: Some(rel.to_string()),
            message: what,
        };
        let iter =
            fs::read_dir(path).map_err(|e| unreadable(format!("cannot read directory: {e}")))?;
        let mut entries = Vec::new();
        for item in iter {
            let item = item.map_err(|e| unreadable(format!("cannot read directory: {e}")))?;
            let metadata = fs::symlink_metadata(item.path()).map_err(|e| Stop {
                kind: DirectoryFailureKind::Unreadable,
                path: Some(join_rel(rel, &item.file_name().to_string_lossy())),
                message: format!("cannot read file type: {e}"),
            })?;
            let file_type = metadata.file_type();
            let kind = if is_link(&file_type) {
                Kind::Link
            } else if file_type.is_dir() {
                Kind::Dir
            } else if file_type.is_file() {
                Kind::File
            } else {
                Kind::Other
            };
            entries.push(Entry {
                name: item.file_name(),
                kind,
            });
        }
        entries.sort_by(|a, b| a.name.as_encoded_bytes().cmp(b.name.as_encoded_bytes()));
        Ok(entries)
    }

    fn count_file(&mut self, rel: &str) -> Result<(), Stop> {
        self.files += 1;
        if self.files > self.limits.max_files {
            return Err(Stop {
                kind: DirectoryFailureKind::TooManyFiles,
                path: Some(rel.to_string()),
                message: format!("more than {} files", self.limits.max_files),
            });
        }
        Ok(())
    }

    fn count_dir(&mut self, rel: &str) -> Result<(), Stop> {
        self.directories += 1;
        if self.directories > self.limits.max_directories {
            return Err(Stop {
                kind: DirectoryFailureKind::TooManyDirectories,
                path: Some(rel.to_string()),
                message: format!("more than {} directories", self.limits.max_directories),
            });
        }
        Ok(())
    }

    fn walk_dir(&mut self, place: &Place) -> Result<(), Stop> {
        let dir_rel = place.components.join("/");
        let entries = self.read_entries(&self.dir_path(place), &dir_rel)?;
        // Stems of the `.rego` files here (regular files and links), to pair fixture directories.
        let rule_stems: HashSet<&str> = entries
            .iter()
            .filter(|e| matches!(e.kind, Kind::File | Kind::Link))
            .filter_map(|e| e.name_str()?.strip_suffix(REGO_EXTENSION))
            .collect();
        let at_root = place.components.is_empty();

        for entry in &entries {
            let display = entry.display_name();
            let rel = place.rel(&display);
            match entry.kind {
                Kind::Link => {
                    self.count_file(&rel)?;
                    self.handle_link(place, entry, &rel, &rule_stems);
                    if self.link_is_fatal(place, entry) {
                        return Err(Stop {
                            kind: DirectoryFailureKind::Symlink,
                            path: Some(rel),
                            message: "symbolic links and reparse points are not allowed here"
                                .to_string(),
                        });
                    }
                }
                Kind::Dir => self.handle_dir(place, entry, &rel, &rule_stems, at_root)?,
                Kind::File | Kind::Other => {
                    if at_root && entry.name_str() == Some(MANIFEST_FILE_NAME) {
                        // Handled by the manifest step.
                        continue;
                    }
                    self.count_file(&rel)?;
                    if entry.kind == Kind::File {
                        let real = self.dir_path(place).join(&entry.name);
                        self.handle_file(place, entry, &rel, real);
                    }
                }
            }
        }
        Ok(())
    }

    /// Records the rule-level failure for a link, if its place gives one.
    fn handle_link(&mut self, place: &Place, entry: &Entry, rel: &str, rule_stems: &HashSet<&str>) {
        if place.in_lib {
            return;
        }
        let Some(name) = entry.name_str() else {
            return;
        };
        if let Some(stem) = name.strip_suffix(REGO_EXTENSION) {
            self.out.rule_file_count += 1;
            let id = self.rule_id(place, stem);
            self.out.rule_files.push(WalkedRuleFile {
                rel_path: rel.to_string(),
                id: id.clone(),
            });
            self.out.rule_failures.push(WalkRuleFailure {
                kind: RuleFailureKind::Symlink,
                rule_id: id,
                path: rel.to_string(),
                message: "rule file is a symbolic link or reparse point".to_string(),
            });
        } else if let Some(stem) = name.strip_suffix(FIXTURES_SUFFIX) {
            if !self.collect_fixtures {
                return;
            }
            if rule_stems.contains(stem) {
                self.out.rule_failures.push(WalkRuleFailure {
                    kind: RuleFailureKind::Symlink,
                    rule_id: self.rule_id(place, stem),
                    path: rel.to_string(),
                    message: "fixture directory is a symbolic link or reparse point".to_string(),
                });
            } else {
                self.push_orphan(place, stem, rel);
            }
        }
    }

    /// Whether a link is a directory-level failure (everything except rule files and fixtures).
    fn link_is_fatal(&self, place: &Place, entry: &Entry) -> bool {
        if place.in_lib {
            return true;
        }
        match entry.name_str() {
            Some(name) => !(name.ends_with(REGO_EXTENSION) || name.ends_with(FIXTURES_SUFFIX)),
            None => true,
        }
    }

    fn rule_id(&self, place: &Place, stem: &str) -> Option<String> {
        if place.invalid_component.is_some() || !is_valid_name(stem) {
            return None;
        }
        let mut parts = vec![self.manifest_name.to_string()];
        parts.extend(place.components.iter().cloned());
        parts.push(stem.to_string());
        Some(parts.join("."))
    }

    fn push_orphan(&mut self, place: &Place, stem: &str, rel: &str) {
        let rule_rel = place.rel(&format!("{stem}{REGO_EXTENSION}"));
        self.out.rule_failures.push(WalkRuleFailure {
            kind: RuleFailureKind::OrphanFixtures,
            rule_id: None,
            path: rel.to_string(),
            message: format!("no rule file {rule_rel} for this fixture directory"),
        });
    }

    fn handle_file(&mut self, place: &Place, entry: &Entry, rel: &str, real: PathBuf) {
        let Some(name) = entry.name_str() else {
            // Cannot tell whether it is a `.rego` file.
            return;
        };
        let Some(stem) = name.strip_suffix(REGO_EXTENSION) else {
            return;
        };
        if place.in_lib {
            self.out.libraries.push(WalkedFile {
                rel_path: rel.to_string(),
                path: real,
            });
            return;
        }
        self.out.rule_file_count += 1;
        self.out.rule_files.push(WalkedRuleFile {
            rel_path: rel.to_string(),
            id: self.rule_id(place, stem),
        });
        if let Some(bad) = &place.invalid_component {
            self.out.rule_failures.push(WalkRuleFailure {
                kind: RuleFailureKind::InvalidName,
                rule_id: None,
                path: rel.to_string(),
                message: format!(
                    "directory name {bad:?} does not match [a-z][a-z0-9_]* or is a Rego keyword"
                ),
            });
        } else if let Some(id) = self.rule_id(place, stem) {
            self.out.rules.push(WalkedRule {
                id,
                rel_path: rel.to_string(),
                path: real,
            });
        } else {
            self.out.rule_failures.push(WalkRuleFailure {
                kind: RuleFailureKind::InvalidName,
                rule_id: None,
                path: rel.to_string(),
                message: format!(
                    "rule name {stem:?} does not match [a-z][a-z0-9_]* or is a Rego keyword"
                ),
            });
        }
    }

    fn handle_dir(
        &mut self,
        place: &Place,
        entry: &Entry,
        rel: &str,
        rule_stems: &HashSet<&str>,
        at_root: bool,
    ) -> Result<(), Stop> {
        if entry.starts_with_dot() {
            return Ok(());
        }
        let name = entry.name_str();
        let fixtures_stem = if place.in_lib {
            None
        } else {
            name.and_then(|n| n.strip_suffix(FIXTURES_SUFFIX))
        };
        self.count_dir(rel)?;
        if fixtures_stem.is_some() && !self.collect_fixtures {
            return Ok(());
        }
        let depth = place.depth() + 1;
        if depth >= self.limits.max_depth {
            let path = self.dir_path(place).join(&entry.name);
            let has_entries = !self.read_entries(&path, rel)?.is_empty();
            if has_entries {
                return Err(Stop {
                    kind: DirectoryFailureKind::DepthExceeded,
                    path: Some(rel.to_string()),
                    message: format!(
                        "directory is deeper than the limit of {} levels and is not empty",
                        self.limits.max_depth
                    ),
                });
            }
            return Ok(());
        }

        let mut child = Place {
            components: place.components.clone(),
            os_components: place.os_components.clone(),
            invalid_component: place.invalid_component.clone(),
            in_lib: place.in_lib,
        };
        child.components.push(entry.display_name());
        child.os_components.push(entry.name.clone());

        if let Some(stem) = fixtures_stem {
            return self.walk_fixture_dir(place, &child, stem, rule_stems.contains(stem));
        }
        if at_root && name == Some(LIBRARY_DIR) {
            child.in_lib = true;
        } else if !child.in_lib
            && child.invalid_component.is_none()
            && !name.is_some_and(is_valid_name)
        {
            child.invalid_component = Some(entry.display_name());
        }
        self.walk_dir(&child)
    }

    fn walk_fixture_dir(
        &mut self,
        parent: &Place,
        place: &Place,
        stem: &str,
        rule_exists: bool,
    ) -> Result<(), Stop> {
        let rel = place.components.join("/");
        let entries = self.read_entries(&self.dir_path(place), &rel)?;
        let mut listed = Vec::new();
        for entry in &entries {
            let display = entry.display_name();
            let item_rel = place.rel(&display);
            match entry.kind {
                Kind::Link => {
                    self.count_file(&item_rel)?;
                    if rule_exists {
                        self.out.rule_failures.push(WalkRuleFailure {
                            kind: RuleFailureKind::Symlink,
                            rule_id: self.rule_id(parent, stem),
                            path: item_rel,
                            message: "fixture is a symbolic link or reparse point".to_string(),
                        });
                    }
                }
                Kind::Dir => {
                    if entry.starts_with_dot() {
                        continue;
                    }
                    self.count_dir(&item_rel)?;
                    if place.depth() + 1 >= self.limits.max_depth {
                        let path = self.dir_path(place).join(&entry.name);
                        if !self.read_entries(&path, &item_rel)?.is_empty() {
                            return Err(Stop {
                                kind: DirectoryFailureKind::DepthExceeded,
                                path: Some(item_rel),
                                message: format!(
                                    "directory is deeper than the limit of {} levels and is not empty",
                                    self.limits.max_depth
                                ),
                            });
                        }
                    }
                    listed.push(FixtureEntry {
                        name: display,
                        os_name: entry.name.clone(),
                        kind: FixtureEntryKind::Directory,
                    });
                }
                Kind::File | Kind::Other => {
                    self.count_file(&item_rel)?;
                    listed.push(FixtureEntry {
                        name: display,
                        os_name: entry.name.clone(),
                        kind: if entry.kind == Kind::File {
                            FixtureEntryKind::File
                        } else {
                            FixtureEntryKind::Other
                        },
                    });
                }
            }
        }
        if rule_exists {
            self.out.fixture_dirs.push(FixtureDir {
                path: rel,
                os_path: self.dir_path(place),
                rule_path: parent.rel(&format!("{stem}{REGO_EXTENSION}")),
                entries: listed,
            });
        } else {
            self.push_orphan(parent, stem, &rel);
        }
        Ok(())
    }
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use super::*;

    /// A real directory `lib/a\x80` and a link `lib/a\u{FFFD}` have the same lossy name. The walk
    /// must read the real directory by its real name, never the link target.
    #[test]
    fn colliding_lossy_names_are_read_by_their_real_names() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("evil.rego"), "package x\n").unwrap();
        let lib = dir.path().join("lib");
        fs::create_dir(&lib).unwrap();
        let real = lib.join(OsStr::from_bytes(b"a\x80"));
        fs::create_dir(&real).unwrap();
        fs::write(real.join("good.rego"), "package yata.lib.t\n").unwrap();
        // Byte order: "a" + 0x80 sorts before "a" + 0xEF 0xBF 0xBD (the UTF-8 form of U+FFFD), so the
        // real directory is visited first and the link second.
        if std::os::unix::fs::symlink(outside.path(), lib.join("a\u{FFFD}")).is_err() {
            return;
        }
        let out = walk(dir.path(), "t", &ResourceLimits::default(), false);
        // The real directory was entered by its real name and its library found...
        assert_eq!(out.libraries.len(), 1, "{:?}", out.libraries);
        assert!(out.libraries[0].rel_path.ends_with("good.rego"));
        assert_eq!(out.libraries[0].path, real.join("good.rego"));
        // ...and the link was reported, not followed.
        assert_eq!(
            out.failure.map(|f| f.kind),
            Some(DirectoryFailureKind::Symlink)
        );
    }

    /// A library under a directory whose name is not UTF-8 is read by its real path: loading
    /// succeeds and a rule can use the library package.
    #[test]
    fn library_under_a_non_utf8_directory_is_loaded_from_its_real_path() {
        use crate::{LoadMode, LoadOptions, YataVersion, load_policies};

        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("yata-policies.json"),
            r#"{"format_version": 1, "name": "t", "version": "0.1.0", "min_yata_version": "0.1.0"}"#,
        )
        .unwrap();
        let lib = dir.path().join("lib").join(OsStr::from_bytes(&[b'a', 128]));
        fs::create_dir_all(&lib).unwrap();
        fs::write(
            lib.join("h.rego"),
            "package yata.lib.t.h

import rego.v1

limit := 7
",
        )
        .unwrap();
        fs::write(
            dir.path().join("r.rego"),
            "package yata.rules.t.r

import data.yata.lib.t.h
import rego.v1

metadata := {\"title\": \"t\", \"severity\": \"low\", \"description\": \"d\", \"remediation\": \"r\", \"target_type\": \"aws.example.widget\", \"references\": [], \"tags\": [sprintf(\"%d\", [h.limit])]}

fail contains 1 if { false }
",
        )
        .unwrap();
        let options = LoadOptions {
            yata_version: YataVersion::new(0, 1, 0),
            limits: ResourceLimits::default(),
            policy_timeout: std::time::Duration::from_secs(5),
            mode: LoadMode::Rules,
        };
        let loaded = load_policies(&[dir.path().to_path_buf()], &options).unwrap();
        assert!(
            loaded.rule_failures.is_empty(),
            "{:?}",
            loaded.rule_failures
        );
        let rule = loaded.rules().next().unwrap();
        assert_eq!(rule.metadata().tags, ["7"]);
    }
}
