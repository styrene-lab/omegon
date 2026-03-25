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
        assert!(text.contains("Context:"), "should contain Context: {text}");
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

#[test]
fn handled_commands_are_in_commands_table() {
    // Negative guard: every match arm in handle_slash_command should have
    // a corresponding entry in COMMANDS (otherwise it's undocumented).
    // We test this by checking that no undocumented command returns Handled/Display.
    let mut app = test_app();
    let tx = test_tx();

    let known_names: std::collections::HashSet<&str> = App::COMMANDS.iter()
        .map(|(name, _, _)| *name)
        .collect();

    // Test a set of plausible undocumented command names
    let undocumented = ["config", "debug", "reload", "undo", "redo",
        "run", "build", "deploy", "test", "profile", "env", "reset"];

    for name in undocumented {
        if known_names.contains(name) { continue; } // skip if it's actually documented
        let cmd = format!("/{name}");
        let result = app.handle_slash_command(&cmd, &tx);
        // Unknown commands should either be NotACommand (not /-prefixed)
        // or Display an error (/-prefixed but unrecognized). They must
        // NOT return Handled (which would silently swallow input).
        assert!(
            matches!(result, SlashResult::NotACommand | SlashResult::Display(_)),
            "/{name} returned Handled but is NOT in COMMANDS table — add it to COMMANDS or remove the handler"
        );
    }
}

#[test]
fn slash_command_aliases_dispatch_correctly() {
    let mut app = test_app();
    let tx = test_tx();

    // /dashboard should resolve (alias for /dash open)
    let result = app.handle_slash_command("/dashboard", &tx);
    assert!(!matches!(result, SlashResult::NotACommand),
        "/dashboard should be handled, not fall through");

    // /version should display build info
    let result = app.handle_slash_command("/version", &tx);
    assert!(matches!(result, SlashResult::Display(_)),
        "/version should display version info");

    // /q should quit
    let result = app.handle_slash_command("/q", &tx);
    assert!(matches!(result, SlashResult::Quit), "/q should quit");
}

#[test]
fn unknown_slash_commands_show_error() {
    let mut app = test_app();
    let tx = test_tx();

    // Unknown commands must NOT return NotACommand (which sends to agent)
    let result = app.handle_slash_command("/foobar", &tx);
    assert!(matches!(result, SlashResult::Display(_)),
        "/foobar should show error, not go to agent");

    // /secret now prefix-matches to /secrets (valid command)
    let result = app.handle_slash_command("/zzz_nonexistent", &tx);
    assert!(matches!(result, SlashResult::Display(_)),
        "/zzz_nonexistent should show error, not go to agent");
}

#[test]
fn slash_prefix_matching_unique() {
    let mut app = test_app();
    let tx = test_tx();

    // /hel should uniquely prefix-match /help
    let result = app.handle_slash_command("/hel", &tx);
    assert!(matches!(result, SlashResult::Display(_)),
        "/hel should prefix-match /help and show help text");
}

#[test]
fn slash_prefix_matching_ambiguous() {
    let mut app = test_app();
    let tx = test_tx();

    // /s matches multiple commands (stats, status, sessions, splash)
    let result = app.handle_slash_command("/s", &tx);
    match result {
        SlashResult::Display(msg) => {
            assert!(msg.contains("Did you mean") || msg.contains("Ambiguous"),
                "/s should show ambiguous message, got: {msg}");
        }
        _ => panic!("/s should be ambiguous, got: {result:?}"),
    }
}

#[test]
fn tutorial_parse_lesson_with_frontmatter() {
    let raw = "---\ntitle: \"The Cockpit\"\n---\n\nWelcome to Omegon!\n\nLook at the bottom.";
    let (title, content) = super::parse_lesson(raw, "01-cockpit.md");
    assert_eq!(title, "The Cockpit");
    assert!(content.contains("Welcome to Omegon!"));
    assert!(!content.contains("title:"));
}

#[test]
fn tutorial_parse_lesson_without_frontmatter() {
    let raw = "Just plain content.\n\nNo frontmatter here.";
    let (title, content) = super::parse_lesson(raw, "02-tools.md");
    assert_eq!(title, "02-tools");
    assert!(content.contains("Just plain content."));
}

