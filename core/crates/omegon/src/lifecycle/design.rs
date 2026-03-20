//! Design-tree parser and writer — frontmatter, sections, tree scanning, mutations.
//!
//! Parses markdown documents in docs/ with YAML frontmatter into DesignNode
//! and DocumentSections structs. Mutation functions write back to the same format.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::types::*;

/// Parse YAML frontmatter from a markdown document.
/// Returns None if no frontmatter delimiter found.
pub fn parse_frontmatter(content: &str) -> Option<HashMap<String, FrontmatterValue>> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];

    let mut result = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_array: Vec<String> = Vec::new();

    for line in yaml.lines() {
        // Array item: "  - something"
        if let Some(item) = line.strip_prefix("  - ").or_else(|| line.strip_prefix("- "))
            && current_key.is_some() {
                current_array.push(strip_quotes(item.trim()));
                continue;
            }

        // Flush previous key
        if let Some(key) = current_key.take() {
            if current_array.is_empty() {
                result.insert(key, FrontmatterValue::List(vec![]));
            } else {
                result.insert(key, FrontmatterValue::List(std::mem::take(&mut current_array)));
            }
        }

        // Key-value pair
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim();

            if value.is_empty() {
                current_key = Some(key);
                current_array = Vec::new();
            } else if value == "[]" {
                result.insert(key, FrontmatterValue::List(vec![]));
            } else if value.starts_with('[') && value.ends_with(']') {
                // Inline array: [a, b, c]
                let items: Vec<String> = value[1..value.len() - 1]
                    .split(',')
                    .map(|s| strip_quotes(s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect();
                result.insert(key, FrontmatterValue::List(items));
            } else {
                result.insert(key, FrontmatterValue::Scalar(strip_quotes(value)));
            }
        }
    }

    // Flush final key
    if let Some(key) = current_key {
        result.insert(key, FrontmatterValue::List(current_array));
    }

    Some(result)
}

/// A frontmatter value — either scalar or list.
#[derive(Debug, Clone)]
pub enum FrontmatterValue {
    Scalar(String),
    List(Vec<String>),
}

impl FrontmatterValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Scalar(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> &[String] {
        match self {
            Self::List(v) => v,
            _ => &[],
        }
    }
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        // Strip inline YAML comments
        s.split(" #").next().unwrap_or(s).trim().to_string()
    }
}

/// Extract the body after the frontmatter.
pub fn extract_body(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            after.trim_start_matches('\n')
        } else {
            content
        }
    } else {
        content
    }
}

/// Build a DesignNode from parsed frontmatter.
pub fn node_from_frontmatter(
    fm: &HashMap<String, FrontmatterValue>,
    file_path: PathBuf,
) -> Option<DesignNode> {
    let id = fm.get("id")?.as_str()?.to_string();
    let title = fm.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let status_str = fm.get("status").and_then(|v| v.as_str()).unwrap_or("seed");
    let status = NodeStatus::parse(status_str).unwrap_or(NodeStatus::Seed);

    Some(DesignNode {
        id,
        title,
        status,
        parent: fm.get("parent").and_then(|v| v.as_str()).map(String::from),
        tags: fm.get("tags").map(|v| v.as_list().to_vec()).unwrap_or_default(),
        dependencies: fm.get("dependencies").map(|v| v.as_list().to_vec()).unwrap_or_default(),
        related: fm.get("related").map(|v| v.as_list().to_vec()).unwrap_or_default(),
        open_questions: fm.get("open_questions").map(|v| v.as_list().to_vec()).unwrap_or_default(),
        branches: fm.get("branches").map(|v| v.as_list().to_vec()).unwrap_or_default(),
        openspec_change: fm.get("openspec_change").and_then(|v| v.as_str()).map(String::from),
        issue_type: fm.get("issue_type").and_then(|v| v.as_str()).and_then(IssueType::parse),
        priority: fm.get("priority").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
        file_path,
    })
}

