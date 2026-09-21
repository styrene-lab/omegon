//! Augment registry — manages active guidance augments and the memory layer stack.
//!
//! This is the runtime counterpart to the armory manifest parser.
//! It handles persona activation/deactivation, tone switching,
//! memory layer isolation, and system prompt assembly.
//!
//! Invariant: the Lex Imperialis is always present and always first.

use serde::{Deserialize, Serialize};

use crate::contribution_loading::{GuardedContributionDirectory, read_file_at};

const MAX_SKILL_ENTRIES: usize = 10_000;
const MAX_SKILL_BYTES: usize = 4 * 1024 * 1024;

/// A fact in the memory system — shared format across all layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindFact {
    pub section: String,
    pub content: String,
    pub confidence: f64,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A loaded persona ready for activation.
#[derive(Debug, Clone)]
pub struct LoadedPersona {
    pub id: String,
    pub name: String,
    pub directive: String,
    pub mind_facts: Vec<MindFact>,
    pub activated_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub badge: Option<String>,
}

/// A loaded tone ready for activation.
#[derive(Debug, Clone)]
pub struct LoadedTone {
    pub id: String,
    pub name: String,
    pub directive: String,
    pub exemplars: Vec<String>,
    pub intensity: ToneIntensity,
}

/// When to apply tone voice at full strength.
#[derive(Debug, Clone)]
pub struct ToneIntensity {
    /// Intensity during design/creative: "full", "muted", "off"
    pub design: String,
    /// Intensity during coding/execution: "full", "muted", "off"
    pub coding: String,
}

impl Default for ToneIntensity {
    fn default() -> Self {
        Self {
            design: "full".into(),
            coding: "muted".into(),
        }
    }
}

/// Policy for resolving activation conflicts between same-context skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillConflictResolution {
    /// Keep the latest provider by load order. Canonical order is bundled/user before project.
    /// This is a non-interactive fallback only; interactive surfaces should recommend merging
    /// conflicting guidance into one project-local skill instead of injecting both.
    #[default]
    MostRecent,
    /// Mark the conflict for operator resolution. Until `/skills resolve` exists, this uses
    /// the same non-interactive fallback as MostRecent to preserve the one-skill-per-slot invariant.
    Prompt,
    /// Drop all participants in a detected conflict. Useful for tests/hardening.
    Error,
}

#[derive(Debug, Clone)]
struct PromptSkillCandidate {
    name: String,
    source: String,
    path: std::path::PathBuf,
    content: String,
    order: usize,
    manifest: omegon_skills::SkillManifest,
}

#[derive(Debug, Clone)]
struct PromptSkillLoadResult {
    skills: Vec<String>,
    snapshots: Vec<LoadedSkillSnapshot>,
    events: Vec<omegon_traits::SkillActivationEvent>,
    health: Vec<crate::contribution_health::ScopeHealth>,
}

#[derive(Debug, Clone)]
pub struct LoadedSkillSnapshot {
    pub name: String,
    pub source: String,
    pub path: std::path::PathBuf,
    pub content: String,
}

fn prompt_skill_ref(candidate: &PromptSkillCandidate) -> String {
    format!("{}/{}", candidate.source, candidate.name)
}

/// Layered memory — each layer has distinct lifecycle rules.
#[derive(Debug, Default)]
pub struct MemoryLayers {
    /// Pinned facts — highest priority, session-scoped.
    pub working: Vec<MindFact>,
    /// Persona mind — seeded on activation, grows during session, cleared on deactivation.
    pub persona: Vec<MindFact>,
    /// Project memory — persists across persona switches.
    pub project: Vec<MindFact>,
}

/// Result of activating a persona.
#[derive(Debug)]
pub struct ActivateResult {
    pub previous_id: Option<String>,
}

/// Result of deactivating a persona.
#[derive(Debug)]
pub struct DeactivateResult {
    pub removed_id: Option<String>,
    pub facts_removed: usize,
}

/// The augment registry — manages active persona, tone, skills, memory, and system prompt assembly.
///
/// Invariant: `lex_imperialis` is always injected first in the system prompt.
/// No operation can remove or reorder it.
pub struct AugmentRegistry {
    pub(crate) loading_health: crate::contribution_health::ContributionHealth,
    lex_imperialis: String,
    active_persona: Option<LoadedPersona>,
    active_tone: Option<LoadedTone>,
    memory: MemoryLayers,
    /// Skill directives loaded from ~/.omegon/skills/ and .omegon/skills/.
    /// Project-local (.omegon/skills/) entries override same-named user-installed
    /// entries so prompt assembly consumes one resolved directive per skill name.
    loaded_skills: Vec<String>,
    loaded_skill_snapshots: Vec<LoadedSkillSnapshot>,
    /// True when skills were loaded from an explicit operator-supplied subset.
    ///
    /// An explicit subset is itself the admission evidence: the parent already
    /// decided which skills this agent needs. Re-judging that choice against
    /// workspace signals would silently override operator intent, so disclosure
    /// is bypassed entirely in this mode.
    explicit_skill_subset: bool,
    skill_activation_events: Vec<omegon_traits::SkillActivationEvent>,
    skill_conflict_resolution: SkillConflictResolution,
}

fn prompt_skill_source_for_order(order: usize) -> String {
    if order == 0 {
        "user".into()
    } else {
        "project".into()
    }
}

fn prompt_skill_activation_event(
    candidate: &PromptSkillCandidate,
    suppressing: Vec<String>,
    resolution: &str,
) -> omegon_traits::SkillActivationEvent {
    let recommendation = (!suppressing.is_empty()).then(|| "merge_project_override".to_string());
    omegon_traits::SkillActivationEvent {
        active_ref: prompt_skill_ref(candidate),
        activation: candidate.manifest.activation.clone(),
        reason: if suppressing.is_empty() {
            "skill_loaded".into()
        } else {
            "activation_conflict".into()
        },
        matched_signals: candidate.manifest.project_signals.clone(),
        suppressing,
        resolution: resolution.into(),
        recommendation,
        injected: true,
    }
}

fn prompt_skill_conflicts(left: &PromptSkillCandidate, right: &PromptSkillCandidate) -> bool {
    if left.name == right.name {
        return false;
    }
    let left_manifest = &left.manifest;
    let right_manifest = &right.manifest;
    let trigger_overlap = left_manifest
        .triggers
        .iter()
        .any(|trigger| right_manifest.triggers.contains(trigger));
    let alias_overlap = left_manifest
        .aliases
        .iter()
        .any(|alias| right_manifest.aliases.contains(alias));
    let activation_overlap = left_manifest.activation.is_some()
        && left_manifest.activation == right_manifest.activation
        && ((!left_manifest.profile.is_empty()
            && left_manifest
                .profile
                .iter()
                .any(|profile| right_manifest.profile.contains(profile)))
            || (!left_manifest.project_signals.is_empty()
                && left_manifest
                    .project_signals
                    .iter()
                    .any(|signal| right_manifest.project_signals.contains(signal))));
    trigger_overlap || alias_overlap || activation_overlap
}

