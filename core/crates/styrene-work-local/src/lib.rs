//! Repository-local work adapter backed by `markplane-core`.
//!
//! Only this crate knows Markplane's model. Consumers see `styrene_work` types.

use chrono::{DateTime, NaiveDate, Utc};
use markplane_core::{
    Epic, EpicStatus, IdPrefix, MarkplaneDocument, Note, NoteStatus, Plan, PlanStatus,
    Project as MarkplaneProject, QueryFilter, ScanScope, StatusCategory, Task, TaskUpdate,
};
use serde_json::json;
use styrene_work::{
    Authority, ExternalRef, MutableWorkSource, Priority, RelationKind, Result, WorkCommand,
    WorkError, WorkId, WorkItem, WorkKind, WorkQuery, WorkRelation, WorkSource, WorkState,
};

pub struct MarkplaneLocalSource {
    project: MarkplaneProject,
}

impl MarkplaneLocalSource {
    pub fn new(project: MarkplaneProject) -> Self {
        Self { project }
    }

    pub fn open(markplane_root: impl Into<std::path::PathBuf>) -> Self {
        Self::new(MarkplaneProject::new(markplane_root.into()))
    }

    pub fn project(&self) -> &MarkplaneProject {
        &self.project
    }

    fn map_error(error: markplane_core::MarkplaneError) -> WorkError {
        WorkError::Source(format!("markplane: {error}"))
    }

    fn id(raw: &str) -> Result<WorkId> {
        WorkId::new("markplane", raw)
    }

    fn raw_id(id: &WorkId) -> Option<&str> {
        let (namespace, value) = id.split();
        (namespace == "markplane").then_some(value)
    }

    fn date(date: NaiveDate) -> Option<DateTime<Utc>> {
        date.and_hms_opt(0, 0, 0)
            .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
    }

    fn priority(priority: &markplane_core::Priority) -> Priority {
        match priority {
            markplane_core::Priority::Critical => Priority::Critical,
            markplane_core::Priority::High => Priority::High,
            markplane_core::Priority::Medium => Priority::Medium,
            markplane_core::Priority::Low => Priority::Low,
            markplane_core::Priority::Someday => Priority::Someday,
        }
    }

    fn task_state(&self, task: &Task) -> WorkState {
        let Ok(config) = self.project.load_config() else {
            return WorkState::Unknown;
        };
        match config.task_category(&task.status) {
            Some(StatusCategory::Draft) => WorkState::Draft,
            Some(StatusCategory::Backlog) => WorkState::Backlog,
            Some(StatusCategory::Planned) => WorkState::Planned,
            Some(StatusCategory::Active) => WorkState::Active,
            Some(StatusCategory::Completed) => WorkState::Completed,
            Some(StatusCategory::Cancelled) => WorkState::Cancelled,
            None => WorkState::Unknown,
        }
    }

    fn task(&self, doc: MarkplaneDocument<Task>, archived: bool) -> Result<WorkItem> {
        let task = doc.frontmatter;
        let mut relations = Vec::new();
        if let Some(epic) = &task.epic {
            relations.push(WorkRelation {
                kind: RelationKind::Contains,
                target: Self::id(epic)?,
            });
        }
        if let Some(plan) = &task.plan {
            relations.push(WorkRelation {
                kind: RelationKind::Specifies,
                target: Self::id(plan)?,
            });
        }
        for target in &task.depends_on {
            relations.push(WorkRelation {
                kind: RelationKind::DependsOn,
                target: Self::id(target)?,
            });
        }
        for target in &task.blocks {
            relations.push(WorkRelation {
                kind: RelationKind::Blocks,
                target: Self::id(target)?,
            });
        }
        for target in &task.related {
            relations.push(WorkRelation {
                kind: RelationKind::Related,
                target: Self::id(target)?,
            });
        }
        let state = if archived {
            WorkState::Archived
        } else {
            self.task_state(&task)
        };
        Ok(WorkItem {
            id: Self::id(&task.id)?,
            kind: WorkKind::Task,
            authority: Authority::Repository,
            title: task.title,
            state,
            priority: Self::priority(&task.priority),
            body: doc.body,
            tags: task.tags,
            assignee: task.assignee,
            relations,
            refs: vec![ExternalRef::new(
                "markplane-file",
                self.project
                    .item_path(&task.id)
                    .map_err(Self::map_error)?
                    .strip_prefix(self.project.root())
                    .map_err(|_| {
                        WorkError::Invalid(format!(
                            "Markplane item {} escaped project root",
                            task.id
                        ))
                    })?
                    .display()
                    .to_string(),
            )],
            revision: None,
            updated_at: Self::date(task.updated),
            metadata: json!({
                "markplane": {
                    "type": task.item_type,
                    "effort": task.effort.to_string(),
                    "position": task.position,
                    "status": task.status,
                }
            }),
        })
    }

