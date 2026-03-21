//! Central tool name registry — the single source of truth for every tool name
//! in the Omegon agent runtime.
//!
//! # Why this exists
//!
//! The Anthropic API rejects requests with duplicate tool names (400 Bad Request).
//! Tools are registered by independent features, providers, and plugins across
//! many files. Without a central registry, duplicates are invisible until runtime.
//!
//! # Rules
//!
//! 1. Every statically-known tool name MUST be declared here as a constant.
//! 2. Features MUST use these constants in `tools()` and `execute()` — not string literals.
//! 3. Each constant is grouped under the module of its owning feature/provider.
//! 4. The `no_duplicates` test enforces uniqueness at compile-test time.
//! 5. Dynamic tools (MCP servers, TOML plugins) are validated at registration
//!    time by `EventBus::finalize()` which rejects collisions with static names.

/// Core primitive tools — owned by `tools::CoreTools`
pub mod core {
    pub const BASH: &str = "bash";
    pub const READ: &str = "read";
    pub const WRITE: &str = "write";
    pub const EDIT: &str = "edit";
    pub const CHANGE: &str = "change";
    pub const SPECULATE_START: &str = "speculate_start";
    pub const SPECULATE_CHECK: &str = "speculate_check";
    pub const SPECULATE_COMMIT: &str = "speculate_commit";
    pub const SPECULATE_ROLLBACK: &str = "speculate_rollback";
    pub const COMMIT: &str = "commit";
    pub const WHOAMI: &str = "whoami";
    pub const CHRONOS: &str = "chronos";
}

/// View tool — owned by `tools::view::ViewProvider`
pub mod view {
    pub const VIEW: &str = "view";
}

/// Web search — owned by `tools::web_search::WebSearchProvider`
pub mod web_search {
    pub const WEB_SEARCH: &str = "web_search";
}

/// Render tools — owned by `tools::render::RenderProvider`
pub mod render {
    pub const RENDER_DIAGRAM: &str = "render_diagram";
    pub const GENERATE_IMAGE_LOCAL: &str = "generate_image_local";
}

/// Local inference — owned by `tools::local_inference::LocalInferenceProvider`
pub mod local_inference {
    pub const ASK_LOCAL_MODEL: &str = "ask_local_model";
    pub const LIST_LOCAL_MODELS: &str = "list_local_models";
    pub const MANAGE_OLLAMA: &str = "manage_ollama";
}

/// Memory tools — owned by `features::memory::MemoryFeature`
pub mod memory {
    pub const MEMORY_STORE: &str = "memory_store";
    pub const MEMORY_RECALL: &str = "memory_recall";
    pub const MEMORY_QUERY: &str = "memory_query";
    pub const MEMORY_ARCHIVE: &str = "memory_archive";
    pub const MEMORY_SUPERSEDE: &str = "memory_supersede";
    pub const MEMORY_CONNECT: &str = "memory_connect";
    pub const MEMORY_FOCUS: &str = "memory_focus";
    pub const MEMORY_RELEASE: &str = "memory_release";
    pub const MEMORY_EPISODES: &str = "memory_episodes";
    pub const MEMORY_COMPACT: &str = "memory_compact";
    pub const MEMORY_SEARCH_ARCHIVE: &str = "memory_search_archive";
    pub const MEMORY_INGEST_LIFECYCLE: &str = "memory_ingest_lifecycle";
}

/// Lifecycle tools (design tree + openspec) — owned by `features::lifecycle`
pub mod lifecycle {
    pub const DESIGN_TREE: &str = "design_tree";
    pub const DESIGN_TREE_UPDATE: &str = "design_tree_update";
    pub const OPENSPEC_MANAGE: &str = "openspec_manage";
}

/// Cleave (decomposition) — owned by `features::cleave`
pub mod cleave {
    pub const CLEAVE_ASSESS: &str = "cleave_assess";
    pub const CLEAVE_RUN: &str = "cleave_run";
}

/// Delegate (subagent) — owned by `features::delegate`
pub mod delegate {
    pub const DELEGATE: &str = "delegate";
    pub const DELEGATE_RESULT: &str = "delegate_result";
    pub const DELEGATE_STATUS: &str = "delegate_status";
}

/// Model budget (tier switching) — owned by `features::model_budget`
pub mod model_budget {
    pub const SET_MODEL_TIER: &str = "set_model_tier";
    pub const SWITCH_TO_OFFLINE_DRIVER: &str = "switch_to_offline_driver";
    pub const SET_THINKING_LEVEL: &str = "set_thinking_level";
}