impl AugmentRegistry {
    /// Create a new registry. The Lex Imperialis content is required and immutable.
    pub fn new(lex_imperialis: String) -> Self {
        Self {
            loading_health: Default::default(),
            lex_imperialis,
            active_persona: None,
            active_tone: None,
            memory: MemoryLayers::default(),
            loaded_skills: Vec::new(),
            loaded_skill_snapshots: Vec::new(),
            explicit_skill_subset: false,
            skill_activation_events: Vec::new(),
            skill_conflict_resolution: SkillConflictResolution::default(),
        }
    }

    /// Load skills from the boot content generation, user root, then project root.
    ///
    /// Call once at session start. Silently skips missing directories.
    pub fn load_skills(&mut self, cwd: &std::path::Path) {
        self.load_skills_subset(cwd, &[]);
    }

    /// Load only a named subset of skills from the canonical locations.
    /// When `allowed` is empty, behaves like `load_skills` and loads all skills.
    pub fn load_skills_subset(&mut self, cwd: &std::path::Path, allowed: &[String]) {
        let explicit_skill_subset = !allowed.is_empty();
        match crate::paths::omegon_home() {
            Ok(home) => self.load_skills_subset_with_home(cwd, &home, allowed),
            Err(error) => {
                self.loading_health.replace_kind(
                    crate::contribution_health::ContributionKind::Skills,
                    vec![crate::contribution_health::ScopeHealth::blocked(
                        crate::contribution_health::ContributionKind::Skills,
                        "home",
                        std::path::Path::new("~/.omegon/skills"),
                        &error,
                    )],
                );
                tracing::warn!(error = %error, "skill loading failed closed");
                self.loaded_skills.clear();
                self.loaded_skill_snapshots.clear();
                self.explicit_skill_subset = explicit_skill_subset;
                self.skill_activation_events.clear();
            }
        }
    }

    pub(crate) fn load_skills_subset_with_home(
        &mut self,
        cwd: &std::path::Path,
        home: &std::path::Path,
        allowed: &[String],
    ) {
        let explicit_skill_subset = !allowed.is_empty();
        let policy = self.skill_conflict_resolution;
        Self::with_guarded_skills(cwd, home, allowed, policy, |result| {
            self.loading_health.replace_kind(
                crate::contribution_health::ContributionKind::Skills,
                result.health,
            );
            self.loaded_skills = result.skills;
            self.loaded_skill_snapshots = result.snapshots;
            self.explicit_skill_subset = explicit_skill_subset;
            self.skill_activation_events = result.events;
        });
    }

