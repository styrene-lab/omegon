//! Shared slash-command registry definitions.
//!
//! This module owns built-in command discovery metadata. Renderer surfaces (TUI,
//! CLI, ACP, and future clients) should project from these registry-shaped rows
//! instead of maintaining surface-local slash-command tables.

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltinCommandSpec {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) subcommands: &'static [&'static str],
    pub(crate) availability: omegon_traits::CommandAvailability,
    pub(crate) safety: omegon_traits::CommandSafety,
}

impl BuiltinCommandSpec {
    const TUI_AND_ACP: omegon_traits::CommandAvailability = omegon_traits::CommandAvailability {
        tui: true,
        cli: false,
        acp: true,
    };

    const TUI_AND_CLI: omegon_traits::CommandAvailability = omegon_traits::CommandAvailability {
        tui: true,
        cli: true,
        acp: false,
    };

    const TUI_CLI_AND_ACP: omegon_traits::CommandAvailability =
        omegon_traits::CommandAvailability {
            tui: true,
            cli: true,
            acp: true,
        };

    const fn with_metadata(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
        availability: omegon_traits::CommandAvailability,
        safety: omegon_traits::CommandSafety,
    ) -> Self {
        Self {
            name,
            description,
            subcommands,
            availability,
            safety,
        }
    }

    const fn with_safety(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
        safety: omegon_traits::CommandSafety,
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            omegon_traits::CommandAvailability::TUI_ONLY,
            safety,
        )
    }

    const fn local(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::LOCAL_ONLY,
        )
    }

    const fn read_only(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::READ_ONLY,
        )
    }

    const fn acp_read_only(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_ACP,
            omegon_traits::CommandSafety::READ_ONLY,
        )
    }

    const fn queue_mutation(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::QUEUE_MUTATION,
        )
    }

    const fn state_changing(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::STATE_CHANGING,
        )
    }

    const fn cli_read_only(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_CLI,
            omegon_traits::CommandSafety::READ_ONLY,
        )
    }

    const fn cli_acp_read_only(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_CLI_AND_ACP,
            omegon_traits::CommandSafety::READ_ONLY,
        )
    }

    const fn cli_queue_mutation(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_CLI,
            omegon_traits::CommandSafety::QUEUE_MUTATION,
        )
    }

    const fn cli_state_changing(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_CLI,
            omegon_traits::CommandSafety::STATE_CHANGING,
        )
    }

    const fn cli_acp_state_changing(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_CLI_AND_ACP,
            omegon_traits::CommandSafety::STATE_CHANGING,
        )
    }

    const fn acp_state_changing(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_ACP,
            omegon_traits::CommandSafety::STATE_CHANGING,
        )
    }

    const fn external_side_effect(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::EXTERNAL_SIDE_EFFECT,
        )
    }

    const fn acp_external_side_effect(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_ACP,
            omegon_traits::CommandSafety::EXTERNAL_SIDE_EFFECT,
        )
    }

    const fn destructive(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_safety(
            name,
            description,
            subcommands,
            omegon_traits::CommandSafety::DESTRUCTIVE,
        )
    }

    const fn acp_destructive(
        name: &'static str,
        description: &'static str,
        subcommands: &'static [&'static str],
    ) -> Self {
        Self::with_metadata(
            name,
            description,
            subcommands,
            Self::TUI_AND_ACP,
            omegon_traits::CommandSafety::DESTRUCTIVE,
        )
    }

    pub(crate) fn to_definition(self) -> omegon_traits::CommandDefinition {
        omegon_traits::CommandDefinition {
            name: self.name.to_string(),
            description: self.description.to_string(),
            subcommands: self
                .subcommands
                .iter()
                .map(|subcommand| (*subcommand).to_string())
                .collect(),
            availability: self.availability,
            safety: self.safety,
        }
    }
}

/// Built-in command definitions in the shared command-registry shape consumed by
/// renderer-neutral projections and non-TUI command discovery surfaces.
pub(crate) fn builtin_command_definitions() -> Vec<omegon_traits::CommandDefinition> {
    BUILTIN_COMMANDS
        .iter()
        .copied()
        .map(BuiltinCommandSpec::to_definition)
        .collect()
}