    fn epic(&self, doc: MarkplaneDocument<Epic>, archived: bool) -> Result<WorkItem> {
        let epic = doc.frontmatter;
        let state = if archived {
            WorkState::Archived
        } else {
            match epic.status {
                EpicStatus::Now => WorkState::Active,
                EpicStatus::Next => WorkState::Planned,
                EpicStatus::Later => WorkState::Backlog,
                EpicStatus::Done => WorkState::Completed,
            }
        };
        Ok(WorkItem {
            id: Self::id(&epic.id)?,
            kind: WorkKind::Initiative,
            authority: Authority::Repository,
            title: epic.title,
            state,
            priority: Self::priority(&epic.priority),
            body: doc.body,
            tags: epic.tags,
            assignee: None,
            relations: epic
                .related
                .iter()
                .map(|target| {
                    Ok(WorkRelation {
                        kind: RelationKind::Related,
                        target: Self::id(target)?,
                    })
                })
                .collect::<Result<_>>()?,
            refs: vec![],
            revision: None,
            updated_at: Self::date(epic.updated),
            metadata: json!({ "markplane": { "target": epic.target, "started": epic.started } }),
        })
    }

    fn plan(&self, doc: MarkplaneDocument<Plan>, archived: bool) -> Result<WorkItem> {
        let plan = doc.frontmatter;
        let state = if archived {
            WorkState::Archived
        } else {
            match plan.status {
                PlanStatus::Draft => WorkState::Draft,
                PlanStatus::Approved => WorkState::Planned,
                PlanStatus::InProgress => WorkState::Active,
                PlanStatus::Done => WorkState::Completed,
            }
        };
        Ok(WorkItem {
            id: Self::id(&plan.id)?,
            kind: WorkKind::Change,
            authority: Authority::Repository,
            title: plan.title,
            state,
            priority: Priority::Unspecified,
            body: doc.body,
            tags: vec!["markplane-plan".into()],
            assignee: None,
            relations: plan
                .implements
                .iter()
                .map(|target| {
                    Ok(WorkRelation {
                        kind: RelationKind::Implements,
                        target: Self::id(target)?,
                    })
                })
                .collect::<Result<_>>()?,
            refs: vec![],
            revision: None,
            updated_at: Self::date(plan.updated),
            metadata: json!({ "projection_only": true }),
        })
    }

    fn note(&self, doc: MarkplaneDocument<Note>, archived: bool) -> Result<WorkItem> {
        let note = doc.frontmatter;
        let state = if archived {
            WorkState::Archived
        } else {
            match note.status {
                NoteStatus::Draft => WorkState::Draft,
                NoteStatus::Active => WorkState::Active,
                NoteStatus::Archived => WorkState::Archived,
            }
        };
        Ok(WorkItem {
            id: Self::id(&note.id)?,
            kind: WorkKind::Note,
            authority: Authority::Repository,
            title: note.title,
            state,
            priority: Priority::Unspecified,
            body: doc.body,
            tags: note.tags,
            assignee: None,
            relations: vec![],
            refs: vec![],
            revision: None,
            updated_at: Self::date(note.updated),
            metadata: json!({ "markplane": { "type": note.note_type } }),
        })
    }

    fn item(&self, raw: &str) -> Result<Option<WorkItem>> {
        let Ok((prefix, _)) = markplane_core::parse_id(raw) else {
            return Ok(None);
        };
        let archived = self.project.is_archived(raw).map_err(Self::map_error)?;
        let item = match prefix {
            IdPrefix::Task => self.task(
                self.project
                    .read_item::<Task>(raw)
                    .map_err(Self::map_error)?,
                archived,
            )?,
            IdPrefix::Epic => self.epic(
                self.project
                    .read_item::<Epic>(raw)
                    .map_err(Self::map_error)?,
                archived,
            )?,
            IdPrefix::Plan => self.plan(
                self.project
                    .read_item::<Plan>(raw)
                    .map_err(Self::map_error)?,
                archived,
            )?,
            IdPrefix::Note => self.note(
                self.project
                    .read_item::<Note>(raw)
                    .map_err(Self::map_error)?,
                archived,
            )?,
        };
        Ok(Some(item))
    }

    fn status_for_state(&self, state: WorkState) -> Result<String> {
        let config = self.project.load_config().map_err(Self::map_error)?;
        let category = match state {
            WorkState::Draft => StatusCategory::Draft,
            WorkState::Backlog => StatusCategory::Backlog,
            WorkState::Planned => StatusCategory::Planned,
            WorkState::Active => StatusCategory::Active,
            WorkState::Completed => StatusCategory::Completed,
            WorkState::Cancelled => StatusCategory::Cancelled,
            other => {
                return Err(WorkError::Rejected(format!(
                    "Markplane cannot represent state {other:?}"
                )));
            }
        };
        config
            .workflows
            .task
            .statuses_in(category)
            .first()
            .cloned()
            .ok_or_else(|| WorkError::Rejected(format!("workflow has no {category} status")))
    }
}