    fn with_guarded_skills<R>(
        cwd: &std::path::Path,
        home: &std::path::Path,
        allowed: &[String],
        policy: SkillConflictResolution,
        publish: impl FnOnce(PromptSkillLoadResult) -> R,
    ) -> R {
        use crate::contribution_health::{ContributionKind::Skills, LoadingState, ScopeHealth};
        let mut health = Vec::new();
        let mut scopes = Vec::new();
        for (root, components, scope, display_root) in [
            (
                home,
                [b"skills".as_slice(), b"".as_slice()],
                "user",
                home.join("skills"),
            ),
            (
                cwd,
                [b".omegon".as_slice(), b"skills".as_slice()],
                "project",
                cwd.join(".omegon/skills"),
            ),
        ] {
            let components = components
                .iter()
                .copied()
                .filter(|component| !component.is_empty())
                .collect::<Vec<_>>();
            match GuardedContributionDirectory::open(
                root,
                &components,
                home,
                omegon_maintenance_contracts::ContributionKind::Skill,
                scope,
            ) {
                Ok(Some(directory)) => scopes.push((scope, display_root, directory)),
                Ok(None) => health.push(ScopeHealth::new(
                    Skills,
                    scope,
                    &display_root,
                    LoadingState::Absent,
                )),
                Err(error) => {
                    health.push(ScopeHealth::blocked(Skills, scope, &display_root, &error));
                    tracing::warn!(scope, error = %error, "skill contribution scope failed closed");
                }
            }
        }

        let mut skills = std::collections::BTreeMap::<String, PromptSkillCandidate>::new();
        let mut order = 0usize;
        if let Some(pack) = crate::content_pack::boot_pack() {
            for asset in pack.assets("skill") {
                let path = std::path::Path::new(&asset.manifest.path);
                if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
                    continue;
                }
                let Some(skill_name) = path
                    .parent()
                    .and_then(std::path::Path::file_name)
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                if !allowed.is_empty() && !allowed.iter().any(|name| name == &skill_name) {
                    continue;
                }
                let Ok(content) = std::str::from_utf8(&asset.bytes).map(str::to_owned) else {
                    continue;
                };
                let (manifest, _body) = omegon_skills::parse_skill_file(&content);
                skills.insert(
                    skill_name.clone(),
                    PromptSkillCandidate {
                        name: skill_name,
                        source: "bundled".into(),
                        path: pack.root.join(path),
                        content,
                        order,
                        manifest,
                    },
                );
                order += 1;
            }
        }
        for (scope, display_root, directory) in &scopes {
            let mut scoped = std::collections::BTreeMap::new();
            let starting_order = order;
            match Self::load_guarded_skill_scope(
                scope,
                display_root,
                directory,
                allowed,
                &mut scoped,
                &mut order,
            ) {
                Ok(()) => {
                    health.push(ScopeHealth::new(
                        Skills,
                        scope,
                        display_root,
                        LoadingState::Loaded {
                            count: scoped.len(),
                        },
                    ));
                    skills.extend(scoped);
                }
                Err(error) => {
                    health.push(ScopeHealth::blocked(Skills, scope, display_root, &error));
                    order = starting_order;
                    tracing::warn!(scope, error = %error, "skill contribution scope failed closed");
                }
            }
        }
        let (candidates, events) =
            Self::resolve_prompt_skill_conflicts(skills.into_values().collect(), policy);
        let snapshots = candidates
            .iter()
            .map(|candidate| LoadedSkillSnapshot {
                name: candidate.name.clone(),
                source: candidate.source.clone(),
                path: candidate.path.clone(),
                content: candidate.content.clone(),
            })
            .collect();
        publish(PromptSkillLoadResult {
            health,
            skills: candidates
                .into_iter()
                .map(|candidate| candidate.content)
                .collect(),
            snapshots,
            events,
        })
    }

    fn load_guarded_skill_scope(
        scope: &str,
        display_root: &std::path::Path,
        directory: &GuardedContributionDirectory,
        allowed: &[String],
        skills: &mut std::collections::BTreeMap<String, PromptSkillCandidate>,
        order: &mut usize,
    ) -> anyhow::Result<()> {
        let mut entries = directory.entry_names(MAX_SKILL_ENTRIES)?;
        entries.sort();
        for raw_name in entries {
            if crate::contribution_loading::is_internal_contribution_entry(&raw_name) {
                continue;
            }
            if !directory.allows(&raw_name)? {
                tracing::info!(scope, skill = %String::from_utf8_lossy(&raw_name), "excluded denied skill");
                continue;
            }
            let Ok(skill_name) = std::str::from_utf8(&raw_name).map(ToOwned::to_owned) else {
                tracing::warn!(scope, "skipping skill with a non-UTF-8 basename");
                continue;
            };
            if !allowed.is_empty() && !allowed.iter().any(|name| name == &skill_name) {
                continue;
            }
            let Some(skill_directory) = directory.open_child_directory(&raw_name)? else {
                continue;
            };
            let Some(bytes) = read_file_at(&skill_directory, b"SKILL.md", MAX_SKILL_BYTES)? else {
                continue;
            };
            let Ok(content) = String::from_utf8(bytes) else {
                tracing::warn!(scope, skill = %skill_name, "skipping non-UTF-8 skill");
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let (manifest, _body) = omegon_skills::parse_skill_file(&content);
            let path = display_root.join(&skill_name);
            skills.insert(
                skill_name.clone(),
                PromptSkillCandidate {
                    name: skill_name,
                    source: scope.into(),
                    path,
                    content,
                    order: *order,
                    manifest,
                },
            );
            *order += 1;
        }
        Ok(())
    }

    /// Configure how prompt assembly resolves skill activation conflicts.
    pub fn set_skill_conflict_resolution(&mut self, policy: SkillConflictResolution) {
        self.skill_conflict_resolution = policy;
    }

    /// Load skill content from an explicit list of directories.
    /// Used by `load_skills` in production and directly by tests to avoid
    /// reading from the real ~/.omegon/skills/ installation.
    fn load_from_dirs(dirs: &[std::path::PathBuf]) -> Vec<String> {
        Self::load_from_dirs_filtered(dirs, &[])
    }

    fn load_from_dirs_filtered(dirs: &[std::path::PathBuf], allowed: &[String]) -> Vec<String> {
        Self::load_from_dirs_filtered_with_policy(dirs, allowed, SkillConflictResolution::default())
            .skills
    }

    fn load_from_dirs_filtered_with_policy(
        dirs: &[std::path::PathBuf],
        allowed: &[String],
        policy: SkillConflictResolution,
    ) -> PromptSkillLoadResult {
        let mut skills = std::collections::BTreeMap::<String, PromptSkillCandidate>::new();
        let mut order = 0usize;
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            let mut entries: Vec<_> = match std::fs::read_dir(dir) {
                Ok(e) => e.filter_map(|e| e.ok()).collect(),
                Err(_) => continue,
            };
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let skill_name = entry.file_name().to_string_lossy().to_string();
                if !allowed.is_empty() && !allowed.iter().any(|name| name == &skill_name) {
                    continue;
                }
                let skill_file = entry.path().join("SKILL.md");
                if let Ok(content) = std::fs::read_to_string(&skill_file)
                    && !content.trim().is_empty()
                {
                    let (manifest, _body) = omegon_skills::parse_skill_file(&content);
                    skills.insert(
                        skill_name.clone(),
                        PromptSkillCandidate {
                            name: skill_name,
                            source: prompt_skill_source_for_order(order),
                            path: entry.path(),
                            content,
                            order,
                            manifest,
                        },
                    );
                    order += 1;
                }
            }
        }
        let (candidates, events) =
            Self::resolve_prompt_skill_conflicts(skills.into_values().collect(), policy);
        let snapshots = candidates
            .iter()
            .map(|candidate| LoadedSkillSnapshot {
                name: candidate.name.clone(),
                source: candidate.source.clone(),
                path: candidate.path.clone(),
                content: candidate.content.clone(),
            })
            .collect();
        PromptSkillLoadResult {
            health: Vec::new(),
            skills: candidates
                .into_iter()
                .map(|candidate| candidate.content)
                .collect(),
            snapshots,
            events,
        }
    }

    fn resolve_prompt_skill_conflicts(
        mut candidates: Vec<PromptSkillCandidate>,
        policy: SkillConflictResolution,
    ) -> (
        Vec<PromptSkillCandidate>,
        Vec<omegon_traits::SkillActivationEvent>,
    ) {
        if candidates.len() < 2 {
            let events = candidates
                .iter()
                .map(|candidate| prompt_skill_activation_event(candidate, Vec::new(), "loaded"))
                .collect();
            return (candidates, events);
        }
        candidates.sort_by_key(|candidate| candidate.order);
        let mut suppressed = std::collections::BTreeSet::new();
        let mut suppressed_by: std::collections::BTreeMap<usize, Vec<String>> =
            std::collections::BTreeMap::new();
        for left in 0..candidates.len() {
            for right in (left + 1)..candidates.len() {
                if suppressed.contains(&left) || suppressed.contains(&right) {
                    continue;
                }
                if !prompt_skill_conflicts(&candidates[left], &candidates[right]) {
                    continue;
                }
                match policy {
                    SkillConflictResolution::MostRecent | SkillConflictResolution::Prompt => {
                        suppressed.insert(left);
                        suppressed_by
                            .entry(right)
                            .or_default()
                            .push(prompt_skill_ref(&candidates[left]));
                    }
                    SkillConflictResolution::Error => {
                        suppressed.insert(left);
                        suppressed.insert(right);
                    }
                }
            }
        }
        let mut kept = Vec::new();
        let mut events = Vec::new();
        for (index, candidate) in candidates.into_iter().enumerate() {
            if suppressed.contains(&index) {
                continue;
            }
            let suppressing = suppressed_by.remove(&index).unwrap_or_default();
            let resolution = if suppressing.is_empty() {
                "loaded"
            } else {
                "selected_by_precedence"
            };
            events.push(prompt_skill_activation_event(
                &candidate,
                suppressing,
                resolution,
            ));
            kept.push(candidate);
        }
        (kept, events)
    }

    /// Access structured skill activation/resolution events produced during skill loading.
    pub fn skill_activation_events(&self) -> &[omegon_traits::SkillActivationEvent] {
        &self.skill_activation_events
    }

    /// Return the number of loaded skills.
    pub fn skill_count(&self) -> usize {
        self.loaded_skills.len()
    }

    /// Access loaded skill content for trusted_paths extraction.
    pub fn skills(&self) -> &[String] {
        &self.loaded_skills
    }

    pub fn skill_snapshots(&self) -> &[LoadedSkillSnapshot] {
        &self.loaded_skill_snapshots
    }

    /// Test-only: load skills from an explicit list of directories,
    /// bypassing the real ~/.omegon/skills/ path.
    #[cfg(test)]
    pub(crate) fn load_skills_from_dirs_for_test(&mut self, dirs: &[std::path::PathBuf]) {
        self.load_skills_from_explicit(dirs);
    }

    /// Test-only: load skills from an explicit list of directories,
    /// bypassing the real ~/.omegon/skills/ path.
    #[cfg(test)]
    fn load_skills_from_explicit(&mut self, dirs: &[std::path::PathBuf]) {
        let result =
            Self::load_from_dirs_filtered_with_policy(dirs, &[], self.skill_conflict_resolution);
        self.loaded_skills = result.skills;
        self.loaded_skill_snapshots = result.snapshots;
        self.skill_activation_events = result.events;
    }

    #[cfg(test)]
    fn load_skills_subset_from_explicit(
        &mut self,
        dirs: &[std::path::PathBuf],
        allowed: &[String],
    ) {
        let result = Self::load_from_dirs_filtered_with_policy(
            dirs,
            allowed,
            self.skill_conflict_resolution,
        );
        self.loaded_skills = result.skills;
        self.loaded_skill_snapshots = result.snapshots;
        self.skill_activation_events = result.events;
    }

    /// Activate a persona. Replaces any previously active persona.
    /// Clears the previous persona's mind facts and loads the new ones.
    pub fn activate_persona(&mut self, persona: LoadedPersona) -> ActivateResult {
        let previous_id = self.active_persona.as_ref().map(|p| p.id.clone());

        // Clear previous persona's mind layer
        self.memory.persona.clear();

        // Load new persona's seed facts
        self.memory.persona = persona.mind_facts.clone();
        self.active_persona = Some(persona);

        ActivateResult { previous_id }
    }

    /// Deactivate the current persona. Clears its mind facts.
    pub fn deactivate_persona(&mut self) -> DeactivateResult {
        match self.active_persona.take() {
            Some(persona) => {
                let facts_removed = self.memory.persona.len();
                self.memory.persona.clear();
                DeactivateResult {
                    removed_id: Some(persona.id),
                    facts_removed,
                }
            }
            None => DeactivateResult {
                removed_id: None,
                facts_removed: 0,
            },
        }
    }

    /// Activate a tone. Replaces any previously active tone.
    pub fn activate_tone(&mut self, tone: LoadedTone) -> Option<String> {
        let previous_id = self.active_tone.as_ref().map(|t| t.id.clone());
        self.active_tone = Some(tone);
        previous_id
    }

    /// Deactivate the current tone.
    pub fn deactivate_tone(&mut self) -> Option<String> {
        self.active_tone.take().map(|t| t.id)
    }

    /// Store a fact into the persona mind layer. Fails if no persona is active.
    pub fn store_persona_fact(&mut self, fact: MindFact) -> Result<(), &'static str> {
        if self.active_persona.is_none() {
            return Err("no active persona — cannot store persona fact");
        }
        self.memory.persona.push(fact);
        Ok(())
    }

    /// Store a fact into the project memory layer.
    pub fn store_project_fact(&mut self, fact: MindFact) {
        self.memory.project.push(fact);
    }

    /// Query all memory layers — merged view in priority order.
    /// Working > Persona > Project.
    pub fn query_all_facts(&self) -> Vec<&MindFact> {
        self.memory
            .working
            .iter()
            .chain(self.memory.persona.iter())
            .chain(self.memory.project.iter())
            .collect()
    }

    /// Assemble the system prompt from all active layers.
    /// Order: Lex Imperialis → Skills → Tone → Persona.
    ///
    /// Every loaded skill contributes its full body. Prefer
    /// [`Self::build_system_prompt_disclosed`] on the interactive path, which
    /// withholds bodies that the workspace and prompt provide no evidence for.
    pub fn build_system_prompt(&self) -> String {
        let mut layers = vec![self.lex_imperialis.as_str()];

        for skill in &self.loaded_skills {
            layers.push(skill.as_str());
        }

        self.finish_system_prompt(layers)
    }

    /// Assemble the system prompt with progressive skill disclosure applied.
    ///
    /// Admitted skills contribute their full body exactly as
    /// [`Self::build_system_prompt`] would. Unadmitted skills collapse to a
    /// single `name — description` line under a shared index header, so the
    /// model can still discover them and pull the body with `skills_get`.
    ///
    /// Admission is decided by declared activation plus workspace evidence
    /// under `root` and the current operator `prompt`. A skill whose manifest
    /// cannot be re-parsed is admitted rather than dropped: losing a capability
    /// silently is worse than spending its tokens.
    pub fn build_system_prompt_disclosed(
        &self,
        root: &std::path::Path,
        prompt: Option<&str>,
    ) -> String {
        let mut layers = vec![self.lex_imperialis.as_str()];
        let mut withheld: Vec<String> = Vec::new();

        // An explicit skill subset is the operator's decision about what this
        // agent needs. Disclosure must not second-guess it.
        if self.explicit_skill_subset {
            return self.build_system_prompt();
        }

        for skill in &self.loaded_skills {
            let (manifest, _body) = omegon_skills::parse_skill_file(skill);
            if manifest.name.is_empty() {
                layers.push(skill.as_str());
                continue;
            }
            let entry = omegon_skills::disclosure::disclose(&manifest, root, prompt);
            if entry.admits_body() {
                layers.push(skill.as_str());
            } else {
                withheld.push(format!("- {} — {}", entry.name, entry.description));
            }
        }

        let index = (!withheld.is_empty()).then(|| {
            format!(
                "# Available skills (not loaded)\n\n\
                 These skills are installed but their bodies are withheld from this \
                 prompt because the workspace and your request provide no evidence \
                 they apply. Call `skills_get` with a name to load one in full.\n\n{}",
                withheld.join("\n")
            )
        });
        if let Some(ref index) = index {
            layers.push(index.as_str());
        }

        self.finish_system_prompt(layers)
    }

    /// Append the tone and persona layers and join. Shared by both assemblers so
    /// disclosure can never reorder or drop a non-skill layer.
    fn finish_system_prompt<'a>(&'a self, mut layers: Vec<&'a str>) -> String {
        if let Some(ref tone) = self.active_tone {
            layers.push(&tone.directive);
        }

        if let Some(ref persona) = self.active_persona {
            layers.push(&persona.directive);
        }

        layers.join("\n\n---\n\n")
    }

    /// Get the active persona, if any.
    pub fn active_persona(&self) -> Option<&LoadedPersona> {
        self.active_persona.as_ref()
    }

    /// Get the active tone, if any.
    pub fn active_tone(&self) -> Option<&LoadedTone> {
        self.active_tone.as_ref()
    }

    /// Direct access to memory layers.
    pub fn memory(&self) -> &MemoryLayers {
        &self.memory
    }

    /// Mutable access to working memory (for pinning facts).
    pub fn working_memory_mut(&mut self) -> &mut Vec<MindFact> {
        &mut self.memory.working
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEX: &str = include_str!("../../../../../data/lex-imperialis.md");

    fn tutor_persona() -> LoadedPersona {
        LoadedPersona {
            id: "dev.styrene.omegon.tutor".into(),
            name: "Socratic Tutor".into(),
            directive: "# Socratic Tutor\n\nYou are a patient, skilled tutor.".into(),
            mind_facts: vec![
                MindFact {
                    section: "Domain".into(),
                    content: "Bloom's Taxonomy: Remember → Understand → Apply → Analyze → Evaluate → Create".into(),
                    confidence: 0.95,
                    source: Some("anderson-krathwohl-2001".into()),
                    tags: vec!["pedagogy".into()],
                },
                MindFact {
                    section: "Domain".into(),
                    content: "Zone of Proximal Development (Vygotsky): the space between independent and guided capability".into(),
                    confidence: 0.95,
                    source: Some("vygotsky-1978".into()),
                    tags: vec!["pedagogy".into()],
                },
            ],
            activated_skills: vec![],
            disabled_tools: vec!["bash".into(), "write".into()],
            badge: Some("📚".into()),
        }
    }

    fn engineer_persona() -> LoadedPersona {
        LoadedPersona {
            id: "dev.styrene.omegon.systems-engineer".into(),
            name: "Systems Engineer".into(),
            directive: "# Systems Engineer\n\nYou are a systems engineering harness.".into(),
            mind_facts: vec![
                MindFact {
                    section: "Domain".into(),
                    content:
                        "Conway's Law: system architecture mirrors org communication structure"
                            .into(),
                    confidence: 0.95,
                    source: Some("conway-1968".into()),
                    tags: vec!["architecture".into()],
                },
                MindFact {
                    section: "Domain".into(),
                    content:
                        "CAP theorem: at most two of Consistency, Availability, Partition tolerance"
                            .into(),
                    confidence: 0.95,
                    source: Some("brewer-2000".into()),
                    tags: vec!["distributed".into()],
                },
                MindFact {
                    section: "Domain".into(),
                    content: "Amdahl's Law: speedup limited by sequential fraction".into(),
                    confidence: 0.95,
                    source: Some("amdahl-1967".into()),
                    tags: vec!["performance".into()],
                },
            ],
            activated_skills: vec!["typescript".into(), "rust".into()],
            disabled_tools: vec![],
            badge: Some("⚙".into()),
        }
    }

    fn watts_tone() -> LoadedTone {
        LoadedTone {
            id: "dev.styrene.omegon.tone.alan-watts".into(),
            name: "Alan Watts".into(),
            directive: "# Alan Watts\n\nSpeak with gentle irreverence and philosophical depth."
                .into(),
            exemplars: vec!["A distributed system is like a jazz ensemble.".into()],
            intensity: ToneIntensity::default(),
        }
    }

    fn concise_tone() -> LoadedTone {
        LoadedTone {
            id: "dev.styrene.omegon.tone.concise".into(),
            name: "Concise".into(),
            directive: "# Concise\n\nBe terse. Maximum signal, minimum words.".into(),
            exemplars: vec![],
            intensity: ToneIntensity {
                design: "full".into(),
                coding: "full".into(),
            },
        }
    }

    // ── Persona activation ───────────────────────────────────

    #[test]
    fn activate_persona_loads_directive_into_prompt() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn activate_persona_loads_mind_facts() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        assert_eq!(reg.memory().persona.len(), 2);
        assert!(
            reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Bloom"))
        );
    }

    #[test]
    fn lex_always_first_in_prompt() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        let prompt = reg.build_system_prompt();
        let lex_pos = prompt.find("Lex Imperialis").unwrap();
        let persona_pos = prompt.find("Socratic Tutor").unwrap();
        assert!(lex_pos < persona_pos);
    }

    #[test]
    fn first_activation_returns_no_previous() {
        let mut reg = AugmentRegistry::new(LEX.into());
        let result = reg.activate_persona(tutor_persona());
        assert!(result.previous_id.is_none());
    }

    // ── Persona deactivation ─────────────────────────────────

    #[test]
    fn deactivate_removes_directive_from_prompt() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.deactivate_persona();

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn deactivate_clears_persona_memory() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        assert!(!reg.memory().persona.is_empty());

        reg.deactivate_persona();
        assert!(reg.memory().persona.is_empty());
    }

    #[test]
    fn deactivate_preserves_project_memory() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.store_project_fact(MindFact {
            section: "Architecture".into(),
            content: "Project uses React with TypeScript".into(),
            confidence: 0.9,
            source: None,
            tags: vec![],
        });

        reg.deactivate_persona();
        assert_eq!(reg.memory().project.len(), 1);
        assert!(reg.memory().project[0].content.contains("React"));
    }

    #[test]
    fn deactivate_returns_removed_info() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        let result = reg.deactivate_persona();

        assert!(result.removed_id.as_deref().unwrap().contains("tutor"));
        assert!(result.facts_removed > 0);
    }

    #[test]
    fn deactivate_noop_when_none_active() {
        let mut reg = AugmentRegistry::new(LEX.into());
        let result = reg.deactivate_persona();
        assert!(result.removed_id.is_none());
        assert_eq!(result.facts_removed, 0);
    }

    // ── Persona switching ────────────────────────────────────

    #[test]
    fn switch_replaces_directive() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.activate_persona(engineer_persona());

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Systems Engineer"));
    }

    #[test]
    fn switch_replaces_mind_facts() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        assert!(
            reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Bloom"))
        );

        reg.activate_persona(engineer_persona());
        assert!(
            !reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Bloom"))
        );
        assert!(
            reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Conway"))
        );
    }

    #[test]
    fn switch_returns_previous_id() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        let result = reg.activate_persona(engineer_persona());
        assert!(result.previous_id.as_deref().unwrap().contains("tutor"));
    }

    #[test]
    fn switch_preserves_project_memory() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.store_project_fact(MindFact {
            section: "Decisions".into(),
            content: "Chose Postgres over SQLite".into(),
            confidence: 0.9,
            source: None,
            tags: vec![],
        });

        reg.activate_persona(engineer_persona());
        assert_eq!(reg.memory().project.len(), 1);
        assert!(reg.memory().project[0].content.contains("Postgres"));
    }

    #[test]
    fn switch_drops_accumulated_persona_facts() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.store_persona_fact(MindFact {
            section: "Domain".into(),
            content: "Student struggles with recursion — use tree metaphors".into(),
            confidence: 0.8,
            source: None,
            tags: vec![],
        })
        .unwrap();

        reg.activate_persona(engineer_persona());
        assert!(
            !reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("recursion"))
        );
        assert!(
            !reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Bloom"))
        );
        assert!(
            reg.memory()
                .persona
                .iter()
                .any(|f| f.content.contains("Conway"))
        );
    }

    // ── Tone activation ──────────────────────────────────────

    #[test]
    fn tone_between_lex_and_persona() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.activate_tone(watts_tone());

        let prompt = reg.build_system_prompt();
        let lex_pos = prompt.find("Lex Imperialis").unwrap();
        let tone_pos = prompt.find("Alan Watts").unwrap();
        let persona_pos = prompt.find("Socratic Tutor").unwrap();
        assert!(lex_pos < tone_pos, "lex before tone");
        assert!(tone_pos < persona_pos, "tone before persona");
    }

    #[test]
    fn tone_works_without_persona() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());

        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Alan Watts"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn tone_switch_replaces() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());
        reg.activate_tone(concise_tone());

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Alan Watts"));
        assert!(prompt.contains("Concise"));
    }

    #[test]
    fn tone_deactivate_removes() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());
        reg.deactivate_tone();
        assert!(!reg.build_system_prompt().contains("Alan Watts"));
    }

    // ── Memory layer isolation ───────────────────────────────

    #[test]
    fn query_all_merges_in_priority_order() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.store_project_fact(MindFact {
            section: "Architecture".into(),
            content: "Uses monorepo".into(),
            confidence: 0.9,
            source: None,
            tags: vec![],
        });
        reg.working_memory_mut().push(MindFact {
            section: "Pinned".into(),
            content: "Focus: auth module".into(),
            confidence: 1.0,
            source: None,
            tags: vec![],
        });

        let all = reg.query_all_facts();
        // Working first
        assert_eq!(all[0].section, "Pinned");
        // Persona facts present
        assert!(all.iter().any(|f| f.content.contains("Bloom")));
        // Project facts present
        assert!(all.iter().any(|f| f.content.contains("monorepo")));
    }

    #[test]
    fn persona_facts_dont_leak_to_project() {
        let mut reg = AugmentRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        assert!(!reg.memory().persona.is_empty());
        assert!(reg.memory().project.is_empty());
    }

    #[test]
    fn store_persona_fact_fails_without_active_persona() {
        let mut reg = AugmentRegistry::new(LEX.into());
        let result = reg.store_persona_fact(MindFact {
            section: "Domain".into(),
            content: "orphan fact".into(),
            confidence: 0.8,
            source: None,
            tags: vec![],
        });
        assert!(result.is_err());
    }

    // ── Lex Imperialis invariants ────────────────────────────

    #[test]
    fn lex_present_with_nothing_active() {
        let reg = AugmentRegistry::new(LEX.into());
        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Lex Imperialis"));
        assert!(prompt.contains("Anti-Sycophancy"));
    }

    #[test]
    fn lex_survives_all_transitions() {
        let mut reg = AugmentRegistry::new(LEX.into());

        reg.activate_persona(tutor_persona());
        reg.activate_tone(watts_tone());
        assert!(reg.build_system_prompt().contains("Lex Imperialis"));

        reg.activate_persona(engineer_persona());
        assert!(reg.build_system_prompt().contains("Lex Imperialis"));

        reg.activate_tone(concise_tone());
        assert!(reg.build_system_prompt().contains("Lex Imperialis"));

        reg.deactivate_persona();
        reg.deactivate_tone();
        assert!(reg.build_system_prompt().contains("Lex Imperialis"));
    }

    #[test]
    fn lex_contains_all_six_directives() {
        let reg = AugmentRegistry::new(LEX.into());
        let prompt = reg.build_system_prompt();
        for directive in [
            "Anti-Sycophancy",
            "Evidence-Based Epistemology",
            "Perfection Is the Enemy of Good",
            "Systems Engineering Harness",
            "Cognitive Honesty",
            "Operator Agency",
        ] {
            assert!(prompt.contains(directive), "missing directive: {directive}");
        }
    }

    /// Write `skills/<name>/SKILL.md` under `dir`, creating parents.
    fn write_skill(dir: &std::path::Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn guarded_skill_loading_excludes_denied_project_skill() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        write_skill(&home.path().join("skills"), "shared", "USER_SKILL_MARKER");
        write_skill(
            &project.path().join(".omegon/skills"),
            "shared",
            "DENIED_PROJECT_MARKER",
        );
        write_skill(
            &project.path().join(".omegon/skills"),
            "allowed",
            "ALLOWED_PROJECT_MARKER",
        );
        deny_skill(
            project.path(),
            &[b".omegon", b"skills"],
            home.path(),
            "project",
            b"shared",
        );

        let registry = load_guarded_registry(project.path(), home.path());
        let prompt = registry.build_system_prompt();
        assert!(prompt.contains("USER_SKILL_MARKER"));
        assert!(prompt.contains("ALLOWED_PROJECT_MARKER"));
        assert!(!prompt.contains("DENIED_PROJECT_MARKER"));
    }

    #[cfg(unix)]
    #[test]
    fn contribution_health_missing_skill_directories_are_absent() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let registry = load_guarded_registry(project.path(), home.path());
        let health = registry.loading_health.snapshot();
        assert_eq!(health.len(), 2);
        assert!(health.iter().all(|scope| matches!(
            scope.outcome,
            crate::contribution_health::LoadingState::Absent
        )));
    }

    #[cfg(unix)]
    #[test]
    fn contribution_health_skill_loading_isolates_malformed_scope() {
        use std::io::Write;

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        write_skill(&home.path().join("skills"), "user", "USER_SCOPE_MARKER");
        write_skill(
            &project.path().join(".omegon/skills"),
            "project",
            "PROJECT_SCOPE_MARKER",
        );
        let authority = initialize_skill_scope(
            project.path(),
            &[b".omegon", b"skills"],
            home.path(),
            "project",
        );
        let state_path = home
            .path()
            .join("maintain/v1/deny")
            .join(authority.to_hex())
            .join("state.json");
        let original = std::fs::read(&state_path).unwrap();
        let mut state = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&state_path)
            .unwrap();
        state.write_all(b"{not-json\n").unwrap();
        state.sync_all().unwrap();

        let mut registry = load_guarded_registry(project.path(), home.path());
        let prompt = registry.build_system_prompt();
        assert!(prompt.contains("USER_SCOPE_MARKER"));
        assert!(!prompt.contains("PROJECT_SCOPE_MARKER"));
        let health = registry.loading_health.snapshot();
        let project_health = health
            .iter()
            .find(|scope| scope.scope == "project")
            .unwrap();
        assert!(project_health.is_blocked());
        assert!(
            project_health
                .summary()
                .contains("invalid_maintenance_record")
        );
        assert!(
            health
                .iter()
                .any(|scope| scope.scope == "user" && !scope.is_blocked())
        );
        std::fs::write(&state_path, original).unwrap();
        registry.load_skills_subset_with_home(project.path(), home.path(), &[]);
        assert!(
            registry
                .loading_health
                .snapshot()
                .iter()
                .all(|scope| !scope.is_blocked())
        );
        assert!(
            registry
                .build_system_prompt()
                .contains("PROJECT_SCOPE_MARKER")
        );
    }

    #[cfg(unix)]
    #[test]
    fn guarded_skill_loading_holds_locks_through_publication() {
        use omegon_maintenance_contracts::{LockMode, MaintenanceStateV1, ProtocolLock};

        let home_path = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        write_skill(&home_path.path().join("skills"), "user", "USER_MARKER");
        write_skill(
            &project.path().join(".omegon/skills"),
            "project",
            "PROJECT_MARKER",
        );
        let user_authority = skill_scope_key(home_path.path().join("skills"), "user");
        let project_authority = skill_scope_key(project.path().join(".omegon/skills"), "project");
        let home = omegon_maintenance_contracts::open_secure_root(home_path.path()).unwrap();
        let state = MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();

        let result = AugmentRegistry::with_guarded_skills(
            project.path(),
            home_path.path(),
            &[],
            SkillConflictResolution::MostRecent,
            |result| {
                for authority in [user_authority, project_authority] {
                    let lock_name = format!("contribution-{authority}.lock");
                    assert!(
                        ProtocolLock::acquire_at(
                            &state.locks,
                            lock_name.as_bytes(),
                            LockMode::Exclusive,
                            false,
                            true,
                        )
                        .is_err()
                    );
                }
                result
            },
        );
        assert!(result.snapshots.iter().any(|skill| skill.name == "user"));
        assert!(result.snapshots.iter().any(|skill| skill.name == "project"));
        assert!(
            result
                .snapshots
                .iter()
                .any(|skill| skill.source == "bundled")
        );
        for authority in [user_authority, project_authority] {
            let lock_name = format!("contribution-{authority}.lock");
            assert!(
                ProtocolLock::acquire_at(
                    &state.locks,
                    lock_name.as_bytes(),
                    LockMode::Exclusive,
                    false,
                    true,
                )
                .is_ok()
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn guarded_skill_loading_skips_opaque_basenames_without_collision() {
        use std::os::unix::ffi::OsStringExt;

        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let skills = project.path().join(".omegon/skills");
        write_skill(&skills, "valid", "VALID_SKILL_MARKER");
        for raw_name in [vec![b'o', 0x80], vec![b'o', 0x81]] {
            let directory = skills.join(std::ffi::OsString::from_vec(raw_name));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("SKILL.md"), "OPAQUE_SKILL_MARKER").unwrap();
        }

        let registry = load_guarded_registry(project.path(), home.path());
        let prompt = registry.build_system_prompt();
        assert_eq!(
            registry
                .loaded_skill_snapshots
                .iter()
                .filter(|skill| skill.source != "bundled")
                .count(),
            1
        );
        assert!(prompt.contains("VALID_SKILL_MARKER"));
        assert!(!prompt.contains("OPAQUE_SKILL_MARKER"));
    }

    #[cfg(unix)]
    fn load_guarded_registry(project: &std::path::Path, home: &std::path::Path) -> AugmentRegistry {
        let mut registry = AugmentRegistry::new(LEX.into());
        registry.load_skills_subset_with_home(project, home, &[]);
        registry
    }

    #[cfg(unix)]
    fn initialize_skill_scope(
        root: &std::path::Path,
        components: &[&[u8]],
        home: &std::path::Path,
        scope: &str,
    ) -> omegon_maintenance_contracts::AuthorityKey {
        GuardedContributionDirectory::open(
            root,
            components,
            home,
            omegon_maintenance_contracts::ContributionKind::Skill,
            scope,
        )
        .unwrap()
        .unwrap()
        .scope_key()
    }

    #[cfg(unix)]
    fn skill_scope_key(
        directory: std::path::PathBuf,
        scope: &str,
    ) -> omegon_maintenance_contracts::AuthorityKey {
        let directory = std::fs::File::open(directory).unwrap();
        let parent = omegon_maintenance_contracts::path_identity(&directory).unwrap();
        omegon_maintenance_contracts::scope_key(
            omegon_maintenance_contracts::ContributionKind::Skill.as_str(),
            scope,
            parent.key,
        )
    }

    #[cfg(unix)]
    fn deny_skill(
        root: &std::path::Path,
        components: &[&[u8]],
        home_path: &std::path::Path,
        scope: &str,
        raw_name: &[u8],
    ) {
        use omegon_maintenance_contracts::{
            AuthorityKey, ContributionKind, DenyRecordV1, DenyState, DenyStateV1, SCHEMA_VERSION,
            derive_key, entry_key, open_secure_dir_at, replace_record_at,
        };
        use sha2::{Digest, Sha256};

        let authority = initialize_skill_scope(root, components, home_path, scope);
        let home = omegon_maintenance_contracts::open_secure_root(home_path).unwrap();
        let state = omegon_maintenance_contracts::MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();
        let deny_directory = open_secure_dir_at(&state.deny, authority.to_hex().as_bytes())
            .unwrap()
            .unwrap();
        let kind = ContributionKind::Skill;
        let entry = entry_key(kind.as_str(), authority, raw_name);
        let request_id = "00000000-0000-0000-0000-000000000001";
        let record = DenyRecordV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny".into(),
            record_id: derive_key(
                "deny",
                &[
                    authority.as_bytes(),
                    entry.as_bytes(),
                    request_id.as_bytes(),
                ],
            ),
            scope_key: authority,
            contribution_kind: kind,
            entry_key: entry,
            raw_name_digest: AuthorityKey::from_bytes(Sha256::digest(raw_name).into()),
            generation: 1,
            state: DenyState::Denied,
            request_id: request_id.into(),
            created_at: "2026-08-17T00:00:00Z".into(),
        };
        let deny = DenyStateV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny_state".into(),
            record_id: derive_key("deny-state", &[authority.as_bytes(), &1_u64.to_be_bytes()]),
            scope_key: authority,
            generation: 1,
            entries: [(entry.to_hex(), record)].into(),
        };
        replace_record_at(&deny_directory, b"state.json", &deny, "deny-skill-test").unwrap();
    }

    #[test]
    fn disclosed_prompt_withholds_unmatched_skills_but_keeps_them_discoverable() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");

        // A signal-gated skill whose marker is absent from the workspace.
        // Body is padded to a realistic size: real bundled skills run 5–10 KB,
        // and the index header is a fixed ~250-byte cost paid once no matter how
        // many skills are withheld. A toy-sized body would not clear that floor.
        let ts_body = format!("TS BODY MARKER\n\n{}", "ts filler line\n".repeat(200));
        write_skill(
            &skills,
            "typescript",
            &format!(
                "+++\nname = \"typescript\"\ndescription = \"Conventions for TypeScript code and tooling\"\nactivation = \"project_detected\"\nproject_signals = [\"tsconfig.json\"]\n+++\n\n{ts_body}"
            ),
        );
        // A signal-gated skill whose marker is present.
        write_skill(
            &skills,
            "rust",
            "+++\nname = \"rust\"\ndescription = \"Conventions for Rust code and tooling\"\nactivation = \"project_detected\"\nproject_signals = [\"Cargo.toml\"]\n+++\n\nRUST BODY MARKER",
        );
        // An always-on skill.
        write_skill(
            &skills,
            "security",
            "+++\nname = \"security\"\ndescription = \"Defensive coding practices for implementation and review\"\nactivation = \"always\"\n+++\n\nSECURITY BODY MARKER",
        );

        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("Cargo.toml"), "[package]\n").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[skills]);
        assert_eq!(reg.skill_count(), 3);

        let full = reg.build_system_prompt();
        let disclosed = reg.build_system_prompt_disclosed(&workspace, None);

        // Every body is present without disclosure.
        for marker in ["TS BODY MARKER", "RUST BODY MARKER", "SECURITY BODY MARKER"] {
            assert!(full.contains(marker), "baseline must contain {marker}");
        }

        // Evidence-backed skills keep their bodies.
        assert!(disclosed.contains("RUST BODY MARKER"));
        assert!(disclosed.contains("SECURITY BODY MARKER"));

        // The unmatched skill loses its body but stays discoverable by name and
        // description — this is the property that makes withholding safe.
        assert!(!disclosed.contains("TS BODY MARKER"));
        assert!(disclosed.contains("# Available skills (not loaded)"));
        assert!(
            disclosed.contains("- typescript — Conventions for TypeScript code and tooling"),
            "withheld skill must remain retrievable:\n{disclosed}"
        );
        assert!(disclosed.contains("skills_get"));

        // Disclosure must strictly shrink the prompt once withheld bodies clear
        // the fixed cost of the index header.
        assert!(
            disclosed.len() < full.len(),
            "disclosed {} !< full {}",
            disclosed.len(),
            full.len()
        );
        // And the saving should be most of the withheld body, not a rounding error.
        assert!(
            full.len() - disclosed.len() > 2_000,
            "expected substantial savings, got {}",
            full.len() - disclosed.len()
        );
    }

    #[test]
    fn disclosed_prompt_admits_on_prompt_trigger_and_preserves_persona_layers() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        write_skill(
            &skills,
            "style",
            "+++\nname = \"style\"\ndescription = \"Canonical design system tokens for visual output\"\nactivation = \"domain_detected\"\ntriggers = [\"diagram\"]\n+++\n\nSTYLE BODY MARKER",
        );

        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[skills]);

        // No evidence, no trigger: withheld.
        let quiet = reg.build_system_prompt_disclosed(&workspace, None);
        assert!(!quiet.contains("STYLE BODY MARKER"));

        // The operator names the trigger: admitted on this turn.
        let asked = reg.build_system_prompt_disclosed(&workspace, Some("draw me a diagram"));
        assert!(
            asked.contains("STYLE BODY MARKER"),
            "prompt trigger must admit the body:\n{asked}"
        );
        // Admitting the only withheld skill removes the index entirely.
        assert!(!asked.contains("# Available skills (not loaded)"));
    }

    /// The load-bearing property of progressive disclosure: a withheld body must
    /// still be *reachable*. This walks the full round trip — withhold, read the
    /// advertised name out of the index, resolve that name from disk, and
    /// confirm the body comes back intact.
    #[test]
    fn a_withheld_skill_is_retrievable_by_the_name_the_index_advertises() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let dir = skills_dir.join("typescript");
        std::fs::create_dir_all(&dir).unwrap();
        let filler = "filler line\n".repeat(200);
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "+++\nname = \"typescript\"\ndescription = \"Conventions for TypeScript code and tooling\"\nactivation = \"project_detected\"\nproject_signals = [\"tsconfig.json\"]\n+++\n\nTS BODY MARKER\n\n{filler}"
            ),
        )
        .unwrap();

        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(std::slice::from_ref(&skills_dir));
        let disclosed = reg.build_system_prompt_disclosed(&workspace, None);

        // The body is withheld.
        assert!(!disclosed.contains("TS BODY MARKER"));

        // Recover the advertised name exactly as a model would read it: from
        // inside the index section, not from arbitrary bullets elsewhere in the
        // prompt.
        let index_section = disclosed
            .split_once("# Available skills (not loaded)")
            .expect("index header must be present")
            .1;
        let advertised = index_section
            .lines()
            .find_map(|line| line.strip_prefix("- ")?.split(" — ").next())
            .expect("index must advertise at least one skill name")
            .to_string();
        assert_eq!(advertised, "typescript");

        // That name must resolve on disk — this is the step that fails if the
        // index ever advertises a manifest name that diverges from the directory.
        let resolved = skills_dir.join(&advertised).join("SKILL.md");
        assert!(
            resolved.exists(),
            "skills_get resolves by directory name; `{advertised}` did not resolve"
        );
        let (manifest, body) =
            omegon_skills::parse_skill_file(&std::fs::read_to_string(&resolved).unwrap());
        assert_eq!(manifest.name, advertised);
        assert!(
            body.contains("TS BODY MARKER"),
            "retrieval must return the body that disclosure withheld"
        );
    }

    #[test]
    fn disclosure_never_drops_a_skill_with_an_unparseable_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("skills");
        // No frontmatter at all — disclosure cannot judge it.
        write_skill(&skills, "legacy", "ORPHAN BODY MARKER");

        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[skills]);

        let disclosed = reg.build_system_prompt_disclosed(&workspace, None);
        assert!(
            disclosed.contains("ORPHAN BODY MARKER"),
            "an unjudgeable skill must be admitted, not silently dropped:\n{disclosed}"
        );
    }

    #[test]
    fn load_skills_from_project_local_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# My Skill\nDo the thing.").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[tmp.path().join("skills")]);

        assert_eq!(reg.skill_count(), 1);
        assert!(reg.build_system_prompt().contains("Do the thing."));
    }

    #[test]
    fn skills_appear_between_lex_and_persona() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "SKILL_MARKER").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[tmp.path().join("skills")]);
        reg.activate_persona(engineer_persona());

        let prompt = reg.build_system_prompt();
        let lex_pos = prompt.find("Lex Imperialis").unwrap();
        let skill_pos = prompt.find("SKILL_MARKER").unwrap();
        let persona_pos = prompt
            .find("You are a systems engineering harness.")
            .unwrap();
        assert!(lex_pos < skill_pos, "skill should follow lex");
        assert!(skill_pos < persona_pos, "persona should follow skill");
    }

    #[test]
    fn activation_conflicts_keep_most_recent_skill_only() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled").join("rust");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(
            bundled.join("SKILL.md"),
            "---
