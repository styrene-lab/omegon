//! Registry-native prompt library command surface.
//!
//! Prompt definitions remain data under `crate::prompts`; this feature owns the
//! `/prompt` routing command so the TUI command palette, CLI remote slash path,
//! and ACP command dispatch can discover one canonical operator surface instead
//! of registering every prompt ID as a slash command.

use async_trait::async_trait;
use omegon_traits::{CommandDefinition, CommandResult, CommandSafety, Feature};

pub struct PromptFeature;

impl PromptFeature {
    pub fn new() -> Self {
        Self
    }

    fn help() -> String {
        [
            "Prompt library",
            "",
            "Usage:",
            "  /prompt list",
            "  /prompt <name>          # shorthand for preview",
            "  /prompt get <name>",
            "  /prompt preview <name>",
            "  /prompt run <name>",
            "  /prompt delete <name>",
            "",
            "Prompt IDs are data, not top-level slash commands. Use /prompt to resolve them.",
        ]
        .join("\n")
    }

    fn list() -> anyhow::Result<String> {
        let prompts = crate::prompts::list_structured()?;
        if prompts.is_empty() {
            return Ok("No prompts found.".into());
        }

        Ok(Self::list_projection(&prompts).render_markdown())
    }

    fn list_projection(
        prompts: &[crate::prompts::PromptEntry],
    ) -> crate::surfaces::palette::PaletteProjection {
        use crate::surfaces::palette::{
            PaletteBadgeTone, PaletteGroupProjection, PaletteProjection, PaletteRowProjection,
        };

        let bundled_total = prompts.iter().filter(|prompt| prompt.bundled).count();
        let bundled_installed = prompts
            .iter()
            .filter(|prompt| prompt.bundled && prompt.installed)
            .count();
        let user_total = prompts
            .iter()
            .filter(|prompt| !prompt.bundled && !prompt.project_local)
            .count();
        let project_total = prompts.iter().filter(|prompt| prompt.project_local).count();

        let action_rows = vec![
            PaletteRowProjection::action(
                "prompt.preview.shorthand",
                "/prompt <name>",
                "preview a prompt by id without registering it as a top-level slash command",
            ),
            PaletteRowProjection::action(
                "prompt.preview",
                "/prompt preview <name>",
                "inspect a prompt body before queue/run boundaries",
            ),
            PaletteRowProjection::action(
                "prompt.run",
                "/prompt run <name>",
                "resolve a prompt for the preview/queue boundary after safety checks",
            ),
            PaletteRowProjection::action(
                "prompt.delete",
                "/prompt delete <name>",
                "delete a user or project prompt definition",
            ),
        ];

        let prompt_rows = prompts
            .iter()
            .map(|prompt| {
                let mut row = PaletteRowProjection::object(
                    format!("prompt.{}", prompt.name),
                    prompt.name.clone(),
                )
                .with_badge(prompt_scope_label(prompt), PaletteBadgeTone::Info)
                .with_badge(prompt_state_label(prompt), prompt_state_tone(prompt))
                .with_command(format!("/prompt preview {}", prompt.name));

                if let Some(title) = prompt
                    .title
                    .as_deref()
                    .filter(|title| *title != prompt.name)
                {
                    row = row.with_metadata(format!("title:{title}"));
                }
                if !prompt.tags.is_empty() {
                    row = row.with_metadata(format!("tags:{}", prompt.tags.join(",")));
                }
                if !prompt.aliases.is_empty() {
                    row = row.with_metadata(format!("aliases:{}", prompt.aliases.join(",")));
                }
                if let Some(description) = prompt
                    .description
                    .as_deref()
                    .filter(|value| !value.is_empty())
                {
                    row = row.with_description(crate::util::truncate(description, 88));
                }
                row
            })
            .collect();

        PaletteProjection::new("Prompt library")
            .with_summary(format!(
                "Bundled {bundled_installed}/{bundled_total} installed · User {user_total} · Project {project_total}"
            ))
            .with_group(PaletteGroupProjection::new("Actions").with_rows(action_rows))
            .with_group(
                PaletteGroupProjection::new("Prompt rows")
                    .with_description("`name` · scope · state · title/tags/aliases")
                    .with_rows(prompt_rows),
            )
            .with_footer("Prompt IDs are data, not top-level slash commands. Use `/prompt preview <name>` for details.")
    }

    fn get(name: &str, include_body: bool) -> anyhow::Result<String> {
        let (manifest, body, path) = crate::prompts::get_prompt(name)?;
        let safety = crate::prompts::safety_verdict(&body);
        let title = manifest.title.as_deref().unwrap_or(name);
        let mut out = format!(
            "Prompt: {title}\nPath: {}\nSafety: {safety:?}\n",
            path.display()
        );
        if let Some(description) = manifest.description.as_deref() {
            out.push_str(&format!("Description: {description}\n"));
        }
        if !manifest.tags.is_empty() {
            out.push_str(&format!("Tags: {}\n", manifest.tags.join(", ")));
        }
        if !manifest.aliases.is_empty() {
            out.push_str(&format!("Aliases: {}\n", manifest.aliases.join(", ")));
        }
        if include_body {
            out.push('\n');
            out.push_str(&body);
        }
        Ok(out)
    }

    fn run(name: &str) -> anyhow::Result<String> {
        let (_manifest, body, path) = crate::prompts::get_prompt(name)?;
        let safety = crate::prompts::safety_verdict(&body);
        if safety.is_blocked() {
            anyhow::bail!("prompt is blocked by safety verdict: {safety:?}");
        }
        Ok(format!(
            "Prompt resolved for preview/queue boundary.\nPath: {}\nSafety: {:?}\n\n{}",
            path.display(),
            safety,
            body
        ))
    }
}

