//! Synchronization between git-native lifecycle artifacts and `omegon-opsx` FSM state.
//!
//! Markdown/OpenSpec files remain the content store. The FSM records lifecycle
//! state for gates and projections. This module names the reconciliation seam
//! so feature adapters do not own disk-to-FSM policy.

use std::path::Path;

use omegon_opsx::{ChangeState, JsonFileStore, Lifecycle as OpsxLifecycle};

use super::spec;
use super::types::ChangeInfo;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub changes_seen: usize,
    pub changes_created: usize,
    pub specs_registered: usize,
    pub progress_updates: usize,
    pub transitions: Vec<SyncTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTransition {
    pub change: String,
    pub from: String,
    pub to: String,
}

pub fn sync_change_from_info(
    opsx: &mut OpsxLifecycle<JsonFileStore>,
    change: &ChangeInfo,
) -> anyhow::Result<SyncReport> {
    let mut report = SyncReport {
        changes_seen: 1,
        ..SyncReport::default()
    };

    if !opsx.state().changes.iter().any(|c| c.name == change.name) {
        opsx.create_change(&change.name, &change.name, None)?;
        report.changes_created += 1;
    }

    for spec in &change.specs {
        opsx.add_spec(&change.name, &spec.domain)?;
        report.specs_registered += 1;
    }

    opsx.update_change_progress(&change.name, change.total_tasks, change.done_tasks)?;
    report.progress_updates += 1;

    transition_if(
        opsx,
        &change.name,
        ChangeState::Proposed,
        ChangeState::Specced,
        !change.specs.is_empty(),
        &mut report,
    )?;
    transition_if(
        opsx,
        &change.name,
        ChangeState::Specced,
        ChangeState::Planned,
        change.total_tasks > 0,
        &mut report,
    )?;

    Ok(report)
}

pub fn sync_changes_from_info(
    opsx: &mut OpsxLifecycle<JsonFileStore>,
    changes: &[ChangeInfo],
) -> anyhow::Result<SyncReport> {
    let mut combined = SyncReport::default();
    for change in changes {
        combined.merge(sync_change_from_info(opsx, change)?);
    }
    Ok(combined)
}

pub fn sync_change_by_name(
    opsx: &mut OpsxLifecycle<JsonFileStore>,
    repo_path: &Path,
    name: &str,
) -> anyhow::Result<(ChangeInfo, SyncReport)> {
    let change = spec::get_change(repo_path, name)
        .ok_or_else(|| anyhow::anyhow!("Change '{name}' not found"))?;
    let report = sync_change_from_info(opsx, &change)?;
    Ok((change, report))
}

pub fn change_state(opsx: &OpsxLifecycle<JsonFileStore>, name: &str) -> Option<ChangeState> {
    opsx.state()
        .changes
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.state)
}

pub fn transition_change_if(
    opsx: &mut OpsxLifecycle<JsonFileStore>,
    name: &str,
    from: ChangeState,
    to: ChangeState,
) -> anyhow::Result<bool> {
    if change_state(opsx, name) == Some(from) {
        opsx.transition_change(name, to)?;
        return Ok(true);
    }
    Ok(false)
}

fn transition_if(
    opsx: &mut OpsxLifecycle<JsonFileStore>,
    name: &str,
    from: ChangeState,
    to: ChangeState,
    condition: bool,
    report: &mut SyncReport,
) -> anyhow::Result<()> {
    if condition && transition_change_if(opsx, name, from, to)? {
        report.transitions.push(SyncTransition {
            change: name.to_string(),
            from: from.as_str().to_string(),
            to: to.as_str().to_string(),
        });
    }
    Ok(())
}

impl SyncReport {
    pub fn merge(&mut self, other: SyncReport) {
        self.changes_seen += other.changes_seen;
        self.changes_created += other.changes_created;
        self.specs_registered += other.specs_registered;
        self.progress_updates += other.progress_updates;
        self.transitions.extend(other.transitions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::types::{ChangeStage, SpecFile};

    fn change(
        name: &str,
        specs: Vec<SpecFile>,
        total_tasks: usize,
        done_tasks: usize,
    ) -> ChangeInfo {
        ChangeInfo {
            name: name.to_string(),
            path: std::path::PathBuf::from("openspec/changes").join(name),
            stage: ChangeStage::Proposed,
            has_proposal: true,
            has_design: false,
            has_specs: !specs.is_empty(),
            has_tasks: total_tasks > 0,
            total_tasks,
            done_tasks,
            task_groups: vec![],
            specs,
        }
    }

    fn spec(domain: &str) -> SpecFile {
        SpecFile {
            domain: domain.to_string(),
            file_path: std::path::PathBuf::from(format!("specs/{domain}.md")),
            requirements: vec![],
        }
    }

    #[test]
    fn sync_creates_change_and_advances_to_specced() {
        let dir = tempfile::tempdir().unwrap();
        let mut opsx = OpsxLifecycle::load(JsonFileStore::new(dir.path())).unwrap();
        let report =
            sync_change_from_info(&mut opsx, &change("demo", vec![spec("demo")], 0, 0)).unwrap();

        assert_eq!(report.changes_seen, 1);
        assert_eq!(report.changes_created, 1);
        assert_eq!(report.specs_registered, 1);
        assert_eq!(change_state(&opsx, "demo"), Some(ChangeState::Specced));
        assert_eq!(report.transitions.len(), 1);
    }

    #[test]
    fn sync_advances_specced_change_to_planned_when_tasks_exist() {
        let dir = tempfile::tempdir().unwrap();
        let mut opsx = OpsxLifecycle::load(JsonFileStore::new(dir.path())).unwrap();
        let report =
            sync_change_from_info(&mut opsx, &change("demo", vec![spec("demo")], 3, 1)).unwrap();

        assert_eq!(change_state(&opsx, "demo"), Some(ChangeState::Planned));
        assert_eq!(report.transitions.len(), 2);
        assert_eq!(report.progress_updates, 1);
    }
}