#[test]
fn tutorial_state_load_and_advance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path().join(".omegon").join("tutorial");
    std::fs::create_dir_all(&tutorial_dir).unwrap();

    std::fs::write(tutorial_dir.join("01-first.md"), "---\ntitle: \"First\"\n---\nLesson one.").unwrap();
    std::fs::write(tutorial_dir.join("02-second.md"), "---\ntitle: \"Second\"\n---\nLesson two.").unwrap();
    std::fs::write(tutorial_dir.join("03-third.md"), "---\ntitle: \"Third\"\n---\nLesson three.").unwrap();

    let mut tut = super::TutorialState::load(&tutorial_dir).unwrap();
    assert_eq!(tut.total(), 3);
    assert_eq!(tut.current, 0);
    assert_eq!(tut.current_lesson().title, "First");
    assert!(!tut.is_last());

    assert!(tut.advance());
    assert_eq!(tut.current, 1);
    assert_eq!(tut.current_lesson().title, "Second");

    assert!(tut.advance());
    assert_eq!(tut.current, 2);
    assert!(tut.is_last());

    assert!(!tut.advance()); // can't go past last

    assert!(tut.go_back());
    assert_eq!(tut.current, 1);

    assert!(!super::TutorialState::load(tmp.path()).is_some()); // no tutorial dir
}

#[test]
fn tutorial_progress_persistence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(tutorial_dir.join("01-a.md"), "---\ntitle: A\n---\nA").unwrap();
    std::fs::write(tutorial_dir.join("02-b.md"), "---\ntitle: B\n---\nB").unwrap();

    {
        let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
        tut.advance();
        // Progress saved automatically
    }

    {
        let tut = super::TutorialState::load(tutorial_dir).unwrap();
        assert_eq!(tut.current, 1, "should resume at lesson 2");
        assert_eq!(tut.current_lesson().title, "B");
    }
}

#[test]
fn tutorial_reset_clears_progress() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(tutorial_dir.join("01-a.md"), "---\ntitle: A\n---\nA").unwrap();
    std::fs::write(tutorial_dir.join("02-b.md"), "---\ntitle: B\n---\nB").unwrap();

    let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
    tut.advance();
    tut.reset();
    assert_eq!(tut.current, 0);
    assert!(!tutorial_dir.join("progress.json").exists());
}

#[test]
fn tutorial_status_line() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(tutorial_dir.join("01-intro.md"), "---\ntitle: Introduction\n---\nHello").unwrap();
    std::fs::write(tutorial_dir.join("02-end.md"), "---\ntitle: Finale\n---\nBye").unwrap();

    let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
    assert!(tut.status_line().contains("1/2"));
    assert!(tut.status_line().contains("Introduction"));

    tut.advance();
    assert!(tut.status_line().contains("2/2"));
    assert!(tut.status_line().contains("(final)"));
}

#[cfg(target_os = "macos")]
#[test]
fn clipboard_format_matching() {
    use super::match_clipboard_image_format;

    // Real osascript output from a screenshot
    let info = "«class PNGf», 29460, «class AVIF», 14396, «class 8BPS», 141278, GIF picture, 9009, «class jp2 », 39826, JPEG picture, 27092, TIFF picture, 792990, «class BMP », 792202, «class TPIC», 58310";
    let result = match_clipboard_image_format(info);
    assert!(result.is_some(), "should match PNGf in real clipboard output");
    let (ext, pb) = result.unwrap();
    assert_eq!(ext, "png");
    assert_eq!(pb, "«class PNGf»");

    // JPEG-only clipboard
    let info = "JPEG picture, 12345";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "jpg");

    // TIFF-only clipboard
    let info = "TIFF picture, 99999";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "tiff");

    // GIF
    let info = "GIF picture, 5000";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "gif");

    // BMP
    let info = "«class BMP », 200000";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "bmp");

    // No image — text only
    let info = "«class utf8», 100, string, 100";
    assert!(match_clipboard_image_format(info).is_none());

    // Empty
    assert!(match_clipboard_image_format("").is_none());

    // The OLD broken matching — UTI strings that never appeared in osascript output
    let info_with_uti = "public.png, 29460";
    // This should NOT match PNGf (it contains "public.png" not "PNGf")
    // But wait — "png" is not in our markers. This correctly returns None.
    // The old code would have matched "public.png" → that was the bug.
    assert!(match_clipboard_image_format(info_with_uti).is_none(),
        "UTI strings should not match — osascript never outputs them");
}

