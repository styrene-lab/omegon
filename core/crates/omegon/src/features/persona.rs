//! Persona feature — exposes persona and tone management as agent-callable tools.
//!
//! Tools:
//! - `switch_persona` — activate a persona by name, or deactivate
//! - `switch_tone` — activate a tone by name, or deactivate
//! - `list_personas` — enumerate available personas and tones

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

use omegon_traits::{
    BusEvent, BusRequest, ContentBlock, Feature, NotifyLevel, ToolDefinition, ToolResult,
};

use crate::plugins::persona_loader;
use crate::plugins::registry::AugmentRegistry;

#[derive(Clone)]
pub struct SharedAugmentRegistry(Arc<Mutex<AugmentRegistry>>);

impl SharedAugmentRegistry {
    pub fn new(registry: AugmentRegistry) -> Self {
        Self(Arc::new(Mutex::new(registry)))
    }

    pub fn lock(&self) -> MutexGuard<'_, AugmentRegistry> {
        self.0.lock().unwrap()
    }
}

/// Feature that exposes persona/tone management as agent tools.
pub struct PersonaFeature {
    registry: SharedAugmentRegistry,
    /// Flag indicating harness status should be refreshed on next turn boundary
    refresh_status_pending: AtomicBool,
    /// Workspace root used to resolve skill project-signal evidence.
    ///
    /// Captured once at construction rather than read per turn: the registry
    /// loaded its skills against this same root, so re-reading the process CWD
    /// each turn could silently disagree with what was loaded.
    workspace_root: std::path::PathBuf,
}

impl PersonaFeature {
    pub fn new(registry: SharedAugmentRegistry) -> Self {
        Self::with_workspace_root(
            registry,
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )
    }

    pub fn with_workspace_root(
        registry: SharedAugmentRegistry,
        workspace_root: std::path::PathBuf,
    ) -> Self {
        Self {
            registry,
            refresh_status_pending: AtomicBool::new(false),
            workspace_root,
        }
    }

    /// Get a reference to the inner registry (for HarnessStatus, etc.)
    pub fn registry(&self) -> std::sync::MutexGuard<'_, AugmentRegistry> {
        self.registry.lock()
    }
}

