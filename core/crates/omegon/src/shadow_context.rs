//! ShadowContext — per-session context corpus and selector.
//!
//! This is the assembly layer above durable storage and turn-local injections.
//! It owns a scored corpus of context entries and selects the subset that fits
//! the current model budget each turn.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::settings::{ContextClass, SelectorPolicy};

pub type EntryId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextKind {
    BaseSystemPrompt,
    ToolSchema,
    ActiveUserTurn,
    IntentDocument,
    CompactionSummary,
    SessionHud,
    WorkingMemoryPin,
    OperatorContextOverride,
    RecentToolOutput,
    FileSnippet,
    DesignNode,
    SpecScenario,
    TaskArtifact,
    MemoryFact,
    EpisodeSummary,
    CodebaseChunk,
}

impl ContextKind {
    pub fn tier(&self) -> u8 {
        match self {
            Self::BaseSystemPrompt | Self::ToolSchema | Self::ActiveUserTurn => 0,
            Self::IntentDocument
            | Self::CompactionSummary
            | Self::SessionHud
            | Self::WorkingMemoryPin
            | Self::OperatorContextOverride => 1,
            Self::RecentToolOutput
            | Self::FileSnippet
            | Self::DesignNode
            | Self::SpecScenario
            | Self::TaskArtifact => 2,
            Self::MemoryFact | Self::EpisodeSummary | Self::CodebaseChunk => 3,
        }
    }