impl WorkSource for MarkplaneLocalSource {
    fn source_id(&self) -> &'static str {
        "markplane-local"
    }

    fn list(&self, query: &WorkQuery) -> Result<Vec<WorkItem>> {
        let mut items = Vec::new();
        for doc in self
            .project
            .list_tasks(&QueryFilter {
                scope: ScanScope::All,
                ..Default::default()
            })
            .map_err(Self::map_error)?
        {
            let archived = self
                .project
                .is_archived(&doc.frontmatter.id)
                .map_err(Self::map_error)?;
            items.push(self.task(doc, archived)?);
        }
        for doc in self.project.list_epics().map_err(Self::map_error)? {
            items.push(self.epic(doc, false)?);
        }
        for doc in self.project.list_plans().map_err(Self::map_error)? {
            items.push(self.plan(doc, false)?);
        }
        for doc in self.project.list_notes().map_err(Self::map_error)? {
            items.push(self.note(doc, false)?);
        }
        items.retain(|item| query.matches(item));
        Ok(items)
    }

    fn get(&self, id: &WorkId) -> Result<Option<WorkItem>> {
        let Some(raw) = Self::raw_id(id) else {
            return Ok(None);
        };
        match self.item(raw) {
            Ok(item) => Ok(item),
            Err(WorkError::Source(message)) if message.contains("not found") => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl MutableWorkSource for MarkplaneLocalSource {
    fn apply(&self, command: &WorkCommand) -> Result<WorkItem> {
        let id = match command {
            WorkCommand::SetState { id, .. } | WorkCommand::SetAssignee { id, .. } => id,
        };
        let raw = Self::raw_id(id)
            .ok_or_else(|| WorkError::Rejected(format!("{id} is not a Markplane item")))?;
        let (prefix, _) = markplane_core::parse_id(raw).map_err(Self::map_error)?;
        if prefix != IdPrefix::Task {
            return Err(WorkError::Rejected(
                "the spike mutates only Markplane tasks".into(),
            ));
        }
        match command {
            WorkCommand::SetState { state, .. } => {
                let status = self.status_for_state(*state)?;
                self.project
                    .update_status(raw, &status)
                    .map_err(Self::map_error)?;
            }
            WorkCommand::SetAssignee { assignee, .. } => {
                let update = TaskUpdate {
                    assignee: match assignee {
                        Some(value) => markplane_core::Patch::Set(value.clone()),
                        None => markplane_core::Patch::Clear,
                    },
                    ..Default::default()
                };
                self.project
                    .update_task(raw, &update)
                    .map_err(Self::map_error)?;
            }
        }
        self.item(raw)?
            .ok_or_else(|| WorkError::NotFound(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use markplane_core::{Effort, Priority as MarkplanePriority};
    use tempfile::TempDir;

    fn source() -> (TempDir, MarkplaneLocalSource, WorkId) {
        let temp = TempDir::new().unwrap();
        let project =
            MarkplaneProject::init(temp.path().join(".markplane"), "Spike", "test").unwrap();
        let task = project
            .create_task(
                "Local work",
                "feature",
                MarkplanePriority::High,
                Effort::Small,
                None,
                vec!["spike".into()],
                None,
            )
            .unwrap();
        let id = MarkplaneLocalSource::id(&task.id).unwrap();
        (temp, MarkplaneLocalSource::new(project), id)
    }

    #[test]
    fn translates_markplane_without_leaking_its_types() {
        let (_temp, source, id) = source();
        let item = source.get(&id).unwrap().unwrap();
        assert_eq!(item.authority, Authority::Repository);
        assert_eq!(item.kind, WorkKind::Task);
        assert_eq!(item.priority, Priority::High);
        assert_eq!(item.tags, vec!["spike"]);
        assert_eq!(
            item.refs[0].locator,
            "backlog/items/".to_string() + id.split().1 + ".md"
        );
        assert!(!item.refs[0].locator.starts_with('/'));
    }

    #[test]
    fn applies_styrene_commands_through_the_adapter() {
        let (_temp, source, id) = source();
        let updated = source
            .apply(&WorkCommand::SetState {
                id: id.clone(),
                state: WorkState::Active,
                expected_revision: None,
            })
            .unwrap();
        assert_eq!(updated.state, WorkState::Active);
        let assigned = source
            .apply(&WorkCommand::SetAssignee {
                id,
                assignee: Some("agent:omegon".into()),
                expected_revision: None,
            })
            .unwrap();
        assert_eq!(assigned.assignee.as_deref(), Some("agent:omegon"));
    }
}
