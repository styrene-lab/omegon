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
        let mut lines = Vec::new();
        let mut char_count = 0;
        let mut facts_injected = 0;
        let mut budget_exhausted = false;

        // Working memory first (highest priority)
        if !working_memory.is_empty() {
            lines.push("## Working Memory (pinned)".to_string());
            for f in working_memory {
                let line = format!("- [{}] {}", f.id, f.content);
                if char_count + line.len() > max_chars {
                    budget_exhausted = true;
                    break;
                }
                char_count += line.len() + 1;
                lines.push(line);
                facts_injected += 1;
            }
            lines.push(String::new());
        }

        // Group facts by section
        let sections = [
            Section::Architecture,
            Section::Decisions,
            Section::Constraints,
            Section::KnownIssues,
            Section::PatternsConventions,
            Section::Specs,
            Section::RecentWork,
        ];

        let section_descriptions = [
            "_System structure, component relationships, key abstractions_",
            "_Choices made and their rationale_",
            "_Requirements, limitations, environment details_",
            "_Bugs, flaky tests, workarounds_",
            "_Code style, project conventions, common approaches_",
            "_Active specifications and design contracts_",
            "_Recent session activity_",
        ];

        for (section, desc) in sections.iter().zip(section_descriptions.iter()) {
            let section_facts: Vec<&Fact> = facts
                .iter()
                .filter(|f| &f.section == section && f.status == FactStatus::Active)
                .collect();
            if section_facts.is_empty() {
                continue;
            }

            let header = format!(
                "## {}\n{}",
                serde_json::to_string(section)
                    .unwrap_or_default()
                    .trim_matches('"'),
                desc
            );
            if char_count + header.len() > max_chars {
                budget_exhausted = true;
                break;
            }
            char_count += header.len() + 1;
            lines.push(header);

            for f in section_facts {
                let line = format!("- {}", f.content);
                if char_count + line.len() > max_chars {
                    budget_exhausted = true;
                    break;
                }
                char_count += line.len() + 1;
                lines.push(line);
                facts_injected += 1;
            }
            lines.push(String::new());
            if budget_exhausted {
                break;
            }
        }

        // Episodes
        let mut episodes_injected = 0;
        if !episodes.is_empty() && !budget_exhausted {
            lines.push("## Recent Sessions".to_string());
            for ep in episodes {
                let line = format!("### {}: {}\n{}", ep.date, ep.title, ep.narrative);
                if char_count + line.len() > max_chars {
                    budget_exhausted = true;
                    break;
                }
                char_count += line.len() + 1;
                lines.push(line);
                episodes_injected += 1;
            }
        }

        let markdown = if lines.is_empty() {
            String::new()
        } else {
            format!(
                "# Project Memory\n\
                _Use `memory_store` proactively when you learn facts worth persisting. \
                Use `memory_recall` before non-trivial tasks to surface relevant context._\n\n{}",
                lines.join("\n")
            )
        };

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
            id: "test".into(),
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
        assert!(ctx.char_count <= 500 + 100); // allow some overhead
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