#[async_trait]
impl Feature for PersonaFeature {
    fn name(&self) -> &str {
        "persona"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::persona::SWITCH_PERSONA.into(),
                label: "switch_persona".into(),
                description: "Switch the active persona identity. Personas carry domain expertise, mind stores, and skill profiles. Use 'off' to deactivate.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Persona name to activate (case-insensitive), or 'off' to deactivate"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Why switching persona"
                        }
                    },
                    "required": ["name"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::persona::SWITCH_TONE.into(),
                label: "switch_tone".into(),
                description: "Switch the conversational tone. Tones modify voice/style without changing expertise. Use 'off' to deactivate.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Tone name to activate (case-insensitive), or 'off' to deactivate"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Why switching tone"
                        }
                    },
                    "required": ["name"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::persona::LIST_PERSONAS.into(),
                label: "list_personas".into(),
                description: "List available personas and tones installed on this system. Shows active status.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: serde_json::Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::persona::SWITCH_PERSONA => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");

                if name == "off" {
                    // Deactivate current persona
                    let result = self.registry.lock().deactivate_persona();
                    self.refresh_status_pending.store(true, Ordering::Relaxed);

                    if result.removed_id.is_some() {
                        return Ok(text_result("Persona deactivated."));
                    } else {
                        return Ok(text_result("No persona was active."));
                    }
                }

                let target = name.to_lowercase();
                let result = persona_loader::with_available(&self.workspace_root, |personas, _| {
                    match personas
                        .iter()
                        .find(|p| {
                            p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target)
                        })
                        .and_then(|available| available.persona())
                        .cloned()
                    {
                        Some(loaded_persona) => {
                            let badge = loaded_persona.badge.clone().unwrap_or_else(|| "⚙".into());
                            let fact_count = loaded_persona.mind_facts.len();
                            let pname = loaded_persona.name.clone();
                            let skills = loaded_persona.activated_skills.join(", ");
                            let activation_result =
                                self.registry.lock().activate_persona(loaded_persona);
                            self.refresh_status_pending.store(true, Ordering::Relaxed);
                            let mut message = format!(
                                "{badge} Persona activated: {pname}\n  Mind facts: {fact_count}\n  Skills: {skills}\n\n\
                                Note: The persona directive and mind facts are now active in the system prompt."
                            );
                            if let Some(prev) = activation_result.previous_id {
                                message.push_str(&format!(
                                    "\n\nPrevious persona ({prev}) was deactivated."
                                ));
                            }
                            text_result(&message)
                        }
                        None => {
                            let available_names: Vec<_> =
                                personas.iter().map(|p| p.name.as_str()).collect();
                            error_result(&format!(
                                "Persona '{name}' not found. Available: {}",
                                if available_names.is_empty() {
                                    "none installed".into()
                                } else {
                                    available_names.join(", ")
                                }
                            ))
                        }
                    }
                });
                Ok(result)
            }

            crate::tool_registry::persona::SWITCH_TONE => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");

                if name == "off" {
                    // Deactivate current tone
                    let removed = self.registry.lock().deactivate_tone();
                    self.refresh_status_pending.store(true, Ordering::Relaxed);

                    if removed.is_some() {
                        return Ok(text_result("Tone deactivated."));
                    } else {
                        return Ok(text_result("No tone was active."));
                    }
                }

                let target = name.to_lowercase();
                let result = persona_loader::with_available(&self.workspace_root, |_, tones| {
                    match tones
                        .iter()
                        .find(|t| {
                            t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target)
                        })
                        .and_then(|available| available.tone())
                        .cloned()
                    {
                        Some(loaded_tone) => {
                            let tname = loaded_tone.name.clone();
                            let exemplar_count = loaded_tone.exemplars.len();
                            let previous = self.registry.lock().activate_tone(loaded_tone);
                            self.refresh_status_pending.store(true, Ordering::Relaxed);
                            let mut message = format!(
                                "♪ Tone activated: {tname}\n  Exemplars: {exemplar_count}\n\n\
                                Note: The tone directive is now active in the system prompt."
                            );
                            if let Some(prev) = previous {
                                message.push_str(&format!(
                                    "\n\nPrevious tone ({prev}) was deactivated."
                                ));
                            }
                            text_result(&message)
                        }
                        None => {
                            let available_names: Vec<_> =
                                tones.iter().map(|t| t.name.as_str()).collect();
                            error_result(&format!(
                                "Tone '{name}' not found. Available: {}",
                                if available_names.is_empty() {
                                    "none installed".into()
                                } else {
                                    available_names.join(", ")
                                }
                            ))
                        }
                    }
                });
                Ok(result)
            }

            crate::tool_registry::persona::LIST_PERSONAS => {
                let (active_persona, active_tone) = {
                    let registry = self.registry.lock();
                    (
                        registry.active_persona().map(|p| p.id.clone()),
                        registry.active_tone().map(|t| t.id.clone()),
                    )
                };
                let out =
                    persona_loader::with_available(&self.workspace_root, |personas, tones| {
                        let mut out = String::new();

                        out.push_str("## Personas\n\n");
                        if personas.is_empty() {
                            out.push_str("No personas installed.\n");
                        } else {
                            for p in personas {
                                let marker = if active_persona.as_ref() == Some(&p.id) {
                                    " ● (active)"
                                } else {
                                    ""
                                };
                                out.push_str(&format!(
                                    "- **{}**{}: {}\n",
                                    p.name, marker, p.description
                                ));
                            }
                        }

                        out.push_str("\n## Tones\n\n");
                        if tones.is_empty() {
                            out.push_str("No tones installed.\n");
                        } else {
                            for t in tones {
                                let marker = if active_tone.as_ref() == Some(&t.id) {
                                    " ● (active)"
                                } else {
                                    ""
                                };
                                out.push_str(&format!(
                                    "- **{}**{}: {}\n",
                                    t.name, marker, t.description
                                ));
                            }
                        }

                        out.push_str("\nInstall plugins with: `omegon plugin install <git-url>`");
                        out
                    });
                Ok(text_result(&out))
            }

            _ => anyhow::bail!("unknown persona tool: {tool_name}"),
        }
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            // On session start, log the active persona/tone
            BusEvent::SessionStart { .. } => {
                let mut requests = Vec::new();
                let registry = self.registry.lock();
                if let Some(persona) = registry.active_persona() {
                    let badge = persona.badge.as_deref().unwrap_or("⚙");
                    requests.push(BusRequest::Notify {
                        message: format!("{badge} Persona: {}", persona.name),
                        level: NotifyLevel::Info,
                    });
                }
                if let Some(tone) = registry.active_tone() {
                    requests.push(BusRequest::Notify {
                        message: format!("♪ Tone: {}", tone.name),
                        level: NotifyLevel::Info,
                    });
                }
                requests
            }
            // Check for refresh flag on turn boundaries
            BusEvent::TurnEnd(_) => {
                if self.refresh_status_pending.load(Ordering::Relaxed) {
                    self.refresh_status_pending.store(false, Ordering::Relaxed);
                    vec![BusRequest::RefreshHarnessStatus]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    fn provide_context(
        &self,
        signals: &omegon_traits::ContextSignals<'_>,
    ) -> Option<omegon_traits::ContextInjection> {
        // Inject persona directive + tone directive as context, with skill
        // bodies disclosed progressively. The operator prompt is threaded in so
        // trigger-activated skills are admitted on the turn they are named
        // rather than being fixed at startup.
        let prompt = self
            .registry
            .lock()
            .build_augmentation_disclosed(&self.workspace_root, Some(signals.user_prompt));
        // Keep the existing replacement key, including an empty replacement when
        // cleared. Returning None would retain the old combined layer indefinitely.

        Some(omegon_traits::ContextInjection {
            source: "persona".into(),
            content: prompt,
            priority: 85, // Optional augmentation; host policy is mandatory base input
            ttl_turns: u32::MAX, // Never expires — always active while persona is on
        })
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        details: json!({}),
    }
}

fn text_result_with_details(text: &str, details: Value) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        details,
    }
}