/// Parse the structured sections from a design document body.
pub fn parse_sections(body: &str) -> DocumentSections {
    let mut sections = DocumentSections::default();

    // Split body into h2 sections
    let mut current_heading = "";
    let mut current_content = String::new();
    let mut h2_blocks: Vec<(&str, String)> = Vec::new();

    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if !current_heading.is_empty() || !current_content.trim().is_empty() {
                h2_blocks.push((current_heading, std::mem::take(&mut current_content)));
            }
            current_heading = heading.trim();
            current_content = String::new();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    if !current_heading.is_empty() || !current_content.trim().is_empty() {
        h2_blocks.push((current_heading, current_content));
    }

    for (heading, content) in h2_blocks {
        match heading {
            "Overview" => sections.overview = content.trim().to_string(),
            "Research" => sections.research = parse_research(&content),
            "Decisions" => sections.decisions = parse_decisions(&content),
            "Open Questions" => sections.open_questions = parse_questions(&content),
            "Implementation Notes" => parse_impl_notes(&content, &mut sections),
            _ => {} // Acceptance Criteria, extra sections — skip for Phase 1a
        }
    }

    sections
}

fn parse_research(content: &str) -> Vec<ResearchEntry> {
    let mut entries = Vec::new();
    let mut current_heading = String::new();
    let mut current_content = String::new();

    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            if !current_heading.is_empty() {
                entries.push(ResearchEntry {
                    heading: std::mem::take(&mut current_heading),
                    content: std::mem::take(&mut current_content).trim().to_string(),
                });
            }
            current_heading = heading.trim().to_string();
            current_content = String::new();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    if !current_heading.is_empty() {
        entries.push(ResearchEntry {
            heading: current_heading,
            content: current_content.trim().to_string(),
        });
    }

    entries
}

fn parse_decisions(content: &str) -> Vec<DesignDecision> {
    let mut decisions = Vec::new();
    let mut current_title = String::new();
    let mut current_status = String::new();
    let mut current_rationale = String::new();
    let mut in_decision = false;

    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            // Flush previous
            if in_decision && !current_title.is_empty() {
                decisions.push(DesignDecision {
                    title: std::mem::take(&mut current_title),
                    status: std::mem::take(&mut current_status),
                    rationale: std::mem::take(&mut current_rationale).trim().to_string(),
                });
            }
            current_title = heading.trim().to_string();
            in_decision = true;
        } else if in_decision {
            if let Some(rest) = line.strip_prefix("**Status:**") {
                current_status = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("**Rationale:**") {
                current_rationale = rest.trim().to_string();
                current_rationale.push('\n');
            } else if !current_rationale.is_empty() || line.trim().is_empty() {
                // Continue rationale
                current_rationale.push_str(line);
                current_rationale.push('\n');
            }
        }
    }
    if in_decision && !current_title.is_empty() {
        decisions.push(DesignDecision {
            title: current_title,
            status: current_status,
            rationale: current_rationale.trim().to_string(),
        });
    }

    decisions
}

fn parse_questions(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .map(|s| s.to_string())
        })
        .collect()
}

fn parse_impl_notes(content: &str, sections: &mut DocumentSections) {
    let mut in_file_scope = false;
    let mut in_constraints = false;

    for line in content.lines() {
        if line.starts_with("### File Scope") || line.starts_with("### file_scope") {
            in_file_scope = true;
            in_constraints = false;
        } else if line.starts_with("### Constraints") || line.starts_with("### constraints") {
            in_file_scope = false;
            in_constraints = true;
        } else if line.starts_with("### ") {
            in_file_scope = false;
            in_constraints = false;
        } else if in_file_scope {
            // Format: - `path` — description (action)
            if let Some(rest) = line.trim().strip_prefix("- ")
                && let Some((path_part, desc)) = rest.split_once(" — ").or_else(|| rest.split_once(" - ")) {
                    let path = path_part.trim().trim_matches('`').to_string();
                    let (description, action) = if desc.ends_with(')') {
                        if let Some(paren) = desc.rfind('(') {
                            (desc[..paren].trim().to_string(), Some(desc[paren + 1..desc.len() - 1].to_string()))
                        } else {
                            (desc.to_string(), None)
                        }
                    } else {
                        (desc.to_string(), None)
                    };
                    sections.impl_file_scope.push(FileScope { path, description, action });
                }
        } else if in_constraints
            && let Some(rest) = line.trim().strip_prefix("- ") {
                sections.impl_constraints.push(rest.to_string());
            }
    }
}

