//! Streaming tool responses — progress with content blocks.
//!
//! During `tools/call` execution, extensions can emit progress notifications
//! that carry typed content blocks (not just metadata). This enables:
//!
//! - Streaming text output as it's generated
//! - Partial markdown renders
//! - Image generation previews
//! - Widget updates mid-execution
//!
//! # MCP shim behavior
//!
//! The MCP shim drops `content` blocks from progress notifications and
//! maps `phase` → `message`. Only numeric `progress`/`total` is forwarded.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::HostProxy;

/// Content block within a progress notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressContent {
    /// Plain text content.
    #[serde(rename = "text")]
    Text { text: String },

    /// Markdown content.
    #[serde(rename = "markdown")]
    Markdown { text: String },
}

/// Parameters for `notifications/tools/progress` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressParams {
    /// Progress token assigned by the host in `tools/call` `_meta`.
    pub progress_token: String,

    /// Current progress value.
    pub progress: u64,

    /// Total expected (0 if unknown).
    #[serde(default)]
    pub total: u64,

    /// Human-readable phase description (e.g. "scanning files").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,

    /// Content blocks — Omegon-specific, dropped by MCP shim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ProgressContent>>,
}

/// Reporter for streaming progress from within a tool execution.
///
/// Created by the SDK when a `tools/call` includes a `_meta.progress_token`.
/// The extension uses this to emit progress notifications and check for
/// cancellation.
#[derive(Clone)]
pub struct ProgressReporter {
    host: HostProxy,
    progress_token: String,
    cancelled: Arc<AtomicBool>,
}