fn error_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("Error: {text}"),
        }],
        details: json!({ "error": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> AugmentRegistry {
        AugmentRegistry::new()
    }

    /// Build signals carrying an operator prompt; other fields are inert here.
    fn signals_with_prompt(prompt: &str) -> omegon_traits::ContextSignals<'_> {
        const NO_TOOLS: &[String] = &[];
        const NO_FILES: &[std::path::PathBuf] = &[];
        omegon_traits::ContextSignals {
            user_prompt: prompt,
            recent_tools: NO_TOOLS,
            recent_files: NO_FILES,
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 200_000,
        }
    }

    #[test]
    fn injected_context_discloses_skills_and_tracks_the_operator_prompt() {
        use omegon_traits::Feature;

        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        for (name, activation, extra, marker) in [
            (
                "style",
                "domain_detected",
                "triggers = [\"diagram\"]",
                "STYLE BODY MARKER",
            ),
            (
                "typescript",
                "project_detected",
                "project_signals = [\"tsconfig.json\"]",
                "TS BODY MARKER",
            ),
        ] {
            let dir = skills_dir.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let filler = "filler line\n".repeat(200);
            std::fs::write(
                dir.join("SKILL.md"),
                format!(
                    "+++\nname = \"{name}\"\ndescription = \"Conventions for {name} work in this project\"\nactivation = \"{activation}\"\n{extra}\n+++\n\n{marker}\n\n{filler}"
                ),
            )
            .unwrap();
        }

        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut registry = test_registry();
        registry.load_skills_from_dirs_for_test(&[skills_dir]);
        let unfiltered = registry.build_augmentation();
        let feature = PersonaFeature::with_workspace_root(
            SharedAugmentRegistry::new(registry),
            workspace.clone(),
        );

        // Neither skill has workspace evidence and the prompt names neither:
        // both bodies are withheld, both stay discoverable.
        let quiet = feature
            .provide_context(&signals_with_prompt("what changed in the release?"))
            .expect("persona layer must still inject");
        assert!(!quiet.content.contains("STYLE BODY MARKER"));
        assert!(!quiet.content.contains("TS BODY MARKER"));
        assert!(quiet.content.contains("# Available skills (not loaded)"));
        assert!(quiet.content.contains("- style —"));
        assert!(quiet.content.contains("- typescript —"));
        assert!(
            quiet.content.len() < unfiltered.len(),
            "disclosed {} !< unfiltered {}",
            quiet.content.len(),
            unfiltered.len()
        );

        // The operator names a trigger: that body is admitted on this turn,
        // and the unrelated skill stays withheld.
        let asked = feature
            .provide_context(&signals_with_prompt("draw me a diagram of the pipeline"))
            .expect("persona layer must still inject");
        assert!(
            asked.content.contains("STYLE BODY MARKER"),
            "trigger must admit the body on the naming turn"
        );
        assert!(!asked.content.contains("TS BODY MARKER"));

        // Priority and TTL are unchanged by disclosure.
        assert_eq!(asked.priority, 85);
        assert_eq!(asked.ttl_turns, u32::MAX);
        assert_eq!(asked.source, "persona");
    }

    #[test]
    fn feature_exposes_three_tools() {
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let tools = feature.tools();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "switch_persona"));
        assert!(tools.iter().any(|t| t.name == "switch_tone"));
        assert!(tools.iter().any(|t| t.name == "list_personas"));
    }

    #[tokio::test]
    async fn list_personas_empty() {
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("list_personas", "c1", json!({}), cancel)
            .await
            .unwrap();
        let text: String = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("Personas"));
        assert!(text.contains("Tones"));
    }

    #[tokio::test]
    async fn switch_persona_not_found() {
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute(
                "switch_persona",
                "c1",
                json!({"name": "nonexistent"}),
                cancel,
            )
            .await
            .unwrap();
        let text: String = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("not found"));
    }

    #[tokio::test]
    async fn switch_tone_not_found() {
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("switch_tone", "c1", json!({"name": "nonexistent"}), cancel)
            .await
            .unwrap();
        let text: String = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("not found"));
    }

    #[test]
    fn provide_context_empty_when_no_persona() {
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let signals = omegon_traits::ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 10000,
        };
        let ctx = feature.provide_context(&signals).unwrap();
        assert_eq!(ctx.source, "persona");
        assert!(
            ctx.content.is_empty(),
            "clear any previous combined injection"
        );
    }

    #[test]
    fn provide_context_includes_persona_directive_after_activation() {
        let mut registry = test_registry();
        registry.activate_persona(crate::plugins::registry::LoadedPersona {
            id: "test.eng".into(),
            name: "Test Engineer".into(),
            directive: "You are a test engineering persona with deep Rust expertise.".into(),
            mind_facts: vec![],
            activated_skills: vec![],
            disabled_tools: vec![],
            badge: Some("🧪".into()),
        });
        let feature = PersonaFeature::new(SharedAugmentRegistry::new(registry));
        let signals = omegon_traits::ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 50000,
        };
        let ctx = feature.provide_context(&signals).unwrap();
        assert!(
            ctx.content.contains("test engineering persona"),
            "should include persona directive: {}",
            ctx.content
        );
        assert!(
            !ctx.content.contains(crate::prompt::CORE_SOURCE),
            "augmentation must not contain host policy: {}",
            ctx.content
        );
        assert_eq!(ctx.priority, 85);
    }

    #[test]
    fn switch_and_clear_replace_legacy_combined_layer_without_expiring_the_host() {
        use crate::plugins::registry::{LoadedPersona, LoadedTone, ToneIntensity};
        let dir = tempfile::tempdir().unwrap();
        let registry = SharedAugmentRegistry::new(test_registry());
        let feature =
            PersonaFeature::with_workspace_root(registry.clone(), dir.path().to_path_buf());
        let base =
            crate::prompt::assemble_host_prompt(dir.path(), &dir.path().join("global.md")).unwrap();
        let mut context = crate::context::ContextManager::new(base.prompt, vec![]);
        context.inject_external(vec![omegon_traits::ContextInjection {
            source: "persona".into(),
            content: "STALE COMBINED LEX".into(),
            priority: 85,
            ttl_turns: u32::MAX,
        }]);
        let conversation = crate::conversation::ConversationState::new();
        let signals = signals_with_prompt("continue");
        registry.lock().activate_tone(LoadedTone {
            id: "tone".into(),
            name: "Tone".into(),
            directive: "TONE MARKER".into(),
            exemplars: vec![],
            intensity: ToneIntensity::default(),
        });
        for name in ["FIRST PERSONA", "SECOND PERSONA"] {
            registry.lock().activate_persona(LoadedPersona {
                id: name.into(),
                name: name.into(),
                directive: name.into(),
                mind_facts: vec![],
                activated_skills: vec![],
                disabled_tools: vec![],
                badge: None,
            });
            context.inject_external(vec![feature.provide_context(&signals).unwrap()]);
            let prompt = context.build_system_prompt("continue", &conversation);
            assert!(!prompt.contains("STALE COMBINED LEX"));
            assert!(prompt.contains(name));
            if name == "SECOND PERSONA" {
                assert!(!prompt.contains("FIRST PERSONA"));
            }
            assert!(prompt.find("TONE MARKER").unwrap() < prompt.find(name).unwrap());
            assert_eq!(prompt.matches(crate::prompt::CORE_SOURCE).count(), 1);
        }
        registry.lock().deactivate_persona();
        registry.lock().deactivate_tone();
        context.inject_external(vec![feature.provide_context(&signals).unwrap()]);
        let prompt = context.build_system_prompt("continue", &conversation);
        for stale in [
            "STALE COMBINED LEX",
            "FIRST PERSONA",
            "SECOND PERSONA",
            "TONE MARKER",
        ] {
            assert!(!prompt.contains(stale));
        }
        assert_eq!(prompt.matches(crate::prompt::CORE_SOURCE).count(), 1);
    }

    #[test]
    fn on_event_session_start_with_persona_notifies() {
        let mut registry = test_registry();
        registry.activate_persona(crate::plugins::registry::LoadedPersona {
            id: "test.eng".into(),
            name: "Test Engineer".into(),
            directive: "You are a test engineer.".into(),
            mind_facts: vec![],
            activated_skills: vec![],
            disabled_tools: vec![],
            badge: Some("🧪".into()),
        });
        let mut feature = PersonaFeature::new(SharedAugmentRegistry::new(registry));
        let requests = feature.on_event(&BusEvent::SessionStart {
            cwd: std::path::PathBuf::from("/tmp"),
            session_id: "test".into(),
        });
        assert!(!requests.is_empty(), "should notify about active persona");
    }

    #[test]
    fn on_event_session_start_no_persona() {
        let mut feature = PersonaFeature::new(SharedAugmentRegistry::new(test_registry()));
        let requests = feature.on_event(&BusEvent::SessionStart {
            cwd: std::path::PathBuf::from("/tmp"),
            session_id: "test".into(),
        });
        // No persona active — no notifications
        assert!(requests.is_empty());
    }
}
