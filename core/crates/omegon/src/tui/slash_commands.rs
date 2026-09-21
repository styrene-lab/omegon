//! TUI slash-command dispatch adapter.

use super::*;
use crate::runtime_commands::{CanonicalSlashCommand, SkillCreateScope, canonical_slash_command};

/// Result of handling a slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlashResult {
    /// Display this text as a system message.
    Display(String),
    /// Command was handled silently (e.g. opened a popup).
    Handled,
    /// Not a recognized command — pass through as user prompt.
    NotACommand,
    /// Quit requested.
    Quit,
}

impl App {
    pub(super) fn handle_slash_command(
        &mut self,
        text: &str,
        tx: &OperatorCommandTx,
    ) -> SlashResult {
        let trimmed = text.trim();
        if !trimmed.starts_with('/') {
            return SlashResult::NotACommand;
        }
        let rest = &trimmed[1..];
        let (cmd, args) = rest.split_once(' ').unwrap_or((rest, ""));
        let args = args.trim();

        // Absolute file paths (e.g. /home/user/file.txt) are not commands
        if cmd.contains('/') {
            return SlashResult::NotACommand;
        }

        // Notify the tutorial overlay that a slash command was executed.
        // This advances Command-triggered steps (e.g. /dash on the Auspex browser step).
        if let Some(ref mut overlay) = self.tutorial_overlay {
            overlay.check_command(cmd);
        }

        match cmd {
            "processes" | "process" | "terminals" => {
                self.open_process_viewer(args);
                SlashResult::Handled
            }
            "help" => {
                if matches!(args, "tutorial" | "tour") {
                    return self.handle_tutorial("", tx);
                }
                if args == "tutorial status" {
                    return self.handle_tutorial("status", tx);
                }
                if args == "tutorial reset" {
                    return self.handle_tutorial("reset", tx);
                }
                if args == "tutorial consent" {
                    return self.handle_tutorial("consent", tx);
                }
                if args == "tutorial demo" {
                    return self.handle_tutorial("demo", tx);
                }
                if args == "copy" {
                    return SlashResult::Display(
                        "Copy contract:
  Ctrl+Shift+Y       copy latest answer as plaintext
  /copy answer       copy latest answer as plaintext
  /copy answer raw   copy latest answer with markdown
  /copy plain        copy selected segment as plaintext
  /copy session      copy full transcript

Scroll transcript:
  PgUp/PgDn          scroll transcript
  Shift+Up/Down      fine scroll transcript"
                            .into(),
                    );
                }
                if args == "mouse" {
                    return SlashResult::Display(
                        "Mouse contract:
  App mouse          wheel/click panes
  Mouse passthrough  terminal drag selects text for this session
  Ctrl+Shift+T       toggle app mouse / mouse passthrough
  /mouse on          restore app mouse
  /mouse off         enable terminal-native drag selection for this session"
                            .into(),
                    );
                }
                if args == "next" {
                    return self.handle_tutorial_next(tx);
                }
                if args == "prev" {
                    return self.handle_tutorial_prev(tx);
                }

                if args.is_empty() || matches!(args, "menu" | "commands") {
                    self.open_command_inventory_menu();
                    return SlashResult::Handled;
                }

                let show_all = args == "all";
                let slim = !show_all && self.settings.lock().ok().is_some_and(|s| s.is_slim());
                // Harness-lifecycle commands hidden in slim/Cruise zone.
                const SLIM_HIDDEN: &[&str] = &["tree", "cleave", "delegate", "milestone"];
                let lines: Vec<String> = self
                    .command_menu_projection()
                    .rows
                    .into_iter()
                    .filter(|row| !slim || !SLIM_HIDDEN.contains(&row.name.as_str()))
                    .map(|row| {
                        let source = row.source.label();
                        let safety = row.safety.class_label();
                        if row.subcommands.is_empty() {
                            format!(
                                "  /{:<12} {}  [{} · {}]",
                                row.name, row.description, source, safety
                            )
                        } else {
                            format!(
                                "  /{:<12} {}  [{}]  [{} · {}]",
                                row.name,
                                row.description,
                                row.subcommands.join("|"),
                                source,
                                safety
                            )
                        }
                    })
                    .collect();
                let suffix = if slim {
                    " /help all for full list."
                } else {
                    ""
                };
                SlashResult::Display(format!(
                    "Commands:\n{}\n\nGuided tour: /help tutorial. Type / to browse. Tab completes.{suffix}",
                    lines.join("\n")
                ))
            }

            "mouse" => match args {
                "" => {
                    self.set_terminal_copy_mode(!self.terminal_copy_mode);
                    SlashResult::Handled
                }
                "on" => {
                    self.set_terminal_copy_mode(false);
                    SlashResult::Handled
                }
                "off" => {
                    self.set_terminal_copy_mode(true);
                    SlashResult::Handled
                }
                _ => SlashResult::Display("Usage: /mouse [on|off]".into()),
            },

            "model" => {
                if args.is_empty() || args == "route" {
                    self.open_model_menu();
                    SlashResult::Handled
                } else if matches!(args, "providers" | "provider") {
                    self.open_model_menu();
                    if let Some(menu) = self.active_menu.as_mut() {
                        menu.state.active_tab = "providers".into();
                        menu.state.selected_row = 0;
                    }
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("model", args) {
                        Some(CanonicalSlashCommand::ModelList) => {
                            self.open_model_selector();
                            SlashResult::Handled
                        }
                        Some(CanonicalSlashCommand::SetModelGrade(grade)) => {
                            let _ = tx.try_send(TuiCommand::SetModelGrade {
                                grade: grade.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Intent → grade {grade}"))
                        }
                        Some(CanonicalSlashCommand::SetModelProvider(provider)) => {
                            let _ = tx.try_send(TuiCommand::SetModelProvider {
                                provider: provider.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Provider Intent → {provider}"))
                        }
                        Some(CanonicalSlashCommand::SetModelPolicy(policy)) => {
                            let _ = tx.try_send(TuiCommand::SetModelPolicy {
                                policy: policy.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Policy Intent → {policy}"))
                        }
                        Some(CanonicalSlashCommand::ModelUnpin) => {
                            let _ = tx.try_send(TuiCommand::ModelUnpin { respond_to: None });
                            SlashResult::Display("Clearing exact model pin".into())
                        }
                        Some(CanonicalSlashCommand::SetModel(model)) => {
                            let _ = tx.try_send(TuiCommand::SetModel {
                                model: model.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model → {model}"))
                        }
                        _ => SlashResult::Display("Usage: /model [list|route|providers|grade <F|D|C|B|A|S>|provider <auto|local|upstream|endpoint>|policy <exact|minimum|nearest>|unpin|<provider:model>]".into()),
                    }
                }
            }

            "think" => {
                if args.is_empty() {
                    // No args → open interactive selector
                    self.open_thinking_selector();
                    SlashResult::Handled
                } else if let Some(command @ CanonicalSlashCommand::ThinkingView) =
                    canonical_slash_command("think", args)
                {
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    else {
                        return SlashResult::Display("Thinking status is unavailable".into());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else if let Some(CanonicalSlashCommand::SetThinking(level)) =
                    canonical_slash_command("think", args)
                {
                    let _ = tx.try_send(TuiCommand::SetThinking {
                        level,
                        respond_to: None,
                    });
                    SlashResult::Display(format!("Thinking → {} {}", level.icon(), level.as_str()))
                } else {
                    SlashResult::Display(format!(
                        "Unknown level: {args}. Options: off, low, medium, high"
                    ))
                }
            }

            "profile" => {
                if args.trim().is_empty() {
                    self.open_profile_menu();
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("profile", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /profile [view|export|capture|apply|mqtt on|mqtt off|extension allow <name>|extension deny <name>|extensions clear|persona <name|off>|tone <name|off>|save --name <name> [--project]|save --user|save --project]".into(),
                    )
                }
            }

            "permissions" | "permission" | "trust" => {
                if let Some(command) = canonical_slash_command(cmd, args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /permissions [list|add <path>|remove <path>]\n\
                         Alias: /trust [list|add <path>|remove <path>]"
                            .into(),
                    )
                }
            }

            "automation" | "autonomy" => {
                if let Some(command) = canonical_slash_command(cmd, args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /automation [status|ask|guarded|flow|autonomous]\n\
                         Alias: /autonomy [status|ask|guarded|flow|autonomous]"
                            .into(),
                    )
                }
            }

            "skills" | "skill" => {
                const USAGE: &str = "Usage: /skills [list|reload|refresh|install [name|skills/name]|create|new [--project|--user]|import [--project|--user] <path>|get <name>|delete <name>]";
                if let Some(command) = canonical_slash_command("skills", args) {
                    match command {
                        CanonicalSlashCommand::SkillsView => match self.open_skills_menu() {
                            Ok(()) => SlashResult::Handled,
                            Err(message) => SlashResult::Display(message),
                        },
                        CanonicalSlashCommand::SkillsHelp => {
                            SlashResult::Display(crate::operator_commands::skills_help_text().into())
                        }
                        CanonicalSlashCommand::SkillsReload => {
                            let result = self.refresh_runtime_substrate();
                            SlashResult::Display(result)
                        }
                        CanonicalSlashCommand::SkillCreate(scope) => {
                            // Queue the skill builder prompt — the agent converses
                            // with the operator to create a new skill.
                            let cwd = self.cwd().to_path_buf();
                            let mut builder_prompt = crate::skills::skill_builder_prompt(&cwd);
                            if let Some(scope) = scope {
                                let scope_label = match scope {
                                    SkillCreateScope::Project => "project-local .omegon/skills",
                                    SkillCreateScope::User => "user-level skills directory",
                                };
                                builder_prompt.push_str(&format!(
                                    "\n\nThe operator requested {scope_label} output. Make that destination explicit before writing files."
                                ));
                            }
                            if let Err(result) = Self::submit_prompt_from_slash(
                                tx,
                                PromptSubmission {
                                    text: builder_prompt,
                                    image_paths: Vec::new(),
                                    submitted_by: "local-tui".to_string(),
                                    via: "tui",
                                    queue_mode: PromptQueueMode::UntilReady,
                                    metadata: PromptMetadata::default(),
                                },
                            ) {
                                return result;
                            }
                            self.queue_mode = PromptQueueMode::UntilReady;
                            tracing::debug!("skill builder submitted to runtime queue");
                            SlashResult::Handled
                        }
                        CanonicalSlashCommand::SkillImport { path, scope } => {
                            let scope_hint = match scope {
                                Some(SkillCreateScope::Project) => " into project-local .omegon/skills",
                                Some(SkillCreateScope::User) => " into the user-level skills directory",
                                None => "",
                            };
                            let safe_path = path.replace('`', "\\`");
                            let prompt = format!(
                                "Import the Omegon skill from `{safe_path}`{scope_hint}. Read and validate the skill frontmatter, copy it to the requested external skill directory, and report any schema or collision issues before overwriting existing files. Do not write to bundled/internal skill paths. After import, tell the operator to run `/skills reload` to activate it in this session, then `/skills get <name>` to inspect it."
                            );
                            if let Err(result) = Self::submit_prompt_from_slash(
                                tx,
                                PromptSubmission {
                                    text: prompt,
                                    image_paths: Vec::new(),
                                    submitted_by: "local-tui".to_string(),
                                    via: "tui",
                                    queue_mode: PromptQueueMode::UntilReady,
                                    metadata: PromptMetadata::default(),
                                },
                            ) {
                                return result;
                            }
                            self.queue_mode = PromptQueueMode::UntilReady;
                            SlashResult::Handled
                        }
                        other => {
                            if let Some(request) =
                                crate::operator_commands::control_request_from_slash_command(&other)
                            {
                                let _ = tx.try_send(TuiCommand::ExecuteControl {
                                    request,
                                    respond_to: None,
                                });
                                SlashResult::Handled
                            } else {
                                SlashResult::Display(USAGE.into())
                            }
                        }
                    }
                } else {
                    SlashResult::Display(USAGE.into())
                }
            }

            "plan" => {
                const USAGE: &str = "Usage: /plan [status|list|set <item> | <item>|approve|execute|advance|skip|clear]";
                match canonical_slash_command("plan", args) {
                    Some(
                        command @ (CanonicalSlashCommand::PlanView
                        | CanonicalSlashCommand::PlanList
                        | CanonicalSlashCommand::PlanSet(_)
                        | CanonicalSlashCommand::PlanApprove
                        | CanonicalSlashCommand::PlanExecute
                        | CanonicalSlashCommand::PlanAdvance
                        | CanonicalSlashCommand::PlanSkip
                        | CanonicalSlashCommand::PlanClear),
                    ) => {
                        let _ = tx.try_send(TuiCommand::UpdatePlan {
                            command,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    }
                    _ => SlashResult::Display(USAGE.into()),
                }
            }

            "extension" | "ext" => {
                if args.trim().is_empty() {
                    self.open_extension_runtime_menu();
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("extension", args) {
                    if matches!(command, CanonicalSlashCommand::RuntimeProcessRestart) {
                        let binary = std::env::current_exe()
                            .map_err(|error| error.to_string())
                            .and_then(|path| path.canonicalize().map_err(|error| error.to_string()));
                        match binary {
                            Ok(binary) => {
                                let args = std::env::args().skip(1).collect::<Vec<_>>();
                                match tx.try_send(TuiCommand::RestartProcess { binary, args }) {
                                    Ok(()) => SlashResult::Handled,
                                    Err(error) => SlashResult::Display(format!(
                                        "Extension restart failed: could not queue graceful restart: {error}"
                                    )),
                                }
                            }
                            Err(error) => SlashResult::Display(format!(
                                "Extension restart failed: could not resolve current executable: {error}"
                            )),
                        }
                    } else if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /extension [list|view|init <name>|get <name>|install <name|url|path>|remove <name>|update [name]|enable <name>|disable <name>|refresh|reload|restart|search [query]]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /extension [list|view|init <name>|get <name>|install <name|url|path>|remove <name>|update [name]|enable <name>|disable <name>|refresh|reload|restart|search [query]]"
                            .into(),
                    )
                }
            }

            "catalog" => {
                if let Some(command) = canonical_slash_command("catalog", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display("Usage: /catalog [list|install|remove <id>]".into())
                    }
                } else {
                    SlashResult::Display("Usage: /catalog [list|install|remove <id>]".into())
                }
            }

            "plugin" => {
                if let Some(command) = canonical_slash_command("plugin", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /plugin [list|install <git-url|local-path>|remove <name>|update [name]]. Use /armory install <path> for registry plugins."
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /plugin [list|install <git-url|local-path>|remove <name>|update [name]]. Use /armory install <path> for registry plugins."
                            .into(),
                    )
                }
            }

            "armory" => {
                if let Some(command) = canonical_slash_command("armory", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /armory [list|browse [query]|search [query]|install <name|skills/name|personas/name|tones/name|examples/name>]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /armory [list|browse [query]|search [query]|install <name|skills/name|personas/name|tones/name|examples/name>]"
                            .into(),
                    )
                }
            }

            "doctor" => {
                let command = canonical_slash_command("doctor", "")
                    .expect("/doctor is a canonical command");
                let request = crate::operator_commands::control_request_from_slash_command(&command)
                    .expect("/doctor has a control request");
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request,
                    respond_to: None,
                });
                SlashResult::Handled
            }

            "runtime" => {
                if args.trim().is_empty() {
                    self.open_extension_runtime_menu();
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("runtime", args) {
                    if matches!(command, CanonicalSlashCommand::RuntimeSubstrateRefresh)
                        && self.agent_active
                    {
                        SlashResult::Display(
                            "Runtime refresh unavailable while a model turn is active. Wait for completion or cancel the turn first.".into(),
                        )
                    } else if matches!(command, CanonicalSlashCommand::RuntimeProcessRestart) {
                        let binary = std::env::current_exe()
                            .map_err(|error| error.to_string())
                            .and_then(|path| path.canonicalize().map_err(|error| error.to_string()));
                        match binary {
                            Ok(binary) => {
                                let args = std::env::args().skip(1).collect::<Vec<_>>();
                                match tx.try_send(TuiCommand::RestartProcess { binary, args }) {
                                    Ok(()) => SlashResult::Handled,
                                    Err(error) => SlashResult::Display(format!(
                                        "Runtime restart failed: could not queue graceful restart: {error}"
                                    )),
                                }
                            }
                            Err(error) => SlashResult::Display(format!(
                                "Runtime restart failed: could not resolve current executable: {error}"
                            )),
                        }
                    } else if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display("Runtime control unavailable.".into())
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /runtime [status|inventory|refresh|reload|hup|kick|restart|hot-restart]".into(),
                    )
                }
            }

            "stats" => {
                if args == "bench" {
                    return self.handle_slash_command("/bench", tx);
                }
                if let Some(command) = canonical_slash_command("stats", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /stats [bench]".into())
                }
            }

            // TUI-local command — reads only rendering state (footer_data,
            // session_start). Not routed through Feature dispatch because
            // piping this state through BusEvent would be worse.
            "bench" | "perf" => {
                let session_secs = self.session_start.elapsed().as_secs();
                let turns = self.turn;
                let input_tokens = self.footer_data.session_input_tokens;
                let output_tokens = self.footer_data.session_output_tokens;
                let ctx_pct = self.footer_data.context_percent;
                let ctx_window = self.footer_data.context_window;
                let model = &self.footer_data.model_id;
                let version = env!("CARGO_PKG_VERSION");

                let avg_turn_secs = if turns > 0 {
                    session_secs as f64 / turns as f64
                } else {
                    0.0
                };
                let tokens_per_turn = if turns > 0 {
                    (input_tokens + output_tokens) / turns as u64
                } else {
                    0
                };

                let rss_mb = get_rss_mb().unwrap_or(0.0);

                SlashResult::Display(format!(
                    "Omegon Performance — v{version}\n\n\
                     Startup\n\
                     ────────────────────────────────\n\
                     Process age:        {session_secs}s\n\
                     RSS memory:         {rss_mb:.1} MB\n\n\
                     Session\n\
                     ────────────────────────────────\n\
                     Model:              {model}\n\
                     Turns:              {turns}\n\
                     Avg turn time:      {avg_turn_secs:.1}s\n\
                     Input tokens:       {input_tokens}\n\
                     Output tokens:      {output_tokens}\n\
                     Tokens/turn:        {tokens_per_turn}\n\
                     Context:            {ctx_pct:.0}% of {ctx_window}"
                ))
            }

            "status" => {
                if let Some(command) = canonical_slash_command("status", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /status".into())
                }
            }

            "workspace" => {
                if args == "role" {
                    self.open_workspace_role_selector();
                    SlashResult::Handled
                } else if args == "kind" {
                    self.open_workspace_kind_selector();
                    SlashResult::Handled
                } else {
                    let command = canonical_slash_command("workspace", args)
                        .unwrap_or(CanonicalSlashCommand::WorkspaceStatusView);
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    else {
                        return SlashResult::Display("Usage: /workspace [status|list|new|destroy|adopt|release|archive|prune|bind|role|kind]".into());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
            }

            "persona" => {
                if args == "create" || args == "new" {
                    let builder_prompt = crate::plugins::persona_loader::persona_builder_prompt();
                    if let Err(result) = Self::submit_prompt_from_slash(
                        tx,
                        PromptSubmission {
                            text: builder_prompt,
                            image_paths: Vec::new(),
                            submitted_by: "local-tui".to_string(),
                            via: "tui",
                            queue_mode: PromptQueueMode::UntilReady,
                            metadata: PromptMetadata::default(),
                        },
                    ) {
                        return result;
                    }
                    self.queue_mode = PromptQueueMode::UntilReady;
                    tracing::debug!("persona builder submitted to runtime queue");
                    SlashResult::Handled
                } else if args == "list" {
                    if let Some(command) = canonical_slash_command("persona", args)
                        && let Some(request) =
                            crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        return SlashResult::Handled;
                    }
                    SlashResult::Display("Usage: /persona [list|create|off|<name>]".into())
                } else if args == "off" {
                    if let Some(ref mut registry) = self.augment_registry {
                        let result = registry.deactivate_persona();
                        match result.removed_id {
                            Some(id) => SlashResult::Display(format!("Persona deactivated: {id}")),
                            None => SlashResult::Display("No persona active.".into()),
                        }
                    } else {
                        SlashResult::Display("Augment registry not initialized.".into())
                    }
                } else if args.is_empty() {
                    self.open_persona_selector();
                    SlashResult::Handled
                } else {
                    // Activate by name (case-insensitive match)
                    let target = args.to_lowercase();
                    let cwd = self.cwd().to_path_buf();
                    let persona = crate::plugins::persona_loader::with_available(
                        &cwd,
                        |personas, _| {
                            personas
                                .iter()
                                .find(|p| {
                                    p.name.to_lowercase() == target
                                        || p.id.to_lowercase().contains(&target)
                                })
                                .and_then(|available| available.persona())
                                .cloned()
                        },
                    );
                    match persona {
                        Some(persona) => {
                                    let name = persona.name.clone();
                                    let badge = persona.badge.clone().unwrap_or_else(|| "⚙".into());
                                    let fact_count = persona.mind_facts.len();
                                    if let Some(ref mut registry) = self.augment_registry {
                                        registry.activate_persona(persona);
                                    }
                                    SlashResult::Display(format!(
                                        "{badge} Persona activated: {name} ({fact_count} mind facts)"
                                    ))
                        }
                        None => SlashResult::Display(format!(
                            "Persona '{args}' not found. Run /persona list to see available, or /persona create to build one."
                        )),
                    }
                }
            }

            "tone" => {
                if args == "off" {
                    if let Some(ref mut registry) = self.augment_registry {
                        let result = registry.deactivate_tone();
                        match result {
                            Some(id) => SlashResult::Display(format!("Tone deactivated: {id}")),
                            None => SlashResult::Display("No tone active.".into()),
                        }
                    } else {
                        SlashResult::Display("Augment registry not initialized.".into())
                    }
                } else if args.is_empty() {
                    self.open_tone_selector();
                    SlashResult::Handled
                } else {
                    let target = args.to_lowercase();
                    let cwd = self.cwd().to_path_buf();
                    let tone = crate::plugins::persona_loader::with_available(&cwd, |_, tones| {
                        tones
                            .iter()
                            .find(|t| {
                                t.name.to_lowercase() == target
                                    || t.id.to_lowercase().contains(&target)
                            })
                            .and_then(|available| available.tone())
                            .cloned()
                    });
                    match tone {
                        Some(tone) => {
                                    let name = tone.name.clone();
                                    if let Some(ref mut registry) = self.augment_registry {
                                        registry.activate_tone(tone);
                                    }
                                    SlashResult::Display(format!("♪ Tone activated: {name}"))
                        }
                        None => SlashResult::Display(format!(
                            "Tone '{args}' not found. Run /tone to list available."
                        )),
                    }
                }
            }

            "detail" | "density" => {
                if args.is_empty() {
                    let current = self.settings().tool_detail;
                    let next = current.next();
                    self.update_and_persist(|s| s.tool_detail = next);
                    SlashResult::Display(format!("Tool density → {}", next.as_str()))
                } else if let Some(mode) = crate::settings::ToolDetail::parse(args) {
                    self.update_and_persist(|s| s.tool_detail = mode);
                    SlashResult::Display(format!("Tool density → {}", mode.as_str()))
                } else {
                    SlashResult::Display(format!(
                        "Unknown density: {args}. Options: lean, compact, detailed, verbose"
                    ))
                }
            }

            "context" => {
                if args.is_empty() {
                    self.open_context_menu();
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("context", args) {
                        Some(CanonicalSlashCommand::ContextStatus) => {
                            let _ = tx.try_send(TuiCommand::ContextStatus { respond_to: None });
                            SlashResult::Handled
                        }
                        Some(CanonicalSlashCommand::ContextCompact) => {
                            let _ = tx.try_send(TuiCommand::ContextCompact { respond_to: None });
                            SlashResult::Display("Requesting context compaction…".into())
                        }
                        Some(CanonicalSlashCommand::ContextClear) => {
                            let _ = tx.try_send(TuiCommand::ContextClear { respond_to: None });
                            SlashResult::Display("Starting fresh context…".into())
                        }
                        Some(CanonicalSlashCommand::ContextRequest { kind, query }) => {
                            let display =
                                format!("Requesting mediated context pack for {kind}: {query}");
                            let Some(request) =
                                crate::operator_commands::control_request_from_slash_command(
                                    &CanonicalSlashCommand::ContextRequest { kind, query },
                                )
                            else {
                                return SlashResult::Display("Context request is unavailable".into());
                            };
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: None,
                            });
                            SlashResult::Display(display)
                        }
                        Some(CanonicalSlashCommand::ContextRequestJson(raw)) => {
                            let Some(request) =
                                crate::operator_commands::control_request_from_slash_command(
                                    &CanonicalSlashCommand::ContextRequestJson(raw),
                                )
                            else {
                                return SlashResult::Display("Context JSON request is unavailable".into());
                            };
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: None,
                            });
                            SlashResult::Display(
                                "Requesting mediated context pack from JSON payload".into(),
                            )
                        }
                        Some(CanonicalSlashCommand::SetContextClass(class)) => {
                            let Some(request) =
                                crate::operator_commands::control_request_from_slash_command(
                                    &CanonicalSlashCommand::SetContextClass(class),
                                )
                            else {
                                return SlashResult::Display("Context class update is unavailable".into());
                            };
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Context Policy → {}", class.label()))
                        }
                        _ => {
                            let (sub, _) = args.split_once(' ').unwrap_or((args, ""));
                            SlashResult::Display(format!(
                                "Unknown context option: {sub}.\n\
                                 Use: /context [status|compact|compress|reset|clear|<class>]\n\
                                 Classes: compact, standard, extended, massive"
                            ))
                        }
                    }
                }
            }

            "new" => {
                let _ = tx.try_send(TuiCommand::ContextClear { respond_to: None });
                SlashResult::Handled
            }

            "resume" => {
                let id = args.trim();
                if id.is_empty() {
                    SlashResult::Display("Usage: /resume <session-id>".into())
                } else {
                    let Some(request) = crate::operator_commands::control_request_from_slash_command(
                        &CanonicalSlashCommand::ResumeSession(id.to_string()),
                    ) else {
                        return SlashResult::Display("Resume is unavailable".into());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Display(format!("Resuming session {id}…"))
                }
            }

            "sessions" => {
                if args.trim().is_empty() {
                    self.open_sessions_menu();
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("sessions", args) {
                    Some(CanonicalSlashCommand::ResumeSession(id)) => {
                        let Some(request) = crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::ResumeSession(id.clone()),
                        ) else {
                            return SlashResult::Display("Resume is unavailable".into());
                        };
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Display(format!("Resuming session {id}…"))
                    }
                    _ => {
                        let _ = tx.try_send(TuiCommand::ListSessions { respond_to: None });
                        SlashResult::Handled
                    }
                }
                }
            }

            "memory" => {
                let sub = args.trim();
                if sub.is_empty() {
                    self.open_memory_menu();
                    SlashResult::Handled
                } else if matches!(sub, "status" | "overview") {
                    SlashResult::Display(self.memory_status_text())
                } else {
                    SlashResult::Display(format!(
                        "Unknown memory command: {sub}\n\nUsage: /memory [status|overview]"
                    ))
                }
            }

            "auth" => match canonical_slash_command("auth", args) {
                Some(CanonicalSlashCommand::AuthView) => {
                    self.open_auth_menu();
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthStatus) => {
                    let _ = tx.try_send(TuiCommand::AuthStatus { respond_to: None });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthLogin(provider)) => {
                    let _ = tx.try_send(TuiCommand::AuthLogin {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthLogout(provider)) => {
                    let _ = tx.try_send(TuiCommand::AuthLogout {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthUnlock) => {
                    let _ = tx.try_send(TuiCommand::AuthUnlock { respond_to: None });
                    SlashResult::Handled
                }
                _ => SlashResult::Display(format!(
                    "Unknown auth command: {args}\n\nUsage:\n  /auth\n  /auth status\n  /auth unlock\n  /auth login <provider>\n  /auth logout <provider>"
                )),
            },

            "update" => {
                let trimmed = args.trim();
                if trimmed == "install" {
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    match info {
                        Some(info) if info.is_newer && info.has_downloadable_archive() => {
                            let args = std::env::args().skip(1).collect::<Vec<_>>();
                            let latest = info.latest.clone();
                            match tx.try_send(TuiCommand::InstallUpdate { info, args }) {
                                Ok(()) => SlashResult::Display(format!(
                                    "Installing v{latest}. Omegon will verify the download, save this session, then restart automatically."
                                )),
                                Err(error) => SlashResult::Display(format!(
                                    "Update was not started: could not queue installation: {error}"
                                )),
                            }
                        }
                        Some(info) if info.is_newer => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(format!(
                                "v{} is published, but the signed archive for this platform is not available yet. Rechecking now; run `/update install` again after the release assets finish publishing.",
                                info.latest
                            ))
                        }
                        Some(_) => SlashResult::Display(
                            "No downloadable update is available for this platform.".into(),
                        ),
                        None => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(
                                "Checking for updates now. Run `/update install` again once the check completes."
                                    .into(),
                            )
                        }
                    }
                } else if let Some(channel_arg) = trimmed.strip_prefix("channel") {
                    let channel_arg = channel_arg.trim();
                    if channel_arg.is_empty() {
                        self.open_update_channel_selector();
                        SlashResult::Handled
                    } else if let Some(channel) = crate::update::UpdateChannel::parse(channel_arg) {
                        self.update_settings(|s| s.update_channel = channel.as_str().to_string());
                        if let Some(tx) = self.update_tx.clone() {
                            crate::update::spawn_check_now(tx, channel);
                        }
                        SlashResult::Display(format!(
                            "Update channel set to {}. Rechecking for updates now.",
                            channel.as_str()
                        ))
                    } else {
                        SlashResult::Display("Usage: /update channel [stable|nightly]".into())
                    }
                } else {
                    // Check if an update is available
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    let channel = self.settings().update_channel;
                    match info {
                        Some(info) if info.is_newer => SlashResult::Display(format!(
                            "🆕 Update available on {channel}: v{} → v{}\n\n{}\n\n{}\n\nCommands:\n  /update install\n  /update channel [stable|nightly]",
                            info.current,
                            info.latest,
                            if info.release_notes.is_empty() {
                                "(no release notes)".into()
                            } else {
                                info.release_notes
                                    .lines()
                                    .take(20)
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            },
                            if !info.has_downloadable_archive() {
                                if let Some(tx) = self.update_tx.clone() {
                                    let channel = crate::update::UpdateChannel::parse(
                                        &self.settings().update_channel,
                                    )
                                    .unwrap_or(crate::update::UpdateChannel::Stable);
                                    crate::update::spawn_check_now(tx, channel);
                                }
                                String::from(
                                    "Release assets for this platform are not available yet. Rechecking now.",
                                )
                            } else {
                                String::from("Run `/update install` to download and restart")
                            },
                        )),
                        _ => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(format!(
                                "✓ No update is currently cached for the {channel} channel. Checking GitHub now.\n\nCommands:\n  /update install         — install a discovered update\n  /update channel stable  — stable releases only\n  /update channel nightly — nightly builds from main\n  /update channel         — show current channel"
                            ))
                        }
                    }
                }
            }

            "init" => {
                let cwd = std::path::Path::new(&self.footer_data.cwd);
                let project_root = crate::setup::find_project_root(cwd);
                match args {
                    "" | "menu" => {
                        self.open_init_menu();
                        SlashResult::Handled
                    }
                    "scan" => {
                        let report = crate::migrate::init_project(&project_root, false);
                        SlashResult::Display(report)
                    }
                    "migrate" => {
                        let report = crate::migrate::init_project(&project_root, true);
                        SlashResult::Display(report)
                    }
                    "profile migrate --project" | "profiles migrate --project" => {
                        match crate::migrate::migrate_legacy_profile_to_registry(
                            &project_root,
                            crate::migrate::InitProfileScope::Project,
                        ) {
                            Ok(message) => SlashResult::Display(message),
                            Err(error) => SlashResult::Display(format!("✗ {error}")),
                        }
                    }
                    "profile migrate --user" | "profiles migrate --user" => {
                        match crate::migrate::migrate_legacy_profile_to_registry(
                            &project_root,
                            crate::migrate::InitProfileScope::User,
                        ) {
                            Ok(message) => SlashResult::Display(message),
                            Err(error) => SlashResult::Display(format!("✗ {error}")),
                        }
                    }
                    _ => SlashResult::Display(format!(
                        "Usage: /init [menu|scan|migrate|profile migrate --project|profile migrate --user]\n\nUnknown subcommand: {args}"
                    )),
                }
            }

            "migrate" => {
                let source = if args.is_empty() { "auto" } else { args };
                let cwd = self.cwd();
                let report = crate::migrate::run(source, cwd);
                SlashResult::Display(report.summary())
            }

            "chronos" => {
                let sub = if args.is_empty() { "week" } else { args };
                match crate::tools::chronos::execute(sub, None, None, None) {
                    Ok(text) => SlashResult::Display(text),
                    Err(e) => SlashResult::Display(format!("✗ {e}")),
                }
            }

            "auspex" => match args {
                "" | "status" => SlashResult::Display(self.auspex_status_text()),
                "open" => {
                    if let Some(ref startup) = self.web_startup {
                        match launch_auspex_with_startup(startup) {
                            Ok(target) => SlashResult::Display(format!(
                                "Launching Auspex via the primary local desktop handoff ({target}).\n\nOmegon is passing native attach metadata for the current live session over `AUSPEX_OMEGON_ATTACH_JSON` with `transport=omegon-ipc`. The embedded browser bridge remains available only as compatibility/debug support behind `/dash`."
                            )),
                            Err(e) => SlashResult::Display(format!("Failed to launch Auspex: {e}")),
                        }
                    } else {
                        let _ = tx.try_send(TuiCommand::StartWebDashboard);
                        SlashResult::Display(
                                "Preparing the local compatibility surface so `/auspex open` can complete the native desktop handoff once startup metadata is available. `/dash` remains the explicit compatibility/debug browser path.".into()
                            )
                    }
                }
                other => SlashResult::Display(format!(
                    "Usage: /auspex status | /auspex open\n\nUnknown subcommand: {other}"
                )),
            },

            "dash" => {
                // /dash remains the compatibility/debug command for opening the browser UI.
                // If the server is already running, open the browser.
                // If not, start it (which auto-opens on ready).
                if let Some(url) = dash_browser_url(self.web_startup.as_ref(), self.web_server_addr)
                {
                    if args == "status" {
                        let detail = self
                            .web_startup
                            .as_ref()
                            .map(|startup| {
                                let (http_security, ws_security) =
                                    startup_transport_security(startup);
                                let warnings = if startup.daemon_status.transport_warnings.is_empty() {
                                    "none".to_string()
                                } else {
                                    startup.daemon_status.transport_warnings.join(" | ")
                                };
                                format!(
                                    "\nstartup: {}\nwebsocket: {}\ntransport: http={}, ws={}\nqueue depth: {}\nprocessed events: {}\ntransport warnings: {}",
                                    startup.startup_url,
                                    startup.ws_url,
                                    format_transport_security(&http_security),
                                    format_transport_security(&ws_security),
                                    startup.daemon_status.queued_events,
                                    startup.daemon_status.processed_events,
                                    warnings,
                                )
                            })
                            .unwrap_or_default();
                        SlashResult::Display(format!(
                            "Auspex compatibility/debug browser path running at {url}{detail}"
                        ))
                    } else {
                        crate::native_io::open_browser(&url);
                        SlashResult::Display(format!(
                            "Opened Auspex compatibility/debug browser path at {url}"
                        ))
                    }
                } else {
                    let _ = tx.try_send(TuiCommand::StartWebDashboard);
                    SlashResult::Display("Starting Auspex compatibility/debug browser path…".into())
                }
            }

            "splash" => {
                // Set flag to replay splash on next draw cycle
                self.replay_splash = true;
                SlashResult::Handled
            }

            "delegate" | "subagent" => {
                if let Some(command) = canonical_slash_command(cmd, args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /delegate status or /subagent status\n\nTo invoke a delegate/subagent, use the delegate agent tool."
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /delegate status or /subagent status\n\nTo invoke a delegate/subagent, use the delegate agent tool."
                            .into(),
                    )
                }
            }

            "subagents" => {
                SlashResult::Display("Use the explicit singular command: /subagent status".into())
            }

            "focus" => SlashResult::Display(
                "Focus mode has been removed. Use Ctrl+O or Tab on an empty composer to toggle the tool detail row."
                    .into(),
            ),

            "ui" => {
                let args = args.trim();
                if let Some(terminal) = args.strip_prefix("terminal ") {
                    return match TerminalPresentation::parse(terminal.trim()) {
                        Ok(value) => {
                            self.base_terminal = value;
                            SlashResult::Display(format!("Terminal → {} (this session); detail: {}", value.name(), self.ui_presentation.level.name()))
                        }
                        Err(message) => SlashResult::Display(message),
                    };
                }
                if let Some(density) = args
                    .strip_prefix("detail ")
                    .or_else(|| args.strip_prefix("density "))
                {
                    return self.handle_slash_command(&format!("/detail {}", density.trim()), tx);
                }
                if matches!(args, "detail" | "density") {
                    return self.handle_slash_command("/detail", tx);
                }
                if args.is_empty() || args == "surfaces" {
                    self.open_ui_menu();
                    SlashResult::Handled
                } else if args == "status" {
                    SlashResult::Display(self.ui_status_text())
                } else if matches!(args, "om" | "lean" | "slim") {
                    let outcome = self.handle_ui_preset_action(SetUiPresetAction {
                        level: UiPresentationLevel::Active,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| "UI → active".into()))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if args == "active" {
                    let outcome = self.handle_ui_preset_action(SetUiPresetAction {
                        level: UiPresentationLevel::Active,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| "UI → active".into()))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if args == "full" {
                    let outcome = self.handle_ui_preset_action(SetUiPresetAction {
                        level: UiPresentationLevel::Full,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => SlashResult::Display(
                            message
                                .unwrap_or_else(|| "UI → full (+ dashboard + instruments)".into()),
                        ),
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("toggle ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let enabled = match surface {
                        UiSurfaceToggle::Dashboard => !self.ui_surfaces.dashboard,
                        UiSurfaceToggle::Instruments => !self.ui_surfaces.instruments,
                        UiSurfaceToggle::Footer => !self.ui_surfaces.footer,
                        UiSurfaceToggle::Activity => !self.ui_surfaces.activity,
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: enabled,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!(
                                    "UI surface {}: {}",
                                    if enabled { "enabled" } else { "disabled" },
                                    surface.label()
                                )
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("show ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: true,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!("UI surface enabled: {}", surface.label())
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("hide ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: false,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!("UI surface disabled: {}", surface.label())
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else {
                    SlashResult::Display(format!(
                        "Unknown UI command: {args}

{}",
                        self.ui_status_text()
                    ))
                }
            }

            "copy" => match args {
                "" | "raw" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Raw);
                    SlashResult::Handled
                }
                "answer" | "answer plain" | "answer plaintext" | "latest plain"
                | "latest plaintext" | "response plain" | "assistant plain" => {
                    self.copy_latest_assistant_response(SegmentExportMode::Plaintext);
                    SlashResult::Handled
                }
                "answer raw" | "latest" | "response" | "assistant" => {
                    self.copy_latest_assistant_response(SegmentExportMode::Raw);
                    SlashResult::Handled
                }
                "plain" | "plaintext" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Plaintext);
                    SlashResult::Handled
                }
                "session" | "all" => {
                    self.copy_full_session();
                    SlashResult::Handled
                }
                _ => SlashResult::Display(
                    "Usage: /copy [raw|plain|answer|answer raw|latest|session]".into(),
                ),
            },

            "transcript" => {
                let allow_suffix = args == "suffix";
                if !matches!(args, "" | "open" | "file" | "suffix") {
                    self.conversation.push_system(
                        "Usage: /transcript [file|open|suffix]\n  file/open: require an exact full-session semantic transcript\n  suffix: export the explicitly labeled exact suffix for mixed lineage",
                    );
                } else {
                    match self.write_exact_semantic_transcript(allow_suffix) {
                        Ok(path) => self.conversation.push_system(&format!(
                            "Exact semantic transcript written:\n  {}",
                            path.display()
                        )),
                        Err(error) => self.conversation.push_system(&format!(
                            "Semantic transcript unavailable: {error}"
                        )),
                    }
                }
                SlashResult::Handled
            }

            "session-export" => {
                match args {
                    "" | "open" | "file" | "md" | "markdown" => {
                        self.export_session_transcript_markdown();
                    }
                    "scrollback" | "native" => {
                        self.print_transcript_to_native_scrollback();
                    }
                    _ => self.conversation.push_system(
                        "Usage: /session-export [file|open|scrollback]\n  Exports the current presentation/evidence view; it does not claim exact transcript semantics.",
                    ),
                }
                SlashResult::Handled
            }

            "tree" => {
                if let Some(command) = canonical_slash_command("tree", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /tree [list|... ]".into())
                }
            }

            "milestone" => self.handle_milestone(args),

            "demo" => self.handle_tutorial(args, tx),

            "variables" | "vars" => self.handle_variables(args, tx),

            "secrets" => self.handle_secrets(args, tx),

            "vault" => {
                if args == "configure" {
                    let options = vec![
                        selector::SelectOption {
                            value: "env".to_string(),
                            label: "Set VAULT_ADDR via environment".to_string(),
                            description: "Write /vault configure env into the editor".to_string(),
                            active: false,
                        },
                        selector::SelectOption {
                            value: "file".to_string(),
                            label: "Create ~/.omegon/vault.json".to_string(),
                            description: "Write /vault configure file into the editor".to_string(),
                            active: false,
                        },
                    ];
                    self.selector = Some(selector::Selector::new(
                        "Vault Configuration — pick a setup flow",
                        options,
                    ));
                    self.selector_kind = Some(SelectorKind::VaultConfigure);
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("vault", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(format!(
                            "Unknown vault subcommand: {args}\nOptions: status, configure, init-policy"
                        ))
                    }
                } else {
                    SlashResult::Display(format!(
                        "Unknown vault subcommand: {args}\nOptions: status, unseal, login, configure, init-policy"
                    ))
                }
            }

            // /connect owns discovery/setup; /login remains a compatibility entry.
            "connect" | "login" => {
                let mut parts = args.split_whitespace();
                let provider = parts.next().unwrap_or("");
                let option = parts.next();
                if parts.next().is_some() || option.is_some_and(|value| value != "--console") {
                    return SlashResult::Display("Usage: /connect [provider] [--console]".into());
                }
                if provider.is_empty() {
                    self.open_auth_menu();
                    SlashResult::Handled
                } else if (provider.eq_ignore_ascii_case(crate::providers::zen::PROVIDER_ID) || provider.eq_ignore_ascii_case("free")) && option.is_none() {
                    self.open_free_model_menu();
                    SlashResult::Handled
                } else if let Some(provider) = crate::auth::provider_by_id(provider) {
                    let key_name = crate::auth::operator_api_key_name(provider);
                    if option == Some("--console") {
                        if let Some(url) = key_name.and_then(crate::capabilities::secrets::secret_console_url) {
                            std::thread::spawn(move || { let _ = open::that(url); });
                            SlashResult::Display("Opening provider key console.".into())
                        } else {
                            SlashResult::Display("This provider has no API-key console. Use /connect to choose a connection method.".into())
                        }
                    } else if provider.auth_method == crate::auth::AuthMethod::ApiKey
                        && let Some(key_name) = key_name
                    {
                        self.editor.start_secret_input(key_name);
                        SlashResult::Display(format!("Paste {key_name} — input hidden"))
                    } else if crate::auth::operator_oauth_setup_supported(provider) {
                        let _ = tx.try_send(TuiCommand::AuthLogin {
                            provider: provider.id.to_string(),
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display("This provider uses external configuration. Use /auth status for credential details and /model to select a route.".into())
                    }
                } else {
                    SlashResult::Display("Unknown provider. Use /connect and Add provider to search available providers.".into())
                }
            }

            // /logout [provider] — alias for /auth logout <provider>
            "logout" => {
                if let Some(CanonicalSlashCommand::AuthLogout(provider)) =
                    canonical_slash_command("logout", args)
                {
                    let _ = tx.try_send(TuiCommand::AuthLogout {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(format!(
                        "Usage: /logout <provider>\n\nProviders: {}",
                        crate::auth::operator_auth_provider_help_list()
                    ))
                }
            }

            // /note <text> — append a deferred investigation note
            "note" => {
                if args.is_empty() {
                    return self.handle_slash_command("/notes", tx);
                }
                if let Some(command) = canonical_slash_command("note", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /note <text>".into())
                }
            }

            // /notes [clear] — show or clear pending notes
            "notes" => {
                if let Some(command) = canonical_slash_command("notes", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /notes [clear]".into())
                }
            }

            // /checkin — interactive triage of what needs attention
            "checkin" => {
                if let Some(command) = canonical_slash_command("checkin", args)
                    && let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /checkin".into())
                }
            }

            "exit" | "quit" => SlashResult::Quit,

            // ── Aliases ─────────────────────────────────────────────
            "shackle" => {
                self.apply_ui_preset(UiSurfaces::lean());
                let Some(request) = crate::operator_commands::control_request_from_slash_command(
                    &CanonicalSlashCommand::SetRuntimeMode { slim: true },
                ) else {
                    return SlashResult::Display("Runtime mode update is unavailable".into());
                };
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request,
                    respond_to: None,
                });
                SlashResult::Display("Shackled: om mode active.".into())
            }
            "unshackle" => {
                self.apply_ui_preset(UiSurfaces::full());
                let Some(request) = crate::operator_commands::control_request_from_slash_command(
                    &CanonicalSlashCommand::SetRuntimeMode { slim: false },
                ) else {
                    return SlashResult::Display("Runtime mode update is unavailable".into());
                };
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request,
                    respond_to: None,
                });
                SlashResult::Display("Unshackled: omegon mode active.".into())
            }
            "warp" => {
                let slim_now = self.settings.lock().ok().is_some_and(|s| s.is_slim());
                let target_slim = !slim_now;
                self.apply_ui_preset(if target_slim {
                    UiSurfaces::lean()
                } else {
                    UiSurfaces::full()
                });
                let Some(request) = crate::operator_commands::control_request_from_slash_command(
                    &CanonicalSlashCommand::SetRuntimeMode { slim: target_slim },
                ) else {
                    return SlashResult::Display("Runtime mode update is unavailable".into());
                };
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request,
                    respond_to: None,
                });
                SlashResult::Display(if target_slim {
                    "Warped to om mode.".into()
                } else {
                    "Warped to omegon mode.".into()
                })
            }
            "thinking" => self.handle_slash_command(&format!("/think {args}"), tx),
            "models" => self.handle_slash_command("/model", tx),
            "settings" | "config" => {
                let target = match args.trim() {
                    "" | "runtime" => None,
                    "model" => Some("/model"),
                    "auth" => Some("/auth"),
                    "skills" => Some("/skills"),
                    "extensions" | "extension" => Some("/extension"),
                    "ui" => Some("/ui"),
                    "context" => Some("/context"),
                    "memory" => Some("/memory"),
                    "profile" => Some("/profile"),
                    "secrets" => Some("/secrets"),
                    "sandbox" => Some("/sandbox"),
                    "updates" | "update" => Some("/update"),
                    _ => {
                        return SlashResult::Display(format!(
                            "Unknown settings area: {args}\n\nUsage: /settings [runtime|model|auth|skills|extensions|ui|context|memory|profile|secrets|sandbox|updates]\nAlias: /config"
                        ));
                    }
                };
                if let Some(command) = target {
                    self.handle_slash_command(command, tx)
                } else {
                    self.open_settings_menu();
                    self.command_panel = None;
                    SlashResult::Handled
                }
            }
            "preferences" | "prefs" => {
                self.open_preferences_selector();
                SlashResult::Handled
            }
            "sandbox" => {
                let sub = args.split_whitespace().next().unwrap_or("");
                match sub {
                    "on" | "enable" => {
                        // Check for container runtime before enabling
                        let runtime = crate::container_runtime::detect();
                        if let Some(ref rt) = runtime {
                            let cwd = self.cwd().to_path_buf();
                            if let Ok(mut s) = self.settings.lock() {
                                s.sandbox = true;
                                let mut profile = crate::settings::Profile::load(&cwd);
                                profile.capture_from(&s);
                                let _ = profile.save(&cwd);
                            }
                            SlashResult::Display(format!(
                                "Sandbox enabled ({rt})\n\n\
                                 Delegate and cleave children will now run inside \
                                 isolated containers with:\n\
                                 - Read-only root filesystem\n\
                                 - No network access\n\
                                 - Workspace mounted at /work\n\n\
                                 /sandbox off     disable\n\
                                 /sandbox status  current state"
                            ))
                        } else {
                            SlashResult::Display(
                                "No container runtime found.\n\n\
                                 Sandbox requires podman or docker:\n\
                                 - macOS:  brew install podman\n\
                                 - Linux:  apt install podman  (or docker)\n\
                                 - NixOS:  nix-env -i podman\n\n\
                                 Podman is preferred (rootless, daemonless)."
                                    .into(),
                            )
                        }
                    }
                    "off" | "disable" => {
                        let cwd = self.cwd().to_path_buf();
                        if let Ok(mut s) = self.settings.lock() {
                            s.sandbox = false;
                            let mut profile = crate::settings::Profile::load(&cwd);
                            profile.capture_from(&s);
                            let _ = profile.save(&cwd);
                        }
                        SlashResult::Display(
                            "Sandbox disabled. Children run as local subprocesses.".into(),
                        )
                    }
                    "" | "status" => {
                        let enabled = self
                            .settings
                            .lock()
                            .ok()
                            .map(|s| s.sandbox)
                            .unwrap_or(false);
                        let runtime = crate::container_runtime::detect();
                        let rt_str = runtime.as_deref().unwrap_or("not found");
                        let status = if enabled { "enabled" } else { "disabled" };
                        SlashResult::Display(format!(
                            "Sandbox: {status}\n\
                             Runtime: {rt_str}\n\n\
                             /sandbox on   enable container isolation\n\
                             /sandbox off  disable (use local subprocesses)"
                        ))
                    }
                    _ => SlashResult::Display("Usage: /sandbox [on|off|status]".into()),
                }
            }
            "version" => SlashResult::Display(format!(
                "Version\n  Omegon:     {}\n  Git SHA:    {}\n  Build Date: {}",
                env!("CARGO_PKG_VERSION"),
                env!("OMEGON_GIT_SHA"),
                env!("OMEGON_BUILD_DATE"),
            )),

            "smoke" => match canonical_slash_command("smoke", args) {
                Some(CanonicalSlashCommand::Smoke(crate::smoke_surface::SmokeCommand::List)) => {
                    SlashResult::Display(crate::smoke_surface::smoke_list_text())
                }
                Some(CanonicalSlashCommand::Smoke(crate::smoke_surface::SmokeCommand::Scenario(scenario))) => {
                    self.launch_surface_smoke(scenario)
                }
                _ => SlashResult::Display("Usage: /smoke [list|cleave|delegate|surface]".into()),
            },
            "q" => SlashResult::Quit,

            "editor" => SlashResult::Display(handle_editor_command(args)),

            "cleave" => {
                // /cleave starts background workers from an interactive session, so disclose
                // subscription-credential automation risk there without warning on normal TUI use.
                if self.footer_data.is_oauth
                    && crate::providers::anthropic_credential_mode()
                        == crate::providers::AnthropicCredentialMode::OAuthOnly
                {
                    self.show_toast(
                        "Anthropic subscription is active. /cleave starts background workers, which may be restricted by Anthropic's \
                         Consumer Terms for Claude.ai / Claude Pro automation. Omegon will \
                         proceed with your requested provider/model; the risk is yours. \
                         Reference: https://www.anthropic.com/legal/consumer-terms",
                        ratatui_toaster::ToastType::Warning,
                    );
                }
                if let Some(command) = canonical_slash_command("cleave", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else if self.bus_commands.iter().any(|c| c.name == "cleave") {
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "cleave".to_string(),
                            args: args.to_string(),
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Cleave extension not loaded. Run omegon from a project directory."
                                .into(),
                        )
                    }
                } else if self.bus_commands.iter().any(|c| c.name == "cleave") {
                    let _ = tx.try_send(TuiCommand::BusCommand {
                        name: "cleave".to_string(),
                        args: args.to_string(),
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Cleave extension not loaded. Run omegon from a project directory.".into(),
                    )
                }
            }

            _ => {
                // Check if a bus feature handles this command
                if self.bus_commands.iter().any(|c| c.name == cmd) {
                    let _ = tx.try_send(TuiCommand::BusCommand {
                        name: cmd.to_string(),
                        args: args.to_string(),
                    });
                    SlashResult::Handled
                } else {
                    // Try prefix match — e.g. "/das" matches "/dash"
                    let matches: Vec<&str> = crate::command_registry::BUILTIN_COMMANDS
                        .iter()
                        .map(|command| command.name)
                        .filter(|name| name.starts_with(cmd) && *name != cmd)
                        .collect();
                    if matches.len() == 1 {
                        // Unique prefix match — execute it
                        let full_cmd = if args.is_empty() {
                            format!("/{}", matches[0])
                        } else {
                            format!("/{} {args}", matches[0])
                        };
                        self.handle_slash_command(&full_cmd, tx)
                    } else if !matches.is_empty() {
                        // Ambiguous prefix
                        SlashResult::Display(format!(
                            "Ambiguous command /{cmd}. Did you mean: {}",
                            matches
                                .iter()
                                .map(|m| format!("/{m}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    } else {
                        // No match at all — show error, do NOT send to agent
                        SlashResult::Display(format!(
                            "Unknown command: /{cmd}. Type /help for commands."
                        ))
                    }
                }
            }
        }
    }
}
