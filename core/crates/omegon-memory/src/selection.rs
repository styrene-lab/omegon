//! Shared memory selection with counted whole-block packing.
use crate::*;
use std::collections::{BTreeMap, HashSet};

pub const DEFAULT_MEMORY_TOKEN_CAP: usize = 1024;
pub const MAX_MEMORY_TOKEN_CAP: usize = 8192;
pub const MAX_CANDIDATES: usize = 512;
const MAX_EXCLUSIONS: usize = 64;

pub fn empty_report(degradation: Option<String>) -> MemorySelectionReport {
    MemorySelectionReport {
        intent: MemorySelectionIntent::Ambient,
        low_signal: false,
        accounting: MemoryTokenAccounting::ConservativeUtf8Bytes,
        budget: 0,
        accounted_tokens: 0,
        selected: vec![],
        exclusions: vec![],
        exclusion_counts: BTreeMap::new(),
        exclusions_truncated: false,
        budget_exhausted: false,
        pin_resolutions: vec![],
        retrieval_degradation: degradation,
    }
}

/// Counters must be deterministic and count the complete supplied text.
pub trait MemoryTokenCounter: Send + Sync {
    fn count(&self, text: &str) -> usize;
    fn accounting(&self) -> MemoryTokenAccounting;
}
pub struct ConservativeUtf8Counter;
impl MemoryTokenCounter for ConservativeUtf8Counter {
    fn count(&self, text: &str) -> usize {
        text.len()
    }
    fn accounting(&self) -> MemoryTokenAccounting {
        MemoryTokenAccounting::ConservativeUtf8Bytes
    }
}

pub struct MemorySelectionInput<'a> {
    pub mind: &'a str,
    pub query: &'a str,
    pub intent: MemorySelectionIntent,
    pub facts: &'a [Fact],
    pub pins: &'a [Fact],
    pub episodes: &'a [Episode],
    pub context: &'a ApplicabilityContext,
    pub host_budget: usize,
    pub memory_cap: usize,
}

impl MemorySelectionReport {
    pub fn exclude(&mut self, id: &str, reason: MemoryExclusionReason) {
        *self.exclusion_counts.entry(reason).or_default() += 1;
        if self.exclusions.len() < MAX_EXCLUSIONS {
            let mut chars = id.chars();
            let id = chars.by_ref().take(2048).collect();
            self.exclusions.push(MemoryExclusion {
                id,
                id_truncated: chars.next().is_some(),
                reason,
            });
        } else {
            self.exclusions_truncated = true;
        }
        if matches!(
            reason,
            MemoryExclusionReason::Budget | MemoryExclusionReason::EpisodeBudget
        ) {
            self.budget_exhausted = true;
        }
    }
}

pub fn low_signal(query: &str) -> bool {
    matches!(
        query.trim().to_ascii_lowercase().as_str(),
        "" | "hi"
            | "hello"
            | "thanks"
            | "thank you"
            | "ok"
            | "okay"
            | "yes"
            | "no"
            | "continue"
            | "proceed"
    )
}

pub fn select(
    input: MemorySelectionInput<'_>,
    counter: &dyn MemoryTokenCounter,
) -> backend::Result<MemorySelection> {
    select_with_renderer(input, counter, &MarkdownRenderer)
}

