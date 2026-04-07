//! Context management provider — handles context_status, request_context, context_compact, context_clear tools.
//!
//! Provides the harness with tools for organic context management:
//! - context_status: show current window usage, token budget
//! - request_context: request bounded, curated context packs
//! - context_compact: compress conversation via LLM
//! - context_clear: clear history, start fresh

use async_trait::async_trait;
use omegon_codescan::{BM25Index, Indexer, ScanCache, SearchScope};
use omegon_memory::{MemoryBackend, Section};
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

use crate::lifecycle::context::LifecycleContextProvider;
use crate::lifecycle::design;
use crate::lifecycle::types::ChangeStage;
use crate::shadow_context::{ContextKind, EntryBody, ShadowContext, ShadowEntry};
use crate::tui::TuiCommand;

fn dispatch_command(command_tx: &SharedCommandTx, command: TuiCommand) -> bool {
    if let Ok(guard) = command_tx.lock()
        && let Some(ref tx) = *guard
    {
        return tx.try_send(command).is_ok();
    }
    false
}

async fn run_context_slash(
    command_tx: &SharedCommandTx,
    args: &str,
) -> anyhow::Result<Option<omegon_traits::SlashCommandResponse>> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if !dispatch_command(
        command_tx,
        TuiCommand::RunSlashCommand {
            name: "context".into(),
            args: args.into(),
            respond_to: Some(reply_tx),
        },
    ) {
        return Ok(None);
    }

    Ok(Some(
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("context slash executor dropped response"))?,
    ))
}

/// Shared context metrics — updated by main loop, read by ContextProvider
#[derive(Debug, Clone)]
pub struct SharedContextMetrics {
    pub tokens_used: usize,
    pub context_window: usize,
    pub context_class: String,
    pub thinking_level: String,
}

impl SharedContextMetrics {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            tokens_used: 0,
            context_window: 200000,
            context_class: "unknown".to_string(),
            thinking_level: "unknown".to_string(),
        }))
    }

    pub fn usage_percent(&self) -> u32 {
        if self.context_window > 0 {
            ((self.tokens_used as f64 / self.context_window as f64) * 100.0).min(100.0) as u32
        } else {
            0
        }
    }

    pub fn update(
        &mut self,
        tokens_used: usize,
        context_window: usize,
        context_class: &str,
        thinking_level: &str,
    ) {
        self.tokens_used = tokens_used;
        self.context_window = context_window;
        self.context_class = context_class.to_string();
        self.thinking_level = thinking_level.to_string();
    }
}

/// Shared command channel — created in main, set after TUI init
pub type SharedCommandTx = Arc<Mutex<Option<mpsc::Sender<TuiCommand>>>>;

pub fn new_shared_command_tx() -> SharedCommandTx {
    Arc::new(Mutex::new(None))
}

pub struct ContextProvider {
    command_tx: SharedCommandTx,
    metrics: Arc<Mutex<SharedContextMetrics>>,
    lifecycle: Option<Arc<Mutex<LifecycleContextProvider>>>,
    memory_backend: Option<Arc<dyn MemoryBackend>>,
    memory_mind: Option<String>,
    repo_path: Option<PathBuf>,
}

impl ContextProvider {
    pub fn new(metrics: Arc<Mutex<SharedContextMetrics>>, command_tx: SharedCommandTx) -> Self {
        Self {
            command_tx,
            metrics,
            lifecycle: None,
            memory_backend: None,
            memory_mind: None,
            repo_path: None,
        }
    }

    pub fn new_with_sources(
        metrics: Arc<Mutex<SharedContextMetrics>>,
        command_tx: SharedCommandTx,
        lifecycle: Option<Arc<Mutex<LifecycleContextProvider>>>,
        memory_backend: Option<Arc<dyn MemoryBackend>>,
        memory_mind: Option<String>,
        repo_path: Option<PathBuf>,
    ) -> Self {
        Self {
            command_tx,
            metrics,
            lifecycle,
            memory_backend,
            memory_mind,
            repo_path,
        }
    }

