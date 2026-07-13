//! Styrene-owned work contracts.
//!
//! Adapters translate local artifacts, lifecycle systems, and the eventual task
//! server into these types. No adapter-specific model is exposed to consumers.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use ulid::Ulid;

pub type Result<T> = std::result::Result<T, WorkError>;

#[derive(Debug, Error)]
pub enum WorkError {
    #[error("work item not found: {0}")]
    NotFound(String),
    #[error("work source rejected the operation: {0}")]
    Rejected(String),
    #[error("work source conflict: {0}")]
    Conflict(String),
    #[error("invalid work data: {0}")]
    Invalid(String),
    #[error("work source failed: {0}")]
    Source(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkId(String);

impl WorkId {
    pub fn new(namespace: &str, value: &str) -> Result<Self> {
        if namespace.is_empty()
            || value.is_empty()
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || value.chars().any(char::is_control)
        {
            return Err(WorkError::Invalid(format!(
                "invalid work id {namespace}:{value}"
            )));
        }
        Ok(Self(format!("{namespace}:{value}")))
    }

    pub fn local() -> Self {
        Self(format!("local:{}", Ulid::new()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn split(&self) -> (&str, &str) {
        self.0.split_once(':').unwrap_or(("", self.0.as_str()))
    }
}

impl fmt::Display for WorkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAuthority {
    Repository,
    OpenSpec,
    ExecutionRuntime,
    TaskServer,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(String);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.chars().any(char::is_control)
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(WorkError::Invalid(format!("invalid source id: {value}")));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Repository,
    Lifecycle,
    Execution,
    Server,
    Derived,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkOrigin {
    pub source_id: SourceId,
    pub source_kind: SourceKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCapabilities {
    pub set_state: bool,
    pub set_assignee: bool,
    pub edit_body: bool,
    pub add_relation: bool,
    pub archive: bool,
    pub delete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Initiative,
    Task,
    Change,
    Note,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Draft,
    Backlog,
    Planned,
    Active,
    Blocked,
    Completed,
    Cancelled,
    Archived,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
    Someday,
    Unspecified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalRef {
    pub kind: String,
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl ExternalRef {
    pub fn new(kind: impl Into<String>, locator: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            locator: locator.into(),
            label: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Contains,
    Blocks,
    DependsOn,
    Related,
    Specifies,
    Implements,
    Projects,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRelation {
    pub kind: RelationKind,
    pub target: WorkId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLifecycle {
    pub category: WorkState,
    pub native_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    pub terminal: bool,
    pub inferred: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkRevision {
    GitBlob { oid: String },
    FileDigest { algorithm: String, value: String },
    Server { version: u64, etag: Option<String> },
    Composite { generation: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkProvenance {
    pub origin: WorkOrigin,
    pub observed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<WorkRevision>,
    pub projection_version: u16,
    #[serde(default)]
    pub inferred_fields: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkFacets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openspec: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Value>,
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: WorkId,
    pub kind: WorkKind,
    pub authority: WorkAuthority,
    pub title: String,
    pub lifecycle: WorkLifecycle,
    pub priority: Priority,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub relations: Vec<WorkRelation>,
    #[serde(default)]
    pub refs: Vec<ExternalRef>,
    #[serde(default)]
    pub capabilities: WorkCapabilities,
    pub provenance: WorkProvenance,
    #[serde(default)]
    pub facets: WorkFacets,
}

#[derive(Clone, Debug, Default)]
pub struct WorkQuery {
    pub kinds: Vec<WorkKind>,
    pub states: Vec<WorkState>,
    pub authorities: Vec<WorkAuthority>,
    pub tags: Vec<String>,
}

impl WorkQuery {
    pub fn matches(&self, item: &WorkItem) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(&item.kind))
            && (self.states.is_empty() || self.states.contains(&item.lifecycle.category))
            && (self.authorities.is_empty() || self.authorities.contains(&item.authority))
            && (self.tags.is_empty() || self.tags.iter().any(|tag| item.tags.contains(tag)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_id_requires_a_safe_namespace_and_value() {
        assert!(WorkId::new("markplane", "TASK-abc12").is_ok());
        assert!(WorkId::new("bad:namespace", "x").is_err());
        assert!(WorkId::new("server", "line\nbreak").is_err());
    }

    #[test]
    fn query_filters_without_owning_storage_semantics() {
        let item = WorkItem {
            id: WorkId::new("local", "one").unwrap(),
            kind: WorkKind::Task,
            authority: WorkAuthority::Repository,
            title: "one".into(),
            lifecycle: WorkLifecycle {
                category: WorkState::Active,
                native_state: "active".into(),
                workflow: None,
                terminal: false,
                inferred: false,
            },
            priority: Priority::Medium,
            body: String::new(),
            tags: vec!["rust".into()],
            assignee: None,
            relations: vec![],
            refs: vec![],
            capabilities: WorkCapabilities::default(),
            provenance: WorkProvenance {
                origin: WorkOrigin {
                    source_id: SourceId::new("test").unwrap(),
                    source_kind: SourceKind::Derived,
                },
                observed_at: Utc::now(),
                revision: None,
                projection_version: 1,
                inferred_fields: vec![],
            },
            facets: WorkFacets::default(),
        };
        assert!(
            WorkQuery {
                states: vec![WorkState::Active],
                tags: vec!["rust".into()],
                ..Default::default()
            }
            .matches(&item)
        );
        assert!(
            !WorkQuery {
                authorities: vec![WorkAuthority::TaskServer],
                ..Default::default()
            }
            .matches(&item)
        );
    }
}