/// Scan a docs/ directory for design documents and build a tree.
pub fn scan_design_docs(docs_dir: &Path) -> HashMap<String, DesignNode> {
    let mut nodes = HashMap::new();

    let entries = match fs::read_dir(docs_dir) {
        Ok(entries) => entries,
        Err(_) => return nodes,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(fm) = parse_frontmatter(&content)
            && let Some(node) = node_from_frontmatter(&fm, path) {
                nodes.insert(node.id.clone(), node);
            }
    }

    // Also scan docs/design/ subdirectory if it exists
    let design_dir = docs_dir.join("design");
    if design_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&design_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if let Some(fm) = parse_frontmatter(&content)
                    && let Some(node) = node_from_frontmatter(&fm, path) {
                        nodes.insert(node.id.clone(), node);
                    }
            }
        }

    nodes
}

/// Get children of a node.
pub fn get_children<'a>(
    nodes: &'a HashMap<String, DesignNode>,
    parent_id: &str,
) -> Vec<&'a DesignNode> {
    nodes
        .values()
        .filter(|n| n.parent.as_deref() == Some(parent_id))
        .collect()
}

/// Read and parse the sections of a design node document.
pub fn read_node_sections(node: &DesignNode) -> Option<DocumentSections> {
    let content = fs::read_to_string(&node.file_path).ok()?;
    let body = extract_body(&content);
    Some(parse_sections(body))
}

/// Build a context injection string for a focused design node.
/// Includes: overview, key decisions, open questions.
pub fn build_context_injection(node: &DesignNode, sections: &DocumentSections) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "[Design: {} {} — {}]",
        node.status.icon(),
        node.id,
        node.title
    ));

    if !sections.overview.is_empty() {
        let overview = if sections.overview.len() > 500 {
            format!("{}...", &sections.overview[..500])
        } else {
            sections.overview.clone()
        };
        lines.push(format!("Overview: {overview}"));
    }

    let decided: Vec<_> = sections
        .decisions
        .iter()
        .filter(|d| d.status == "decided")
        .collect();
    if !decided.is_empty() {
        lines.push("Key decisions:".to_string());
        for d in decided {
            lines.push(format!("  - {} — {}", d.title, truncate(&d.rationale, 150)));
        }
    }

    if !node.open_questions.is_empty() {
        lines.push("Open questions:".to_string());
        for q in &node.open_questions {
            lines.push(format!("  - {q}"));
        }
    }

    lines.join("\n")
}

// ═══════════════════════════════════════════════════════════════════════════
// Mutation functions — write design documents
// ═══════════════════════════════════════════════════════════════════════════

/// Serialize a DesignNode back to YAML frontmatter + markdown body.
fn serialize_frontmatter(node: &DesignNode) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("id: {}", node.id),
        format!("title: \"{}\"", node.title.replace('"', "\\\"" )),
        format!("status: {}", node.status.as_str()),
    ];

    if let Some(ref parent) = node.parent {
        lines.push(format!("parent: {parent}"));
    }

    if !node.tags.is_empty() {
        // Quote tags that contain commas or spaces to prevent parse ambiguity
        let formatted: Vec<String> = node.tags.iter().map(|t| {
            if t.contains(',') || t.contains(' ') {
                format!("\"{}\"", t.replace('"', "\\\""))
            } else {
                t.clone()
            }
        }).collect();
        lines.push(format!("tags: [{}]", formatted.join(", ")));
    } else {
        lines.push("tags: []".into());
    }

    if !node.open_questions.is_empty() {
        lines.push("open_questions:".into());
        for q in &node.open_questions {
            lines.push(format!("  - \"{}\"", q.replace('"', "\\\"")));
        }
    } else {
        lines.push("open_questions: []".into());
    }

    if !node.dependencies.is_empty() {
        lines.push("dependencies:".into());
        for d in &node.dependencies {
            lines.push(format!("  - {d}"));
        }
    } else {
        lines.push("dependencies: []".into());
    }

    if !node.related.is_empty() {
        lines.push("related:".into());
        for r in &node.related {
            lines.push(format!("  - {r}"));
        }
    } else {
        lines.push("related: []".into());
    }

    if !node.branches.is_empty() {
        lines.push("branches:".into());
        for b in &node.branches {
            lines.push(format!("  - {b}"));
        }
    }

    if let Some(ref change) = node.openspec_change {
        lines.push(format!("openspec_change: {change}"));
    }

    if let Some(ref issue_type) = node.issue_type {
        let s = match issue_type {
            IssueType::Epic => "epic",
            IssueType::Feature => "feature",
            IssueType::Task => "task",
            IssueType::Bug => "bug",
            IssueType::Chore => "chore",
        };
        lines.push(format!("issue_type: {s}"));
    }

    if let Some(priority) = node.priority {
        lines.push(format!("priority: {priority}"));
    }

    lines.push("---".into());
    lines.join("\n")
}