    fn request_max_items(req: &Value) -> usize {
        req.get("max_items")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(3)
            .clamp(1, 4)
    }

    fn pack_shadow() -> ShadowContext {
        ShadowContext::new(crate::settings::SelectorPolicy {
            model_window: 4_096,
            requested_class: crate::settings::ContextClass::Squad,
            reply_reserve: 512,
            tool_schema_reserve: 256,
        })
    }

    fn select_pack(
        kind_heading: &str,
        query: &str,
        reason: &str,
        mut entries: Vec<ShadowEntry>,
    ) -> Option<String> {
        if entries.is_empty() {
            tracing::debug!(kind = kind_heading, query, "request_context: no candidate entries");
            return None;
        }
        let candidate_ids = entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        let mut shadow = Self::pack_shadow();
        for entry in entries.drain(..) {
            shadow.upsert(entry);
        }
        tracing::debug!(
            kind = kind_heading,
            query,
            reason,
            candidate_count = candidate_ids.len(),
            candidate_ids = ?candidate_ids,
            "request_context: selecting pack candidates"
        );
        let selected = shadow.select_for_turn_with_budget(1, query, 900);
        let body = shadow.render_selection(&selected);
        if body.trim().is_empty() {
            tracing::debug!(kind = kind_heading, query, "request_context: empty pack after selection");
            None
        } else {
            tracing::debug!(
                kind = kind_heading,
                query,
                selected = selected.selected_ids.len(),
                total_tokens = selected.total_tokens,
                selected_ids = ?selected.selected_ids,
                "request_context: pack selected"
            );
            Some(format!("### {kind_heading}\n- Reason: {reason}\n- Query: {query}\n{body}"))
        }
    }

    fn summarize_decisions(&self, query: &str, reason: &str, max_items: usize) -> Option<String> {
        let lifecycle = self.lifecycle.as_ref()?;
        let provider = lifecycle.lock().ok()?;
        let mut entries = Vec::new();

        if let Some(node_id) = provider.focused_node_id()
            && let Some(node) = provider.get_node(node_id)
            && let Some(sections) = design::read_node_sections(node)
        {
            for (idx, decision) in sections
                .decisions
                .iter()
                .filter(|d| d.status == "decided" || d.status == "exploring")
                .enumerate()
            {
                let hay = format!("{} {} {}", node.title, decision.title, decision.rationale)
                    .to_lowercase();
                if query.is_empty() || hay.contains(&query.to_lowercase()) {
                    let mut entry = ShadowEntry::new(
                        format!("decision:{}:{idx}", node.id),
                        ContextKind::DesignNode,
                        EntryBody::Inline(format!(
                            "- {} / {} — {} [{}]\n  rationale: {}",
                            node.id, node.title, decision.title, decision.status, decision.rationale
                        )),
                    );
                    entry.priority = if decision.status == "decided" { 140 } else { 100 };
                    entry.diversity_key = Some(format!("design-node:{}", node.id));
                    entry.diversity_cap = Some(2);
                    entries.push(entry);
                }
                if entries.len() >= max_items {
                    break;
                }
            }
        }

        Self::select_pack("Decisions", query, reason, entries)
    }

