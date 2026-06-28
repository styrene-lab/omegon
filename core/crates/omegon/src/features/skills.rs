//! Skills feature — exposes the skills surface as agent-callable tools.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::features::persona::SharedAugmentRegistry;

pub struct SkillsFeature {
    registry: SharedAugmentRegistry,
}

impl SkillsFeature {
    pub fn new(registry: SharedAugmentRegistry) -> Self {
        Self { registry }
    }

    fn reload_skills(&self) {
        if let Ok(cwd) = std::env::current_dir() {
            self.registry.lock().load_skills(&cwd);
        }
    }
}

#[async_trait]
impl Feature for SkillsFeature {
    fn name(&self) -> &str {
        "skills"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_LIST.into(),
                label: "skills_list".into(),
                description: "List resolved Omegon skills with source, editability, reloadability, shadow, and conflict metadata.".into(),
                parameters: json!({ "type": "object", "properties": {} }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_GET.into(),
                label: "skills_get".into(),
                description: "Read one resolved skill's manifest, body, path, and source metadata.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Skill name to inspect" }
                    },
                    "required": ["name"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::TargetedRepoInspection],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_RELOAD.into(),
                label: "skills_reload".into(),
                description: "Reload user and project skills into the current agent session.".into(),
                parameters: json!({ "type": "object", "properties": {} }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_CREATE.into(),
                label: "skills_create".into(),
                description: "Create or overwrite a project-local or user-level Omegon SKILL.md from explicit manifest fields and markdown body.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Kebab-case or human-readable skill name" },
                        "description": { "type": "string", "description": "One-line skill description" },
                        "body": { "type": "string", "description": "Markdown directive body after frontmatter" },
                        "scope": { "type": "string", "enum": ["project", "user"], "default": "project" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "aliases": { "type": "array", "items": { "type": "string" } },
                        "triggers": { "type": "array", "items": { "type": "string" } },
                        "activation": { "type": "string", "description": "Activation hint such as intent_detected, project_detected, domain_detected, lifecycle_gated, or always" },
                        "profile": { "type": "array", "items": { "type": "string" } },
                        "project_signals": { "type": "array", "items": { "type": "string" } },
                        "posture": { "type": "string" },
                        "max_turns": { "type": "integer", "minimum": 1 },
                        "force": { "type": "boolean", "default": false }
                    },
                    "required": ["name", "description", "body"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Mutation, omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_IMPORT.into(),
                label: "skills_import".into(),
                description: "Import an existing SKILL.md or skill bundle directory into project-local or user-level skills.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to SKILL.md or containing skill directory" },
                        "scope": { "type": "string", "enum": ["project", "user"], "default": "project" },
                        "force": { "type": "boolean", "default": false }
                    },
                    "required": ["path"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Mutation, omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_INSTALL.into(),
                label: "skills_install".into(),
                description: "Install all bundled skills, or install one public Armory skill by name/spec such as security or skills/security.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Optional public Armory skill name/spec. Omit to install bundled skills." }
                    }
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_DELETE.into(),
                label: "skills_delete".into(),
                description: "Delete an external project-local or user-level skill. Bundled and extension skills are not deleted.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Skill name to delete" }
                    },
                    "required": ["name"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::skills::SKILLS_LIST => {
                let entries = crate::skills::list_structured()?;
                let mut out = String::from("# Skills\n\n");
                for entry in &entries {
                    out.push_str(&format!(
                        "- **{}** [{}]{}{}: {}\n",
                        entry.name,
                        entry.source,
                        if entry.editable { " editable" } else { "" },
                        if entry.reloadable { " reloadable" } else { "" },
                        entry.description
                    ));
                }
                Ok(text_result_with_details(
                    &out,
                    serde_json::to_value(entries)?,
                ))
            }
            crate::tool_registry::skills::SKILLS_GET => {
                let name = required_str(&args, "name")?;
                let details = crate::skills::get_skill_details(name)?;
                let mut out = format!(
                    "# Skill: {}\n\nPath: {}\n\nDescription: {}\n",
                    details.manifest.name,
                    details.path.display(),
                    details.manifest.description
                );
                if let Some(entry) = &details.entry {
                    out.push_str(&format!(
                        "Source: {}\nEditable: {}\nReloadable: {}\n",
                        entry.source, entry.editable, entry.reloadable
                    ));
                }
                out.push_str("\n## Body\n\n");
                out.push_str(&details.body);
                Ok(text_result_with_details(
                    &out,
                    json!({
                        "manifest": details.manifest,
                        "path": details.path,
                        "entry": details.entry,
                    }),
                ))
            }
            crate::tool_registry::skills::SKILLS_RELOAD => {
                self.reload_skills();
                Ok(text_result(
                    "Reloaded user and project skills into this agent session.",
                ))
            }
            crate::tool_registry::skills::SKILLS_CREATE => {
                let result = create_skill_file(&args)?;
                self.reload_skills();
                Ok(result)
            }
            crate::tool_registry::skills::SKILLS_IMPORT => {
                let path = PathBuf::from(required_str(&args, "path")?);
                let scope = skill_scope(&args);
                let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
                let summary =
                    crate::skills::import_skill(&path, scope == SkillToolScope::Project, force)?;
                self.reload_skills();
                Ok(text_result_with_details(
                    &format!(
                        "Imported {} skill '{}' to {}",
                        summary.scope,
                        summary.name,
                        summary.destination.display()
                    ),
                    serde_json::to_value(summary)?,
                ))
            }
            crate::tool_registry::skills::SKILLS_INSTALL => {
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let result = if let Some(name) = name {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    crate::armory::install(name, crate::armory::ArmoryInstallKind::Skill, &cwd)
                        .await
                        .map(|summary| {
                            text_result_with_details(
                                &format!(
                                    "{}. Reloaded skills in this agent session.",
                                    summary.message
                                ),
                                serde_json::to_value(summary).unwrap_or(Value::Null),
                            )
                        })
                } else {
                    crate::skills::install_bundled_skills().map(|summary| {
                        text_result_with_details(
                            &format!(
                                "Installed {} bundled skill(s), updated {} under {}. Reloaded skills in this agent session.",
                                summary.installed,
                                summary.updated,
                                summary.destination.display()
                            ),
                            serde_json::to_value(summary).unwrap_or(Value::Null),
                        )
                    })
                };
                match result {
                    Ok(result) => {
                        self.reload_skills();
                        Ok(result)
                    }
                    Err(err) => anyhow::bail!("failed to install skill: {err}"),
                }
            }
            crate::tool_registry::skills::SKILLS_DELETE => {
                let name = required_str(&args, "name")?;
                let summary = crate::skills::delete_external_skill(name)?;
                self.reload_skills();
                Ok(text_result_with_details(
                    &format!(
                        "Deleted {} skill '{}' from {}",
                        summary.scope,
                        summary.name,
                        summary.path.display()
                    ),
                    serde_json::to_value(summary)?,
                ))
            }
            _ => anyhow::bail!("unknown skills tool: {tool_name}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillToolScope {
    Project,
    User,
}

fn skill_scope(args: &Value) -> SkillToolScope {
    match args.get("scope").and_then(Value::as_str) {
        Some("user") => SkillToolScope::User,
        _ => SkillToolScope::Project,
    }
}

fn string_vec(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn create_skill_file(args: &Value) -> anyhow::Result<ToolResult> {
    let name = required_str(args, "name")?;
    let description = required_str(args, "description")?;
    let body = required_str(args, "body")?;
    let slug = crate::skills::validate_skill_name(name)?;
    let scope = skill_scope(args);
    let base = match scope {
        SkillToolScope::Project => std::env::current_dir()?.join(".omegon/skills"),
        SkillToolScope::User => crate::paths::omegon_home()?.join("skills"),
    };
    let destination = base.join(&slug);
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    if destination.exists() {
        if !force {
            anyhow::bail!(
                "skill '{}' already exists at {}; pass force=true to overwrite",
                slug,
                destination.display()
            );
        }
        std::fs::remove_dir_all(&destination)?;
    }

    let manifest = omegon_skills::SkillManifest {
        name: slug.clone(),
        description: description.to_string(),
        id: Some(uuid::Uuid::new_v4().to_string()),
        version: None,
        tags: string_vec(args, "tags"),
        aliases: string_vec(args, "aliases"),
        triggers: string_vec(args, "triggers"),
        activation: args
            .get("activation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        profile: string_vec(args, "profile"),
        project_signals: string_vec(args, "project_signals"),
        trusted_paths: Vec::new(),
        output_path: None,
        output_format: None,
        max_turns: args
            .get("max_turns")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        posture: args
            .get("posture")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        provenance: None,
    };

    std::fs::create_dir_all(&destination)?;
    std::fs::write(destination.join("SKILL.md"), manifest.to_skill_file(body))?;

    let details = json!({
        "name": slug,
        "scope": match scope { SkillToolScope::Project => "project", SkillToolScope::User => "user" },
        "path": destination.display().to_string(),
        "file": destination.join("SKILL.md").display().to_string(),
    });
    Ok(text_result_with_details(
        &format!(
            "Created {} skill '{}' at {}",
            details["scope"].as_str().unwrap_or("external"),
            details["name"].as_str().unwrap_or(name),
            details["path"].as_str().unwrap_or("")
        ),
        details,
    ))
}

fn required_str<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing required field '{key}'"))
}

fn text_result(text: &str) -> ToolResult {
    text_result_with_details(text, json!({}))
}

fn text_result_with_details(text: &str, details: Value) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature() -> SkillsFeature {
        SkillsFeature::new(SharedAugmentRegistry::new(
            crate::plugins::registry::AugmentRegistry::new("Test Lex Imperialis.".into()),
        ))
    }

    #[test]
    fn exposes_skills_agent_tools() {
        let tools = feature().tools();
        assert!(tools.iter().any(|tool| tool.name == "skills_list"));
        assert!(tools.iter().any(|tool| tool.name == "skills_get"));
        assert!(tools.iter().any(|tool| tool.name == "skills_create"));
        assert!(tools.iter().any(|tool| tool.name == "skills_import"));
        assert!(tools.iter().any(|tool| tool.name == "skills_install"));
        assert!(tools.iter().any(|tool| tool.name == "skills_delete"));
        assert!(tools.iter().any(|tool| tool.name == "skills_reload"));
    }
}