impl ProgressReporter {
    /// Create a new progress reporter.
    pub fn new(host: HostProxy, progress_token: String) -> Self {
        Self {
            host,
            progress_token,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Emit a progress notification with optional content.
    pub async fn report(
        &self,
        progress: u64,
        total: u64,
        phase: Option<&str>,
        content: Option<Vec<ProgressContent>>,
    ) -> crate::Result<()> {
        let params = ToolProgressParams {
            progress_token: self.progress_token.clone(),
            progress,
            total,
            phase: phase.map(String::from),
            content,
        };
        self.host
            .notify(
                "notifications/tools/progress",
                serde_json::to_value(&params)?,
            )
            .await
    }

    /// Emit a simple text progress update.
    pub async fn report_text(
        &self,
        progress: u64,
        total: u64,
        phase: &str,
        text: &str,
    ) -> crate::Result<()> {
        self.report(
            progress,
            total,
            Some(phase),
            Some(vec![ProgressContent::Text {
                text: text.to_string(),
            }]),
        )
        .await
    }

    /// Emit a widget update mid-execution.
    pub async fn update_widget(&self, widget_id: &str, data: Value) -> crate::Result<()> {
        self.host
            .notify(
                "notifications/widgets/updated",
                serde_json::json!({
                    "widget_id": widget_id,
                    "data": data,
                }),
            )
            .await
    }

    /// Check if this request has been cancelled by the host.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Mark this request as cancelled (called by the message router).
    #[allow(dead_code, reason = "wired when cancellation routing is implemented")]
    pub(crate) fn mark_cancelled(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Get the progress token.
    pub fn progress_token(&self) -> &str {
        &self.progress_token
    }

    /// Get a handle to the cancellation flag (for the router to set).
    #[allow(dead_code, reason = "wired when cancellation routing is implemented")]
    pub(crate) fn cancelled_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}

/// Parameters for `notifications/cancelled` notification (host → ext).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledParams {
    /// The request ID that was cancelled.
    pub request_id: Value,

    /// Reason for cancellation.
    #[serde(default = "default_reason")]
    pub reason: String,
}

fn default_reason() -> String {
    "user_cancelled".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::{Mutex, mpsc};

    fn make_host_proxy() -> (HostProxy, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(16);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = HostProxy::new(tx, pending);
        (proxy, rx)
    }

    #[test]
    fn test_progress_params_roundtrip() {
        let params = ToolProgressParams {
            progress_token: "p-42".to_string(),
            progress: 30,
            total: 100,
            phase: Some("scanning files".to_string()),
            content: Some(vec![ProgressContent::Text {
                text: "Found 247 Rust files...".to_string(),
            }]),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: ToolProgressParams = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.progress_token, "p-42");
        assert_eq!(parsed.progress, 30);
        assert_eq!(parsed.total, 100);
        assert_eq!(parsed.phase.as_deref(), Some("scanning files"));
        assert!(parsed.content.is_some());
    }

    #[test]
    fn test_progress_params_minimal() {
        let json = r#"{"progress_token":"p-1","progress":50}"#;
        let parsed: ToolProgressParams = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.progress, 50);
        assert_eq!(parsed.total, 0);
        assert!(parsed.phase.is_none());
        assert!(parsed.content.is_none());
    }

    #[test]
    fn test_progress_content_text() {
        let content = ProgressContent::Text {
            text: "Processing...".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"text"#));
    }

    #[test]
    fn test_progress_content_markdown() {
        let content = ProgressContent::Markdown {
            text: "# Results\n- Item 1".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"markdown"#));
    }

    #[test]
    fn test_cancelled_params_roundtrip() {
        let params = CancelledParams {
            request_id: serde_json::json!(42),
            reason: "user_cancelled".to_string(),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: CancelledParams = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.request_id, 42);
        assert_eq!(parsed.reason, "user_cancelled");
    }

    #[test]
    fn test_cancellation_flag() {
        let (proxy, _rx) = make_host_proxy();
        let reporter = ProgressReporter::new(proxy, "p-1".to_string());

        assert!(!reporter.is_cancelled());

        reporter.mark_cancelled();

        assert!(reporter.is_cancelled());
    }

    #[test]
    fn test_cancellation_flag_shared() {
        let (proxy, _rx) = make_host_proxy();
        let reporter = ProgressReporter::new(proxy, "p-1".to_string());
        let flag = reporter.cancelled_flag();

        assert!(!reporter.is_cancelled());

        // Simulate router setting the flag
        flag.store(true, Ordering::Relaxed);

        assert!(reporter.is_cancelled());
    }

    #[tokio::test]
    async fn test_progress_reporter_report() {
        let (proxy, mut rx) = make_host_proxy();
        let reporter = ProgressReporter::new(proxy, "p-42".to_string());

        reporter
            .report(
                10,
                100,
                Some("scanning"),
                Some(vec![ProgressContent::Text {
                    text: "Found 10 files".to_string(),
                }]),
            )
            .await
            .unwrap();

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["method"], "notifications/tools/progress");
        assert_eq!(parsed["params"]["progress_token"], "p-42");
        assert_eq!(parsed["params"]["progress"], 10);
        assert_eq!(parsed["params"]["total"], 100);
        assert_eq!(parsed["params"]["phase"], "scanning");
    }

    #[tokio::test]
    async fn test_progress_reporter_report_text() {
        let (proxy, mut rx) = make_host_proxy();
        let reporter = ProgressReporter::new(proxy, "p-1".to_string());

        reporter
            .report_text(50, 100, "processing", "Half done")
            .await
            .unwrap();

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["params"]["progress"], 50);
        assert_eq!(parsed["params"]["content"][0]["type"], "text");
        assert_eq!(parsed["params"]["content"][0]["text"], "Half done");
    }

    #[tokio::test]
    async fn test_progress_reporter_update_widget() {
        let (proxy, mut rx) = make_host_proxy();
        let reporter = ProgressReporter::new(proxy, "p-1".to_string());

        reporter
            .update_widget("analysis", serde_json::json!({"status": "running"}))
            .await
            .unwrap();

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();

        assert_eq!(parsed["method"], "notifications/widgets/updated");
        assert_eq!(parsed["params"]["widget_id"], "analysis");
        assert_eq!(parsed["params"]["data"]["status"], "running");
    }
}