    fn summarize_specs(&self, query: &str, reason: &str, max_items: usize) -> Option<String> {
        let lifecycle = self.lifecycle.as_ref()?;
        let provider = lifecycle.lock().ok()?;
        let mut entries = Vec::new();
        let query_lower = query.to_lowercase();

        for change in provider
            .changes()
            .iter()
            .filter(|c| matches!(c.stage, ChangeStage::Implementing | ChangeStage::Verifying | ChangeStage::Planned | ChangeStage::Specified))
        {
            for spec in &change.specs {
                for req in &spec.requirements {
                    let req_hay = format!("{} {} {}", spec.domain, req.title, req.description)
                        .to_lowercase();
                    if query.is_empty() || req_hay.contains(&query_lower) {
                        let mut entry = ShadowEntry::new(
                            format!("spec:req:{}:{}", change.name, req.title),
                            ContextKind::SpecScenario,
                            EntryBody::Inline(format!(
                                "- {} / {} — {}\n  {}",
                                change.name, spec.domain, req.title, req.description
                            )),
                        );
                        entry.priority = 120;
                        entry.diversity_key = Some(format!("spec:req:{}", req.title));
                        entry.diversity_cap = Some(1);
                        entries.push(entry);
                    }
                    for scenario in &req.scenarios {
                        let scenario_hay = format!(
                            "{} {} {} {} {}",
                            req.title, scenario.title, scenario.given, scenario.when, scenario.then
                        )
                        .to_lowercase();
                        if query.is_empty() || scenario_hay.contains(&query_lower) {
                            let mut entry = ShadowEntry::new(
                                format!("spec:scn:{}:{}", change.name, scenario.title),
                                ContextKind::SpecScenario,
                                EntryBody::Inline(format!(
                                    "- {} / {} / {}\n  Given {}\n  When {}\n  Then {}",
                                    change.name,
                                    spec.domain,
                                    scenario.title,
                                    scenario.given,
                                    scenario.when,
                                    scenario.then
                                )),
                            );
                            entry.priority = 130;
                            entry.diversity_key = Some(format!("spec:req:{}", req.title));
                            entry.diversity_cap = Some(1);
                            entries.push(entry);
                        }
                        if entries.len() >= max_items {
                            break;
                        }
                    }
                    if entries.len() >= max_items {
                        break;
                    }
                }
                if entries.len() >= max_items {
                    break;
                }
            }
            if entries.len() >= max_items {
                break;
            }
        }

        Self::select_pack("Specs", query, reason, entries)
    }

    async fn summarize_memory(
        &self,
        query: &str,
        reason: &str,
        max_items: usize,
    ) -> Option<String> {
        let backend = self.memory_backend.as_ref()?;
        let mind = self.memory_mind.as_deref()?;
        let results = backend.fts_search(mind, query, max_items).await.ok()?;
        let entries = results
            .into_iter()
            .enumerate()
            .map(|(idx, scored)| {
                let mut entry = ShadowEntry::new(
                    format!("memory:{idx}:{}", scored.fact.id),
                    ContextKind::MemoryFact,
                    EntryBody::Inline(format!(
                        "- [{}] {}\n  score: {:.2}",
                        match scored.fact.section {
                            Section::Architecture => "Architecture",
                            Section::Decisions => "Decisions",
                            Section::Constraints => "Constraints",
                            Section::KnownIssues => "Known Issues",
                            Section::PatternsConventions => "Patterns & Conventions",
                            Section::Specs => "Specs",
                            Section::RecentWork => "Recent Work",
                        },
                        scored.fact.content,
                        scored.score
                    )),
                );
                entry.priority = 80;
                entry.diversity_key = Some(format!("memory-section:{:?}", scored.fact.section));
                entry.diversity_cap = Some(2);
                entry
            })
            .collect::<Vec<_>>();
        Self::select_pack("Memory", query, reason, entries)
    }

    fn summarize_code(&self, query: &str, reason: &str, max_items: usize) -> Option<String> {
        let repo_path = self.repo_path.as_ref()?;
        let db_path = repo_path.join(".omegon").join("codescan.db");
        let mut cache = ScanCache::open(&db_path).ok()?;
        Indexer::run(repo_path, &mut cache).ok()?;
        let code_chunks = cache.all_code_chunks().ok()?;
        let knowledge_chunks = cache.all_knowledge_chunks().ok()?;
        let idx = BM25Index::build(&code_chunks, &knowledge_chunks);
        let results = idx.search(query, SearchScope::Code, max_items);
        let entries = results
            .into_iter()
            .enumerate()
            .map(|(idx, r)| {
                let mut entry = ShadowEntry::new(
                    format!("code:{idx}:{}:{}", r.file, r.start_line),
                    ContextKind::CodebaseChunk,
                    EntryBody::Inline(format!(
                        "- {}:{}-{} [{}]\n  score: {:.2}\n  {}",
                        r.file,
                        r.start_line,
                        r.end_line,
                        r.label,
                        r.score,
                        r.preview.chars().take(240).collect::<String>().replace('\n', " · ")
                    )),
                );
                entry.priority = 90;
                entry.diversity_key = Some(format!("code-file:{}", r.file));
                entry.diversity_cap = Some(2);
                entry
            })
            .collect::<Vec<_>>();
        Self::select_pack("Code", query, reason, entries)
    }
}