/// Tool management — owned by `features::manage_tools`
pub mod manage_tools {
    pub const MANAGE_TOOLS: &str = "manage_tools";
}

/// Auth status — owned by `features::auth`
pub mod auth {
    pub const AUTH_STATUS: &str = "auth_status";
}

/// Harness settings — owned by `features::harness_settings`
pub mod harness_settings {
    pub const HARNESS_SETTINGS: &str = "harness_settings";
}

/// Persona system — owned by `features::persona`
pub mod persona {
    pub const SWITCH_PERSONA: &str = "switch_persona";
    pub const SWITCH_TONE: &str = "switch_tone";
    pub const LIST_PERSONAS: &str = "list_personas";
}

// ─── Registry query ─────────────────────────────────────────────────────────

/// All statically-declared tool names. Used by `EventBus::finalize()` to
/// detect collisions with dynamic tools (MCP, plugins).
/// All statically-declared tool names. Used by `EventBus::finalize()` to
/// detect collisions with dynamic tools (MCP, plugins).
///
/// **Maintenance rule**: every `pub const` above MUST appear here.
/// The `registry_count_is_current` test will catch omissions.
pub fn all_static_names() -> Vec<&'static str> {
    vec![
        // core (12)
        core::BASH, core::READ, core::WRITE, core::EDIT, core::CHANGE,
        core::SPECULATE_START, core::SPECULATE_CHECK, core::SPECULATE_COMMIT,
        core::SPECULATE_ROLLBACK, core::COMMIT, core::WHOAMI, core::CHRONOS,
        // view (1)
        view::VIEW,
        // web_search (1)
        web_search::WEB_SEARCH,
        // render (2)
        render::RENDER_DIAGRAM, render::GENERATE_IMAGE_LOCAL,
        // local_inference (3)
        local_inference::ASK_LOCAL_MODEL, local_inference::LIST_LOCAL_MODELS,
        local_inference::MANAGE_OLLAMA,
        // memory (12)
        memory::MEMORY_STORE, memory::MEMORY_RECALL, memory::MEMORY_QUERY,
        memory::MEMORY_ARCHIVE, memory::MEMORY_SUPERSEDE, memory::MEMORY_CONNECT,
        memory::MEMORY_FOCUS, memory::MEMORY_RELEASE, memory::MEMORY_EPISODES,
        memory::MEMORY_COMPACT, memory::MEMORY_SEARCH_ARCHIVE,
        memory::MEMORY_INGEST_LIFECYCLE,
        // lifecycle (3)
        lifecycle::DESIGN_TREE, lifecycle::DESIGN_TREE_UPDATE,
        lifecycle::OPENSPEC_MANAGE,
        // cleave (2)
        cleave::CLEAVE_ASSESS, cleave::CLEAVE_RUN,
        // delegate (3)
        delegate::DELEGATE, delegate::DELEGATE_RESULT, delegate::DELEGATE_STATUS,
        // model_budget (3)
        model_budget::SET_MODEL_TIER, model_budget::SWITCH_TO_OFFLINE_DRIVER,
        model_budget::SET_THINKING_LEVEL,
        // manage_tools (1)
        manage_tools::MANAGE_TOOLS,
        // auth (1)
        auth::AUTH_STATUS,
        // harness_settings (1)
        harness_settings::HARNESS_SETTINGS,
        // persona (3)
        persona::SWITCH_PERSONA, persona::SWITCH_TONE, persona::LIST_PERSONAS,
    ]
    // Total: 12+1+1+2+3+12+3+2+3+3+1+1+1+3 = 48
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_duplicate_tool_names() {
        let names = all_static_names();
        let mut seen = HashSet::new();
        let mut dupes = Vec::new();
        for name in &names {
            if !seen.insert(name) {
                dupes.push(*name);
            }
        }
        assert!(
            dupes.is_empty(),
            "Duplicate tool names in registry: {:?}",
            dupes
        );
    }

    #[test]
    fn all_names_are_non_empty() {
        for name in all_static_names() {
            assert!(!name.is_empty(), "Empty tool name in registry");
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "Tool name '{}' must be lowercase ascii + underscores",
                name
            );
        }
    }

    #[test]
    fn registry_count_is_current() {
        // Update this count when adding tools. Forces awareness of registry size.
        let names = all_static_names();
        assert_eq!(
            names.len(),
            48,
            "Tool registry count changed — update this test. Current tools: {:?}",
            names
        );
    }
}
