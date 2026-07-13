//! Styrene-owned typed relations between otherwise independent work sources.
//!
//! The sidecar is deliberately outside Markplane and OpenSpec formats. It adds
//! graph edges without making either upstream system understand the other.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use styrene_work::{
    RelationKind, Result, WorkError, WorkId, WorkItem, WorkQuery, WorkRelation, WorkSource,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationFile {
    version: u32,
    #[serde(default)]
    relations: Vec<RelationRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationRecord {
    source: String,
    kind: RelationKind,
    target: String,
}

pub struct RelationOverlay<'a> {
    source: &'a dyn WorkSource,
    edges: HashMap<WorkId, Vec<WorkRelation>>,
}

impl<'a> RelationOverlay<'a> {
    pub fn from_path(source: &'a dyn WorkSource, path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        validate_sidecar_path(&path)?;
        if !path.exists() {
            return Ok(Self {
                source,
                edges: HashMap::new(),
            });
        }
        let relation_file: RelationFile = serde_yaml::from_slice(&fs::read(&path)?)
            .map_err(|error| WorkError::Invalid(format!("invalid relation sidecar: {error}")))?;
        if relation_file.version != 1 {
            return Err(WorkError::Invalid(format!(
                "unsupported relation sidecar version {}",
                relation_file.version
            )));
        }
        let mut edges: HashMap<WorkId, Vec<WorkRelation>> = HashMap::new();
        let mut seen = HashSet::new();
        for record in relation_file.relations {
            let source_id = parse_id(&record.source)?;
            let target_id = parse_id(&record.target)?;
            if source_id == target_id {
                return Err(WorkError::Invalid(format!(
                    "self relation is not allowed: {source_id}"
                )));
            }
            let key = (source_id.clone(), record.kind, target_id.clone());
            if !seen.insert(key) {
                return Err(WorkError::Invalid(format!(
                    "duplicate relation: {source_id} {:?} {target_id}",
                    record.kind
                )));
            }
            edges.entry(source_id).or_default().push(WorkRelation {
                kind: record.kind,
                target: target_id,
            });
        }
        Ok(Self { source, edges })
    }

    pub fn validate_targets(&self, known: &[WorkItem]) -> Result<()> {
        let ids: HashSet<_> = known.iter().map(|item| item.id.clone()).collect();
        for (source, relations) in &self.edges {
            if !ids.contains(source) {
                return Err(WorkError::NotFound(format!("relation source {source}")));
            }
            for relation in relations {
                if !ids.contains(&relation.target) {
                    return Err(WorkError::NotFound(format!(
                        "relation target {}",
                        relation.target
                    )));
                }
            }
        }
        Ok(())
    }

    fn apply_edges(&self, item: &mut WorkItem) {
        if let Some(relations) = self.edges.get(&item.id) {
            item.relations.extend(relations.iter().cloned());
        }
    }
}

impl WorkSource for RelationOverlay<'_> {
    fn source_id(&self) -> &'static str {
        "relation-overlay"
    }

    fn list(&self, query: &WorkQuery) -> Result<Vec<WorkItem>> {
        let mut items = self.source.list(query)?;
        for item in &mut items {
            self.apply_edges(item);
        }
        Ok(items)
    }

    fn get(&self, id: &WorkId) -> Result<Option<WorkItem>> {
        let mut item = self.source.get(id)?;
        if let Some(item) = &mut item {
            self.apply_edges(item);
        }
        Ok(item)
    }
}

fn parse_id(value: &str) -> Result<WorkId> {
    let (namespace, id) = value
        .split_once(':')
        .ok_or_else(|| WorkError::Invalid(format!("relation ID is not namespaced: {value}")))?;
    WorkId::new(namespace, id)
}

fn validate_sidecar_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.file_name().is_none()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WorkError::Invalid(format!(
            "unsafe relation sidecar path: {}",
            path.display()
        )));
    }
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(WorkError::Invalid(format!(
            "relation sidecar must not be a symlink: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use styrene_work::{Authority, Priority, WorkKind, WorkState};
    use tempfile::TempDir;

    struct StaticSource(Vec<WorkItem>);

    impl WorkSource for StaticSource {
        fn source_id(&self) -> &'static str {
            "static"
        }

        fn list(&self, query: &WorkQuery) -> Result<Vec<WorkItem>> {
            Ok(self
                .0
                .iter()
                .filter(|item| query.matches(item))
                .cloned()
                .collect())
        }

        fn get(&self, id: &WorkId) -> Result<Option<WorkItem>> {
            Ok(self.0.iter().find(|item| &item.id == id).cloned())
        }
    }

    fn item(id: &str, authority: Authority) -> WorkItem {
        WorkItem {
            id: parse_id(id).unwrap(),
            kind: WorkKind::Task,
            authority,
            title: id.into(),
            state: WorkState::Active,
            priority: Priority::Unspecified,
            body: String::new(),
            tags: vec![],
            assignee: None,
            relations: vec![],
            refs: vec![],
            revision: None,
            updated_at: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn overlays_typed_cross_source_relation() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("relations.yaml");
        fs::write(
            &path,
            "version: 1\nrelations:\n  - source: markplane:TASK-one\n    kind: implements\n    target: openspec:change\n",
        )
        .unwrap();
        let source = StaticSource(vec![
            item("markplane:TASK-one", Authority::Repository),
            item("openspec:change", Authority::OpenSpec),
        ]);
        let overlay = RelationOverlay::from_path(&source, path).unwrap();
        let all = overlay.list(&WorkQuery::default()).unwrap();
        overlay.validate_targets(&all).unwrap();
        assert_eq!(all[0].relations[0].kind, RelationKind::Implements);
        assert_eq!(all[0].relations[0].target.as_str(), "openspec:change");
    }

    #[test]
    fn rejects_missing_targets_and_duplicate_edges() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("relations.yaml");
        let edge =
            "  - source: markplane:TASK-one\n    kind: implements\n    target: openspec:missing\n";
        fs::write(&path, format!("version: 1\nrelations:\n{edge}{edge}")).unwrap();
        let source = StaticSource(vec![item("markplane:TASK-one", Authority::Repository)]);
        assert!(matches!(
            RelationOverlay::from_path(&source, path),
            Err(WorkError::Invalid(_))
        ));
    }
}