fn prompt_scope_label(prompt: &crate::prompts::PromptEntry) -> &'static str {
    if prompt.project_local {
        "project"
    } else if prompt.bundled {
        "bundled"
    } else {
        "user"
    }
}

fn prompt_state_label(prompt: &crate::prompts::PromptEntry) -> &'static str {
    if prompt.project_local {
        "local"
    } else if prompt.installed {
        "installed"
    } else if prompt.bundled {
        "available"
    } else {
        "installed"
    }
}

fn prompt_state_tone(
    prompt: &crate::prompts::PromptEntry,
) -> crate::surfaces::palette::PaletteBadgeTone {
    if prompt.project_local || prompt.installed {
        crate::surfaces::palette::PaletteBadgeTone::Success
    } else if prompt.bundled {
        crate::surfaces::palette::PaletteBadgeTone::Neutral
    } else {
        crate::surfaces::palette::PaletteBadgeTone::Info
    }
}

#[async_trait]
impl Feature for PromptFeature {
    fn name(&self) -> &str {
        "prompt"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "prompt".into(),
            description: "List, preview, and resolve reusable prompt definitions".into(),
            subcommands: vec![
                "list".into(),
                "<name>".into(),
                "get".into(),
                "preview".into(),
                "run".into(),
                "submit".into(),
                "delete".into(),
            ],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: CommandSafety::QUEUE_MUTATION,
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name != "prompt" {
            return CommandResult::NotHandled;
        }

        let args = args.trim();
        let result = if args.is_empty() || args == "help" {
            Ok(Self::help())
        } else {
            let (subcommand, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            let rest = rest.trim();
            match subcommand {
                "list" => Self::list(),
                "get" | "preview" if !rest.is_empty() => Self::get(rest, true),
                "run" | "submit" if !rest.is_empty() => Self::run(rest),
                "delete" if !rest.is_empty() => crate::prompts::delete_prompt(rest)
                    .map(|scope| format!("Deleted {scope} prompt '{rest}'")),
                "get" | "preview" | "run" | "submit" | "delete" => Err(anyhow::anyhow!(
                    "/prompt {subcommand} requires a prompt name"
                )),
                name => Self::get(name, true),
            }
        };

        match result {
            Ok(output) => CommandResult::Display(output),
            Err(err) => CommandResult::Display(format!("/prompt failed: {err}")),
        }
    }
}

impl Default for PromptFeature {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::{CommandSafetyClass, Feature};

    #[test]
    fn prompt_command_declares_modern_palette_metadata() {
        let feature = PromptFeature::new();
        let commands = feature.commands();
        let command = commands.iter().find(|cmd| cmd.name == "prompt").unwrap();
        assert!(command.availability.tui);
        assert!(command.availability.cli);
        assert!(command.availability.acp);
        assert_eq!(command.safety.class, CommandSafetyClass::QueueMutation);
        assert!(command.safety.prompt_injection_sensitive);
        assert!(command.subcommands.contains(&"list".to_string()));
        assert!(command.subcommands.contains(&"preview".to_string()));
    }

    #[test]
    fn prompt_command_help_reinforces_prompt_ids_are_data() {
        let mut feature = PromptFeature::new();
        let result = feature.handle_command("prompt", "help");
        match result {
            CommandResult::Display(text) => {
                assert!(text.contains("/prompt <name>"));
                assert!(text.contains("Prompt IDs are data"));
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn prompt_list_renders_shared_palette_rows() {
        let prompts = vec![
            crate::prompts::PromptEntry {
                name: "review".into(),
                id: None,
                title: Some("Review".into()),
                description: Some("Review the current change".into()),
                tags: vec!["coding".into()],
                aliases: vec!["rev".into()],
                bundled: true,
                installed: false,
                project_local: false,
                path: String::new(),
            },
            crate::prompts::PromptEntry {
                name: "team-sync".into(),
                id: None,
                title: Some("Team Sync".into()),
                description: None,
                tags: vec![],
                aliases: vec![],
                bundled: false,
                installed: true,
                project_local: true,
                path: ".omegon/prompts/team-sync.md".into(),
            },
        ];

        let rendered = PromptFeature::list_projection(&prompts).render_markdown();

        assert!(rendered.starts_with("## Prompt library"));
        assert!(rendered.contains("Bundled 0/1 installed · User 0 · Project 1"));
        assert!(rendered.contains("### Actions"));
        assert!(rendered.contains("`/prompt <name>`"));
        assert!(rendered.contains("`/prompt preview <name>`"));
        assert!(rendered.contains("`/prompt run <name>`"));
        assert!(rendered.contains("### Prompt rows"));
        assert!(rendered.contains("- `review` — bundled · available · title:Review · tags:coding · aliases:rev · Review the current change"));
        assert!(rendered.contains("- `team-sync` — project · local · title:Team Sync"));
        assert!(!rendered.contains("Prompt library\n\n- Review (bundled)"));
        assert!(rendered.contains("Prompt IDs are data"));
    }

    #[test]
    fn prompt_name_shorthand_previews_prompt() {
        let mut feature = PromptFeature::new();
        let result = feature.handle_command("prompt", "init");
        match result {
            CommandResult::Display(text) => {
                assert!(text.contains("Prompt:"));
                assert!(text.contains("Project Init"));
                assert!(text.contains("Safety:"));
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }
}