    pub fn compressible(&self) -> bool {
        matches!(
            self,
            Self::RecentToolOutput
                | Self::FileSnippet
                | Self::EpisodeSummary
                | Self::CodebaseChunk
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryBody {
    Inline(String),
    FileRef {
        path: PathBuf,
        byte_range: Option<(usize, usize)>,
    },
    Compressed {
        original_id: EntryId,
        compressed_text: String,
    },
}

impl EntryBody {
    pub fn materialize(&self) -> String {
        match self {
            Self::Inline(text) => text.clone(),
            Self::FileRef { path, .. } => path.display().to_string(),
            Self::Compressed {
                compressed_text, ..
            } => compressed_text.clone(),
        }
    }

    pub fn token_estimate(&self) -> usize {
        self.materialize().len() / 4
    }
}

#[derive(Debug, Clone)]
pub struct ShadowEntry {
    pub id: EntryId,
    pub kind: ContextKind,
    pub body: EntryBody,
    pub token_estimate: usize,
    pub priority: i32,
    pub relevance: f32,
    pub recency: f32,
    pub mandatory: bool,
    pub pinned: bool,
    pub ttl_turns: Option<u32>,
    pub last_included_turn: Option<u32>,
    pub last_scored_turn: u32,
    pub diversity_key: Option<String>,
    pub diversity_cap: Option<usize>,
}

impl ShadowEntry {
    pub fn new(id: impl Into<String>, kind: ContextKind, body: EntryBody) -> Self {
        let token_estimate = body.token_estimate();
        Self {
            id: id.into(),
            kind,
            body,
            token_estimate,
            priority: 0,
            relevance: 0.0,
            recency: 1.0,
            mandatory: false,
            pinned: false,
            ttl_turns: None,
            last_included_turn: None,
            last_scored_turn: 0,
            diversity_key: None,
            diversity_cap: None,
        }
    }

    pub fn tier_weight(&self) -> f32 {
        match self.kind.tier() {
            0 => 10_000.0,
            1 => 1_000.0,
            2 => 100.0,
            _ => 10.0,
        }
    }

    pub fn priority_weight(&self) -> f32 {
        self.priority as f32
    }

    pub fn relevance_weight(&self) -> f32 {
        self.relevance * 10.0
    }

    pub fn recency_weight(&self) -> f32 {
        self.recency
    }

    pub fn combined_score(&self) -> f32 {
        self.tier_weight()
            + self.priority_weight()
            + self.relevance_weight()
            + self.recency_weight()
    }

    pub fn is_expired(&self, turn: u32) -> bool {
        self.ttl_turns
            .is_some_and(|ttl| turn.saturating_sub(self.last_scored_turn) >= ttl)
    }
}

#[derive(Debug, Clone)]
pub struct SelectedContext {
    pub selected_ids: Vec<EntryId>,
    pub total_tokens: usize,
    pub policy: SelectorPolicy,
}

#[derive(Debug, Clone)]
pub struct ShadowContext {
    entries: Vec<ShadowEntry>,
    selector_policy: SelectorPolicy,
}

impl ShadowContext {
    pub fn new(selector_policy: SelectorPolicy) -> Self {
        Self {
            entries: Vec::new(),
            selector_policy,
        }
    }

    pub fn selector_policy(&self) -> SelectorPolicy {
        self.selector_policy
    }

    pub fn set_selector_policy(&mut self, selector_policy: SelectorPolicy) {
        self.selector_policy = selector_policy;
    }

    pub fn upsert(&mut self, mut entry: ShadowEntry) {
        entry.token_estimate = entry.body.token_estimate();
        if let Some(slot) = self.entries.iter_mut().find(|existing| existing.id == entry.id) {
            *slot = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn remove_by_source_prefix(&mut self, prefix: &str) {
        self.entries.retain(|entry| !entry.id.starts_with(prefix));
    }

    pub fn retain_nonexpired(&mut self, turn: u32) {
        self.entries.retain(|entry| !entry.is_expired(turn));
    }

    pub fn select_for_turn(&mut self, turn: u32, user_prompt: &str) -> SelectedContext {
        let budget = self.selector_policy.assembly_budget();
        self.select_for_turn_with_budget(turn, user_prompt, budget)
    }

    pub fn select_for_turn_with_budget(
        &mut self,
        turn: u32,
        user_prompt: &str,
        budget: usize,
    ) -> SelectedContext {
        self.retain_nonexpired(turn);

        for entry in &mut self.entries {
            entry.last_scored_turn = turn;
            let content = entry.body.materialize();
            let prompt_lower = user_prompt.to_lowercase();
            let content_lower = content.to_lowercase();
            entry.relevance = if prompt_lower.is_empty() {
                0.0
            } else if content_lower.contains(&prompt_lower) {
                1.0
            } else {
                let overlap = prompt_lower
                    .split_whitespace()
                    .filter(|word| content_lower.contains(word))
                    .count();
                overlap as f32 / prompt_lower.split_whitespace().count().max(1) as f32
            };
            entry.recency = match entry.last_included_turn {
                Some(last) => 1.0 / (turn.saturating_sub(last).max(1) as f32),
                None => 0.5,
            };
        }

        let mut ordered: Vec<_> = self.entries.iter_mut().collect();
        ordered.sort_by(|a, b| {
            b.combined_score()
                .total_cmp(&a.combined_score())
                .then_with(|| b.priority.cmp(&a.priority))
                .then_with(|| a.token_estimate.cmp(&b.token_estimate))
                .then_with(|| a.id.cmp(&b.id))
        });

        let candidate_audit = ordered
            .iter()
            .map(|entry| {
                format!(
                    "{} kind={:?} tier={} tier_w={:.3} prio={} prio_w={:.3} rel={:.3} rel_w={:.3} rec={:.3} rec_w={:.3} tok={} score={:.3}",
                    entry.id,
                    entry.kind,
                    entry.kind.tier(),
                    entry.tier_weight(),
                    entry.priority,
                    entry.priority_weight(),
                    entry.relevance,
                    entry.relevance_weight(),
                    entry.recency,
                    entry.recency_weight(),
                    entry.token_estimate,
                    entry.combined_score(),
                )
            })
            .collect::<Vec<_>>();

        tracing::debug!(
            turn,
            budget,
            candidate_count = ordered.len(),
            requested_class = %self.selector_policy.requested_class.short(),
            actual_class = %self.selector_policy.actual_class().short(),
            candidates = ?candidate_audit,
            "shadow_context: selection starting"
        );

        let mut total_tokens = 0usize;
        let mut selected_ids = Vec::new();
        let mut dropped_ids = Vec::new();
        let mut diversity_counts: HashMap<String, usize> = HashMap::new();

        for entry in ordered {
            if entry.kind.tier() == 0 || entry.mandatory || entry.pinned {
                total_tokens += entry.token_estimate;
                entry.last_included_turn = Some(turn);
                if let Some(key) = &entry.diversity_key {
                    *diversity_counts.entry(key.clone()).or_default() += 1;
                }
                selected_ids.push(entry.id.clone());
                tracing::debug!(
                    id = %entry.id,
                    kind = ?entry.kind,
                    priority = entry.priority,
                    tier = entry.kind.tier(),
                    tier_weight = entry.tier_weight(),
                    priority_weight = entry.priority_weight(),
                    relevance = entry.relevance,
                    relevance_weight = entry.relevance_weight(),
                    recency = entry.recency,
                    recency_weight = entry.recency_weight(),
                    diversity_key = ?entry.diversity_key,
                    diversity_cap = ?entry.diversity_cap,
                    tokens = entry.token_estimate,
                    score = entry.combined_score(),
                    selected = true,
                    reason = "mandatory_or_pinned",
                    running_total = total_tokens,
                    "shadow_context: entry decision"
                );
                continue;
            }

            if let (Some(key), Some(cap)) = (&entry.diversity_key, entry.diversity_cap) {
                let seen = diversity_counts.get(key).copied().unwrap_or(0);
                if seen >= cap {
                    dropped_ids.push(entry.id.clone());
                    tracing::debug!(
                        id = %entry.id,
                        kind = ?entry.kind,
                        priority = entry.priority,
                        tier = entry.kind.tier(),
                        tier_weight = entry.tier_weight(),
                        priority_weight = entry.priority_weight(),
                        relevance = entry.relevance,
                        relevance_weight = entry.relevance_weight(),
                        recency = entry.recency,
                        recency_weight = entry.recency_weight(),
                        diversity_key = ?entry.diversity_key,
                        diversity_cap = ?entry.diversity_cap,
                        tokens = entry.token_estimate,
                        score = entry.combined_score(),
                        selected = false,
                        reason = "diversity_cap",
                        running_total = total_tokens,
                        "shadow_context: entry decision"
                    );
                    continue;
                }
            }

            if total_tokens + entry.token_estimate <= budget {
                total_tokens += entry.token_estimate;
                entry.last_included_turn = Some(turn);
                if let Some(key) = &entry.diversity_key {
                    *diversity_counts.entry(key.clone()).or_default() += 1;
                }
                selected_ids.push(entry.id.clone());
                tracing::debug!(
                    id = %entry.id,
                    kind = ?entry.kind,
                    priority = entry.priority,
                    tier = entry.kind.tier(),
                    tier_weight = entry.tier_weight(),
                    priority_weight = entry.priority_weight(),
                    relevance = entry.relevance,
                    relevance_weight = entry.relevance_weight(),
                    recency = entry.recency,
                    recency_weight = entry.recency_weight(),
                    diversity_key = ?entry.diversity_key,
                    diversity_cap = ?entry.diversity_cap,
                    tokens = entry.token_estimate,
                    score = entry.combined_score(),
                    selected = true,
                    reason = "fits_budget",
                    running_total = total_tokens,
                    "shadow_context: entry decision"
                );
            } else {
                dropped_ids.push(entry.id.clone());
                tracing::debug!(
                    id = %entry.id,
                    kind = ?entry.kind,
                    priority = entry.priority,
                    tier = entry.kind.tier(),
                    tier_weight = entry.tier_weight(),
                    priority_weight = entry.priority_weight(),
                    relevance = entry.relevance,
                    relevance_weight = entry.relevance_weight(),
                    recency = entry.recency,
                    recency_weight = entry.recency_weight(),
                    diversity_key = ?entry.diversity_key,
                    diversity_cap = ?entry.diversity_cap,
                    tokens = entry.token_estimate,
                    score = entry.combined_score(),
                    selected = false,
                    reason = "over_budget",
                    running_total = total_tokens,
                    "shadow_context: entry decision"
                );
            }
        }

        tracing::debug!(
            turn,
            budget,
            selected = selected_ids.len(),
            dropped = dropped_ids.len(),
            total_tokens,
            selected_ids = ?selected_ids,
            dropped_ids = ?dropped_ids,
            "shadow_context: selection complete"
        );

        SelectedContext {
            selected_ids,
            total_tokens,
            policy: self.selector_policy,
        }
    }

    pub fn render_selection(&self, selected: &SelectedContext) -> String {
        let mut ordered = Vec::new();
        for id in &selected.selected_ids {
            if let Some(entry) = self.entries.iter().find(|entry| &entry.id == id) {
                ordered.push(entry.body.materialize());
            }
        }
        ordered.join("\n\n")
    }

    pub fn actual_class(&self) -> ContextClass {
        self.selector_policy.actual_class()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SelectorPolicy {
        SelectorPolicy {
            model_window: 10_000,
            requested_class: ContextClass::Legion,
            reply_reserve: 1_000,
            tool_schema_reserve: 500,
        }
    }

    #[test]
    fn selector_keeps_tier_zero_even_over_budget() {
        let mut shadow = ShadowContext::new(policy());
        let mut base = ShadowEntry::new(
            "base",
            ContextKind::BaseSystemPrompt,
            EntryBody::Inline("x".repeat(40_000)),
        );
        base.mandatory = true;
        shadow.upsert(base);
        shadow.upsert(ShadowEntry::new(
            "hud",
            ContextKind::SessionHud,
            EntryBody::Inline("hud".into()),
        ));

        let selected = shadow.select_for_turn(1, "hud");
        assert!(selected.selected_ids.iter().any(|id| id == "base"));
    }

    #[test]
    fn selector_prefers_higher_tier_entries() {
        let mut shadow = ShadowContext::new(policy());
        shadow.upsert(ShadowEntry::new(
            "tier3",
            ContextKind::MemoryFact,
            EntryBody::Inline("relevant words here".repeat(100)),
        ));
        shadow.upsert(ShadowEntry::new(
            "tier1",
            ContextKind::SessionHud,
            EntryBody::Inline("small hud".into()),
        ));

        let selected = shadow.select_for_turn(1, "relevant");
        let pos_tier1 = selected.selected_ids.iter().position(|id| id == "tier1");
        let pos_tier3 = selected.selected_ids.iter().position(|id| id == "tier3");
        assert!(pos_tier1.is_some());
        assert!(pos_tier3.is_some());
        assert!(pos_tier1 < pos_tier3);
    }

    #[test]
    fn selector_prefers_higher_priority_within_tier() {
        let mut shadow = ShadowContext::new(policy());
        let mut lower = ShadowEntry::new(
            "lower",
            ContextKind::TaskArtifact,
            EntryBody::Inline("shared prompt words".into()),
        );
        lower.priority = 10;
        let mut higher = ShadowEntry::new(
            "higher",
            ContextKind::TaskArtifact,
            EntryBody::Inline("shared prompt words".into()),
        );
        higher.priority = 100;
        shadow.upsert(lower);
        shadow.upsert(higher);

        let selected = shadow.select_for_turn(7, "shared prompt");
        let pos_high = selected.selected_ids.iter().position(|id| id == "higher").unwrap();
        let pos_low = selected.selected_ids.iter().position(|id| id == "lower").unwrap();
        assert!(pos_high < pos_low);
    }

    #[test]
    fn selector_enforces_diversity_cap() {
        let mut shadow = ShadowContext::new(policy());
        let mut a = ShadowEntry::new(
            "code:a",
            ContextKind::CodebaseChunk,
            EntryBody::Inline("selector policy alpha".into()),
        );
        a.priority = 90;
        a.diversity_key = Some("file:src/main.rs".into());
        a.diversity_cap = Some(1);

        let mut b = ShadowEntry::new(
            "code:b",
            ContextKind::CodebaseChunk,
            EntryBody::Inline("selector policy beta".into()),
        );
        b.priority = 80;
        b.diversity_key = Some("file:src/main.rs".into());
        b.diversity_cap = Some(1);

        shadow.upsert(a);
        shadow.upsert(b);

        let selected = shadow.select_for_turn(1, "selector policy");
        assert_eq!(selected.selected_ids.len(), 1);
        assert_eq!(selected.selected_ids[0], "code:a");
    }
}