/// Serialize a full design document (frontmatter + sections).
/// Uses writeln! to a String to avoid double-newline accumulation.
fn serialize_document(node: &DesignNode, sections: &DocumentSections) -> String {
    use std::fmt::Write;
    let mut out = serialize_frontmatter(node);
    writeln!(out).unwrap();
    writeln!(out, "\n# {}", node.title).unwrap();

    if !sections.overview.is_empty() {
        writeln!(out, "\n## Overview\n").unwrap();
        // Trim trailing whitespace from overview to prevent accumulation
        write!(out, "{}", sections.overview.trim_end()).unwrap();
        writeln!(out).unwrap();
    }

    if !sections.research.is_empty() {
        writeln!(out, "\n## Research").unwrap();
        for entry in &sections.research {
            writeln!(out, "\n### {}\n", entry.heading).unwrap();
            write!(out, "{}", entry.content.trim_end()).unwrap();
            writeln!(out).unwrap();
        }
    }

    if !sections.decisions.is_empty() {
        writeln!(out, "\n## Decisions").unwrap();
        for dec in &sections.decisions {
            writeln!(out, "\n### {}\n", dec.title).unwrap();
            writeln!(out, "**Status:** {}\n", dec.status).unwrap();
            writeln!(out, "**Rationale:** {}", dec.rationale).unwrap();
        }
    }

    if !sections.open_questions.is_empty() {
        writeln!(out, "\n## Open Questions\n").unwrap();
        for q in &sections.open_questions {
            writeln!(out, "- {q}").unwrap();
        }
    }

    if !sections.impl_file_scope.is_empty() || !sections.impl_constraints.is_empty() {
        writeln!(out, "\n## Implementation Notes").unwrap();

        if !sections.impl_file_scope.is_empty() {
            writeln!(out, "\n### File Scope\n").unwrap();
            for fs in &sections.impl_file_scope {
                let action = fs.action.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
                writeln!(out, "- `{}` — {}{}", fs.path, fs.description, action).unwrap();
            }
        }

        if !sections.impl_constraints.is_empty() {
            writeln!(out, "\n### Constraints\n").unwrap();
            for c in &sections.impl_constraints {
                writeln!(out, "- {c}").unwrap();
            }
        }
    }

    // Ensure file ends with exactly one newline
    let trimmed = out.trim_end().to_string();
    trimmed + "\n"
}

/// Create a new design node document.
pub fn create_node(
    docs_dir: &Path,
    id: &str,
    title: &str,
    parent: Option<&str>,
    status: Option<&str>,
    tags: &[String],
    overview: &str,
) -> anyhow::Result<DesignNode> {
    let _ = fs::create_dir_all(docs_dir);
    let file_path = docs_dir.join(format!("{id}.md"));

    if file_path.exists() {
        anyhow::bail!("Design node '{id}' already exists at {}", file_path.display());
    }

    let status = status
        .and_then(NodeStatus::parse)
        .unwrap_or(NodeStatus::Seed);

    let node = DesignNode {
        id: id.to_string(),
        title: title.to_string(),
        status,
        parent: parent.map(String::from),
        tags: tags.to_vec(),
        dependencies: vec![],
        related: vec![],
        open_questions: vec![],
        branches: vec![],
        openspec_change: None,
        issue_type: None,
        priority: None,
        file_path: file_path.clone(),
    };

    let sections = DocumentSections {
        overview: overview.to_string(),
        ..Default::default()
    };

    let content = serialize_document(&node, &sections);
    fs::write(&file_path, content)?;
    Ok(node)
}

