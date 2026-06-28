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
    pub const VALIDATE: &str = "validate";
    pub const CHANGE: &str = "change";
    pub const COMMIT: &str = "commit";
    pub const WHOAMI: &str = "whoami";
    pub const CHRONOS: &str = "chronos";
    pub const SERVE: &str = "serve";
    pub const TERMINAL: &str = "terminal";
    pub const TRUST_DIRECTORY: &str = "trust_directory";
    pub const NEX_CAPABILITY: &str = "nex_capability";
    pub const NEX_SUBSTRATE: &str = "nex_substrate";
    pub const PLAN: &str = "plan";
    pub const WAIT_FOR_OPERATOR: &str = "wait_for_operator";
}

/// View tool — owned by `tools::view::ViewProvider`
pub mod view {
    pub const VIEW: &str = "view";
}

/// Render tools — owned by `tools::render::RenderProvider`
pub mod render {
    pub const RENDER_DIAGRAM: &str = "render_diagram";
}

/// Web search — owned by `tools::web_search::WebSearchProvider`
pub mod web_search {
    pub const WEB_SEARCH: &str = "web_search";
    pub const WEB_FETCH: &str = "web_fetch";
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
    pub const LIFECYCLE_DOCTOR: &str = "lifecycle_doctor";
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
    pub const DELEGATE_CANCEL: &str = "delegate_cancel";
}

/// Agent journal — owned by `features::session_log`
pub mod session_log {
    pub const SESSION_LOG: &str = "agent_journal";
}

