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

        let mut out = String::from("Prompt library\n\n");
        for prompt in prompts {
            let scope = if prompt.project_local {
                "project"
            } else if prompt.bundled {
                "bundled"
            } else {
                "user"
            };
            let title = prompt.title.as_deref().unwrap_or(&prompt.name);
            let description = prompt.description.as_deref().unwrap_or("");
            if description.is_empty() {
                out.push_str(&format!("- {} ({scope})\n", title));
            } else {
                out.push_str(&format!("- {} ({scope}) — {}\n", title, description));
            }
        }
        Ok(out)
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
            out.push_str("\n");
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
                "get".into(),
                "preview".into(),
                "run".into(),
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
                _ => Ok(Self::help()),
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
            CommandResult::Display(text) => assert!(text.contains("Prompt IDs are data")),
            other => panic!("unexpected command result: {other:?}"),
        }
    }
}
