//! Audit log — append-only record of guard decisions.

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::guards::GuardDecision;

pub struct AuditLog {
    path: PathBuf,
}

#[derive(Serialize)]
struct AuditEntry {
    timestamp: String,
    tool: String,
    decision: String,
    reason: String,
    path: String,
}

impl AuditLog {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join("secrets-audit.jsonl"),
        }
    }

    /// Log a guard decision. Failures are silently ignored (audit is best-effort).
    pub fn log_guard(&self, tool_name: &str, _args: &Value, decision: &GuardDecision) {
        let (decision_str, reason, path) = match decision {
            GuardDecision::Block { reason, path } => ("block", reason.as_str(), path.as_str()),
            GuardDecision::Warn { reason, path } => ("warn", reason.as_str(), path.as_str()),
        };

        let entry = AuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            tool: tool_name.to_string(),
            decision: decision_str.to_string(),
            reason: reason.to_string(),
            path: path.to_string(),
        };

        if let Ok(line) = serde_json::to_string(&entry) {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
            {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}
