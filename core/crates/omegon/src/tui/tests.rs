//! TUI integration tests — slash commands, selectors, event handling.
//!
//! These test the App struct as a state machine: feed inputs, check outputs.
//! No terminal rendering — uses App::new() with test settings.

use super::*;
use crate::settings::{ContextClass, Settings, ThinkingLevel};
use tokio::sync::mpsc;

fn test_settings() -> crate::settings::SharedSettings {
    std::sync::Arc::new(std::sync::Mutex::new(Settings::new("anthropic:claude-sonnet-4-6")))
}

fn test_app() -> App {
    App::new(test_settings())
}

fn test_tx() -> mpsc::Sender<TuiCommand> {
    let (tx, _rx) = mpsc::channel(16);
    tx
}

// ═══════════════════════════════════════════════════════════════════
// Slash command routing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_help_returns_display() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/help", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Commands:"), "should list commands: {text}");
    }
}

#[test]
fn slash_stats_returns_session_info() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/stats", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Duration"), "should show duration: {text}");
        assert!(text.contains("Turns"), "should show turns: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_status_returns_bootstrap_panel() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/status", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Omegon"), "should contain Omegon: {text}");
        assert!(text.contains("Routing"), "should contain Routing: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_unknown_is_not_a_command() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/nonexistent_command_xyz", &tx);
    // Unknown commands are either NotACommand or Display with error
    assert!(!matches!(result, SlashResult::Handled));
}

#[test]
fn slash_exit_returns_quit() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/exit", &tx);
    assert!(matches!(result, SlashResult::Quit));
}

#[test]
fn slash_compact_returns_handled() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/compact", &tx);
    // Compact either returns Handled or Display with confirmation
    assert!(!matches!(result, SlashResult::NotACommand));
}

#[test]
fn slash_persona_no_args_lists_personas() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona", &tx);
    if let SlashResult::Display(text) = result {
        // Either shows available personas or "no personas installed"
        assert!(
            text.contains("persona") || text.contains("Persona") || text.contains("installed"),
            "should mention personas: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_persona_off_deactivates() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona off", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("deactivated") || text.contains("No persona"),
            "should confirm deactivation: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_tone_no_args_lists_tones() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("tone") || text.contains("Tone") || text.contains("installed"),
            "should mention tones: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_tone_off_deactivates() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone off", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("deactivated") || text.contains("No tone"),
            "should confirm deactivation: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_auth_no_args_shows_status() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/auth", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("auth") || text.contains("Provider") || text.contains("status"),
            "should show auth info: {text}"
        );
    } else {
        // May return Handled if it opens an overlay
        assert!(matches!(result, SlashResult::Handled | SlashResult::Display(_)));
    }
}

#[test]
fn slash_memory_returns_stats() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/memory", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("memory") || text.contains("facts") || text.contains("Facts"),
            "should show memory info: {text}"
        );
    } else {
        panic!("expected Display result, got {:?}", std::mem::discriminant(&result));
    }
}

#[test]
fn slash_think_with_level_changes_settings() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/think high", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.to_lowercase().contains("high"), "should confirm high: {text}");
    }
    let s = app.settings.lock().unwrap();
    assert_eq!(s.thinking, ThinkingLevel::High);
}

#[test]
fn slash_think_no_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/think", &tx);
    assert!(matches!(result, SlashResult::Handled), "should open selector");
    assert!(app.selector.is_some(), "selector should be open");
    assert!(matches!(app.selector_kind, Some(SelectorKind::ThinkingLevel)));
}

#[test]
fn not_a_slash_command_passes_through() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("hello world", &tx);
    assert!(matches!(result, SlashResult::NotACommand));
}

// ═══════════════════════════════════════════════════════════════════
// Selector overlays
// ═══════════════════════════════════════════════════════════════════

#[test]
fn model_selector_opens() {
    let mut app = test_app();
    app.open_model_selector();
    assert!(app.selector.is_some());
    assert!(matches!(app.selector_kind, Some(SelectorKind::Model)));
}

#[test]
fn thinking_selector_opens() {
    let mut app = test_app();
    app.open_thinking_selector();
    assert!(app.selector.is_some());
    assert!(matches!(app.selector_kind, Some(SelectorKind::ThinkingLevel)));
}

#[test]
fn context_selector_opens() {
    let mut app = test_app();
    app.open_context_selector();
    assert!(app.selector.is_some());
    assert!(matches!(app.selector_kind, Some(SelectorKind::ContextClass)));
}

#[test]
fn context_selector_confirm_changes_settings() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_context_selector();

    // Navigate down to select a non-default option and confirm
    if let Some(ref mut sel) = app.selector {
        sel.move_down(); // Move to second option (Maniple)
    }
    let _msg = app.confirm_selector(&tx);

    // Check that settings were updated
    let s = app.settings.lock().unwrap();
    // Should be Maniple (second option) or whatever the selector landed on
    assert_ne!(s.context_class, ContextClass::Squad, "should have changed from default Squad");
}

// ═══════════════════════════════════════════════════════════════════
// Event handling
// ═══════════════════════════════════════════════════════════════════

#[test]
fn harness_status_changed_updates_footer() {
    let mut app = test_app();

    let status = crate::status::HarnessStatus {
        context_class: "Clan".into(),
        thinking_level: "High".into(),
        active_persona: Some(crate::status::PersonaSummary {
            id: "test".into(),
            name: "Test Persona".into(),
            badge: "🧪".into(),
            mind_facts_count: 10,
            activated_skills: vec!["rust".into()],
            disabled_tools: vec![],
        }),
        ..Default::default()
    };

    let status_json = serde_json::to_value(&status).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged {
        status_json,
    });

    // Footer should now reflect the new status
    assert!(app.footer_data.harness.active_persona.is_some());
    assert_eq!(app.footer_data.harness.active_persona.as_ref().unwrap().name, "Test Persona");
    assert_eq!(app.footer_data.harness.context_class, "Clan");
}

#[test]
fn harness_status_changed_detects_persona_transition() {
    let mut app = test_app();

    // Set initial state with no persona
    let initial = crate::status::HarnessStatus::default();
    let initial_json = serde_json::to_value(&initial).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged {
        status_json: initial_json,
    });

    // Now switch to a persona
    let with_persona = crate::status::HarnessStatus {
        active_persona: Some(crate::status::PersonaSummary {
            id: "eng".into(),
            name: "Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 5,
            activated_skills: vec![],
            disabled_tools: vec![],
        }),
        ..Default::default()
    };
    let persona_json = serde_json::to_value(&with_persona).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged {
        status_json: persona_json,
    });

    // The previous_harness_status should have been set for diffing
    assert!(app.previous_harness_status.is_some());
    // Current footer should have the persona
    assert!(app.footer_data.harness.active_persona.is_some());
}

// ═══════════════════════════════════════════════════════════════════
// Command table completeness
// ═══════════════════════════════════════════════════════════════════

#[test]
fn all_commands_in_table_are_handled() {
    let mut app = test_app();
    let tx = test_tx();

    for (name, _desc, _subs) in App::COMMANDS {
        let cmd = format!("/{name}");
        let result = app.handle_slash_command(&cmd, &tx);
        // Every command in the table should be recognized (not NotACommand)
        assert!(
            !matches!(result, SlashResult::NotACommand),
            "command /{name} returned NotACommand — it's in COMMANDS but not handled"
        );
    }
}