/// Built-in slash command registry rows. Feature/user commands are merged with
/// these via `CommandMenuProjection`; keep new metadata on this registry-shaped
/// spec instead of adding renderer-local autocomplete tables.
pub(crate) const BUILTIN_COMMANDS: &[BuiltinCommandSpec] = &[
    BuiltinCommandSpec::cli_acp_read_only("help", "open command inventory", &[]),
    BuiltinCommandSpec::read_only(
        "copy",
        "copy selected segment, latest answer, or session",
        &["raw", "plain", "answer", "latest", "session"],
    ),
    BuiltinCommandSpec::state_changing(
        "transcript",
        "write a clean clickable Markdown transcript",
        &["file", "open", "scrollback"],
    ),
    BuiltinCommandSpec::state_changing(
        "mouse",
        "toggle pane mouse interaction mode",
        &["on", "off"],
    ),
    BuiltinCommandSpec::cli_acp_state_changing(
        "model",
        "open model routing menu or switch model",
        &["list", "grade", "unpin"],
    ),
    BuiltinCommandSpec::cli_acp_state_changing(
        "think",
        "set thinking level",
        &["off", "minimal", "low", "medium", "high"],
    ),
    BuiltinCommandSpec::cli_state_changing(
        "profile",
        "open profile menu or manage runtime profile defaults",
        &[
            "view",
            "capture",
            "save",
            "save --active",
            "save --project",
            "save --user",
            "apply",
            "mqtt",
            "extension",
            "persona",
            "tone",
        ],
    ),
    BuiltinCommandSpec::cli_acp_read_only(
        "stats",
        "session telemetry and performance metrics",
        &["bench"],
    ),
    BuiltinCommandSpec::cli_queue_mutation("new", "quick alias for /context reset", &[]),
    BuiltinCommandSpec::state_changing(
        "ui",
        "open UI controls menu or toggle surfaces",
        &[
            "status", "lean", "full", "show", "hide", "toggle", "detail", "density",
        ],
    ),
    BuiltinCommandSpec::cli_queue_mutation(
        "context",
        "open context menu or manage context lifecycle",
        &[
            "status", "compact", "reset", "clear", "request", "standard", "extended", "massive",
        ],
    ),
    BuiltinCommandSpec::cli_queue_mutation(
        "plan",
        "manage session plan gate and progress",
        &[
            "status", "list", "set", "approve", "execute", "advance", "skip", "clear",
        ],
    ),
    BuiltinCommandSpec::cli_read_only(
        "sessions",
        "open saved sessions menu",
        &["list", "all", "resume"],
    ),
    BuiltinCommandSpec::cli_read_only(
        "memory",
        "open memory overview menu",
        &["status", "overview"],
    ),
    BuiltinCommandSpec::state_changing("settings", "open settings menu", &[]),
    BuiltinCommandSpec::cli_acp_state_changing(
        "skills",
        "manage bundled, user, project-local, and armory skills",
        &[
            "list",
            "--help",
            "-h",
            "help",
            "reload",
            "refresh",
            "install",
            "install <name>",
            "create",
            "create --project",
            "create --user",
            "new",
            "new --project",
            "new --user",
            "import <path>",
            "import --project <path>",
            "import --user <path>",
            "get <name>",
            "delete <name>",
        ],
    ),
    BuiltinCommandSpec::cli_acp_state_changing(
        "skill",
        "alias for /skills",
        &[
            "list",
            "--help",
            "-h",
            "help",
            "reload",
            "refresh",
            "install",
            "install <name>",
            "create",
            "create --project",
            "create --user",
            "new",
            "new --project",
            "new --user",
            "import <path>",
            "import --project <path>",
            "import --user <path>",
            "get <name>",
            "delete <name>",
        ],
    ),
    BuiltinCommandSpec::acp_external_side_effect(
        "extension",
        "manage extensions (armory name, URL, path)",
        &[
            "list", "get", "install", "remove", "update", "enable", "disable", "search",
        ],
    ),
    BuiltinCommandSpec::external_side_effect(
        "plugin",
        "manage local or git plugins",
        &["list", "install", "remove", "update"],
    ),
    BuiltinCommandSpec::acp_external_side_effect(
        "armory",
        "browse and install extensions, plugins, skills, and agents",
        &["browse", "search", "list", "install"],
    ),
    BuiltinCommandSpec::acp_destructive(
        "catalog",
        "browse and manage agent catalog",
        &["list", "install", "remove"],
    ),
    BuiltinCommandSpec::queue_mutation(
        "cleave",
        "show cleave status or trigger decomposition",
        &["status"],
    ),
    BuiltinCommandSpec::with_metadata(
        "auth",
        "authentication management",
        &[
            "status",
            "unlock",
            "login",
            "logout",
            "anthropic",
            "openai",
            "openai-codex",
            "openrouter",
            "ollama-cloud",
            "github",
        ],
        BuiltinCommandSpec::TUI_CLI_AND_ACP,
        omegon_traits::CommandSafety::EXTERNAL_SIDE_EFFECT,
    ),
    BuiltinCommandSpec::cli_read_only(
        "chronos",
        "date/time context",
        &[
            "week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all",
        ],
    ),
    BuiltinCommandSpec::state_changing(
        "init",
        "initialize project — scan & migrate agent conventions",
        &["scan", "migrate"],
    ),
    BuiltinCommandSpec::external_side_effect(
        "update",
        "check for and install updates",
        &["install", "channel"],
    ),
    BuiltinCommandSpec::state_changing(
        "migrate",
        "import from other tools",
        &["auto", "claude-code", "pi", "codex", "cursor", "aider"],
    ),
    BuiltinCommandSpec::external_side_effect(
        "auspex",
        "primary local desktop handoff — show status or open Auspex",
        &["status", "open"],
    ),
    BuiltinCommandSpec::acp_destructive(
        "secrets",
        "manage stored secrets",
        &[
            "list",
            "status",
            "configure",
            "set",
            "get",
            "delete",
            "remove",
            "rm",
        ],
    ),
    BuiltinCommandSpec::state_changing(
        "vault",
        "Vault status and management",
        &["status", "configure", "init-policy"],
    ),
    BuiltinCommandSpec::acp_state_changing(
        "persona",
        "switch persona, list, create, or deactivate",
        &["list", "create", "off"],
    ),
    BuiltinCommandSpec::state_changing("tone", "switch tone (or 'off' to deactivate)", &["off"]),
    BuiltinCommandSpec::queue_mutation(
        "delegate",
        "subagent/delegate task management",
        &["status"],
    ),
    BuiltinCommandSpec::queue_mutation(
        "subagent",
        "alias for /delegate; inspect subagent tasks",
        &["status"],
    ),
    BuiltinCommandSpec::cli_acp_read_only(
        "status",
        "show harness status (providers, MCP, secrets, routing)",
        &[],
    ),
    BuiltinCommandSpec::cli_read_only(
        "tree",
        "show design tree summary",
        &["list", "frontier", "ready", "blocked"],
    ),
    BuiltinCommandSpec::state_changing(
        "milestone",
        "release milestone management",
        &["freeze", "status"],
    ),
    BuiltinCommandSpec::cli_queue_mutation(
        "notes",
        "capture, show, clear, or triage pending notes",
        &["add", "clear", "checkin"],
    ),
    BuiltinCommandSpec::state_changing(
        "editor",
        "integrate omegon with an editor/IDE",
        &["zed", "vscode", "status"],
    ),
    BuiltinCommandSpec::state_changing(
        "preferences",
        "open preferences menu (model, thinking, density, etc.)",
        &[],
    ),
    BuiltinCommandSpec::state_changing(
        "permissions",
        "view grants and always-allow persistence",
        &["list", "add", "remove", "keys"],
    ),
    BuiltinCommandSpec::state_changing(
        "automation",
        "tune ask/proceed gates without changing permissions",
        &["status", "ask", "guarded", "flow", "autonomous"],
    ),
    BuiltinCommandSpec::state_changing(
        "sandbox",
        "toggle agent sandbox isolation (OCI containers)",
        &["on", "off", "status"],
    ),
    BuiltinCommandSpec::cli_read_only("version", "show build version and git sha", &[]),
    BuiltinCommandSpec::local("q", "quit alias", &[]),
    BuiltinCommandSpec::local("quit", "quit alias", &[]),
    BuiltinCommandSpec::local("exit", "quit (or double Ctrl+C)", &[]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_command_names_are_unique() {
        let mut names = HashSet::new();
        for command in BUILTIN_COMMANDS {
            assert!(
                names.insert(command.name),
                "duplicate builtin command: {}",
                command.name
            );
        }
    }

    #[test]
    fn builtin_definitions_preserve_availability_and_safety() {
        let definitions = builtin_command_definitions();
        let update = definitions
            .iter()
            .find(|definition| definition.name == "update")
            .expect("/update definition");
        assert!(update.availability.tui);
        assert!(!update.availability.acp);
        assert_eq!(
            update.safety.class,
            omegon_traits::CommandSafetyClass::ExternalSideEffect
        );
        assert!(update.safety.requires_confirmation);

        let auth = definitions
            .iter()
            .find(|definition| definition.name == "auth")
            .expect("/auth definition");
        assert!(auth.availability.tui);
        assert!(auth.availability.acp);
        assert!(auth.availability.cli);
    }
}