name: rust
description: Bundled Rust
activation: project_detected
profile: [coding]
project_signals: [Cargo.toml]
---

BUNDLED_RUST_MARKER
",
        )
        .unwrap();
        let extension = tmp.path().join("extension").join("recro-rust-dev");
        std::fs::create_dir_all(&extension).unwrap();
        std::fs::write(
            extension.join("SKILL.md"),
            "---
name: recro-rust-dev
description: Recro Rust
activation: project_detected
profile: [coding]
project_signals: [Cargo.toml]
---

RECRO_RUST_MARKER
",
        )
        .unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[tmp.path().join("bundled"), tmp.path().join("extension")]);

        let prompt = reg.build_system_prompt();
        assert_eq!(reg.skill_count(), 1);
        assert!(prompt.contains("RECRO_RUST_MARKER"));
        assert!(!prompt.contains("BUNDLED_RUST_MARKER"));
    }

    #[test]
    fn activation_conflict_error_policy_drops_all_participants() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first").join("rust");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(
            first.join("SKILL.md"),
            "---
name: rust
description: Rust
activation: intent_detected
triggers: [rust]
---

FIRST_MARKER
",
        )
        .unwrap();
        let second = tmp.path().join("second").join("recro-rust-dev");
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(
            second.join("SKILL.md"),
            "---
name: recro-rust-dev
description: Recro Rust
activation: intent_detected
triggers: [rust]
---

SECOND_MARKER
",
        )
        .unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.set_skill_conflict_resolution(SkillConflictResolution::Error);
        reg.load_skills_from_explicit(&[tmp.path().join("first"), tmp.path().join("second")]);

        let prompt = reg.build_system_prompt();
        assert_eq!(reg.skill_count(), 0);
        assert!(!prompt.contains("FIRST_MARKER"));
        assert!(!prompt.contains("SECOND_MARKER"));
    }

    #[test]
    fn later_skill_dirs_override_same_named_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let user_dir = tmp.path().join("user").join("shared");
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(user_dir.join("SKILL.md"), "USER_SHARED_MARKER").unwrap();
        let project_dir = tmp.path().join("project").join("shared");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("SKILL.md"), "PROJECT_SHARED_MARKER").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[tmp.path().join("user"), tmp.path().join("project")]);

        let prompt = reg.build_system_prompt();
        assert_eq!(reg.skill_count(), 1);
        assert!(prompt.contains("PROJECT_SHARED_MARKER"));
        assert!(!prompt.contains("USER_SHARED_MARKER"));
    }

    #[test]
    fn empty_skill_files_not_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("empty-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "   \n   ").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_from_explicit(&[tmp.path().join("skills")]);
        assert_eq!(reg.skill_count(), 0);
    }

    #[test]
    fn load_skills_subset_filters_by_skill_name() {
        let tmp = tempfile::tempdir().unwrap();
        let rust_dir = tmp.path().join("skills").join("rust");
        std::fs::create_dir_all(&rust_dir).unwrap();
        std::fs::write(rust_dir.join("SKILL.md"), "# Rust\nUse cargo test.").unwrap();
        let security_dir = tmp.path().join("skills").join("security");
        std::fs::create_dir_all(&security_dir).unwrap();
        std::fs::write(security_dir.join("SKILL.md"), "# Security\nValidate input.").unwrap();

        let mut reg = AugmentRegistry::new(LEX.into());
        reg.load_skills_subset_from_explicit(
            &[tmp.path().join("skills")],
            &["security".to_string()],
        );

        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Validate input."));
        assert!(!prompt.contains("Use cargo test."));
    }

    #[test]
    fn missing_skills_dir_is_silent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reg = AugmentRegistry::new(LEX.into());
        // Pass a nonexistent dir — should load nothing, not panic
        reg.load_skills_from_explicit(&[tmp.path().join("nonexistent").join("skills")]);
        assert_eq!(reg.skill_count(), 0);
    }
}