/// Update a design node's frontmatter by rewriting the document.
/// The `mutate` closure receives the mutable node for changes.
pub fn update_node(
    node: &mut DesignNode,
    mutate: impl FnOnce(&mut DesignNode),
) -> anyhow::Result<()> {
    // Read existing content to preserve the body
    let content = fs::read_to_string(&node.file_path)?;
    let body = extract_body(&content);
    let mut sections = parse_sections(body);

    // Apply the mutation
    mutate(node);

    // Sync sections with mutated node state — frontmatter is the source of truth
    // for open_questions, but the body also has an ## Open Questions section.
    // Without this sync, questions added/removed via frontmatter mutation would
    // be overwritten by the stale body section on the next update_node call.
    sections.open_questions = node.open_questions.clone();

    // Rewrite
    let new_content = serialize_document(node, &sections);
    fs::write(&node.file_path, &new_content)?;
    Ok(())
}

/// Append a research entry to a design document.
pub fn add_research(node: &DesignNode, heading: &str, content_text: &str) -> anyhow::Result<()> {
    let content = fs::read_to_string(&node.file_path)?;
    let body = extract_body(&content);
    let mut sections = parse_sections(body);

    sections.research.push(ResearchEntry {
        heading: heading.to_string(),
        content: content_text.to_string(),
    });

    let new_content = serialize_document(node, &sections);
    fs::write(&node.file_path, &new_content)?;
    Ok(())
}

/// Add a decision to a design document.
pub fn add_decision(
    node: &DesignNode,
    title: &str,
    status: &str,
    rationale: &str,
) -> anyhow::Result<()> {
    let content = fs::read_to_string(&node.file_path)?;
    let body = extract_body(&content);
    let mut sections = parse_sections(body);

    sections.decisions.push(DesignDecision {
        title: title.to_string(),
        status: status.to_string(),
        rationale: rationale.to_string(),
    });

    let new_content = serialize_document(node, &sections);
    fs::write(&node.file_path, &new_content)?;
    Ok(())
}

