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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    Repository,
    OpenSpec,
    TaskServer,
    Derived,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: WorkId,
    pub kind: WorkKind,
    pub authority: Authority,
    pub title: String,
    pub state: WorkState,
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
    pub revision: Option<u64>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Default)]
pub struct WorkQuery {
    pub kinds: Vec<WorkKind>,
    pub states: Vec<WorkState>,
    pub authorities: Vec<Authority>,
    pub tags: Vec<String>,
}

impl WorkQuery {
    pub fn matches(&self, item: &WorkItem) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(&item.kind))
            && (self.states.is_empty() || self.states.contains(&item.state))
            && (self.authorities.is_empty() || self.authorities.contains(&item.authority))
            && (self.tags.is_empty() || self.tags.iter().any(|tag| item.tags.contains(tag)))
    }
}

pub trait WorkSource {
    fn source_id(&self) -> &'static str;
    fn list(&self, query: &WorkQuery) -> Result<Vec<WorkItem>>;
    fn get(&self, id: &WorkId) -> Result<Option<WorkItem>>;
}

#[derive(Clone, Debug)]
pub enum WorkCommand {
    SetState {
        id: WorkId,
        state: WorkState,
        expected_revision: Option<u64>,
    },
    SetAssignee {
        id: WorkId,
        assignee: Option<String>,
        expected_revision: Option<u64>,
    },
}

pub trait MutableWorkSource: WorkSource {
    fn apply(&self, command: &WorkCommand) -> Result<WorkItem>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projection {
    pub title: String,
    pub generated_at: DateTime<Utc>,
    pub items: Vec<WorkItem>,
}

pub struct WorkGraph<'a> {
    sources: Vec<&'a dyn WorkSource>,
}

impl<'a> WorkGraph<'a> {
    pub fn new(sources: Vec<&'a dyn WorkSource>) -> Self {
        Self { sources }
    }

    pub fn project(&self, title: impl Into<String>, query: &WorkQuery) -> Result<Projection> {
        let mut items = Vec::new();
        for source in &self.sources {
            items.extend(source.list(query)?);
        }
        items.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        Ok(Projection {
            title: title.into(),
            generated_at: Utc::now(),
            items,
        })
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
            authority: Authority::Repository,
            title: "one".into(),
            state: WorkState::Active,
            priority: Priority::Medium,
            body: String::new(),
            tags: vec!["rust".into()],
            assignee: None,
            relations: vec![],
            refs: vec![],
            revision: None,
            updated_at: None,
            metadata: Value::Null,
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
                authorities: vec![Authority::TaskServer],
                ..Default::default()
            }
            .matches(&item)
        );
    }
}
