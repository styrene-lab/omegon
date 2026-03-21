//! Persona feature — exposes persona and tone management as agent-callable tools.
//!
//! Tools:
//! - `switch_persona` — activate a persona by name, or deactivate
//! - `switch_tone` — activate a tone by name, or deactivate
//! - `list_personas` — enumerate available personas and tones

use async_trait::async_trait;
use serde_json::json;
use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

use omegon_traits::{
    BusEvent, BusRequest, ContentBlock, Feature, NotifyLevel,
    ToolDefinition, ToolResult,
};

use crate::plugins::persona_loader;
use crate::plugins::registry::PluginRegistry;

/// Feature that exposes persona/tone management as agent tools.
pub struct PersonaFeature {
    registry: Mutex<PluginRegistry>,
    /// Flag indicating harness status should be refreshed on next turn boundary
    refresh_status_pending: AtomicBool,
}

impl PersonaFeature {
    pub fn new(registry: PluginRegistry) -> Self {
        Self { 
            registry: Mutex::new(registry),
            refresh_status_pending: AtomicBool::new(false),
        }
    }

    /// Get a reference to the inner registry (for HarnessStatus, etc.)
    pub fn registry(&self) -> std::sync::MutexGuard<'_, PluginRegistry> {
        self.registry.lock().unwrap()
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
            },
            ToolDefinition {
                name: crate::tool_registry::persona::LIST_PERSONAS.into(),
                label: "list_personas".into(),
                description: "List available personas and tones installed on this system. Shows active status.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                }),
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
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if name == "off" {
                    // Deactivate current persona
                    let result = self.registry.lock().unwrap().deactivate_persona();
                    self.refresh_status_pending.store(true, Ordering::Relaxed);
                    
                    if result.removed_id.is_some() {
                        return Ok(text_result("Persona deactivated."));
                    } else {
                        return Ok(text_result("No persona was active."));
                    }
                }

                let (personas, _) = persona_loader::scan_available();
                let target = name.to_lowercase();

                match personas.iter().find(|p| p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target)) {
                    Some(available) => {
                        match persona_loader::load_persona(&available.path) {
                            Ok(loaded_persona) => {
                                let badge = loaded_persona.badge.clone().unwrap_or_else(|| "⚙".into());
                                let fact_count = loaded_persona.mind_facts.len();
                                let pname = loaded_persona.name.clone();
                                let skills = loaded_persona.activated_skills.join(", ");

                                // Actually activate the persona
                                let activation_result = self.registry.lock().unwrap().activate_persona(loaded_persona);
                                self.refresh_status_pending.store(true, Ordering::Relaxed);

                                let mut message = format!(
                                    "{badge} Persona activated: {pname}\n  Mind facts: {fact_count}\n  Skills: {skills}\n\n\
                                    Note: The persona directive and mind facts are now active in the system prompt."
                                );
                                
                                if let Some(prev) = activation_result.previous_id {
                                    message.push_str(&format!("\n\nPrevious persona ({}) was deactivated.", prev));
                                }

                                Ok(text_result(&message))
                            }
                            Err(e) => Ok(error_result(&format!("Failed to load persona '{name}': {e}"))),
                        }
                    }
                    None => {
                        let available_names: Vec<_> = personas.iter().map(|p| p.name.as_str()).collect();
                        Ok(error_result(&format!(
                            "Persona '{name}' not found. Available: {}",
                            if available_names.is_empty() { "none installed".into() } else { available_names.join(", ") }
                        )))
                    }
                }
            }

            crate::tool_registry::persona::SWITCH_TONE => {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if name == "off" {
                    // Deactivate current tone
                    let removed = self.registry.lock().unwrap().deactivate_tone();
                    self.refresh_status_pending.store(true, Ordering::Relaxed);
                    
                    if removed.is_some() {
                        return Ok(text_result("Tone deactivated."));
                    } else {
                        return Ok(text_result("No tone was active."));
                    }
                }

                let (_, tones) = persona_loader::scan_available();
                let target = name.to_lowercase();

                match tones.iter().find(|t| t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target)) {
                    Some(available) => {
                        match persona_loader::load_tone(&available.path) {
                            Ok(loaded_tone) => {
                                let tname = loaded_tone.name.clone();
                                let exemplar_count = loaded_tone.exemplars.len();

                                // Actually activate the tone
                                let previous = self.registry.lock().unwrap().activate_tone(loaded_tone);
                                self.refresh_status_pending.store(true, Ordering::Relaxed);

                                let mut message = format!(
                                    "♪ Tone activated: {tname}\n  Exemplars: {exemplar_count}\n\n\
                                    Note: The tone directive is now active in the system prompt."
                                );
                                
                                if let Some(prev) = previous {
                                    message.push_str(&format!("\n\nPrevious tone ({}) was deactivated.", prev));
                                }

                                Ok(text_result(&message))
                            }
                            Err(e) => Ok(error_result(&format!("Failed to load tone '{name}': {e}"))),
                        }
                    }
                    None => {
                        let available_names: Vec<_> = tones.iter().map(|t| t.name.as_str()).collect();
                        Ok(error_result(&format!(
                            "Tone '{name}' not found. Available: {}",
                            if available_names.is_empty() { "none installed".into() } else { available_names.join(", ") }
                        )))
                    }
                }
            }

            crate::tool_registry::persona::LIST_PERSONAS => {
                let (personas, tones) = persona_loader::scan_available();
                let registry = self.registry.lock().unwrap();
                let active_persona = registry.active_persona().map(|p| &p.id);
                let active_tone = registry.active_tone().map(|t| &t.id);

                let mut out = String::new();

                out.push_str("## Personas\n\n");
                if personas.is_empty() {
                    out.push_str("No personas installed.\n");
                } else {
                    for p in &personas {
                        let marker = if active_persona == Some(&p.id) { " ● (active)" } else { "" };
                        out.push_str(&format!("- **{}**{}: {}\n", p.name, marker, p.description));
                    }
                }

                out.push_str("\n## Tones\n\n");
                if tones.is_empty() {
                    out.push_str("No tones installed.\n");
                } else {
                    for t in &tones {
                        let marker = if active_tone == Some(&t.id) { " ● (active)" } else { "" };
                        out.push_str(&format!("- **{}**{}: {}\n", t.name, marker, t.description));
                    }
                }

                out.push_str("\nInstall plugins with: `omegon plugin install <git-url>`");

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
                let registry = self.registry.lock().unwrap();
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
            BusEvent::TurnEnd { .. } => {
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

    fn provide_context(&self, _signals: &omegon_traits::ContextSignals<'_>) -> Option<omegon_traits::ContextInjection> {
        // Inject persona directive + tone directive as context
        let prompt = self.registry.lock().unwrap().build_system_prompt();
        if prompt.is_empty() {
            return None;
        }

        Some(omegon_traits::ContextInjection {
            source: "persona".into(),
            content: prompt,
            priority: 85, // Just below Lex Imperialis (embedded at compile time)
            ttl_turns: u32::MAX, // Never expires — always active while persona is on
        })
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: text.to_string() }],
        details: json!({}),
    }
}