fn formatted(
    facts: &[renderer::MemoryFactBlock<'_>],
    episodes: &[&Episode],
    context: &ApplicabilityContext,
    renderer: &dyn ContextRenderer,
) -> String {
    renderer.render_memory_blocks(facts, episodes, context)
}

pub fn select_with_renderer(
    input: MemorySelectionInput<'_>,
    counter: &dyn MemoryTokenCounter,
    renderer: &dyn ContextRenderer,
) -> backend::Result<MemorySelection> {
    input.context.validate()?;
    if input.memory_cap > MAX_MEMORY_TOKEN_CAP {
        return Err(MemoryError::InvalidMutation(
            "memory token cap exceeds 8192".into(),
        ));
    }
    let budget = input.host_budget.min(input.memory_cap);
    let mut report = MemorySelectionReport {
        intent: input.intent,
        low_signal: input.intent == MemorySelectionIntent::Ambient && low_signal(input.query),
        accounting: counter.accounting(),
        budget,
        accounted_tokens: 0,
        selected: vec![],
        exclusions: vec![],
        exclusion_counts: BTreeMap::new(),
        exclusions_truncated: false,
        budget_exhausted: false,
        pin_resolutions: vec![],
        retrieval_degradation: None,
    };
    if budget == 0 {
        let count = input
            .pins
            .len()
            .saturating_add(input.facts.len())
            .saturating_add(input.episodes.len());
        if count > 0 {
            report
                .exclusion_counts
                .insert(MemoryExclusionReason::Budget, count);
            report.budget_exhausted = true;
            report.exclusions_truncated = true;
        }
        return Ok(MemorySelection {
            markdown: String::new(),
            report,
        });
    }
    let mut selected = Vec::new();
    let mut episodes = Vec::new();
    let mut seen = HashSet::new();
    let mut markdown = String::new();
    let filter = SearchFilter {
        context: Some(input.context.clone()),
        ..Default::default()
    };
    let overflow = input
        .pins
        .len()
        .saturating_add(input.facts.len())
        .saturating_sub(MAX_CANDIDATES);
    if overflow > 0 {
        report
            .exclusion_counts
            .insert(MemoryExclusionReason::InputBound, overflow);
        report.exclusions_truncated = true;
    }
    for (fact, pinned) in input
        .pins
        .iter()
        .map(|fact| (fact, true))
        .chain(input.facts.iter().map(|fact| (fact, false)))
        .take(MAX_CANDIDATES)
    {
        let reason = if fact.content.len() > 65_536 || fact.id.len() > 2048 {
            Some(MemoryExclusionReason::InputBound)
        } else if fact.status != FactStatus::Active {
            Some(MemoryExclusionReason::Lifecycle)
        } else if fact.mind != input.mind
            || filter.applicability_status(fact) == ApplicabilityStatus::Inapplicable
        {
            Some(MemoryExclusionReason::Applicability)
        } else if decay::ambient_score(1.0, fact).is_none() {
            Some(MemoryExclusionReason::Confidence)
        } else if !seen.insert(fact.id.clone()) {
            Some(MemoryExclusionReason::Duplicate)
        } else if !pinned
            && input.intent == MemorySelectionIntent::Ambient
            && low_signal(input.query)
        {
            Some(MemoryExclusionReason::LowSignal)
        } else {
            None
        };
        if let Some(reason) = reason {
            report.exclude(&fact.id, reason);
            continue;
        }
        selected.push(renderer::MemoryFactBlock {
            fact,
            pinned,
            applicability: filter.applicability_status(fact),
        });
        let proposed = formatted(&selected, &episodes, input.context, renderer);
        if counter.count(&proposed) > budget {
            selected.pop();
            report.exclude(&fact.id, MemoryExclusionReason::Budget);
            continue;
        }
        markdown = proposed;
        report.selected.push(MemoryEvidenceHandle {
            id: fact.id.clone(),
            version: Some(fact.version),
            kind: if pinned { "pin" } else { "fact" }.into(),
        });
    }
    let overflow = input.episodes.len().saturating_sub(16);
    if overflow > 0 {
        *report
            .exclusion_counts
            .entry(MemoryExclusionReason::InputBound)
            .or_default() += overflow;
        report.exclusions_truncated = true;
    }
    for episode in input.episodes.iter().take(16) {
        if episode
            .narrative
            .len()
            .saturating_add(episode.title.len())
            .saturating_add(episode.date.len())
            > 65_536
            || episode.id.len() > 2048
        {
            report.exclude(&episode.id, MemoryExclusionReason::InputBound);
            continue;
        }
        if episode.mind != input.mind {
            report.exclude(&episode.id, MemoryExclusionReason::Applicability);
            continue;
        }
        if low_signal(input.query) || input.query.split_whitespace().count() < 2 {
            report.exclude(&episode.id, MemoryExclusionReason::LowSignal);
            continue;
        }
        if episodes.iter().any(|old: &&Episode| old.id == episode.id) {
            report.exclude(&episode.id, MemoryExclusionReason::Duplicate);
            continue;
        }
        episodes.push(episode);
        let proposed_episode = formatted(&[], &episodes, input.context, renderer);
        let proposed = formatted(&selected, &episodes, input.context, renderer);
        if counter.count(&proposed_episode) > budget / 4 {
            episodes.pop();
            report.exclude(&episode.id, MemoryExclusionReason::EpisodeBudget);
            continue;
        }
        if counter.count(&proposed) > budget {
            episodes.pop();
            report.exclude(&episode.id, MemoryExclusionReason::Budget);
            continue;
        }
        markdown = proposed;
        report.selected.push(MemoryEvidenceHandle {
            id: episode.id.clone(),
            version: None,
            kind: "episode".into(),
        });
    }
    report.accounted_tokens = if markdown.is_empty() {
        0
    } else {
        counter.count(&markdown)
    };
    Ok(MemorySelection { markdown, report })
}

pub struct ResolvedPins {
    pub facts: Vec<Fact>,
    pub replacements: Vec<MemoryPinResolution>,
    pub missing: Vec<String>,
}
pub async fn resolve_pins(
    backend: &dyn MemoryBackend,
    mind: &str,
    ids: &[String],
) -> backend::Result<ResolvedPins> {
    let mut result = ResolvedPins {
        facts: vec![],
        replacements: vec![],
        missing: vec![],
    };
    for id in ids.iter().take(64) {
        let Some(fact) = backend.get_fact_record(mind, id).await? else {
            result.missing.push(id.clone());
            continue;
        };
        if fact.status == FactStatus::Superseded
            && let Some(replacement) = backend
                .superseding_fact(id)
                .await?
                .filter(|fact| fact.mind == mind)
        {
            result.replacements.push(MemoryPinResolution {
                requested_id: id.clone(),
                replacement_id: replacement.id.clone(),
            });
            result.facts.push(replacement);
        } else {
            result.facts.push(fact);
        }
    }
    Ok(result)
}

pub async fn retrieve_and_select(
    backend: &dyn MemoryBackend,
    request: &MemorySelectionRequest,
    counter: &dyn MemoryTokenCounter,
) -> backend::Result<MemorySelection> {
    retrieve_and_select_with_renderer(backend, request, counter, &MarkdownRenderer).await
}

pub async fn retrieve_and_select_with_renderer(
    backend: &dyn MemoryBackend,
    request: &MemorySelectionRequest,
    counter: &dyn MemoryTokenCounter,
    renderer: &dyn ContextRenderer,
) -> backend::Result<MemorySelection> {
    if request.pins.len() > 64 {
        return Err(MemoryError::InvalidMutation(
            "memory selection accepts at most 64 pins".into(),
        ));
    }
    let filter = SearchFilter {
        context: Some(request.context.clone()),
        ..Default::default()
    }
    .resolved()?;
    let enabled = request.host_budget.min(request.memory_cap) > 0;
    let pins = if enabled {
        resolve_pins(backend, &request.mind, &request.pins).await?
    } else {
        ResolvedPins {
            facts: vec![],
            replacements: vec![],
            missing: vec![],
        }
    };
    let task_signal = !request.query.trim().is_empty()
        && (request.intent == MemorySelectionIntent::Explicit || !low_signal(&request.query));
    let facts = if enabled && task_signal {
        service::context_facts_filtered(
            backend,
            &request.mind,
            Some(&request.query),
            request.fetch_limit.min(MAX_CANDIDATES),
            &filter,
        )
        .await?
    } else {
        vec![]
    };
    let episodes = if enabled
        && !low_signal(&request.query)
        && request.query.split_whitespace().count() >= 2
    {
        backend
            .search_episodes(&request.mind, &request.query, 4)
            .await?
    } else {
        vec![]
    };
    let mut selected = select_with_renderer(
        MemorySelectionInput {
            mind: &request.mind,
            query: &request.query,
            intent: request.intent,
            facts: &facts,
            pins: &pins.facts,
            episodes: &episodes,
            context: filter.context.as_ref().expect("resolved context"),
            host_budget: request.host_budget,
            memory_cap: request.memory_cap,
        },
        counter,
        renderer,
    )?;
    selected.report.pin_resolutions = pins.replacements;
    for id in pins.missing {
        selected
            .report
            .exclude(&id, MemoryExclusionReason::MissingPin);
    }
    Ok(selected)
}