/// Model budget / intent controls — owned by `features::model_budget`
pub mod model_budget {
    pub const SET_MODEL_INTENT: &str = "set_model_intent";
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

/// Context tools — owned by `features::context`
pub mod context {
    pub const CONTEXT_STATUS: &str = "context_status";
    pub const CONTEXT_COMPACT: &str = "context_compact";
    pub const CONTEXT_CLEAR: &str = "context_clear";
    pub const REQUEST_CONTEXT: &str = "request_context";
}

/// Persona system — owned by `features::persona`
pub mod persona {
    pub const SWITCH_PERSONA: &str = "switch_persona";
    pub const SWITCH_TONE: &str = "switch_tone";
    pub const LIST_PERSONAS: &str = "list_personas";
}

/// Skills surface — owned by `features::persona` until skills have their own augment registry handle.
pub mod skills {
    pub const SKILLS_LIST: &str = "skills_list";
    pub const SKILLS_GET: &str = "skills_get";
    pub const SKILLS_CREATE: &str = "skills_create";
    pub const SKILLS_IMPORT: &str = "skills_import";
    pub const SKILLS_INSTALL: &str = "skills_install";
    pub const SKILLS_DELETE: &str = "skills_delete";
    pub const SKILLS_RELOAD: &str = "skills_reload";
}

/// Code and knowledge scanning — owned by `tools::codebase_search`
pub mod codescan {
    pub const CODEBASE_SEARCH: &str = "codebase_search";
    pub const CODEBASE_INDEX: &str = "codebase_index";
}

/// Secret management — owned by `tools::secret_tools::SecretToolsProvider`
pub mod secrets {
    pub const SECRET_SET: &str = "secret_set";
    pub const SECRET_LIST: &str = "secret_list";
    pub const SECRET_DELETE: &str = "secret_delete";
}

/// Mutation (evolutionary skill/diagnostic creation) — owned by `features::mutation`
pub mod mutation {
    pub const MUTATION_REVIEW: &str = "mutation_review";
    pub const MUTATION_ACCEPT: &str = "mutation_accept";
    pub const MUTATION_REJECT: &str = "mutation_reject";
    pub const MUTATION_STATS: &str = "mutation_stats";
}

/// Loop jobs — owned by `features::loop_jobs`
pub mod loop_jobs {
    pub const LOOP_LIST: &str = "loop_list";
    pub const LOOP_CREATE: &str = "loop_create";
    pub const LOOP_STATUS: &str = "loop_status";
    pub const LOOP_STOP: &str = "loop_stop";
}

// ─── Registry query ─────────────────────────────────────────────────────────

/// All statically-declared tool names. Used by `EventBus::finalize()` to
/// detect collisions with dynamic tools (MCP, plugins).
/// All statically-declared tool names. Used by `EventBus::finalize()` to
/// detect collisions with dynamic tools (MCP, plugins).
///
/// **Maintenance rule**: every `pub const` above MUST appear here.
/// The `registry_count_is_current` test will catch omissions.
/// Number of statically registered tools (for splash screen display).
pub const TOOL_COUNT: usize = 79;

pub fn all_static_names() -> Vec<&'static str> {
    vec![
        // core (16)
        core::BASH,
        core::READ,
        core::WRITE,
        core::EDIT,
        core::VALIDATE,
        core::CHANGE,
        core::COMMIT,
        core::WHOAMI,
        core::CHRONOS,
        core::SERVE,
        core::TERMINAL,
        core::TRUST_DIRECTORY,
        core::NEX_CAPABILITY,
        core::NEX_SUBSTRATE,
        core::PLAN,
        core::WAIT_FOR_OPERATOR,
        // view (1)
        view::VIEW,
        // render (1)
        render::RENDER_DIAGRAM,
        // web_search (2)
        web_search::WEB_SEARCH,
        web_search::WEB_FETCH,
        // local_inference (3)
        local_inference::ASK_LOCAL_MODEL,
        local_inference::LIST_LOCAL_MODELS,
        local_inference::MANAGE_OLLAMA,
        // memory (12)
        memory::MEMORY_STORE,
        memory::MEMORY_RECALL,
        memory::MEMORY_QUERY,
        memory::MEMORY_ARCHIVE,
        memory::MEMORY_SUPERSEDE,
        memory::MEMORY_CONNECT,
        memory::MEMORY_FOCUS,
        memory::MEMORY_RELEASE,
        memory::MEMORY_EPISODES,
        memory::MEMORY_COMPACT,
        memory::MEMORY_SEARCH_ARCHIVE,
        memory::MEMORY_INGEST_LIFECYCLE,
        // lifecycle (4)
        lifecycle::DESIGN_TREE,
        lifecycle::DESIGN_TREE_UPDATE,
        lifecycle::OPENSPEC_MANAGE,
        lifecycle::LIFECYCLE_DOCTOR,
        // cleave (2)
        cleave::CLEAVE_ASSESS,
        cleave::CLEAVE_RUN,
        // delegate (3)
        delegate::DELEGATE,
        delegate::DELEGATE_RESULT,
        delegate::DELEGATE_STATUS,
        delegate::DELEGATE_CANCEL,
        // session_log (1)
        session_log::SESSION_LOG,
        // model_budget (3)
        model_budget::SET_MODEL_INTENT,
        model_budget::SWITCH_TO_OFFLINE_DRIVER,
        model_budget::SET_THINKING_LEVEL,
        // manage_tools (1)
        manage_tools::MANAGE_TOOLS,
        // auth (1)
        auth::AUTH_STATUS,
        // harness_settings (1)
        harness_settings::HARNESS_SETTINGS,
        // context (4)
        context::CONTEXT_STATUS,
        context::CONTEXT_COMPACT,
        context::CONTEXT_CLEAR,
        context::REQUEST_CONTEXT,
        // persona (3)
        persona::SWITCH_PERSONA,
        persona::SWITCH_TONE,
        persona::LIST_PERSONAS,
        // skills (7)
        skills::SKILLS_LIST,
        skills::SKILLS_GET,
        skills::SKILLS_CREATE,
        skills::SKILLS_IMPORT,
        skills::SKILLS_INSTALL,
        skills::SKILLS_DELETE,
        skills::SKILLS_RELOAD,
        // codescan (2)
        codescan::CODEBASE_SEARCH,
        codescan::CODEBASE_INDEX,
        // secrets (3)
        secrets::SECRET_SET,
        secrets::SECRET_LIST,
        secrets::SECRET_DELETE,
        // mutation (4)
        mutation::MUTATION_REVIEW,
        mutation::MUTATION_ACCEPT,
        mutation::MUTATION_REJECT,
        mutation::MUTATION_STATS,
        // loop_jobs (4)
        loop_jobs::LOOP_LIST,
        loop_jobs::LOOP_CREATE,
        loop_jobs::LOOP_STATUS,
        loop_jobs::LOOP_STOP,
    ]
    // Total: 79
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
        // Update TOOL_COUNT when adding tools. Forces awareness of registry size.
        let names = all_static_names();
        assert_eq!(
            names.len(),
            TOOL_COUNT,
            "Tool registry count changed — update TOOL_COUNT (currently {TOOL_COUNT}). Current tools: {:?}",
            names
        );
    }
}
