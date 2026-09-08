//! MarkdownRenderer — default ContextRenderer for LLM system prompt injection.

use crate::backend::ContextRenderer;
use crate::types::*;

/// Renders facts and episodes as a markdown block for LLM context injection.
pub struct MarkdownRenderer;

impl ContextRenderer for MarkdownRenderer {
    fn render_context(
        &self,
        facts: &[Fact],
        episodes: &[Episode],
        working_memory: &[Fact],
        max_chars: usize,
    ) -> RenderedContext {
        const PREAMBLE: &str = "# Project Memory\n_Use `memory_store` proactively when you learn facts worth persisting. Use `memory_recall` before non-trivial tasks to surface relevant context._";

        let preamble_chars = PREAMBLE.chars().count();
        if preamble_chars > max_chars {
            return RenderedContext {
                markdown: String::new(),
                facts_injected: 0,
                episodes_injected: 0,
                char_count: 0,
                budget_exhausted: true,
            };
        }

        let mut markdown = PREAMBLE.to_string();
        let mut char_count = preamble_chars;
        let mut facts_injected = 0;
        let mut budget_exhausted = false;

        let append_block = |markdown: &mut String, char_count: &mut usize, block: &str| {
            let added = 2usize.saturating_add(block.chars().count());
            if char_count.saturating_add(added) > max_chars {
                false
            } else {
                markdown.push_str("\n\n");
                markdown.push_str(block);
                *char_count += added;
                true
            }
        };

        // Preserve the domain selector's ordering. Section presentation must not
        // promote irrelevant Architecture facts ahead of a ranked constraint.
        let mut included_ids = std::collections::HashSet::new();
        let mut previous_heading = String::new();
        for (fact, pinned) in working_memory
            .iter()
            .map(|fact| (fact, true))
            .chain(facts.iter().map(|fact| (fact, false)))
        {
            if fact.status != FactStatus::Active || included_ids.contains(&fact.id) {
                continue;
            }
            let heading = if pinned {
                "Working Memory (pinned)".to_string()
            } else {
                serde_json::to_string(&fact.section)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string()
            };
            let line = format!("- [{}] {}", fact.id, fact.content);
            let block = if heading == previous_heading {
                line
            } else {
                format!("## {heading}\n{line}")
            };
            if append_block(&mut markdown, &mut char_count, &block) {
                previous_heading = heading;
                included_ids.insert(&fact.id);
                facts_injected += 1;
            } else {
                budget_exhausted = true;
            }
        }

        // Episodes
        let mut episodes_injected = 0;
        for ep in episodes {
            let heading = if episodes_injected == 0 {
                "## Recent Sessions\n"
            } else {
                ""
            };
            let block = format!("{heading}### {}: {}\n{}", ep.date, ep.title, ep.narrative);
            if append_block(&mut markdown, &mut char_count, &block) {
                episodes_injected += 1;
            } else {
                budget_exhausted = true;
            }
        }

        if facts_injected == 0 && episodes_injected == 0 {
            markdown.clear();
            char_count = 0;
        }

        RenderedContext {
            markdown,
            facts_injected,
            episodes_injected,
            char_count,
            budget_exhausted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fact(section: Section, content: &str) -> Fact {
        Fact {
            id: content.into(),
            mind: "test".into(),
            content: content.into(),
            section,
            status: FactStatus::Active,
            confidence: 1.0,
            reinforcement_count: 1,
            decay_rate: 0.05,
            decay_profile: DecayProfileName::Standard,
            last_reinforced: "2026-01-01".into(),
            created_at: "2026-01-01".into(),
            version: 1,
            superseded_by: None,
            source: None,
            content_hash: None,
            last_accessed: None,
            created_session: None,
            superseded_at: None,
            archived_at: None,
            jj_change_id: None,
            persona_id: None,
            layer: "project".into(),
            tags: vec![],
        }
    }

    #[test]
    fn empty_facts_produce_empty_markdown() {
        let r = MarkdownRenderer;
        let ctx = r.render_context(&[], &[], &[], 12000);
        assert!(ctx.markdown.is_empty());
        assert_eq!(ctx.facts_injected, 0);
    }

    #[test]
    fn renders_facts_by_section() {
        let r = MarkdownRenderer;
        let facts = vec![
            make_fact(Section::Architecture, "System uses microservices"),
            make_fact(Section::Decisions, "Chose PostgreSQL over MySQL"),
        ];
        let ctx = r.render_context(&facts, &[], &[], 12000);
        assert!(ctx.markdown.contains("Architecture"));
        assert!(ctx.markdown.contains("microservices"));
        assert!(ctx.markdown.contains("Decisions"));
        assert!(ctx.markdown.contains("PostgreSQL"));
        assert_eq!(ctx.facts_injected, 2);
    }

    #[test]
    fn respects_budget() {
        let r = MarkdownRenderer;
        let facts: Vec<Fact> = (0..100)
            .map(|i| {
                make_fact(
                    Section::Architecture,
                    &format!("Fact number {i} with some content padding to use space"),
                )
            })
            .collect();
        let ctx = r.render_context(&facts, &[], &[], 500);
        assert!(ctx.budget_exhausted);
        assert!(ctx.facts_injected < 100);
        assert!(ctx.markdown.chars().count() <= 500);
        assert_eq!(ctx.char_count, ctx.markdown.chars().count());
    }

    #[test]
    fn tiny_budget_returns_no_partial_markdown_or_utf8_panic() {
        let r = MarkdownRenderer;
        let facts = vec![make_fact(Section::Architecture, "unicode: 🧠 memory")];
        let ctx = r.render_context(&facts, &[], &[], 8);
        assert!(ctx.markdown.is_empty());
        assert!(ctx.budget_exhausted);
    }

    #[test]
    fn working_memory_first() {
        let r = MarkdownRenderer;
        let facts = vec![make_fact(Section::Architecture, "Regular fact")];
        let wm = vec![make_fact(Section::Decisions, "Pinned important fact")];
        let ctx = r.render_context(&facts, &[], &wm, 12000);
        let wm_pos = ctx.markdown.find("Pinned important").unwrap();
        let regular_pos = ctx.markdown.find("Regular fact").unwrap();
        assert!(wm_pos < regular_pos, "working memory should come first");
    }
}
