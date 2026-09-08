//! Contained, retry-convergent synchronization with a configured Codex vault.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::backend::MemoryBackend;
use crate::types::*;

const DEFAULT_SUBDIR: &str = "ai/memory";
const MAX_VAULT_FILES: usize = 10_000;
const MAX_VAULT_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_VAULT_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_REPORT_PATHS: usize = 256;
const MAX_REPORT_COUNT: usize = 10_000;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum VaultSyncError {
    #[error("invalid vault path: {0}")]
    InvalidPath(String),
    #[error("vault input changed or could not be read: {0}")]
    TransientRead(String),
    #[error("vault storage operation failed: {0}")]
    Storage(String),
    #[error("vault publication completed but directory durability failed for {path}: {error}")]
    PublishedButDirectorySyncFailed { path: PathBuf, error: String },
    #[error("memory operation failed: {0}")]
    Memory(#[from] crate::MemoryError),
    #[error("vault synchronization cancelled")]
    Cancelled,
    #[error("invalid vault input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, VaultSyncError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishOutcome {
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializeReport {
    pub sections_written: usize,
    pub facts_written: usize,
    pub files_changed_total: usize,
    pub files_truncated: bool,
    /// Paths are always relative to the validated vault root and bounded.
    pub files_written: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub facts_imported: usize,
    pub facts_skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReinforcementReport {
    pub facts_reinforced: usize,
    pub references_dangling: usize,
    pub references_superseded_total: usize,
    pub references_superseded_truncated: bool,
    pub references_superseded: Vec<SupersededReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersededReference {
    pub note_path: PathBuf,
    pub old_fact_id: String,
    pub new_fact_id: String,
}

#[derive(Debug)]
struct Snapshot {
    relative_path: PathBuf,
    content: String,
    content_hash: String,
}

fn check_cancel(cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        Err(VaultSyncError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn validate_vault_root(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| VaultSyncError::InvalidPath(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VaultSyncError::InvalidPath(
            "vault root must be an existing non-symlink directory".into(),
        ));
    }
    fs::canonicalize(path).map_err(|error| VaultSyncError::InvalidPath(error.to_string()))
}

fn validate_subdir(subdir: &str) -> Result<PathBuf> {
    let path = Path::new(subdir);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VaultSyncError::InvalidPath(
            "vault subdirectory must contain relative normal components only".into(),
        ));
    }
    Ok(path.to_path_buf())
}

fn validate_existing_path(root: &Path, relative: &Path, directory: bool) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(VaultSyncError::InvalidPath(
                "non-normal path component".into(),
            ));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(VaultSyncError::InvalidPath(format!(
                    "symlink traversal rejected: {}",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(VaultSyncError::TransientRead(error.to_string())),
        }
    }
    let path = root.join(relative);
    if path.exists() {
        let canonical = fs::canonicalize(&path)
            .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        if !canonical.starts_with(root) {
            return Err(VaultSyncError::InvalidPath(
                "path escapes vault root".into(),
            ));
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        if directory != metadata.is_dir() {
            return Err(VaultSyncError::InvalidPath(format!(
                "unexpected vault path type: {}",
                relative.display()
            )));
        }
    }
    Ok(())
}

fn snapshot_markdown(
    root: &Path,
    relative_dir: Option<&Path>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Snapshot>> {
    snapshot_markdown_with_limit(root, relative_dir, cancelled, MAX_VAULT_SNAPSHOT_BYTES)
}

fn snapshot_markdown_with_limit(
    root: &Path,
    relative_dir: Option<&Path>,
    cancelled: &dyn Fn() -> bool,
    aggregate_limit: u64,
) -> Result<Vec<Snapshot>> {
    check_cancel(cancelled)?;
    let start_relative = relative_dir.unwrap_or_else(|| Path::new(""));
    if !start_relative.as_os_str().is_empty() {
        validate_existing_path(root, start_relative, true)?;
    }
    let start = root.join(start_relative);
    if !start.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![start_relative.to_path_buf()];
    let mut paths = Vec::new();
    let mut metadata_bytes = 0_u64;
    while let Some(relative_dir) = pending.pop() {
        check_cancel(cancelled)?;
        let directory = root.join(&relative_dir);
        let entries = fs::read_dir(&directory)
            .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        for entry in entries {
            check_cancel(cancelled)?;
            let entry = entry.map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
            let name = entry.file_name();
            let relative = relative_dir.join(&name);
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
            if metadata.file_type().is_symlink() {
                return Err(VaultSyncError::InvalidPath(format!(
                    "symlink traversal rejected: {}",
                    relative.display()
                )));
            }
            if metadata.is_dir() {
                if !name.to_string_lossy().starts_with('.') {
                    pending.push(relative);
                }
            } else if metadata.is_file()
                && entry.path().extension().and_then(OsStr::to_str) == Some("md")
            {
                if metadata.len() > MAX_VAULT_FILE_BYTES {
                    return Err(VaultSyncError::TransientRead(format!(
                        "vault input exceeds {MAX_VAULT_FILE_BYTES} bytes: {}",
                        relative.display()
                    )));
                }
                metadata_bytes = metadata_bytes.checked_add(metadata.len()).ok_or_else(|| {
                    VaultSyncError::TransientRead("vault snapshot byte count overflowed".into())
                })?;
                if metadata_bytes > aggregate_limit {
                    return Err(VaultSyncError::TransientRead(format!(
                        "vault snapshot exceeds {aggregate_limit} aggregate bytes"
                    )));
                }
                paths.push(relative);
                if paths.len() > MAX_VAULT_FILES {
                    return Err(VaultSyncError::TransientRead(format!(
                        "vault input exceeds {MAX_VAULT_FILES} files"
                    )));
                }
            }
        }
    }
    paths.sort();
    let mut snapshots = Vec::with_capacity(paths.len());
    let mut actual_bytes = 0_u64;
    for relative_path in paths {
        check_cancel(cancelled)?;
        validate_existing_path(root, &relative_path, false)?;
        let bytes = fs::read(root.join(&relative_path))
            .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        if bytes.len() as u64 > MAX_VAULT_FILE_BYTES {
            return Err(VaultSyncError::TransientRead(format!(
                "vault input exceeds {MAX_VAULT_FILE_BYTES} bytes: {}",
                relative_path.display()
            )));
        }
        actual_bytes = actual_bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| {
                VaultSyncError::TransientRead("vault snapshot byte count overflowed".into())
            })?;
        if actual_bytes > aggregate_limit {
            return Err(VaultSyncError::TransientRead(format!(
                "vault snapshot exceeds {aggregate_limit} aggregate bytes"
            )));
        }
        let content = String::from_utf8(bytes)
            .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        let content_hash = hex::encode(Sha256::digest(content.as_bytes()));
        snapshots.push(Snapshot {
            relative_path,
            content,
            content_hash,
        });
    }
    Ok(snapshots)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn ensure_output_dir(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(VaultSyncError::InvalidPath("non-normal output path".into()));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(VaultSyncError::InvalidPath(format!(
                    "unsafe output directory: {}",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = current.parent().ok_or_else(|| {
                    VaultSyncError::InvalidPath("output directory has no parent".into())
                })?;
                fs::create_dir(&current)
                    .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
                sync_directory(parent)
                    .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
            }
            Err(error) => return Err(VaultSyncError::Storage(error.to_string())),
        }
    }
    Ok(())
}

fn preflight_output(root: &Path, relative: &Path, max_compare_bytes: u64) -> Result<()> {
    let parent = relative
        .parent()
        .ok_or_else(|| VaultSyncError::InvalidPath("output has no parent".into()))?;
    ensure_output_dir(root, parent)?;
    validate_existing_path(root, relative, false)?;
    let destination = root.join(relative);
    match fs::metadata(&destination) {
        Ok(metadata) if metadata.len() <= max_compare_bytes => {
            fs::read(&destination)
                .map_err(|error| VaultSyncError::TransientRead(error.to_string()))?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(VaultSyncError::TransientRead(error.to_string())),
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_existing(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_existing(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for this call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn atomic_publish_contained(
    root: &Path,
    relative: &Path,
    content: &[u8],
    max_compare_bytes: u64,
) -> Result<PublishOutcome> {
    let root = validate_vault_root(root)?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VaultSyncError::InvalidPath(
            "output path must contain relative normal components only".into(),
        ));
    }
    let parent = relative
        .parent()
        .ok_or_else(|| VaultSyncError::InvalidPath("output has no parent".into()))?;
    preflight_output(&root, relative, max_compare_bytes)?;
    let destination = root.join(relative);
    match fs::metadata(&destination) {
        Ok(metadata) if metadata.len() <= max_compare_bytes => {
            let existing = fs::read(&destination)
                .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
            if existing == content {
                return Ok(PublishOutcome { changed: false });
            }
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(VaultSyncError::Storage(error.to_string())),
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| VaultSyncError::InvalidPath("non-UTF-8 output filename".into()))?;
    let temporary = destination.with_file_name(format!(
        ".{name}.omegon-{}-{sequence}.tmp",
        std::process::id()
    ));
    let result: Result<()> = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
        file.write_all(content)
            .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
        file.sync_all()
            .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
        drop(file);
        validate_existing_path(&root, relative, false)?;
        replace_existing(&temporary, &destination)
            .map_err(|error| VaultSyncError::Storage(error.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    sync_directory(&root.join(parent)).map_err(|error| {
        VaultSyncError::PublishedButDirectorySyncFailed {
            path: relative.to_path_buf(),
            error: error.to_string(),
        }
    })?;
    Ok(PublishOutcome { changed: true })
}

fn atomic_publish(root: &Path, relative: &Path, content: &[u8]) -> Result<bool> {
    atomic_publish_contained(root, relative, content, content.len() as u64)
        .map(|outcome| outcome.changed)
}

fn parse_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    let after_open = trimmed
        .strip_prefix("+++")?
        .trim_start_matches(['\r', '\n']);
    let close = after_open.find("\n+++")?;
    let body = after_open[close + 4..].trim_start_matches(['\r', '\n']);
    Some((&after_open[..close], body))
}

fn extract_toml_value<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    frontmatter.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix(key)?
            .trim()
            .strip_prefix('=')?
            .trim();
        value
            .strip_prefix('"')?
            .split_once('"')
            .map(|(value, _)| value)
    })
}

fn extract_toml_string_array(frontmatter: &str, key: &str) -> Vec<String> {
    frontmatter
        .lines()
        .find_map(|line| {
            let value = line
                .trim()
                .strip_prefix(key)?
                .trim()
                .strip_prefix('=')?
                .trim();
            value.strip_prefix('[')?.strip_suffix(']')
        })
        .map(|values| {
            values
                .split(',')
                .map(|value| value.trim().trim_matches('"'))
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn topic_to_section(topic: &str) -> Option<Section> {
    match topic.to_ascii_lowercase().as_str() {
        "architecture" => Some(Section::Architecture),
        "decisions" => Some(Section::Decisions),
        "constraints" => Some(Section::Constraints),
        "known issues" | "known-issues" | "knownissues" => Some(Section::KnownIssues),
        "patterns & conventions" | "patterns-conventions" | "patterns" | "conventions" => {
            Some(Section::PatternsConventions)
        }
        "specs" | "specifications" => Some(Section::Specs),
        "recent work" | "recent-work" | "recentwork" => Some(Section::RecentWork),
        _ => None,
    }
}

pub fn section_to_slug(section: &Section) -> &'static str {
    match section {
        Section::Architecture => "architecture",
        Section::Decisions => "decisions",
        Section::Constraints => "constraints",
        Section::KnownIssues => "known-issues",
        Section::PatternsConventions => "patterns-conventions",
        Section::Specs => "specs",
        Section::RecentWork => "recent-work",
    }
}

pub fn section_description(section: &Section) -> &'static str {
    match section {
        Section::Architecture => "System structure, component relationships, key abstractions",
        Section::Decisions => "Choices made and their rationale",
        Section::Constraints => "Requirements, limitations, environment details",
        Section::KnownIssues => "Bugs, flaky tests, workarounds",
        Section::PatternsConventions => "Code style, project conventions, common approaches",
        Section::Specs => "Active specifications and design contracts",
        Section::RecentWork => "Recent session activity",
    }
}

fn section_display_name(section: &Section) -> &'static str {
    match section {
        Section::Architecture => "Architecture",
        Section::Decisions => "Decisions",
        Section::Constraints => "Constraints",
        Section::KnownIssues => "Known Issues",
        Section::PatternsConventions => "Patterns & Conventions",
        Section::Specs => "Specs",
        Section::RecentWork => "Recent Work",
    }
}

fn declared_note_id(frontmatter: &str) -> Option<&str> {
    extract_toml_value(frontmatter, "id").filter(|id| !id.is_empty())
}

fn update_note_identity(hash: &mut Sha256, note: &Snapshot, declared_id: Option<&str>) {
    match declared_id {
        Some(id) => {
            hash.update(b"id");
            hash.update([0]);
            hash.update(id.as_bytes());
        }
        None => {
            hash.update(b"path");
            hash.update([0]);
            hash.update(note.relative_path.as_os_str().as_encoded_bytes());
        }
    }
}

fn operation_id(
    kind: &str,
    note: &Snapshot,
    declared_id: Option<&str>,
    mind: &str,
    fact_id: Option<&str>,
) -> String {
    let mut hash = Sha256::new();
    update_note_identity(&mut hash, note, declared_id);
    hash.update([0]);
    hash.update(note.content_hash.as_bytes());
    hash.update([0]);
    hash.update(mind.as_bytes());
    hash.update([0]);
    hash.update(fact_id.unwrap_or_default().as_bytes());
    format!("vault-{kind}-{}", hex::encode(hash.finalize()))
}

fn note_source_identity(note: &Snapshot, declared_id: Option<&str>) -> String {
    let mut hash = Sha256::new();
    update_note_identity(&mut hash, note, declared_id);
    format!("codex-vault:{}", hex::encode(hash.finalize()))
}

fn report_increment(value: &mut usize) {
    *value = value.saturating_add(1).min(MAX_REPORT_COUNT);
}

pub async fn import_from_vault(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
) -> Result<ImportReport> {
    import_from_vault_cancellable(backend, vault_path, mind, &|| false).await
}

pub async fn import_from_vault_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<ImportReport> {
    let root = validate_vault_root(vault_path)?;
    let subdir = validate_subdir(DEFAULT_SUBDIR)?;
    let snapshots = snapshot_markdown(&root, Some(&subdir), cancelled)?;
    let mut declared_ids = HashMap::new();
    for note in &snapshots {
        let Some((frontmatter, _)) = parse_frontmatter(&note.content) else {
            continue;
        };
        if extract_toml_value(frontmatter, "kind") != Some("memory_fact") {
            continue;
        }
        if let Some(id) = declared_note_id(frontmatter)
            && let Some(previous) = declared_ids.insert(id.to_owned(), note.relative_path.clone())
        {
            return Err(VaultSyncError::InvalidInput(format!(
                "duplicate declared memory-fact id {id:?} in {} and {}",
                previous.display(),
                note.relative_path.display()
            )));
        }
    }
    let mut report = ImportReport {
        facts_imported: 0,
        facts_skipped: 0,
    };
    for note in snapshots {
        check_cancel(cancelled)?;
        let Some((frontmatter, body)) = parse_frontmatter(&note.content) else {
            report_increment(&mut report.facts_skipped);
            continue;
        };
        if extract_toml_value(frontmatter, "kind") != Some("memory_fact") || body.trim().is_empty()
        {
            report_increment(&mut report.facts_skipped);
            continue;
        }
        let section = extract_toml_value(frontmatter, "topic")
            .or_else(|| extract_toml_value(frontmatter, "title"))
            .and_then(topic_to_section)
            .unwrap_or(Section::Architecture);
        let declared_id = declared_note_id(frontmatter);
        let source_identity = note_source_identity(&note, declared_id);
        let active = backend.list_facts(mind, FactFilter::default()).await?;
        let current = active
            .iter()
            .find(|fact| fact.source.as_deref() == Some(source_identity.as_str()))
            .cloned();
        if current
            .as_ref()
            .is_some_and(|fact| fact.content == body.trim() && fact.section == section)
        {
            report_increment(&mut report.facts_skipped);
            continue;
        }
        let content_match = active
            .into_iter()
            .find(|fact| fact.content == body.trim() && fact.section == section);
        if current.is_none() && content_match.is_some() {
            report_increment(&mut report.facts_skipped);
            continue;
        }
        let request = StoreFact {
            mind: mind.into(),
            content: body.trim().into(),
            section: section.clone(),
            decay_profile: DecayProfileName::Standard,
            source: Some(source_identity),
        };
        let (operation_id, mutation) = match (current, content_match) {
            (Some(fact), Some(existing)) => (
                operation_id(
                    "reconcile-alias",
                    &note,
                    declared_id,
                    mind,
                    Some(&format!(
                        "{}:{}>{}:{}",
                        fact.id, fact.version, existing.id, existing.version
                    )),
                ),
                MemoryMutation::SupersedeFactWithExisting {
                    fact: FactPrecondition {
                        id: fact.id,
                        expected_version: fact.version,
                    },
                    replacement: FactPrecondition {
                        id: existing.id,
                        expected_version: existing.version,
                    },
                },
            ),
            (Some(fact), None) => (
                operation_id(
                    "reconcile",
                    &note,
                    declared_id,
                    mind,
                    Some(&format!("{}:{}", fact.id, fact.version)),
                ),
                MemoryMutation::SupersedeFact {
                    fact: FactPrecondition {
                        id: fact.id,
                        expected_version: fact.version,
                    },
                    replacement: request,
                },
            ),
            (None, None) => {
                let superseded = backend
                    .list_facts(
                        mind,
                        FactFilter {
                            section: Some(section),
                            status: Some(FactStatus::Superseded),
                        },
                    )
                    .await?
                    .into_iter()
                    .filter(|fact| fact.content == body.trim())
                    .max_by_key(|fact| fact.version);
                let (kind, predecessor) = superseded
                    .map(|fact| ("restore", format!("{}:{}", fact.id, fact.version)))
                    .unwrap_or_else(|| ("import", "new".into()));
                (
                    operation_id(kind, &note, declared_id, mind, Some(&predecessor)),
                    MemoryMutation::StoreFact { request },
                )
            }
            (None, Some(_)) => unreachable!("active content alias handled above"),
        };
        let outcome = backend.apply_mutation(&operation_id, mutation).await?;
        if outcome.replayed {
            report_increment(&mut report.facts_skipped);
        } else {
            report_increment(&mut report.facts_imported);
        }
    }
    Ok(report)
}

pub async fn reinforce_referenced_facts(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
) -> Result<ReinforcementReport> {
    reinforce_referenced_facts_cancellable(backend, vault_path, &|| false).await
}

pub async fn reinforce_referenced_facts_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<ReinforcementReport> {
    let root = validate_vault_root(vault_path)?;
    let snapshots = snapshot_markdown(&root, None, cancelled)?;
    let mut report = ReinforcementReport {
        facts_reinforced: 0,
        references_dangling: 0,
        references_superseded_total: 0,
        references_superseded_truncated: false,
        references_superseded: Vec::new(),
    };
    for note in snapshots {
        check_cancel(cancelled)?;
        let Some((frontmatter, _)) = parse_frontmatter(&note.content) else {
            continue;
        };
        let declared_id = declared_note_id(frontmatter);
        for fact_id in extract_toml_string_array(frontmatter, "related_facts") {
            check_cancel(cancelled)?;
            match backend.get_fact(&fact_id).await? {
                Some(fact) if fact.status == FactStatus::Active => {
                    let outcome = backend
                        .apply_mutation(
                            &operation_id(
                                "reinforce",
                                &note,
                                declared_id,
                                &fact.mind,
                                Some(&fact_id),
                            ),
                            MemoryMutation::ReinforceFactOnce { fact_id },
                        )
                        .await?;
                    if !outcome.replayed {
                        report_increment(&mut report.facts_reinforced);
                    }
                }
                Some(_) => report_increment(&mut report.references_dangling),
                None => {
                    if let Some(replacement) = backend.superseding_fact(&fact_id).await? {
                        report.references_superseded_total += 1;
                        if report.references_superseded.len() < MAX_REPORT_PATHS {
                            report.references_superseded.push(SupersededReference {
                                note_path: note.relative_path.clone(),
                                old_fact_id: fact_id,
                                new_fact_id: replacement.id,
                            });
                        }
                    } else {
                        report_increment(&mut report.references_dangling);
                    }
                }
            }
        }
    }
    report.references_superseded_truncated =
        report.references_superseded_total > report.references_superseded.len();
    Ok(report)
}

pub async fn materialize_to_vault(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
) -> Result<MaterializeReport> {
    materialize_to_vault_cancellable(backend, vault_path, mind, &|| false).await
}

pub async fn materialize_to_vault_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<MaterializeReport> {
    materialize_to_vault_with_subdir_cancellable(
        backend,
        vault_path,
        mind,
        DEFAULT_SUBDIR,
        cancelled,
    )
    .await
}

pub async fn materialize_to_vault_with_subdir(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    subdir: &str,
) -> Result<MaterializeReport> {
    materialize_to_vault_with_subdir_cancellable(backend, vault_path, mind, subdir, &|| false).await
}

async fn materialize_to_vault_with_subdir_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    subdir: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<MaterializeReport> {
    check_cancel(cancelled)?;
    let root = validate_vault_root(vault_path)?;
    let subdir = validate_subdir(subdir)?;
    let mut outputs = Vec::new();
    let mut index_rows = Vec::new();
    let all_facts = backend
        .list_facts(
            mind,
            FactFilter {
                section: None,
                status: Some(FactStatus::Active),
            },
        )
        .await?;
    check_cancel(cancelled)?;
    for section in Section::all() {
        check_cancel(cancelled)?;
        let mut facts: Vec<Fact> = all_facts
            .iter()
            .filter(|fact| fact.section == *section)
            .cloned()
            .collect();
        let slug = section_to_slug(section);
        let relative = subdir.join(format!("{slug}.md"));
        let previously_materialized = fs::symlink_metadata(root.join(&relative)).is_ok();
        if facts.is_empty() && !previously_materialized {
            continue;
        }
        facts.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.id.cmp(&right.id))
        });
        let name = section_display_name(section);
        let stable_date = facts
            .iter()
            .map(|fact| fact.created_at.get(..10).unwrap_or("1970-01-01"))
            .max()
            .unwrap_or("1970-01-01");
        let mut content = format!(
            "+++\nid = \"memory-section-{slug}\"\nkind = \"memory_section\"\ntags = [\"memory\", \"{slug}\"]\n\n[data]\nsection = \"{name}\"\nfact_count = {}\nlast_updated = \"{stable_date}\"\nmind = \"{mind}\"\n+++\n\n# {name}\n\n_{}_\n\n",
            facts.len(),
            section_description(section)
        );
        for fact in &facts {
            content.push_str(&format!(
                "- {} [confidence: {:.2}, id: {}]\n",
                fact.content, fact.confidence, fact.id
            ));
        }
        if !facts.is_empty() {
            index_rows.push((slug, facts.len(), stable_date.to_string()));
        }
        outputs.push((relative, content, facts.len()));
    }
    let index_relative = subdir.join("_index.md");
    if !index_rows.is_empty() || fs::symlink_metadata(root.join(&index_relative)).is_ok() {
        let mut index = "# Project Memory\n\n| Section | Facts | Last Updated |\n|---------|-------|-------------|\n".to_string();
        for (slug, count, date) in index_rows {
            index.push_str(&format!("| [[{slug}]] | {count} | {date} |\n"));
        }
        outputs.push((index_relative, index, 0));
    }
    for (relative, content, _) in &outputs {
        check_cancel(cancelled)?;
        preflight_output(&root, relative, content.len() as u64)?;
    }
    let mut sections_written = 0usize;
    let mut facts_written = 0usize;
    let mut files_changed_total = 0usize;
    let mut files_written = Vec::new();
    for (relative, content, fact_count) in outputs {
        check_cancel(cancelled)?;
        if atomic_publish(&root, &relative, content.as_bytes())? {
            files_changed_total += 1;
            if relative.file_name() != Some(OsStr::new("_index.md")) {
                report_increment(&mut sections_written);
            }
            facts_written = facts_written
                .saturating_add(fact_count)
                .min(MAX_REPORT_COUNT);
            if files_written.len() < MAX_REPORT_PATHS {
                files_written.push(relative);
            }
        }
    }
    Ok(MaterializeReport {
        sections_written,
        facts_written,
        files_changed_total,
        files_truncated: files_changed_total > files_written.len(),
        files_written,
    })
}

pub async fn materialize_episodes_to_vault(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    limit: usize,
) -> Result<usize> {
    materialize_episodes_to_vault_cancellable(backend, vault_path, mind, limit, &|| false).await
}

pub async fn materialize_episodes_to_vault_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<usize> {
    materialize_episodes_to_vault_with_subdir_cancellable(
        backend,
        vault_path,
        mind,
        limit,
        DEFAULT_SUBDIR,
        cancelled,
    )
    .await
}

pub async fn materialize_episodes_to_vault_with_subdir(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    limit: usize,
    subdir: &str,
) -> Result<usize> {
    materialize_episodes_to_vault_with_subdir_cancellable(
        backend,
        vault_path,
        mind,
        limit,
        subdir,
        &|| false,
    )
    .await
}

async fn materialize_episodes_to_vault_with_subdir_cancellable(
    backend: &dyn MemoryBackend,
    vault_path: &Path,
    mind: &str,
    limit: usize,
    subdir: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<usize> {
    check_cancel(cancelled)?;
    let root = validate_vault_root(vault_path)?;
    let subdir = validate_subdir(subdir)?.join("episodes");
    let episodes = backend.list_episodes(mind, limit).await?;
    check_cancel(cancelled)?;
    let mut by_date = std::collections::BTreeMap::<String, Vec<Episode>>::new();
    for episode in episodes {
        check_cancel(cancelled)?;
        by_date
            .entry(episode.date.clone())
            .or_default()
            .push(episode);
    }
    let mut outputs = Vec::with_capacity(by_date.len());
    for (date, mut episodes) in by_date {
        check_cancel(cancelled)?;
        episodes.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let tool_calls: u64 = episodes
            .iter()
            .map(|episode| u64::from(episode.tool_calls_count.unwrap_or(0)))
            .sum();
        let mut content = format!(
            "+++\nid = \"episode-{date}\"\nkind = \"memory_episode\"\ntags = [\"memory\", \"episode\", \"{date}\"]\n\n[data]\ndate = \"{date}\"\nmind = \"{mind}\"\nepisode_count = {}\ntool_calls = {tool_calls}\n+++\n",
            episodes.len()
        );
        for episode in episodes {
            content.push_str(&format!(
                "\n## {}\n\n- id: `{}`\n- created_at: `{}`\n- tool_calls: {}\n\n{}\n",
                episode.title,
                episode.id,
                episode.created_at,
                episode.tool_calls_count.unwrap_or(0),
                episode.narrative
            ));
        }
        outputs.push((subdir.join(format!("{date}.md")), content));
    }
    for (relative, content) in &outputs {
        check_cancel(cancelled)?;
        preflight_output(&root, relative, content.len() as u64)?;
    }
    let mut written = 0;
    for (relative, content) in outputs {
        check_cancel(cancelled)?;
        written += usize::from(atomic_publish(&root, &relative, content.as_bytes())?);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryBackend, SqliteBackend};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    async fn seed(backend: &dyn MemoryBackend) -> Fact {
        backend
            .store_fact(StoreFact {
                mind: "default".into(),
                content: "Stable architecture".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: Some("test".into()),
            })
            .await
            .unwrap()
            .fact
    }

    async fn seed_facts(backend: &dyn MemoryBackend, section: Section, count: usize) {
        for i in 0..count {
            backend
                .store_fact(StoreFact {
                    mind: "default".into(),
                    content: format!("Fact {i} for {}", section_display_name(&section)),
                    section: section.clone(),
                    decay_profile: DecayProfileName::Standard,
                    source: Some("test".into()),
                })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn materialize_report_counts_correctly() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        seed_facts(backend.as_ref(), Section::Architecture, 3).await;
        seed_facts(backend.as_ref(), Section::Decisions, 2).await;
        let report = materialize_to_vault(backend.as_ref(), temp.path(), "default")
            .await
            .unwrap();
        assert_eq!(report.sections_written, 2);
        assert_eq!(report.facts_written, 5);
        assert_eq!(report.files_written.len(), 3);
        let architecture =
            fs::read_to_string(temp.path().join("ai/memory/architecture.md")).unwrap();
        assert!(architecture.contains("kind = \"memory_section\""));
        assert!(architecture.contains("fact_count = 3"));
        assert!(architecture.contains("# Architecture"));
        assert!(temp.path().join("ai/memory/decisions.md").exists());
        let index = fs::read_to_string(temp.path().join("ai/memory/_index.md")).unwrap();
        assert!(index.contains("[[architecture]]"));
        assert!(index.contains("[[decisions]]"));
    }

    #[test]
    fn section_slug_roundtrip() {
        for section in Section::all() {
            let slug = section_to_slug(section);
            assert!(!slug.is_empty());
            assert!(
                slug.chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
            );
            assert!(!section_description(section).is_empty());
        }
    }

    #[tokio::test]
    async fn import_skips_materializer_files() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        let memory = temp.path().join("ai/memory");
        fs::create_dir_all(&memory).unwrap();
        fs::write(
            memory.join("architecture.md"),
            "+++\nkind = \"memory_section\"\n+++\nBody\n",
        )
        .unwrap();
        fs::write(
            memory.join("episode.md"),
            "+++\nkind = \"memory_episode\"\n+++\nBody\n",
        )
        .unwrap();
        fs::write(memory.join("codex.md"), "+++\nkind = \"memory_fact\"\ntopic = \"Architecture\"\n+++\nThe API uses REST with JSON payloads\n").unwrap();
        let report = import_from_vault(backend.as_ref(), temp.path(), "default")
            .await
            .unwrap();
        assert_eq!(report.facts_imported, 1);
        assert_eq!(report.facts_skipped, 2);
        let facts = backend
            .list_facts(
                "default",
                FactFilter {
                    section: Some(Section::Architecture),
                    status: Some(FactStatus::Active),
                },
            )
            .await
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert!(facts[0].content.contains("REST with JSON"));
        assert!(
            facts[0]
                .source
                .as_deref()
                .unwrap()
                .starts_with("codex-vault:")
        );
    }

    #[test]
    fn parse_frontmatter_works() {
        let (frontmatter, body) = parse_frontmatter(
            "+++\nkind = \"memory_fact\"\ntopic = \"Decisions\"\n+++\n\nThe body here\n",
        )
        .unwrap();
        assert!(frontmatter.contains("kind = \"memory_fact\""));
        assert!(body.contains("The body here"));
    }

    #[test]
    fn parse_frontmatter_returns_none_without_delimiters() {
        assert!(parse_frontmatter("# Regular markdown\n").is_none());
    }

    #[test]
    fn extract_toml_value_works() {
        let frontmatter = "kind = \"memory_fact\"\ntopic = \"Architecture\"\nfact_count = 3";
        assert_eq!(extract_toml_value(frontmatter, "kind"), Some("memory_fact"));
        assert_eq!(
            extract_toml_value(frontmatter, "topic"),
            Some("Architecture")
        );
        assert_eq!(extract_toml_value(frontmatter, "missing"), None);
    }

    #[test]
    fn extract_toml_string_array_works() {
        assert_eq!(
            extract_toml_string_array("related_facts = [\"abc123\", \"def456\"]", "related_facts"),
            vec!["abc123", "def456"]
        );
    }

    #[test]
    fn extract_toml_string_array_empty() {
        assert!(extract_toml_string_array("related_facts = []", "related_facts").is_empty());
    }

    #[test]
    fn extract_toml_string_array_missing() {
        assert!(extract_toml_string_array("kind = \"note\"", "related_facts").is_empty());
    }

    #[tokio::test]
    async fn reinforce_referenced_facts_reinforces_active() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        let fact = seed(backend.as_ref()).await;
        fs::create_dir_all(temp.path().join("notes")).unwrap();
        fs::write(
            temp.path().join("notes/design.md"),
            format!(
                "+++\nkind = \"note\"\nrelated_facts = [\"{}\"]\n+++\nBody\n",
                fact.id
            ),
        )
        .unwrap();
        let report = reinforce_referenced_facts(backend.as_ref(), temp.path())
            .await
            .unwrap();
        assert_eq!(report.facts_reinforced, 1);
        assert_eq!(report.references_dangling, 0);
        assert!(report.references_superseded.is_empty());
        assert!(
            backend
                .get_fact(&fact.id)
                .await
                .unwrap()
                .unwrap()
                .reinforcement_count
                > fact.reinforcement_count
        );
    }

    #[tokio::test]
    async fn reinforce_referenced_facts_detects_dangling() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("notes")).unwrap();
        fs::write(
            temp.path().join("notes/orphan.md"),
            "+++\nrelated_facts = [\"missing\"]\n+++\nBody\n",
        )
        .unwrap();
        let report = reinforce_referenced_facts(backend.as_ref(), temp.path())
            .await
            .unwrap();
        assert_eq!(report.facts_reinforced, 0);
        assert_eq!(report.references_dangling, 1);
    }

    #[tokio::test]
    async fn reinforce_referenced_facts_reports_active_superseding_fact() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        let original = seed(backend.as_ref()).await;
        let replacement = backend
            .supersede_fact(
                &original.id,
                StoreFact {
                    mind: "default".into(),
                    content: "Replacement architecture".into(),
                    section: Section::Architecture,
                    decay_profile: DecayProfileName::Standard,
                    source: Some("test".into()),
                },
            )
            .await
            .unwrap();
        fs::write(
            temp.path().join("reference.md"),
            format!("+++\nrelated_facts = [\"{}\"]\n+++\nBody\n", original.id),
        )
        .unwrap();
        let report = reinforce_referenced_facts(backend.as_ref(), temp.path())
            .await
            .unwrap();
        assert_eq!(report.facts_reinforced, 0);
        assert_eq!(report.references_dangling, 0);
        assert_eq!(report.references_superseded_total, 1);
        assert_eq!(report.references_superseded[0].old_fact_id, original.id);
        assert_eq!(report.references_superseded[0].new_fact_id, replacement.id);
    }

    #[tokio::test]
    async fn reinforce_skips_dotdirs() {
        let backend = Arc::new(InMemoryBackend::new());
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".codex")).unwrap();
        fs::write(
            temp.path().join(".codex/internal.md"),
            "+++\nrelated_facts = [\"missing\"]\n+++\nBody\n",
        )
        .unwrap();
        let report = reinforce_referenced_facts(backend.as_ref(), temp.path())
            .await
            .unwrap();
        assert_eq!(report.facts_reinforced, 0);
        assert_eq!(report.references_dangling, 0);
    }

    #[tokio::test]
    async fn unchanged_import_and_reinforcement_are_idempotent_across_reopen() {
        let temp = tempdir().unwrap();
        let vault = temp.path().join("vault");
        fs::create_dir_all(vault.join("ai/memory")).unwrap();
        fs::write(
            vault.join("ai/memory/import.md"),
            "+++\nkind = \"memory_fact\"\ntopic = \"Architecture\"\n+++\nImported fact\n",
        )
        .unwrap();
        let database = temp.path().join("facts.db");
        let fact_id = {
            let backend = SqliteBackend::open(&database).unwrap();
            let fact = seed(&backend).await;
            fs::write(
                vault.join("note.md"),
                format!("+++\nrelated_facts = [\"{}\"]\n+++\nNote\n", fact.id),
            )
            .unwrap();
            assert_eq!(
                import_from_vault(&backend, &vault, "default")
                    .await
                    .unwrap()
                    .facts_imported,
                1
            );
            assert_eq!(
                reinforce_referenced_facts(&backend, &vault)
                    .await
                    .unwrap()
                    .facts_reinforced,
                1
            );
            fact.id
        };
        let backend = SqliteBackend::open(&database).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );
        assert_eq!(
            reinforce_referenced_facts(&backend, &vault)
                .await
                .unwrap()
                .facts_reinforced,
            0
        );
        let before = backend
            .get_fact(&fact_id)
            .await
            .unwrap()
            .unwrap()
            .reinforcement_count;
        fs::write(
            vault.join("note.md"),
            format!("+++\nrelated_facts = [\"{fact_id}\"]\n+++\nChanged note\n"),
        )
        .unwrap();
        assert_eq!(
            reinforce_referenced_facts(&backend, &vault)
                .await
                .unwrap()
                .facts_reinforced,
            1
        );
        assert_eq!(
            backend
                .get_fact(&fact_id)
                .await
                .unwrap()
                .unwrap()
                .reinforcement_count,
            before + 1
        );
    }

    #[tokio::test]
    async fn materialization_is_deterministic_and_does_not_rewrite() {
        let backend = InMemoryBackend::new();
        seed(&backend).await;
        let temp = tempdir().unwrap();
        let first = materialize_to_vault(&backend, temp.path(), "default")
            .await
            .unwrap();
        assert!(!first.files_written.is_empty());
        let path = temp.path().join("ai/memory/architecture.md");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = materialize_to_vault(&backend, temp.path(), "default")
            .await
            .unwrap();
        assert!(second.files_written.is_empty());
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }

    #[tokio::test]
    async fn changed_and_changed_back_note_reconcile_with_lineage_across_reopen() {
        let temp = tempdir().unwrap();
        let vault = temp.path().join("vault");
        let note = vault.join("ai/memory/note.md");
        fs::create_dir_all(note.parent().unwrap()).unwrap();
        let database = temp.path().join("facts.db");
        let write_note = |content: &str| {
            fs::write(
                &note,
                format!("+++\nkind = \"memory_fact\"\ntopic = \"Architecture\"\n+++\n{content}\n"),
            )
            .unwrap();
        };

        write_note("version one");
        let first_id = {
            let backend = SqliteBackend::open(&database).unwrap();
            assert_eq!(
                import_from_vault(&backend, &vault, "default")
                    .await
                    .unwrap()
                    .facts_imported,
                1
            );
            backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap()[0]
                .id
                .clone()
        };
        write_note("version two");
        let second_id = {
            let backend = SqliteBackend::open(&database).unwrap();
            assert_eq!(
                import_from_vault(&backend, &vault, "default")
                    .await
                    .unwrap()
                    .facts_imported,
                1
            );
            let facts = backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap();
            assert_eq!(facts.len(), 1);
            assert_eq!(facts[0].content, "version two");
            assert_eq!(facts[0].superseded_by.as_deref(), Some(first_id.as_str()));
            facts[0].id.clone()
        };
        write_note("version one");
        let backend = SqliteBackend::open(&database).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let facts = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "version one");
        assert_ne!(facts[0].id, first_id);
        assert_eq!(facts[0].superseded_by.as_deref(), Some(second_id.as_str()));
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );
    }

    #[tokio::test]
    async fn same_note_imports_independently_for_multiple_minds() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("ai/memory")).unwrap();
        fs::write(
            temp.path().join("ai/memory/shared.md"),
            "+++\nid = \"shared-note\"\nkind = \"memory_fact\"\n+++\nShared content\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, temp.path(), "mind-a")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        assert_eq!(
            import_from_vault(&backend, temp.path(), "mind-b")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        assert_eq!(
            backend
                .list_facts("mind-a", FactFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            backend
                .list_facts("mind-b", FactFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn cancelled_snapshot_does_not_mutate_memory() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("ai/memory")).unwrap();
        for index in 0..4 {
            fs::write(
                temp.path().join(format!("ai/memory/{index}.md")),
                format!(
                    "+++\nid = \"cancel-{index}\"\nkind = \"memory_fact\"\n+++\nFact {index}\n"
                ),
            )
            .unwrap();
        }
        let checks = AtomicUsize::new(0);
        let result = import_from_vault_cancellable(&backend, temp.path(), "default", &|| {
            checks.fetch_add(1, Ordering::Relaxed) >= 3
        })
        .await;
        assert!(matches!(result, Err(VaultSyncError::Cancelled)));
        assert!(
            backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn snapshot_rejects_aggregate_metadata_before_retaining_contents() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("ai/memory")).unwrap();
        for name in ["one.md", "two.md"] {
            let file = fs::File::create(temp.path().join("ai/memory").join(name)).unwrap();
            file.set_len(60).unwrap();
        }
        let root = validate_vault_root(temp.path()).unwrap();
        let result =
            snapshot_markdown_with_limit(&root, Some(Path::new("ai/memory")), &|| false, 100);
        assert!(matches!(result, Err(VaultSyncError::TransientRead(message))
            if message.contains("aggregate bytes")));
    }

    #[tokio::test]
    async fn duplicate_declared_ids_fail_before_any_import() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("ai/memory")).unwrap();
        for name in ["one.md", "two.md"] {
            fs::write(
                temp.path().join("ai/memory").join(name),
                "+++\nid = \"duplicate\"\nkind = \"memory_fact\"\n+++\nContent\n",
            )
            .unwrap();
        }
        let result = import_from_vault(&backend, temp.path(), "default").await;
        assert!(matches!(result, Err(VaultSyncError::InvalidInput(message))
            if message.contains("duplicate declared memory-fact id")));
        assert!(
            backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn declared_id_alias_restores_superseded_content_without_reinforcement() {
        let temp = tempdir().unwrap();
        let vault = temp.path().join("vault");
        let memory = vault.join("ai/memory");
        fs::create_dir_all(&memory).unwrap();
        let first = memory.join("a.md");
        let second = memory.join("b.md");
        fs::write(
            &first,
            "+++\nid = \"alias-a\"\nkind = \"memory_fact\"\n+++\nShared\n",
        )
        .unwrap();
        fs::write(
            &second,
            "+++\nid = \"alias-b\"\nkind = \"memory_fact\"\n+++\nShared\n",
        )
        .unwrap();
        let backend = SqliteBackend::open(&temp.path().join("memory.db")).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let original = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(original.reinforcement_count, 1);

        fs::write(
            &first,
            "+++\nid = \"alias-a\"\nkind = \"memory_fact\"\n+++\nChanged\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            2
        );
        let active = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
        let restored = active
            .iter()
            .find(|fact| fact.content == "Shared")
            .unwrap()
            .clone();
        let first_change = active
            .iter()
            .find(|fact| fact.content == "Changed")
            .unwrap()
            .clone();
        assert_eq!(restored.reinforcement_count, 1);
        let stable_version = restored.version;

        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );
        let unchanged = backend.get_fact(&restored.id).await.unwrap().unwrap();
        assert_eq!(unchanged.version, stable_version);
        assert_eq!(unchanged.reinforcement_count, 1);

        fs::write(
            &first,
            "+++\nid = \"alias-a\"\nkind = \"memory_fact\"\n+++\nShared\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let active = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, restored.id);
        assert_eq!(active[0].content, "Shared");
        assert_eq!(active[0].reinforcement_count, 1);
        assert_eq!(
            backend
                .superseding_fact(&original.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            restored.id
        );
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );

        fs::write(
            &first,
            "+++\nid = \"alias-a\"\nkind = \"memory_fact\"\n+++\nChanged\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let active = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(active.len(), 2);
        let second_change = active
            .iter()
            .find(|fact| fact.content == "Changed")
            .unwrap()
            .clone();
        assert_eq!(second_change.reinforcement_count, 1);

        fs::write(
            &first,
            "+++\nid = \"alias-a\"\nkind = \"memory_fact\"\n+++\nShared\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let active = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, restored.id);
        assert_eq!(active[0].reinforcement_count, 1);
        for historical in [&original, &first_change, &second_change] {
            assert_eq!(
                backend
                    .superseding_fact(&historical.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
                restored.id
            );
        }
        assert!(
            backend
                .superseding_fact(&restored.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn declared_id_survives_note_move_and_preserves_lineage_across_reopen() {
        let temp = tempdir().unwrap();
        let vault = temp.path().join("vault");
        let memory = vault.join("ai/memory");
        fs::create_dir_all(memory.join("moved")).unwrap();
        let original_path = memory.join("original.md");
        let moved_path = memory.join("moved/note.md");
        fs::write(
            &original_path,
            "+++\nid = \"stable-note\"\nkind = \"memory_fact\"\n+++\nOriginal\n",
        )
        .unwrap();
        let database = temp.path().join("memory.db");
        let backend = SqliteBackend::open(&database).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let original = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap()
            .remove(0);
        drop(backend);

        fs::rename(&original_path, &moved_path).unwrap();
        let backend = SqliteBackend::open(&database).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );
        fs::write(
            &moved_path,
            "+++\nid = \"stable-note\"\nkind = \"memory_fact\"\n+++\nChanged\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let changed = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(
            backend
                .superseding_fact(&original.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            changed.id
        );
        drop(backend);

        fs::write(
            &moved_path,
            "+++\nid = \"stable-note\"\nkind = \"memory_fact\"\n+++\nOriginal\n",
        )
        .unwrap();
        let backend = SqliteBackend::open(&database).unwrap();
        assert_eq!(
            import_from_vault(&backend, &vault, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let restored = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap()
            .remove(0);
        assert_eq!(restored.content, "Original");
        assert_eq!(
            backend
                .superseding_fact(&changed.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            restored.id
        );
    }

    #[tokio::test]
    async fn note_without_declared_id_keeps_path_identity() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        let memory = temp.path().join("ai/memory");
        fs::create_dir_all(memory.join("moved")).unwrap();
        let original = memory.join("original.md");
        fs::write(&original, "+++\nkind = \"memory_fact\"\n+++\nContent\n").unwrap();
        import_from_vault(&backend, temp.path(), "default")
            .await
            .unwrap();
        let moved = memory.join("moved/note.md");
        fs::rename(&original, &moved).unwrap();
        fs::write(moved, "+++\nkind = \"memory_fact\"\n+++\nMoved content\n").unwrap();
        assert_eq!(
            import_from_vault(&backend, temp.path(), "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        assert_eq!(
            backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn note_identity_survives_vault_relocation() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        let first = temp.path().join("first-vault");
        let second = temp.path().join("second-vault");
        fs::create_dir_all(first.join("ai/memory")).unwrap();
        fs::write(
            first.join("ai/memory/note.md"),
            "+++\nid = \"relocatable\"\nkind = \"memory_fact\"\n+++\nBefore move\n",
        )
        .unwrap();
        import_from_vault(&backend, &first, "default")
            .await
            .unwrap();
        fs::rename(&first, &second).unwrap();
        assert_eq!(
            import_from_vault(&backend, &second, "default")
                .await
                .unwrap()
                .facts_imported,
            0
        );
        fs::write(
            second.join("ai/memory/note.md"),
            "+++\nid = \"relocatable\"\nkind = \"memory_fact\"\n+++\nAfter move\n",
        )
        .unwrap();
        assert_eq!(
            import_from_vault(&backend, &second, "default")
                .await
                .unwrap()
                .facts_imported,
            1
        );
        let facts = backend
            .list_facts("default", FactFilter::default())
            .await
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "After move");
    }

    #[tokio::test]
    async fn same_date_episodes_aggregate_deterministically_without_rewrite() {
        let backend = InMemoryBackend::new();
        for (title, narrative, calls) in [
            ("First", "Narrative one", 2),
            ("Second", "Narrative two", 3),
        ] {
            backend
                .store_episode(StoreEpisode {
                    mind: "default".into(),
                    title: title.into(),
                    narrative: narrative.into(),
                    date: Some("2026-08-25".into()),
                    affected_nodes: vec![],
                    affected_changes: vec![],
                    files_changed: vec![],
                    tags: vec![],
                    tool_calls_count: Some(calls),
                    formation: None,
                })
                .await
                .unwrap();
        }
        let temp = tempdir().unwrap();
        assert_eq!(
            materialize_episodes_to_vault(&backend, temp.path(), "default", 20)
                .await
                .unwrap(),
            1
        );
        let path = temp.path().join("ai/memory/episodes/2026-08-25.md");
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("episode_count = 2"));
        assert!(content.contains("tool_calls = 5"));
        assert!(content.contains("Narrative one"));
        assert!(content.contains("Narrative two"));
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(
            materialize_episodes_to_vault(&backend, temp.path(), "default", 20)
                .await
                .unwrap(),
            0
        );
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }

    #[tokio::test]
    async fn empty_section_and_index_are_reconciled_without_deletion() {
        let backend = InMemoryBackend::new();
        let fact = seed(&backend).await;
        let temp = tempdir().unwrap();
        materialize_to_vault(&backend, temp.path(), "default")
            .await
            .unwrap();
        backend.archive_facts(&[&fact.id]).await.unwrap();
        let report = materialize_to_vault(&backend, temp.path(), "default")
            .await
            .unwrap();
        assert_eq!(report.sections_written, 1);
        assert_eq!(report.facts_written, 0);
        let section = fs::read_to_string(temp.path().join("ai/memory/architecture.md")).unwrap();
        assert!(section.contains("fact_count = 0"));
        assert!(!section.contains("Stable architecture"));
        let index = fs::read_to_string(temp.path().join("ai/memory/_index.md")).unwrap();
        assert!(!index.contains("[[architecture]]"));
        assert!(
            materialize_to_vault(&backend, temp.path(), "default")
                .await
                .unwrap()
                .files_written
                .is_empty()
        );
    }

    #[test]
    fn traversal_and_symlink_escape_are_rejected() {
        let _temp = tempdir().unwrap();
        assert!(validate_subdir("../escape").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(_temp.path(), _temp.path().join("linked")).unwrap();
            assert!(validate_vault_root(&_temp.path().join("linked")).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_input_aborts_before_any_import_and_output_escape_is_rejected() {
        let backend = InMemoryBackend::new();
        let temp = tempdir().unwrap();
        let vault = temp.path().join("vault");
        let outside = temp.path().join("outside");
        fs::create_dir_all(vault.join("ai/memory")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            vault.join("ai/memory/valid.md"),
            "+++\nkind = \"memory_fact\"\n+++\nMust not import\n",
        )
        .unwrap();
        fs::write(outside.join("escape.md"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.join("escape.md"), vault.join("ai/memory/linked.md"))
            .unwrap();
        assert!(
            import_from_vault(&backend, &vault, "default")
                .await
                .is_err()
        );
        assert!(
            backend
                .list_facts("default", FactFilter::default())
                .await
                .unwrap()
                .is_empty()
        );

        fs::remove_file(vault.join("ai/memory/linked.md")).unwrap();
        fs::remove_dir_all(vault.join("ai/memory")).unwrap();
        std::os::unix::fs::symlink(&outside, vault.join("ai/memory")).unwrap();
        seed(&backend).await;
        assert!(
            materialize_to_vault(&backend, &vault, "default")
                .await
                .is_err()
        );
        assert!(!outside.join("architecture.md").exists());
    }
}