fn error_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: format!("Error: {text}") }],
        details: json!({ "error": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> PluginRegistry {
        PluginRegistry::new("Test Lex Imperialis.".into())
    }

    #[test]
    fn feature_exposes_three_tools() {
        let feature = PersonaFeature::new(test_registry());
        let tools = feature.tools();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "switch_persona"));
        assert!(tools.iter().any(|t| t.name == "switch_tone"));
        assert!(tools.iter().any(|t| t.name == "list_personas"));
    }

    #[tokio::test]
    async fn list_personas_empty() {
        let feature = PersonaFeature::new(test_registry());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("list_personas", "c1", json!({}), cancel).await.unwrap();
        let text: String = result.content.iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("Personas"));
        assert!(text.contains("Tones"));
    }

    #[tokio::test]
    async fn switch_persona_not_found() {
        let feature = PersonaFeature::new(test_registry());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute(
            "switch_persona", "c1",
            json!({"name": "nonexistent"}),
            cancel,
        ).await.unwrap();
        let text: String = result.content.iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("not found"));
    }

    #[tokio::test]
    async fn switch_tone_not_found() {
        let feature = PersonaFeature::new(test_registry());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute(
            "switch_tone", "c1",
            json!({"name": "nonexistent"}),
            cancel,
        ).await.unwrap();
        let text: String = result.content.iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("not found"));
    }

    #[test]
    fn provide_context_empty_when_no_persona() {
        let feature = PersonaFeature::new(test_registry());
        let signals = omegon_traits::ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 10000,
        };
        // Lex Imperialis is always present, so context should be non-empty
        let ctx = feature.provide_context(&signals);
        assert!(ctx.is_some(), "should inject Lex Imperialis even with no persona");
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
        let feature = PersonaFeature::new(registry);
        let signals = omegon_traits::ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 50000,
        };
        let ctx = feature.provide_context(&signals).unwrap();
        assert!(ctx.content.contains("test engineering persona"), "should include persona directive: {}", ctx.content);
        assert!(ctx.content.contains("Lex Imperialis"), "should still include Lex: {}", ctx.content);
        assert_eq!(ctx.priority, 85);
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
        let mut feature = PersonaFeature::new(registry);
        let requests = feature.on_event(&BusEvent::SessionStart {
            cwd: std::path::PathBuf::from("/tmp"),
            session_id: "test".into(),
        });
        assert!(!requests.is_empty(), "should notify about active persona");
    }

    #[test]
    fn on_event_session_start_no_persona() {
        let mut feature = PersonaFeature::new(test_registry());
        let requests = feature.on_event(&BusEvent::SessionStart {
            cwd: std::path::PathBuf::from("/tmp"),
            session_id: "test".into(),
        });
        // No persona active — no notifications
        assert!(requests.is_empty());
    }
}