/// Add implementation notes to a design document.
pub fn add_impl_notes(
    node: &DesignNode,
    file_scope: &[FileScope],
    constraints: &[String],
) -> anyhow::Result<()> {
    let content = fs::read_to_string(&node.file_path)?;
    let body = extract_body(&content);
    let mut sections = parse_sections(body);

    sections.impl_file_scope.extend(file_scope.iter().cloned());
    sections.impl_constraints.extend(constraints.iter().cloned());

    let new_content = serialize_document(node, &sections);
    fs::write(&node.file_path, &new_content)?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DOC: &str = r#"---
id: test-node
title: "Test Node — with special chars"
status: exploring
parent: parent-node
tags: [rust, test, lifecycle]
open_questions:
  - How does X work?
  - What about Y?
dependencies: []
related: [other-node]
branches: [feature/test-node]
openspec_change: test-change
issue_type: feature
priority: 2
---

# Test Node — with special chars

## Overview

This is the overview text.
It spans multiple lines.

## Research

### First research topic

Content of the first topic.

### Second research topic

Content of the second topic.

## Decisions

### Use approach A over B

**Status:** decided

**Rationale:** Approach A is simpler and handles all edge cases.

### Consider approach C

**Status:** exploring

**Rationale:** Approach C might work but needs more investigation.

## Open Questions

- How does X work?
- What about Y?

## Implementation Notes

### File Scope

- `src/foo.rs` — Main implementation (new)
- `src/bar.rs` — Updated types (modified)

### Constraints

- Must handle UTF-8 correctly
- No external YAML parser dependency
"#;

    #[test]
    fn parse_frontmatter_full() {
        let fm = parse_frontmatter(SAMPLE_DOC).unwrap();
        assert_eq!(fm.get("id").unwrap().as_str(), Some("test-node"));
        assert_eq!(fm.get("title").unwrap().as_str(), Some("Test Node — with special chars"));
        assert_eq!(fm.get("status").unwrap().as_str(), Some("exploring"));
        assert_eq!(fm.get("parent").unwrap().as_str(), Some("parent-node"));
        assert_eq!(fm.get("tags").unwrap().as_list(), &["rust", "test", "lifecycle"]);
        assert_eq!(fm.get("open_questions").unwrap().as_list().len(), 2);
        assert_eq!(fm.get("dependencies").unwrap().as_list().len(), 0);
        assert_eq!(fm.get("related").unwrap().as_list(), &["other-node"]);
        assert_eq!(fm.get("branches").unwrap().as_list(), &["feature/test-node"]);
        assert_eq!(fm.get("openspec_change").unwrap().as_str(), Some("test-change"));
        assert_eq!(fm.get("issue_type").unwrap().as_str(), Some("feature"));
        assert_eq!(fm.get("priority").unwrap().as_str(), Some("2"));
    }

    #[test]
    fn node_from_frontmatter_full() {
        let fm = parse_frontmatter(SAMPLE_DOC).unwrap();
        let node = node_from_frontmatter(&fm, PathBuf::from("docs/test-node.md")).unwrap();
        assert_eq!(node.id, "test-node");
        assert_eq!(node.status, NodeStatus::Exploring);
        assert_eq!(node.parent.as_deref(), Some("parent-node"));
        assert_eq!(node.tags, vec!["rust", "test", "lifecycle"]);
        assert_eq!(node.open_questions.len(), 2);
        assert_eq!(node.issue_type, Some(IssueType::Feature));
        assert_eq!(node.priority, Some(2));
        assert_eq!(node.openspec_change.as_deref(), Some("test-change"));
    }

    #[test]
    fn parse_sections_full() {
        let body = extract_body(SAMPLE_DOC);
        let sections = parse_sections(body);

        assert!(sections.overview.contains("overview text"));
        assert_eq!(sections.research.len(), 2);
        assert_eq!(sections.research[0].heading, "First research topic");
        assert!(sections.research[0].content.contains("first topic"));

        assert_eq!(sections.decisions.len(), 2);
        assert_eq!(sections.decisions[0].title, "Use approach A over B");
        assert_eq!(sections.decisions[0].status, "decided");
        assert!(sections.decisions[0].rationale.contains("simpler"));
        assert_eq!(sections.decisions[1].status, "exploring");

        assert_eq!(sections.open_questions.len(), 2);
        assert_eq!(sections.open_questions[0], "How does X work?");

        assert_eq!(sections.impl_file_scope.len(), 2);
        assert_eq!(sections.impl_file_scope[0].path, "src/foo.rs");
        assert_eq!(sections.impl_file_scope[0].action.as_deref(), Some("new"));
        assert_eq!(sections.impl_file_scope[1].action.as_deref(), Some("modified"));

        assert_eq!(sections.impl_constraints.len(), 2);
        assert!(sections.impl_constraints[0].contains("UTF-8"));
    }

    #[test]
    fn extract_body_strips_frontmatter() {
        let body = extract_body(SAMPLE_DOC);
        assert!(body.starts_with("# Test Node"));
        assert!(!body.contains("---\nid:"));
    }

    #[test]
    fn context_injection_format() {
        let node = DesignNode {
            id: "test".into(),
            title: "Test Node".into(),
            status: NodeStatus::Decided,
            parent: None,
            tags: vec![],
            dependencies: vec![],
            related: vec![],
            open_questions: vec!["How?".into()],
            branches: vec![],
            openspec_change: None,
            issue_type: None,
            priority: None,
            file_path: PathBuf::new(),
        };
        let sections = DocumentSections {
            overview: "Short overview".into(),
            decisions: vec![DesignDecision {
                title: "Use X".into(),
                status: "decided".into(),
                rationale: "Because Y".into(),
            }],
            ..Default::default()
        };
        let injection = build_context_injection(&node, &sections);
        assert!(injection.contains("● test — Test Node"));
        assert!(injection.contains("Short overview"));
        assert!(injection.contains("Use X"));
        assert!(injection.contains("How?"));
    }

    #[test]
    fn empty_frontmatter_returns_none() {
        assert!(parse_frontmatter("No frontmatter here").is_none());
    }

    #[test]
    fn frontmatter_missing_id_returns_none() {
        let content = "---\ntitle: No ID\nstatus: seed\n---\n# No ID";
        let fm = parse_frontmatter(content).unwrap();
        assert!(node_from_frontmatter(&fm, PathBuf::new()).is_none());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn scan_real_docs_directory() {
        // Test against the actual Omegon docs/ directory
        let docs_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap()
            .join("docs");

        if !docs_dir.exists() {
            tracing::debug!("Skipping: docs/ not found at {}", docs_dir.display());
            return;
        }

        let nodes = scan_design_docs(&docs_dir);
        if nodes.is_empty() {
            tracing::debug!("Skipping: no design nodes found in {}", docs_dir.display());
            return;
        }

        // Verify known nodes exist
        let known_ids = ["rust-phase-1", "rust-compaction", "rust-lifecycle-crates"];
        for id in &known_ids {
            assert!(nodes.contains_key(*id), "Missing expected node: {id}");
        }

        // Verify all nodes have valid status
        for (id, node) in &nodes {
            assert!(!id.is_empty(), "Node has empty id");
            assert!(!node.title.is_empty(), "Node {id} has empty title");
            // Status was parsed from frontmatter so it's guaranteed valid
        }

        // Parse sections for a known node
        if let Some(node) = nodes.get("rust-lifecycle-crates") {
            let sections = read_node_sections(node).unwrap();
            assert!(!sections.overview.is_empty(), "rust-lifecycle-crates should have overview");
            assert!(!sections.decisions.is_empty(), "rust-lifecycle-crates should have decisions");
        }

        tracing::debug!("Scanned {} design nodes from {}", nodes.len(), docs_dir.display());
    }
}

#[cfg(test)]
mod mutation_tests {
    use super::*;

    #[test]
    fn create_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");

        let node = create_node(&docs, "new-node", "New Node", Some("parent"), None, &["rust".into(), "test".into()], "Overview text.").unwrap();
        assert_eq!(node.id, "new-node");
        assert_eq!(node.status, NodeStatus::Seed);

        // Read it back
        let content = fs::read_to_string(&node.file_path).unwrap();
        let fm = parse_frontmatter(&content).unwrap();
        let read_node = node_from_frontmatter(&fm, node.file_path.clone()).unwrap();
        assert_eq!(read_node.id, "new-node");
        assert_eq!(read_node.title, "New Node");
        assert_eq!(read_node.parent.as_deref(), Some("parent"));
        assert_eq!(read_node.tags, vec!["rust", "test"]);
    }

    #[test]
    fn create_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        create_node(&docs, "dup", "Dup", None, None, &[], "").unwrap();
        assert!(create_node(&docs, "dup", "Dup2", None, None, &[], "").is_err());
    }

    #[test]
    fn update_node_preserves_body() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        let mut node = create_node(&docs, "upd", "Update Test", None, None, &[], "Original overview.").unwrap();

        // Add a decision first
        add_decision(&node, "Use X", "decided", "Because Y").unwrap();

        // Now update status — body should be preserved
        let content_before = fs::read_to_string(&node.file_path).unwrap();
        assert!(content_before.contains("Use X"));

        update_node(&mut node, |n| { n.status = NodeStatus::Decided; }).unwrap();
        assert_eq!(node.status, NodeStatus::Decided);

        let content_after = fs::read_to_string(&node.file_path).unwrap();
        assert!(content_after.contains("Use X"), "decision should be preserved after status update");
        assert!(content_after.contains("decided"), "frontmatter should show new status");
        assert!(content_after.contains("Original overview"), "overview should be preserved");
    }

    #[test]
    fn add_research_appends() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        let node = create_node(&docs, "res", "Research Test", None, None, &[], "").unwrap();

        add_research(&node, "First Topic", "Content of first.").unwrap();
        add_research(&node, "Second Topic", "Content of second.").unwrap();

        let sections = read_node_sections(&node).unwrap();
        assert_eq!(sections.research.len(), 2);
        assert_eq!(sections.research[0].heading, "First Topic");
        assert_eq!(sections.research[1].heading, "Second Topic");
    }

    #[test]
    fn add_impl_notes_appends() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        let node = create_node(&docs, "impl", "Impl Test", None, None, &[], "").unwrap();

        add_impl_notes(&node, &[
            FileScope { path: "src/foo.rs".into(), description: "Main impl".into(), action: Some("new".into()) },
        ], &["Must handle UTF-8".into()]).unwrap();

        let sections = read_node_sections(&node).unwrap();
        assert_eq!(sections.impl_file_scope.len(), 1);
        assert_eq!(sections.impl_file_scope[0].path, "src/foo.rs");
        assert_eq!(sections.impl_constraints.len(), 1);
    }

    #[test]
    fn update_questions() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        let mut node = create_node(&docs, "q", "Q Test", None, None, &[], "").unwrap();

        update_node(&mut node, |n| {
            n.open_questions.push("Question 1?".into());
            n.open_questions.push("Question 2?".into());
        }).unwrap();

        let content = fs::read_to_string(&node.file_path).unwrap();
        let fm = parse_frontmatter(&content).unwrap();
        let read = node_from_frontmatter(&fm, node.file_path.clone()).unwrap();
        assert_eq!(read.open_questions.len(), 2);

        // Remove one
        update_node(&mut node, |n| {
            n.open_questions.retain(|q| q != "Question 1?");
        }).unwrap();

        let content = fs::read_to_string(&node.file_path).unwrap();
        let fm = parse_frontmatter(&content).unwrap();
        let read = node_from_frontmatter(&fm, node.file_path.clone()).unwrap();
        assert_eq!(read.open_questions.len(), 1);
        assert_eq!(read.open_questions[0], "Question 2?");
    }

    #[test]
    fn serialization_handles_special_chars() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        let node = create_node(&docs, "special", "Node with \"quotes\" and — dashes", None, None, &[], "").unwrap();

        let content = fs::read_to_string(&node.file_path).unwrap();
        let fm = parse_frontmatter(&content).unwrap();
        let read = node_from_frontmatter(&fm, node.file_path).unwrap();
        assert!(read.title.contains("quotes"));
        assert!(read.title.contains("—"));
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;

    #[test]
    fn serialize_parse_roundtrip_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");

        // Create a node with all sections populated
        let node = create_node(&docs, "rt", "Round Trip Test", Some("parent"), Some("exploring"),
            &["rust".into(), "test".into()], "Overview text here.").unwrap();

        // Add content to all sections
        add_research(&node, "Topic A", "Research content A.").unwrap();
        add_decision(&node, "Use X", "decided", "Because Y.").unwrap();
        add_impl_notes(&node, &[FileScope {
            path: "src/foo.rs".into(),
            description: "Main impl".into(),
            action: Some("new".into()),
        }], &["Must handle UTF-8".into()]).unwrap();

        // Add a question
        let mut node = {
            let content = fs::read_to_string(&node.file_path).unwrap();
            let fm = parse_frontmatter(&content).unwrap();
            node_from_frontmatter(&fm, node.file_path.clone()).unwrap()
        };
        update_node(&mut node, |n| {
            n.open_questions.push("What about Z?".into());
        }).unwrap();

        // Read the content after first write
        let content_v1 = fs::read_to_string(&node.file_path).unwrap();

        // Do a no-op update (should not change the file)
        update_node(&mut node, |_| {}).unwrap();
        let content_v2 = fs::read_to_string(&node.file_path).unwrap();

        assert_eq!(content_v1, content_v2,
            "no-op update should produce identical output\nv1:\n{content_v1}\n\nv2:\n{content_v2}");

        // Do another no-op update (third write — should still be stable)
        update_node(&mut node, |_| {}).unwrap();
        let content_v3 = fs::read_to_string(&node.file_path).unwrap();
        assert_eq!(content_v2, content_v3, "third write should still be stable");
    }

    #[test]
    fn question_sync_between_frontmatter_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");

        let mut node = create_node(&docs, "qs", "Question Sync", None, None, &[], "").unwrap();

        // Add questions via update_node
        update_node(&mut node, |n| {
            n.open_questions.push("Q1?".into());
            n.open_questions.push("Q2?".into());
        }).unwrap();

        // Remove one question
        update_node(&mut node, |n| {
            n.open_questions.retain(|q| q != "Q1?");
        }).unwrap();

        // Re-read and verify both frontmatter and body are in sync
        let content = fs::read_to_string(&node.file_path).unwrap();
        let fm = parse_frontmatter(&content).unwrap();
        let read_node = node_from_frontmatter(&fm, node.file_path.clone()).unwrap();

        assert_eq!(read_node.open_questions, vec!["Q2?"],
            "frontmatter should only have Q2");

        let body = extract_body(&content);
        let sections = parse_sections(body);
        assert_eq!(sections.open_questions, vec!["Q2?"],
            "body should only have Q2");

        // Another update should be stable
        let mut node = read_node;
        node.file_path = docs.join("qs.md");
        update_node(&mut node, |_| {}).unwrap();
        let content_after = fs::read_to_string(&node.file_path).unwrap();
        let sections_after = parse_sections(extract_body(&content_after));
        assert_eq!(sections_after.open_questions, vec!["Q2?"],
            "questions should remain stable after re-write");
    }
}
