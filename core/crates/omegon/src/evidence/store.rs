use super::schema::{ClaimRecord, EvidenceEdge, EvidenceRecord};
use super::support::ClaimSupportSummary;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EvidenceStore {
    root: PathBuf,
    claims: Vec<ClaimRecord>,
    records: Vec<EvidenceRecord>,
    edges: Vec<EvidenceEdge>,
}

impl EvidenceStore {
    pub fn load(project_root: &Path) -> Result<Self> {
        let root = project_root.join(".omegon/evidence");
        Ok(Self {
            claims: read_jsonl(&root.join("claims.jsonl"))?,
            records: read_jsonl(&root.join("records.jsonl"))?,
            edges: read_jsonl(&root.join("edges.jsonl"))?,
            root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn claim(&self, id: &str) -> Option<&ClaimRecord> {
        self.claims.iter().find(|claim| claim.id == id)
    }

    pub fn support_summary(&self, claim_id: &str) -> ClaimSupportSummary {
        let by_id: HashMap<&str, &EvidenceRecord> = self
            .records
            .iter()
            .map(|record| (record.id.as_str(), record))
            .collect();
        let mut summary = ClaimSupportSummary::new(claim_id.to_string());
        for edge in self.edges.iter().filter(|edge| edge.to_id == claim_id) {
            let Some(record) = by_id.get(edge.from_id.as_str()).copied() else {
                continue;
            };
            match edge.kind.as_str() {
                "supports" => summary.supports.push(record.clone()),
                "refutes" => summary.refutes.push(record.clone()),
                "stale_against" | "stale" => summary.stale.push(record.clone()),
                "supersedes" => summary.supersedes.push(record.clone()),
                _ => {}
            }
        }
        summary.finalize(self.claim(claim_id).is_some())
    }

    pub fn records_for_subject(&self, subject: &str) -> Vec<&EvidenceRecord> {
        self.records
            .iter()
            .filter(|record| record.subjects.iter().any(|s| s == subject))
            .collect()
    }

    pub fn neighbors(&self, id: &str) -> EvidenceNeighborhood {
        EvidenceNeighborhood {
            id: id.to_string(),
            outgoing: self
                .edges
                .iter()
                .filter(|edge| edge.from_id == id)
                .cloned()
                .collect(),
            incoming: self
                .edges
                .iter()
                .filter(|edge| edge.to_id == id)
                .cloned()
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceNeighborhood {
    pub id: String,
    pub outgoing: Vec<EvidenceEdge>,
    pub incoming: Vec<EvidenceEdge>,
}

fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut rows = Vec::new();
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse {} line {} as JSONL",
                path.display(),
                idx + 1
            )
        })?);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::support::ClaimSupportStatus;
    use serde_json::json;
    use tempfile::tempdir;

    fn write_fixture(root: &Path, edge_kind: &str) {
        let dir = root.join(".omegon/evidence");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("claims.jsonl"),
            json!({
                "schema": "claim-record/v1",
                "id": "claim:test",
                "kind": "test",
                "text": "test claim",
                "status": "asserted",
                "scope": [],
                "created_at_ms": 1,
                "metadata": {}
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("records.jsonl"),
            json!({
                "schema": "evidence-record/v1",
                "id": "evidence:test",
                "provider": "test",
                "kind": "test",
                "status": "ok",
                "subjects": ["subject:test"],
                "claims": ["claim:test"],
                "artifacts": [],
                "source_state": {},
                "created_at_ms": 1,
                "metadata": {}
            })
            .to_string()
                + "\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("edges.jsonl"),
            json!({
                "schema": "evidence-edge/v1",
                "from": "evidence:test",
                "to": "claim:test",
                "kind": edge_kind,
                "created_at_ms": 1
            })
            .to_string()
                + "\n",
        )
        .unwrap();
    }

    #[test]
    fn summarizes_supported_claim() {
        let dir = tempdir().unwrap();
        write_fixture(dir.path(), "supports");
        let store = EvidenceStore::load(dir.path()).unwrap();
        let summary = store.support_summary("claim:test");
        assert_eq!(summary.status, ClaimSupportStatus::Supported);
        assert_eq!(summary.supports.len(), 1);
        assert_eq!(store.records_for_subject("subject:test").len(), 1);
    }

    #[test]
    fn summarizes_refuted_claim() {
        let dir = tempdir().unwrap();
        write_fixture(dir.path(), "refutes");
        let store = EvidenceStore::load(dir.path()).unwrap();
        let summary = store.support_summary("claim:test");
        assert_eq!(summary.status, ClaimSupportStatus::Refuted);
        assert_eq!(summary.refutes.len(), 1);
        assert_eq!(store.neighbors("claim:test").incoming.len(), 1);
    }

    #[test]
    fn missing_claim_is_unknown() {
        let dir = tempdir().unwrap();
        let store = EvidenceStore::load(dir.path()).unwrap();
        assert_eq!(
            store.support_summary("claim:missing").status,
            ClaimSupportStatus::Unknown
        );
    }
}
