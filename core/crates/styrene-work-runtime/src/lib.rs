//! Async source refresh and immutable aggregate snapshots for Styrene work.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use styrene_work_model::{
    Result, SourceId, SourceKind, WorkAuthority, WorkCapabilities, WorkItem, WorkQuery,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkSourceDescriptor {
    pub id: SourceId,
    pub kind: SourceKind,
    pub authority: WorkAuthority,
    pub capabilities: WorkCapabilities,
    pub schema_version: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkWarning {
    pub source_id: SourceId,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    pub descriptor: WorkSourceDescriptor,
    pub observed_at: DateTime<Utc>,
    pub items: Vec<WorkItem>,
    pub stale: bool,
}

#[derive(Clone, Debug)]
pub struct RefreshContext {
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub enum SourceRefresh {
    Current(SourceSnapshot),
    Stale {
        snapshot: SourceSnapshot,
        reason: String,
    },
    Unavailable {
        descriptor: WorkSourceDescriptor,
        reason: String,
    },
    Invalid {
        descriptor: WorkSourceDescriptor,
        reason: String,
    },
}

#[async_trait]
pub trait WorkSource: Send + Sync {
    fn descriptor(&self) -> WorkSourceDescriptor;

    async fn refresh(
        &self,
        previous: Option<&SourceSnapshot>,
        context: &RefreshContext,
    ) -> Result<SourceRefresh>;
}

#[derive(Clone, Debug)]
pub struct WorkSnapshot {
    pub generation: u64,
    pub generated_at: DateTime<Utc>,
    pub items: Arc<[WorkItem]>,
    pub sources: Arc<[SourceSnapshot]>,
    pub warnings: Arc<[WorkWarning]>,
}

impl WorkSnapshot {
    pub fn query(&self, query: &WorkQuery) -> Vec<&WorkItem> {
        self.items
            .iter()
            .filter(|item| query.matches(item))
            .collect()
    }

    pub fn get(&self, id: &styrene_work_model::WorkId) -> Option<&WorkItem> {
        self.items.iter().find(|item| &item.id == id)
    }
}

pub struct WorkRuntime {
    sources: Vec<Arc<dyn WorkSource>>,
    generation: u64,
    snapshot: WorkSnapshot,
}

impl WorkRuntime {
    pub fn new(sources: Vec<Arc<dyn WorkSource>>) -> Self {
        Self {
            sources,
            generation: 0,
            snapshot: WorkSnapshot {
                generation: 0,
                generated_at: Utc::now(),
                items: Arc::from([]),
                sources: Arc::from([]),
                warnings: Arc::from([]),
            },
        }
    }

    pub fn snapshot(&self) -> &WorkSnapshot {
        &self.snapshot
    }

    pub async fn refresh(&mut self) -> Result<&WorkSnapshot> {
        let previous: HashMap<_, _> = self
            .snapshot
            .sources
            .iter()
            .map(|snapshot| (snapshot.descriptor.id.clone(), snapshot))
            .collect();
        let context = RefreshContext { now: Utc::now() };
        let mut snapshots = Vec::new();
        let mut warnings = Vec::new();

        for source in &self.sources {
            let descriptor = source.descriptor();
            match source
                .refresh(previous.get(&descriptor.id).copied(), &context)
                .await?
            {
                SourceRefresh::Current(snapshot) => snapshots.push(snapshot),
                SourceRefresh::Stale {
                    mut snapshot,
                    reason,
                } => {
                    snapshot.stale = true;
                    warnings.push(WorkWarning {
                        source_id: descriptor.id,
                        code: "source_stale".into(),
                        message: reason,
                    });
                    snapshots.push(snapshot);
                }
                SourceRefresh::Unavailable { descriptor, reason } => warnings.push(WorkWarning {
                    source_id: descriptor.id,
                    code: "source_unavailable".into(),
                    message: reason,
                }),
                SourceRefresh::Invalid { descriptor, reason } => warnings.push(WorkWarning {
                    source_id: descriptor.id,
                    code: "source_invalid".into(),
                    message: reason,
                }),
            }
        }

        let mut items: Vec<_> = snapshots
            .iter()
            .flat_map(|snapshot| snapshot.items.iter().cloned())
            .collect();
        items.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        self.generation += 1;
        self.snapshot = WorkSnapshot {
            generation: self.generation,
            generated_at: context.now,
            items: Arc::from(items),
            sources: Arc::from(snapshots),
            warnings: Arc::from(warnings),
        };
        Ok(&self.snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use styrene_work_model::{SourceId, SourceKind, WorkAuthority, WorkCapabilities};

    struct MissingSource;

    #[async_trait]
    impl WorkSource for MissingSource {
        fn descriptor(&self) -> WorkSourceDescriptor {
            WorkSourceDescriptor {
                id: SourceId::new("missing").unwrap(),
                kind: SourceKind::Server,
                authority: WorkAuthority::TaskServer,
                capabilities: WorkCapabilities::default(),
                schema_version: 1,
            }
        }

        async fn refresh(
            &self,
            _previous: Option<&SourceSnapshot>,
            _context: &RefreshContext,
        ) -> Result<SourceRefresh> {
            Ok(SourceRefresh::Unavailable {
                descriptor: self.descriptor(),
                reason: "offline".into(),
            })
        }
    }

    #[tokio::test]
    async fn partial_failure_produces_warning_and_snapshot() {
        let mut runtime = WorkRuntime::new(vec![Arc::new(MissingSource)]);
        let snapshot = runtime.refresh().await.unwrap();
        assert_eq!(snapshot.generation, 1);
        assert!(snapshot.items.is_empty());
        assert_eq!(snapshot.warnings[0].code, "source_unavailable");
    }
}