// ═══════════════════════════════════════════════════════════════════
// /note and /notes commands
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_note_with_text_persists_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // Write a note
    let result = app.handle_slash_command("/note look into this later", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Noted"), "should confirm note: {text}");
        assert!(text.contains("1 entries"), "should count 1 entry: {text}");
    } else {
        panic!("expected Display result");
    }

    // Verify file exists and contains the note
    let notes_path = tmp.path().join(".omegon").join("notes.md");
    let content = std::fs::read_to_string(&notes_path).expect("notes file should exist");
    assert!(content.contains("look into this later"), "note text should be persisted: {content}");
    assert!(content.starts_with("- ["), "should have timestamp prefix: {content}");

    // Write a second note and verify count
    let result2 = app.handle_slash_command("/note second thing", &tx);
    if let SlashResult::Display(text) = result2 {
        assert!(text.contains("2 entries"), "should count 2 entries: {text}");
    }
}

#[test]
fn slash_note_without_args_shows_notes() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/note", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("note"), "should mention notes: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_notes_clear_returns_display() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/notes clear", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("cleared"), "should confirm clear: {text}");
    } else {
        panic!("expected Display result");
    }
}

// ═══════════════════════════════════════════════════════════════════
// /checkin command
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_checkin_with_notes_shows_note_count() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // No notes → should NOT mention notes in checkin
    let result = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = &result {
        assert!(!text.contains("pending note"), "no notes yet: {text}");
    }

    // Add a note
    app.handle_slash_command("/note investigate flaky test", &tx);

    // Now checkin should show the note count
    let result2 = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = result2 {
        assert!(text.contains("1 pending note"), "should show note count: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_checkin_with_opsx_changes_shows_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // Create a fake OpenSpec change directory
    let change_dir = tmp.path().join("openspec").join("changes").join("my-feature");
    std::fs::create_dir_all(&change_dir).unwrap();

    let result = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("OpenSpec"), "should show OpenSpec changes: {text}");
        assert!(text.contains("my-feature"), "should name the change: {text}");
    } else {
        panic!("expected Display result");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Login selector
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_login_selector_opens_with_provider_catalog() {
    let mut app = test_app();
    app.open_login_selector();
    assert!(app.selector.is_some(), "selector should be open");
    let selector = app.selector.as_ref().unwrap();
    assert!(selector.options.len() >= 9, "should have at least 9 providers, got {}", selector.options.len());
    // Verify structure: each option has a value and label
    for opt in &selector.options {
        assert!(!opt.value.is_empty(), "option value should not be empty");
        assert!(!opt.label.is_empty(), "option label should not be empty");
    }
    // Unconfigured providers should NOT have checkmark
    let has_unconfigured = selector.options.iter().any(|o| !o.active);
    assert!(has_unconfigured, "at least some providers should be unconfigured in test env");
}

// ═══════════════════════════════════════════════════════════════════
// Recovery hints
// ═══════════════════════════════════════════════════════════════════

#[test]
fn recovery_hint_rate_limit() {
    let hint = App::recovery_hint(None, "Error: 429 Too Many Requests");
    assert!(hint.contains("Rate limited"), "should suggest rate limit recovery: {hint}");
}

#[test]
fn recovery_hint_unauthorized() {
    let hint = App::recovery_hint(None, "HTTP 401 Unauthorized");
    assert!(hint.contains("/login"), "should suggest login: {hint}");
}

#[test]
fn recovery_hint_no_false_positive_on_status_codes() {
    // A path containing "401" should NOT trigger the auth hint
    let hint = App::recovery_hint(None, "Error reading /var/lib/app/401/config.json");
    assert!(hint.is_empty(), "path with 401 should not trigger auth hint: {hint}");
}

#[test]
fn recovery_hint_ollama_connection() {
    let hint = App::recovery_hint(None, "Connection refused to ollama at localhost:11434");
    assert!(hint.contains("ollama serve"), "should suggest starting ollama: {hint}");
}

#[test]
fn recovery_hint_context_window() {
    let hint = App::recovery_hint(None, "context_length_exceeded: too many tokens");
    assert!(hint.contains("/compact"), "should suggest compact: {hint}");
}

#[test]
fn recovery_hint_no_match() {
    let hint = App::recovery_hint(None, "some random error");
    assert!(hint.is_empty(), "should return empty for unknown errors");
}
