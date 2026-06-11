//! OpenSpec archive transaction helpers.
//!
//! OpenSpec archive moves content on disk while `omegon-opsx` persists the
//! lifecycle state. These helpers keep the transaction marker and recovery
//! policy in the lifecycle domain instead of the tool adapter.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use omegon_opsx::{ChangeState, JsonFileStore, Lifecycle as OpsxLifecycle, OpsxError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSpecArchiveTransaction {
    pub version: u32,
    pub op: String,
    pub change: String,
    pub from_state: String,
    pub to_state: String,
    pub change_dir: String,
    pub archive_dir: String,
    pub phase: String,
}

impl OpenSpecArchiveTransaction {
    pub fn new(repo_path: &Path, change: &str, from_state: ChangeState) -> Self {
        Self {
            version: 1,
            op: "openspec_archive".to_string(),
            change: change.to_string(),
            from_state: from_state.as_str().to_string(),
            to_state: ChangeState::Archived.as_str().to_string(),
            change_dir: repo_path
                .join("openspec/changes")
                .join(change)
                .to_string_lossy()
                .to_string(),
            archive_dir: repo_path
                .join("openspec/archive")
                .join(change)
                .to_string_lossy()
                .to_string(),
            phase: "intent_written".to_string(),
        }
    }
}

pub fn archive_tx_dir(repo_path: &Path) -> PathBuf {
    repo_path.join("ai/lifecycle/transactions")
}

pub fn archive_tx_path(repo_path: &Path, change: &str) -> PathBuf {
    archive_tx_dir(repo_path).join(format!("openspec-archive-{change}.json"))
}

pub fn write_archive_tx(repo_path: &Path, tx: &OpenSpecArchiveTransaction) -> anyhow::Result<()> {
    let path = archive_tx_path(repo_path, &tx.change);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(tx)?;
    std::fs::write(&path, json)?;
    if let Ok(file) = std::fs::OpenOptions::new().read(true).open(&path) {
        let _ = file.sync_all();
    }
    Ok(())
}

pub fn remove_archive_tx(repo_path: &Path, change: &str) -> anyhow::Result<()> {
    let path = archive_tx_path(repo_path, change);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn recover_archive_transactions(
    repo_path: &Path,
    opsx: &Arc<Mutex<OpsxLifecycle<JsonFileStore>>>,
) -> anyhow::Result<Vec<String>> {
    let tx_dir = archive_tx_dir(repo_path);
    let entries = match std::fs::read_dir(&tx_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    let mut recovered = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let tx: OpenSpecArchiveTransaction = serde_json::from_str(&content)?;
        if tx.op != "openspec_archive" {
            continue;
        }
        let change_dir = PathBuf::from(&tx.change_dir);
        let archive_dir = PathBuf::from(&tx.archive_dir);
        let change_exists = change_dir.exists();
        let archive_exists = archive_dir.exists();

        match (change_exists, archive_exists) {
            (true, false) => {
                std::fs::remove_file(&path)?;
                recovered.push(format!(
                    "removed stale archive transaction for '{}'",
                    tx.change
                ));
            }
            (false, true) => {
                {
                    let mut opsx = opsx.lock().unwrap();
                    if opsx_change_state(&opsx, &tx.change) != Some(ChangeState::Archived) {
                        opsx.force_transition_change(
                            &tx.change,
                            ChangeState::Archived,
                            "recovering interrupted OpenSpec archive transaction",
                        )?;
                    }
                }
                std::fs::remove_file(&path)?;
                recovered.push(format!("completed interrupted archive for '{}'", tx.change));
            }
            (true, true) => anyhow::bail!(
                "archive transaction conflict for '{}': both {} and {} exist",
                tx.change,
                change_dir.display(),
                archive_dir.display()
            ),
            (false, false) => anyhow::bail!(
                "archive transaction conflict for '{}': neither {} nor {} exists",
                tx.change,
                change_dir.display(),
                archive_dir.display()
            ),
        }
    }
    Ok(recovered)
}

pub fn archive_content_with_tx(
    repo_path: &Path,
    change: &str,
    from_state: ChangeState,
) -> Result<(), OpsxError> {
    let change_dir = repo_path.join("openspec/changes").join(change);
    let archive_dir = repo_path.join("openspec/archive").join(change);
    let archive_parent = repo_path.join("openspec/archive");
    let mut tx = OpenSpecArchiveTransaction::new(repo_path, change, from_state);
    write_archive_tx(repo_path, &tx)
        .map_err(|err| OpsxError::StoreError(format!("write archive transaction: {err}")))?;
    std::fs::create_dir_all(&archive_parent)
        .map_err(|err| opsx_store_error(&format!("mkdir {}", archive_parent.display()), err))?;
    std::fs::rename(&change_dir, &archive_dir).map_err(|err| {
        opsx_store_error(
            &format!(
                "archive {} to {}",
                change_dir.display(),
                archive_dir.display()
            ),
            err,
        )
    })?;
    tx.phase = "content_moved".to_string();
    write_archive_tx(repo_path, &tx)
        .map_err(|err| OpsxError::StoreError(format!("write archive transaction: {err}")))
}

pub fn rollback_archive_content(repo_path: &Path, change: &str) -> Result<(), OpsxError> {
    let change_dir = repo_path.join("openspec/changes").join(change);
    let archive_dir = repo_path.join("openspec/archive").join(change);
    if archive_dir.exists() && !change_dir.exists() {
        std::fs::rename(&archive_dir, &change_dir).map_err(|err| {
            opsx_store_error(
                &format!(
                    "rollback archive {} to {}",
                    archive_dir.display(),
                    change_dir.display()
                ),
                err,
            )
        })?;
    }
    remove_archive_tx(repo_path, change).map_err(|err| {
        OpsxError::StoreError(format!("remove archive transaction after rollback: {err}"))
    })?;
    Ok(())
}

fn opsx_change_state(opsx: &OpsxLifecycle<JsonFileStore>, name: &str) -> Option<ChangeState> {
    opsx.state()
        .changes
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.state)
}