#[async_trait]
impl Feature for ContextProvider {
    fn name(&self) -> &str {
        "context-provider"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::context::CONTEXT_STATUS.into(),
                label: "Context Status".into(),
                description: "Show current context window usage, token count, and compression statistics.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::context::REQUEST_CONTEXT.into(),
                label: "Request Context".into(),
                description: "Request a compact context pack before making multiple exploratory tool calls. Best for session orientation and recent runtime evidence; returns curated summaries, not raw dumps.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "requests": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 3,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": {"type": "string", "enum": ["session_state", "recent_runtime", "code", "memory", "decisions", "specs"]},
                                    "query": {"type": "string"},
                                    "reason": {"type": "string"},
                                    "max_items": {"type": "integer", "minimum": 1, "maximum": 4},
                                    "scope": {"type": "array", "items": {"type": "string"}}
                                },
                                "required": ["kind", "query", "reason"]
                            }
                        }
                    },
                    "required": ["requests"]
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::context::CONTEXT_COMPACT.into(),
                label: "Compact Context".into(),
                description: "Compress the conversation history via LLM summarization, freeing tokens for new work.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::context::CONTEXT_CLEAR.into(),
                label: "Clear Context".into(),
                description: "Clear all conversation history and start fresh. Archives the current session first.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        _args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::context::CONTEXT_STATUS => {
                let dispatched = dispatch_command(&self.command_tx, TuiCommand::ContextStatus);
                let metrics = self.metrics.lock().unwrap();
                let pct = metrics.usage_percent();
                let result_text = format!(
                    "Context: {}/{} tokens ({}%)\nClass: {}\nThinking: {}",
                    metrics.tokens_used,
                    metrics.context_window,
                    pct,
                    metrics.context_class,
                    metrics.thinking_level
                );

                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: result_text }],
                    details: json!({
                        "tokens_used": metrics.tokens_used,
                        "context_window": metrics.context_window,
                        "usage_percent": pct,
                        "class": metrics.context_class,
                        "thinking": metrics.thinking_level,
                        "dispatched": dispatched,
                    }),
                })
            }

            crate::tool_registry::context::REQUEST_CONTEXT => {
                let requests = _args
                    .get("requests")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| anyhow::anyhow!("request_context requires a requests array"))?;
                if requests.len() > 3 {
                    anyhow::bail!("request_context accepts at most 3 requests per call");
                }

                tracing::debug!(request_count = requests.len(), raw = ?_args, "request_context: received requests");

                let metrics = {
                    let metrics = self.metrics.lock().unwrap();
                    metrics.clone()
                };
                let mut sections = Vec::new();
                let mut supported = 0usize;
                let mut unsupported = 0usize;

                for req in requests {
                    let kind = req.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let query = req.get("query").and_then(|v| v.as_str()).unwrap_or("");
                    let reason = req.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    match kind {
                        "session_state" => {
                            supported += 1;
                            sections.push(format!(
                                "### Session State\n- Why selected: session orientation request for `{query}`\n- Reason: {reason}\n- Current context: {}/{} tokens ({}%)\n- Policy: {}\n- Thinking: {}",
                                metrics.tokens_used,
                                metrics.context_window,
                                metrics.usage_percent(),
                                metrics.context_class,
                                metrics.thinking_level
                            ));
                        }
                        "recent_runtime" => {
                            supported += 1;
                            sections.push(format!(
                                "### Recent Runtime\n- Why selected: recent runtime evidence request for `{query}`\n- Reason: {reason}\n- Current runtime snapshot: context {}/{} tokens ({}%), policy {}, thinking {}",
                                metrics.tokens_used,
                                metrics.context_window,
                                metrics.usage_percent(),
                                metrics.context_class,
                                metrics.thinking_level
                            ));
                        }
                        "decisions" => {
                            if let Some(pack) = self.summarize_decisions(query, reason, Self::request_max_items(req)) {
                                supported += 1;
                                sections.push(pack);
                            } else {
                                unsupported += 1;
                                sections.push(format!(
                                    "### decisions\n- Reason: {reason}\n- Query: {query}\n- Status: no focused lifecycle decision context matched this request."
                                ));
                            }
                        }
                        "specs" => {
                            if let Some(pack) = self.summarize_specs(query, reason, Self::request_max_items(req)) {
                                supported += 1;
                                sections.push(pack);
                            } else {
                                unsupported += 1;
                                sections.push(format!(
                                    "### specs\n- Reason: {reason}\n- Query: {query}\n- Status: no active spec scenarios matched this request."
                                ));
                            }
                        }
                        "memory" => {
                            if let Some(pack) = self.summarize_memory(query, reason, Self::request_max_items(req)).await {
                                supported += 1;
                                sections.push(pack);
                            } else {
                                unsupported += 1;
                                sections.push(format!(
                                    "### memory\n- Reason: {reason}\n- Query: {query}\n- Status: no memory facts matched this request."
                                ));
                            }
                        }
                        "code" => {
                            if let Some(pack) = self.summarize_code(query, reason, Self::request_max_items(req)) {
                                supported += 1;
                                sections.push(pack);
                            } else {
                                unsupported += 1;
                                sections.push(format!(
                                    "### code\n- Reason: {reason}\n- Query: {query}\n- Status: no code chunks matched this request."
                                ));
                            }
                        }
                        other => {
                            anyhow::bail!("unknown request_context kind: {other}");
                        }
                    }
                }

                let summary = format!(
                    "Retrieved {} supported context pack(s); {} request(s) still require dedicated tools.",
                    supported, unsupported
                );
                let mut blocks = vec![ContentBlock::Text { text: summary.clone() }];
                blocks.push(ContentBlock::Text {
                    text: sections.join("\n\n"),
                });
                Ok(ToolResult {
                    content: blocks,
                    details: json!({
                        "supported": supported,
                        "unsupported": unsupported,
                        "context_window": metrics.context_window,
                        "tokens_used": metrics.tokens_used,
                        "thinking": metrics.thinking_level,
                        "class": metrics.context_class,
                    }),
                })
            }

            crate::tool_registry::context::CONTEXT_COMPACT => {
                let response = run_context_slash(&self.command_tx, "compact").await?;
                let (text, accepted, dispatched) = if let Some(response) = response {
                    (
                        response.output.unwrap_or_else(|| "Context compaction completed.".into()),
                        response.accepted,
                        true,
                    )
                } else {
                    (
                        "Context compaction is unavailable in this mode (no interactive session command channel).".into(),
                        false,
                        false,
                    )
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: json!({ "dispatched": dispatched, "accepted": accepted }),
                })
            }

            crate::tool_registry::context::CONTEXT_CLEAR => {
                let response = run_context_slash(&self.command_tx, "clear").await?;
                let (text, accepted, dispatched) = if let Some(response) = response {
                    (
                        response.output.unwrap_or_else(|| "Context cleared.".into()),
                        response.accepted,
                        true,
                    )
                } else {
                    (
                        "Context clear is unavailable in this mode (no interactive session command channel).".into(),
                        false,
                        false,
                    )
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: json!({ "dispatched": dispatched, "accepted": accepted }),
                })
            }

            _ => Err(anyhow::anyhow!("unknown context tool: {}", tool_name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect_text(result: &ToolResult) -> &str {
        match &result.content[0] {
            ContentBlock::Text { text } => text,
            other => panic!("unexpected content block: {other:?}"),
        }
    }

    #[tokio::test]
    async fn context_status_reports_current_metrics_snapshot() {
        let metrics = SharedContextMetrics::new();
        {
            let mut m = metrics.lock().unwrap();
            m.update(96_433, 272_000, "Maniple (272k)", "medium");
        }
        let command_tx = new_shared_command_tx();
        let provider = ContextProvider::new(metrics, command_tx);
        let result = provider
            .execute(
                crate::tool_registry::context::CONTEXT_STATUS,
                "call-2",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");

        match &result.content[0] {
            ContentBlock::Text { text } => {
                assert!(
                    text.contains("Context: 96433/272000 tokens (35%)"),
                    "unexpected text: {text}"
                );
                assert!(
                    text.contains("Class: Maniple (272k)"),
                    "unexpected text: {text}"
                );
                assert!(text.contains("Thinking: medium"), "unexpected text: {text}");
            }
            other => panic!("unexpected content block: {other:?}"),
        }
        assert_eq!(result.details["tokens_used"], 96_433);
        assert_eq!(result.details["context_window"], 272_000);
        assert_eq!(result.details["usage_percent"], 35);
    }

    #[tokio::test]
    async fn compact_tool_reports_when_no_command_channel_is_available() {
        let metrics = SharedContextMetrics::new();
        let command_tx = new_shared_command_tx();
        let provider = ContextProvider::new(metrics, command_tx);
        let result = provider
            .execute(
                crate::tool_registry::context::CONTEXT_COMPACT,
                "call-1",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");

        match &result.content[0] {
            ContentBlock::Text { text } => {
                assert!(
                    text.contains("unavailable in this mode"),
                    "unexpected text: {text}"
                );
            }
            other => panic!("unexpected content block: {other:?}"),
        }
        assert_eq!(result.details["dispatched"], false);
        assert_eq!(result.details["accepted"], false);
    }

    #[tokio::test]
    async fn compact_tool_waits_for_structured_slash_response() {
        let metrics = SharedContextMetrics::new();
        let command_tx = new_shared_command_tx();
        let rx = {
            let (tx, rx) = mpsc::channel(1);
            *command_tx.lock().unwrap() = Some(tx);
            rx
        };
        let provider = ContextProvider::new(metrics, command_tx);

        let exec = tokio::spawn(async move {
            provider
                .execute(
                    crate::tool_registry::context::CONTEXT_COMPACT,
                    "call-3",
                    json!({}),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .expect("tool result")
        });

        let mut rx = rx;
        let command = rx.recv().await.expect("context slash command");
        match command {
            TuiCommand::RunSlashCommand {
                name,
                args,
                respond_to,
            } => {
                assert_eq!(name, "context");
                assert_eq!(args, "compact");
                respond_to
                    .expect("responder")
                    .send(omegon_traits::SlashCommandResponse {
                        accepted: true,
                        output: Some("Context compressed. Now using 1234 tokens.".into()),
                    })
                    .expect("send response");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let result = exec.await.expect("join");
        assert_eq!(
            expect_text(&result),
            "Context compressed. Now using 1234 tokens."
        );
        assert_eq!(result.details["dispatched"], true);
        assert_eq!(result.details["accepted"], true);
    }

    #[tokio::test]
    async fn clear_tool_waits_for_structured_slash_failure() {
        let metrics = SharedContextMetrics::new();
        let command_tx = new_shared_command_tx();
        let rx = {
            let (tx, rx) = mpsc::channel(1);
            *command_tx.lock().unwrap() = Some(tx);
            rx
        };
        let provider = ContextProvider::new(metrics, command_tx);

        let exec = tokio::spawn(async move {
            provider
                .execute(
                    crate::tool_registry::context::CONTEXT_CLEAR,
                    "call-4",
                    json!({}),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .expect("tool result")
        });

        let mut rx = rx;
        let command = rx.recv().await.expect("context slash command");
        match command {
            TuiCommand::RunSlashCommand {
                name,
                args,
                respond_to,
            } => {
                assert_eq!(name, "context");
                assert_eq!(args, "clear");
                respond_to
                    .expect("responder")
                    .send(omegon_traits::SlashCommandResponse {
                        accepted: false,
                        output: Some("clear failed".into()),
                    })
                    .expect("send response");
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let result = exec.await.expect("join");
        assert_eq!(expect_text(&result), "clear failed");
        assert_eq!(result.details["dispatched"], true);
        assert_eq!(result.details["accepted"], false);
    }

    #[tokio::test]
    async fn request_context_returns_compact_session_pack() {
        let metrics = SharedContextMetrics::new();
        {
            let mut m = metrics.lock().unwrap();
            m.update(96_433, 272_000, "Maniple (272k)", "medium");
        }
        let provider = ContextProvider::new(metrics, new_shared_command_tx());
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-1",
                json!({
                    "requests": [
                        {
                            "kind": "session_state",
                            "query": "orient me before planning",
                            "reason": "Need session context before exploratory reads"
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result
            .content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("Retrieved 1 supported context pack"),
            "unexpected text: {text}"
        );
        assert!(text.contains("Session State"), "unexpected text: {text}");
        assert!(text.contains("96433/272000"), "unexpected text: {text}");
    }

    #[tokio::test]
    async fn request_context_returns_decision_pack_from_focused_node() {
        let tmp = tempfile::tempdir().unwrap();
        let docs_dir = tmp.path().join("docs");
        std::fs::create_dir_all(&docs_dir).unwrap();
        let doc_path = docs_dir.join("decision-node.md");
        std::fs::write(
            &doc_path,
            "---\nid: decision-node\ntitle: Decision Node\nstatus: exploring\nopen_questions: []\ndependencies: []\nrelated: []\n---\n\n# Decision Node\n\n## Overview\n\nOverview.\n\n## Decisions\n\n### Use selector policy\n\n**Status:** decided\n\n**Rationale:** Keeps request shaping bounded.\n",
        )
        .unwrap();
        let mut lifecycle = LifecycleContextProvider::new(tmp.path());
        lifecycle.set_focus(Some("decision-node".into()));

        let provider = ContextProvider::new_with_sources(
            SharedContextMetrics::new(),
            new_shared_command_tx(),
            Some(Arc::new(Mutex::new(lifecycle))),
            None,
            None,
            None,
        );
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-decisions",
                json!({
                    "requests": [
                        {
                            "kind": "decisions",
                            "query": "selector policy",
                            "reason": "Need architectural decision context"
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result.content.iter().filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        assert!(text.contains("### Decisions"), "unexpected text: {text}");
        assert!(text.contains("Use selector policy"), "unexpected text: {text}");
    }

    #[tokio::test]
    async fn request_context_returns_memory_pack() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(omegon_memory::InMemoryBackend::new());
        backend
            .store_fact(omegon_memory::StoreFact {
                mind: "test".into(),
                content: "Selector policy must remain bounded and mediated".into(),
                section: Section::Decisions,
                decay_profile: omegon_memory::DecayProfileName::Standard,
                source: None,
            })
            .await
            .unwrap();

        let provider = ContextProvider::new_with_sources(
            SharedContextMetrics::new(),
            new_shared_command_tx(),
            None,
            Some(backend),
            Some("test".into()),
            None,
        );
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-memory",
                json!({
                    "requests": [
                        {
                            "kind": "memory",
                            "query": "selector policy mediated",
                            "reason": "Need prior decision memory"
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result.content.iter().filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        assert!(text.contains("### Memory"), "unexpected text: {text}");
        assert!(text.contains("bounded and mediated"), "unexpected text: {text}");
    }

    #[tokio::test]
    async fn request_context_returns_specs_pack_from_active_change() {
        let tmp = tempfile::tempdir().unwrap();
        let spec_dir = tmp
            .path()
            .join("openspec")
            .join("changes")
            .join("ctx-pack")
            .join("specs");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(
            tmp.path().join("openspec").join("changes").join("ctx-pack").join("proposal.md"),
            "# proposal\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("openspec").join("changes").join("ctx-pack").join("design.md"),
            "# design\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("openspec").join("changes").join("ctx-pack").join("tasks.md"),
            "- [ ] task\n",
        )
        .unwrap();
        std::fs::write(
            spec_dir.join("context.md"),
            "# context — Delta Spec\n\n## ADDED Requirements\n\n### Requirement: Context requests are mediated\n\nrequest_context must return bounded packs.\n\n#### Scenario: Session orientation request\nGiven the model lacks orientation\nWhen it calls request_context\nThen the harness returns a bounded pack\n",
        )
        .unwrap();

        let lifecycle = LifecycleContextProvider::new(tmp.path());
        let provider = ContextProvider::new_with_sources(
            SharedContextMetrics::new(),
            new_shared_command_tx(),
            Some(Arc::new(Mutex::new(lifecycle))),
            None,
            None,
            None,
        );
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-specs",
                json!({
                    "requests": [
                        {
                            "kind": "specs",
                            "query": "bounded pack",
                            "reason": "Need current behavioral contract"
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result.content.iter().filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        assert!(text.contains("### Specs"), "unexpected text: {text}");
        assert!(text.contains("Context requests are mediated") || text.contains("Session orientation request"), "unexpected text: {text}");
    }

    #[tokio::test]
    async fn request_context_returns_code_pack() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src").join("main.rs"),
            "fn selector_policy() { println!(\"selector policy\"); }\nfn other() {}\n",
        )
        .unwrap();

        let provider = ContextProvider::new_with_sources(
            SharedContextMetrics::new(),
            new_shared_command_tx(),
            None,
            None,
            None,
            Some(tmp.path().to_path_buf()),
        );
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-code",
                json!({
                    "requests": [
                        {
                            "kind": "code",
                            "query": "selector policy",
                            "reason": "Need exact implementation orientation"
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result.content.iter().filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        assert!(text.contains("### Code"), "unexpected text: {text}");
        assert!(text.contains("src/main.rs"), "unexpected text: {text}");
        assert!(text.contains("selector_policy") || text.contains("selector policy"), "unexpected text: {text}");
    }

    #[tokio::test]
    async fn request_context_code_pack_caps_same_file_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(
            tmp.path().join("src").join("main.rs"),
            "fn selector_policy() { println!(\"selector policy\"); }\nfn selector_policy_helper() { println!(\"selector policy helper\"); }\nfn selector_policy_debug() { println!(\"selector policy debug\"); }\n",
        )
        .unwrap();

        let provider = ContextProvider::new_with_sources(
            SharedContextMetrics::new(),
            new_shared_command_tx(),
            None,
            None,
            None,
            Some(tmp.path().to_path_buf()),
        );
        let result = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-code-diversity",
                json!({
                    "requests": [
                        {
                            "kind": "code",
                            "query": "selector policy",
                            "reason": "Need code orientation",
                            "max_items": 4
                        }
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("tool result");
        let text = result.content.iter().filter_map(|c| match c { ContentBlock::Text { text } => Some(text.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        let occurrences = text.matches("src/main.rs:").count();
        assert!(occurrences <= 2, "expected at most 2 snippets from same file, got {occurrences}: {text}");
    }

    #[tokio::test]
    async fn request_context_rejects_too_many_requests() {
        let provider = ContextProvider::new(SharedContextMetrics::new(), new_shared_command_tx());
        let err = provider
            .execute(
                crate::tool_registry::context::REQUEST_CONTEXT,
                "call-ctx-2",
                json!({
                    "requests": [
                        {"kind": "session_state", "query": "a", "reason": "a"},
                        {"kind": "session_state", "query": "b", "reason": "b"},
                        {"kind": "session_state", "query": "c", "reason": "c"},
                        {"kind": "session_state", "query": "d", "reason": "d"}
                    ]
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect_err("should reject oversized request batch");
        assert!(
            err.to_string().contains("at most 3 requests"),
            "unexpected error: {err}"
        );
    }
}
