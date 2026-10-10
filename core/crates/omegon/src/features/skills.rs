//! Skills feature — exposes the skills surface as agent-callable tools.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::features::persona::SharedAugmentRegistry;

pub struct SkillsFeature {
    registry: SharedAugmentRegistry,
    cwd: PathBuf,
    home: PathBuf,
    allowed_skills: Vec<String>,
}

impl SkillsFeature {
    pub fn new(
        registry: SharedAugmentRegistry,
        cwd: PathBuf,
        home: PathBuf,
        allowed_skills: Vec<String>,
    ) -> Self {
        Self {
            registry,
            cwd,
            home,
            allowed_skills,
        }
    }

    fn reload_skills(&self) {
        self.registry.lock().load_skills_subset_with_home(
            &self.cwd,
            &self.home,
            &self.allowed_skills,
        );
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
                description: "List active Omegon skills from the current session's admitted snapshot with source and manifest metadata.".into(),
                parameters: json!({ "type": "object", "properties": {} }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::skills::SKILLS_GET.into(),
                label: "skills_get".into(),
                description: "Read one active skill's admitted manifest, body, path, and source metadata from the current session snapshot.".into(),
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
                let registry = self.registry.lock();
                let entries = registry.skill_snapshots();
                let mut out = String::from("# Skills\n\n");
                let mut details = Vec::with_capacity(entries.len());
                for entry in entries {
                    let (manifest, _) = omegon_skills::parse_skill_file(&entry.content);
                    out.push_str(&format!(
                        "- **{}** [{}]: {}\n",
                        entry.name, entry.source, manifest.description
                    ));
                    details.push(json!({
                        "name": entry.name,
                        "source": entry.source,
                        "path": entry.path,
                        "description": manifest.description,
                        "manifest": manifest,
                    }));
                }
                Ok(text_result_with_details(&out, Value::Array(details)))
            }
            crate::tool_registry::skills::SKILLS_GET => {
                let name = required_str(&args, "name")?;
                let registry = self.registry.lock();
                let snapshot = registry
                    .skill_snapshots()
                    .iter()
                    .find(|skill| skill.name == name)
                    .ok_or_else(|| anyhow::anyhow!("active skill '{name}' not found"))?;
                let (manifest, body) = omegon_skills::parse_skill_file(&snapshot.content);
                // Bundled snapshots name SKILL.md; guarded user/project snapshots
                // name its directory. Keep historical path metadata and expose an
                // unambiguous model-visible base for supporting references.
                let base = if snapshot.source == "bundled" {
                    snapshot.path.parent().unwrap_or(&snapshot.path)
                } else {
                    &snapshot.path
                };
                let mut out = format!(
                    "# Skill: {}\n\nPath: {}\n\nBase directory: {}\n\nDescription: {}\n",
                    manifest.name,
                    snapshot.path.display(),
                    base.display(),
                    manifest.description
                );
                out.push_str(&format!("Source: {}\n", snapshot.source));
                out.push_str("\n## Body\n\n");
                out.push_str(&body);
                Ok(text_result_with_details(
                    &out,
                    json!({
                        "manifest": manifest,
                        "path": snapshot.path,
                        "source": snapshot.source,
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
                let result = create_skill_file(&args, &self.cwd, &self.home)?;
                self.reload_skills();
                Ok(result)
            }
            crate::tool_registry::skills::SKILLS_IMPORT => {
                let requested = PathBuf::from(required_str(&args, "path")?);
                let path = if requested.is_absolute() {
                    requested
                } else {
                    self.cwd.join(requested)
                };
                let scope = skill_scope(&args);
                let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
                let summary = if scope == SkillToolScope::Project {
                    crate::skills::import_project_skill_guarded(
                        &path, &self.cwd, &self.home, force,
                    )?
                } else {
                    crate::skills::import_skill_at_root(&path, None, force)?
                };
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
                    crate::armory::install(name, crate::armory::ArmoryInstallKind::Skill, &self.cwd)
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
                let summary =
                    match crate::skills::delete_project_skill_guarded(name, &self.cwd, &self.home)?
                    {
                        Some(summary) => summary,
                        None => crate::skills::delete_user_skill_at_home(name, &self.home)?,
                    };
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

fn create_skill_file(
    args: &Value,
    cwd: &std::path::Path,
    home: &std::path::Path,
) -> anyhow::Result<ToolResult> {
    let name = required_str(args, "name")?;
    let description = required_str(args, "description")?;
    let body = required_str(args, "body")?;
    let slug = crate::skills::validate_skill_name(name)?;
    let scope = skill_scope(args);
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
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
    let content = manifest.to_skill_file(body);
    let destination = match scope {
        SkillToolScope::Project => {
            let directory =
                crate::contribution_loading::GuardedContributionMutationDirectory::open_or_create(
                    cwd,
                    &[b".omegon", b"skills"],
                    home,
                    omegon_maintenance_contracts::ContributionKind::Skill,
                    "project",
                )?;
            directory.write_single_file_directory(
                slug.as_bytes(),
                b"SKILL.md",
                content.as_bytes(),
                force,
            )?;
            cwd.join(".omegon/skills").join(&slug)
        }
        SkillToolScope::User => {
            let destination = home.join("skills").join(&slug);
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
            std::fs::create_dir_all(&destination)?;
            std::fs::write(destination.join("SKILL.md"), content)?;
            destination
        }
    };

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
        SkillsFeature::new(
            SharedAugmentRegistry::new(crate::plugins::registry::AugmentRegistry::new()),
            std::env::current_dir().unwrap(),
            crate::paths::omegon_home().unwrap(),
            Vec::new(),
        )
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

    #[cfg(unix)]
    #[tokio::test]
    async fn reload_preserves_workspace_and_explicit_skill_subset() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        for (name, marker) in [
            ("allowed", "ALLOWED_MARKER"),
            ("excluded", "EXCLUDED_MARKER"),
        ] {
            let directory = project.path().join(".omegon/skills").join(name);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: Test skill\nactivation: intent_detected\ntriggers: [{name}]\n---\n\n{marker}"
                ),
            )
            .unwrap();
        }
        let registry = SharedAugmentRegistry::new(crate::plugins::registry::AugmentRegistry::new());
        let feature = SkillsFeature::new(
            registry.clone(),
            project.path().to_path_buf(),
            home.path().to_path_buf(),
            vec!["allowed".into()],
        );

        feature.reload_skills();

        {
            let registry = registry.lock();
            let prompt = registry.build_augmentation();
            let disclosed = registry.build_augmentation_disclosed(project.path(), None);
            assert_eq!(registry.skill_count(), 1);
            assert!(prompt.contains("ALLOWED_MARKER"));
            assert!(!prompt.contains("EXCLUDED_MARKER"));
            assert!(disclosed.contains("ALLOWED_MARKER"));
        }
        assert!(
            feature
                .execute(
                    crate::tool_registry::skills::SKILLS_GET,
                    "test",
                    json!({ "name": "excluded" }),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .is_err()
        );
        let listed = feature
            .execute(
                crate::tool_registry::skills::SKILLS_LIST,
                "test",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(listed.details.as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scoped_extension_skills_are_discoverable_retrievable_and_triggered_without_granting_tools()
     {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        // Filename signals alone must not adopt extension authoring.
        std::fs::write(project.path().join("manifest.toml"), "[extension]").unwrap();
        let registry = SharedAugmentRegistry::new(crate::plugins::registry::AugmentRegistry::new());
        let feature = SkillsFeature::new(
            registry.clone(),
            project.path().to_path_buf(),
            home.path().to_path_buf(),
            vec![],
        );
        feature.reload_skills();
        let listed = feature
            .execute(
                "skills_list",
                "inventory",
                json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        for existing in ["likec4", "code-act"] {
            assert!(
                listed
                    .details
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["name"] == existing),
                "new extension guidance must not displace {existing}"
            );
        }
        for (name, trigger, marker, reference) in [
            (
                "scry",
                "Generate an image with Scry",
                "Do not guess",
                "references/usage.md",
            ),
            (
                "extension-authoring",
                "Debug the Omegon extension SDK",
                "Do not recreate",
                "references/authoring.md",
            ),
        ] {
            assert!(
                listed
                    .details
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["name"] == name && entry["source"] == "bundled")
            );
            let quiet = registry
                .lock()
                .build_augmentation_disclosed(project.path(), Some("make a small text edit"));
            assert!(
                quiet.contains(&format!("- {name} —")),
                "deferred skill must remain discoverable"
            );
            assert!(!quiet.contains(marker));
            let triggered = registry
                .lock()
                .build_augmentation_disclosed(project.path(), Some(trigger));
            assert!(triggered.contains(marker));
            let retrieved = feature
                .execute(
                    "skills_get",
                    "retrieve",
                    json!({"name":name}),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .unwrap();
            let text = retrieved
                .content
                .iter()
                .filter_map(|block| block.as_text())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(marker));
            assert_eq!(retrieved.details["source"], "bundled");
            let path = std::path::Path::new(retrieved.details["path"].as_str().unwrap());
            assert!(text.contains(&format!(
                "Base directory: {}",
                path.parent().unwrap().display()
            )));
            let body = std::fs::read_to_string(path.parent().unwrap().join(reference)).unwrap();
            if name == "scry" {
                for essential in [
                    "list_models",
                    "Do not guess model names",
                    "LoRAs are applied in order",
                    "Report the output path",
                ] {
                    assert!(body.contains(essential));
                }
                assert!(text.contains("actual Scry"));
                assert!(text.contains("does not grant tools"));
            } else {
                assert!(body.contains("execute_tool"));
                assert!(body.contains("not sandboxed"));
            }
        }
        assert!(
            feature
                .tools()
                .iter()
                .all(|tool| tool.name != "generate" && tool.name != "vox_reply")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bundled_install_repairs_supporting_assets_with_unchanged_entries_and_user_precedence()
    {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let pack = crate::content_pack::boot_pack().unwrap();
        let note = home.path().join("skills/scry/operator-notes.txt");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "USER WORK").unwrap();
        let fresh = crate::skills::install_bundled_skills_at(home.path(), &pack).unwrap();
        assert!(fresh.installed >= 2);
        assert_eq!(fresh.updated, 0);
        for asset in pack.assets("skill").filter(|asset| {
            std::path::Path::new(&asset.manifest.path)
                .components()
                .count()
                >= 3
        }) {
            assert_eq!(
                std::fs::read(home.path().join(&asset.manifest.path)).unwrap(),
                asset.bytes.as_ref()
            );
        }
        let registry = SharedAugmentRegistry::new(crate::plugins::registry::AugmentRegistry::new());
        let feature = SkillsFeature::new(
            registry,
            project.path().to_path_buf(),
            home.path().to_path_buf(),
            vec!["scry".into(), "extension-authoring".into()],
        );
        for repair in [false, true] {
            if repair {
                std::fs::remove_file(home.path().join("skills/scry/references/usage.md")).unwrap();
                std::fs::write(
                    home.path()
                        .join("skills/extension-authoring/references/authoring.md"),
                    "STALE REFERENCE",
                )
                .unwrap();
                // Entries already match; supporting assets must still be compared.
                for name in ["scry", "extension-authoring"] {
                    let relative = format!("skills/{name}/SKILL.md");
                    assert_eq!(
                        std::fs::read_to_string(home.path().join(&relative)).unwrap(),
                        pack.text(&relative).unwrap()
                    );
                }
                let summary = crate::skills::install_bundled_skills_at(home.path(), &pack).unwrap();
                assert_eq!(summary.installed, 0);
                assert_eq!(
                    summary.updated, 2,
                    "support-only repairs count as updated skills"
                );
            }
            feature.reload_skills();
            for (name, reference) in [
                ("scry", "usage.md"),
                ("extension-authoring", "authoring.md"),
            ] {
                let result = feature
                    .execute(
                        "skills_get",
                        "installed-reference",
                        json!({"name":name}),
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    result.details["source"], "user",
                    "installed scope must actually override bundled"
                );
                let path = std::path::Path::new(result.details["path"].as_str().unwrap());
                assert!(path.starts_with(home.path()));
                let text = result
                    .content
                    .iter()
                    .filter_map(|block| block.as_text())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(text.contains(&format!("Path: {}", path.display())));
                assert!(text.contains(&format!("references/{reference}")));
                let base = std::path::Path::new(
                    text.lines()
                        .find_map(|line| line.strip_prefix("Base directory: "))
                        .expect("model-visible reference base"),
                );
                assert_eq!(base, home.path().join("skills").join(name));
                assert_eq!(
                    std::fs::read_to_string(base.join("references").join(reference)).unwrap(),
                    pack.text(&format!("skills/{name}/references/{reference}"))
                        .unwrap()
                );
            }
        }
        let unchanged = crate::skills::install_bundled_skills_at(home.path(), &pack).unwrap();
        assert_eq!((unchanged.installed, unchanged.updated), (0, 0));
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "USER WORK");
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_create_rejects_symlinked_contribution_root() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), project.path().join(".omegon")).unwrap();
        let args = json!({
            "name": "escaped",
            "description": "must not escape",
            "body": "ESCAPE_MARKER",
            "scope": "project",
        });

        assert!(create_skill_file(&args, project.path(), home.path()).is_err());
        assert!(!outside.path().join("skills/escaped/SKILL.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_create_is_atomic_and_requires_force_to_replace() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let initial = json!({
            "name": "atomic",
            "description": "initial",
            "body": "INITIAL_MARKER",
            "scope": "project",
        });
        create_skill_file(&initial, project.path(), home.path()).unwrap();
        let replacement = json!({
            "name": "atomic",
            "description": "replacement",
            "body": "REPLACEMENT_MARKER",
            "scope": "project",
        });
        assert!(create_skill_file(&replacement, project.path(), home.path()).is_err());
        let path = project.path().join(".omegon/skills/atomic/SKILL.md");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("INITIAL_MARKER")
        );
        let stale = project
            .path()
            .join(".omegon/skills/atomic/scripts/stale.sh");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "#!/bin/sh\nexit 1\n").unwrap();

        let mut forced = replacement;
        forced["force"] = Value::Bool(true);
        create_skill_file(&forced, project.path(), home.path()).unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("REPLACEMENT_MARKER"));
        assert!(!content.contains("INITIAL_MARKER"));
        assert!(!stale.exists());
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_import_replaces_bundle_and_skips_source_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("SKILL.md"),
            "---\nname: imported\ndescription: Imported skill\n---\n\nIMPORTED_MARKER",
        )
        .unwrap();
        let scripts = source.path().join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let script = scripts.join("run.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), scripts.join("outside-link")).unwrap();

        crate::skills::import_project_skill_guarded(
            source.path(),
            project.path(),
            home.path(),
            false,
        )
        .unwrap();
        let destination = project.path().join(".omegon/skills/imported");
        std::fs::write(destination.join("stale.txt"), "stale").unwrap();
        crate::skills::import_project_skill_guarded(
            source.path(),
            project.path(),
            home.path(),
            true,
        )
        .unwrap();

        assert!(!destination.join("stale.txt").exists());
        assert!(!destination.join("scripts/outside-link").exists());
        assert_ne!(
            std::fs::metadata(destination.join("scripts/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o100,
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_delete_unlinks_nested_symlinks_without_following_them() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let skill = project.path().join(".omegon/skills/removable");
        std::fs::create_dir_all(skill.join("nested")).unwrap();
        std::fs::write(skill.join("SKILL.md"), "REMOVABLE").unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), skill.join("nested/outside-link")).unwrap();

        let summary =
            crate::skills::delete_project_skill_guarded("removable", project.path(), home.path())
                .unwrap()
                .unwrap();

        assert_eq!(summary.scope, "project");
        assert!(!skill.exists());
        assert!(outside.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_delete_rejects_symlinked_contribution_root() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_skill = outside.path().join("skills/escaped");
        std::fs::create_dir_all(&outside_skill).unwrap();
        std::fs::write(outside_skill.join("SKILL.md"), "OUTSIDE").unwrap();
        symlink(outside.path(), project.path().join(".omegon")).unwrap();

        assert!(
            crate::skills::delete_project_skill_guarded("escaped", project.path(), home.path(),)
                .is_err()
        );
        assert!(outside_skill.join("SKILL.md").exists());
    }
}