fn opsx_store_error(context: &str, err: std::io::Error) -> OpsxError {
    OpsxError::StoreError(format!("{context}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opsx_for(
        repo: &Path,
        change: &str,
        state: ChangeState,
    ) -> Arc<Mutex<OpsxLifecycle<JsonFileStore>>> {
        let mut opsx = OpsxLifecycle::load(JsonFileStore::new(repo)).unwrap();
        opsx.create_change(change, change, None).unwrap();
        if state != ChangeState::Proposed {
            opsx.force_transition_change(change, state, "test setup")
                .unwrap();
        }
        Arc::new(Mutex::new(opsx))
    }

    #[test]
    fn recovery_removes_stale_intent_when_change_still_active() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let change = "demo";
        std::fs::create_dir_all(repo.join("openspec/changes").join(change)).unwrap();
        let tx = OpenSpecArchiveTransaction::new(repo, change, ChangeState::Verifying);
        write_archive_tx(repo, &tx).unwrap();
        let opsx = opsx_for(repo, change, ChangeState::Verifying);

        let recovered = recover_archive_transactions(repo, &opsx).unwrap();

        assert_eq!(
            recovered,
            vec!["removed stale archive transaction for 'demo'"]
        );
        assert!(!archive_tx_path(repo, change).exists());
        assert_eq!(
            opsx_change_state(&opsx.lock().unwrap(), change),
            Some(ChangeState::Verifying)
        );
    }

    #[test]
    fn recovery_completes_state_when_content_was_moved() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let change = "demo";
        std::fs::create_dir_all(repo.join("openspec/archive").join(change)).unwrap();
        let mut tx = OpenSpecArchiveTransaction::new(repo, change, ChangeState::Verifying);
        tx.phase = "content_moved".to_string();
        write_archive_tx(repo, &tx).unwrap();
        let opsx = opsx_for(repo, change, ChangeState::Verifying);

        let recovered = recover_archive_transactions(repo, &opsx).unwrap();

        assert_eq!(recovered, vec!["completed interrupted archive for 'demo'"]);
        assert!(!archive_tx_path(repo, change).exists());
        assert_eq!(
            opsx_change_state(&opsx.lock().unwrap(), change),
            Some(ChangeState::Archived)
        );
    }

    #[test]
    fn recovery_refuses_when_both_change_and_archive_exist() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let change = "demo";
        std::fs::create_dir_all(repo.join("openspec/changes").join(change)).unwrap();
        std::fs::create_dir_all(repo.join("openspec/archive").join(change)).unwrap();
        let tx = OpenSpecArchiveTransaction::new(repo, change, ChangeState::Verifying);
        write_archive_tx(repo, &tx).unwrap();
        let opsx = opsx_for(repo, change, ChangeState::Verifying);

        let err = recover_archive_transactions(repo, &opsx)
            .unwrap_err()
            .to_string();

        assert!(err.contains("both"));
        assert!(archive_tx_path(repo, change).exists());
    }

    #[test]
    fn recovery_refuses_when_neither_change_nor_archive_exist() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let change = "demo";
        let tx = OpenSpecArchiveTransaction::new(repo, change, ChangeState::Verifying);
        write_archive_tx(repo, &tx).unwrap();
        let opsx = opsx_for(repo, change, ChangeState::Verifying);

        let err = recover_archive_transactions(repo, &opsx)
            .unwrap_err()
            .to_string();

        assert!(err.contains("neither"));
        assert!(archive_tx_path(repo, change).exists());
    }
}
