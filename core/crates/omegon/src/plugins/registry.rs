//! Plugin registry — manages active personas, tones, and the memory layer stack.
//!
//! This is the runtime counterpart to the armory manifest parser.
//! It handles persona activation/deactivation, tone switching,
//! memory layer isolation, and system prompt assembly.
//!
//! Invariant: the Lex Imperialis is always present and always first.

use serde::{Deserialize, Serialize};

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

/// The plugin registry — manages active persona, tone, and system prompt assembly.
///
/// Invariant: `lex_imperialis` is always injected first in the system prompt.
/// No operation can remove or reorder it.
pub struct PluginRegistry {
    lex_imperialis: String,
    active_persona: Option<LoadedPersona>,
    active_tone: Option<LoadedTone>,
    memory: MemoryLayers,
    /// Skill directives loaded from ~/.omegon/skills/ and .omegon/skills/.
    /// Project-local (.omegon/skills/) entries follow bundled ones; last writer on
    /// same name wins, so project-local overrides bundled.
    loaded_skills: Vec<String>,
}

impl PluginRegistry {
    /// Create a new registry. The Lex Imperialis content is required and immutable.
    pub fn new(lex_imperialis: String) -> Self {
        Self {
            lex_imperialis,
            active_persona: None,
            active_tone: None,
            memory: MemoryLayers::default(),
            loaded_skills: Vec::new(),
        }
    }

    /// Load skills from the two canonical locations:
    ///   1. `~/.omegon/skills/<name>/SKILL.md`  — bundled / user-installed
    ///   2. `<cwd>/.omegon/skills/<name>/SKILL.md` — project-local (appended last,
    ///      so project-local content follows bundled in the prompt)
    ///
    /// Call once at session start. Silently skips missing directories.
    pub fn load_skills(&mut self, cwd: &std::path::Path) {
        self.load_skills_subset(cwd, &[]);
    }

    /// Load only a named subset of skills from the canonical locations.
    /// When `allowed` is empty, behaves like `load_skills` and loads all skills.
    pub fn load_skills_subset(&mut self, cwd: &std::path::Path, allowed: &[String]) {
        let bundled = dirs::home_dir().map(|h| h.join(".omegon").join("skills"));
        let project = cwd.join(".omegon").join("skills");
        let dirs: Vec<std::path::PathBuf> = bundled
            .into_iter()
            .chain(std::iter::once(project))
            .collect();
        self.loaded_skills = Self::load_from_dirs_filtered(&dirs, allowed);
    }

    /// Load skill content from an explicit list of directories.
    /// Used by `load_skills` in production and directly by tests to avoid
    /// reading from the real ~/.omegon/skills/ installation.
    fn load_from_dirs(dirs: &[std::path::PathBuf]) -> Vec<String> {
        Self::load_from_dirs_filtered(dirs, &[])
    }

    fn load_from_dirs_filtered(dirs: &[std::path::PathBuf], allowed: &[String]) -> Vec<String> {
        let mut skills = Vec::new();
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
                if let Ok(content) = std::fs::read_to_string(&skill_file) {
                    if !content.trim().is_empty() {
                        skills.push(content);
                    }
                }
            }
        }
        skills
    }

    /// Return the number of loaded skills.
    pub fn skill_count(&self) -> usize {
        self.loaded_skills.len()
    }

    /// Test-only: load skills from an explicit list of directories,
    /// bypassing the real ~/.omegon/skills/ path.
    #[cfg(test)]
    fn load_skills_from_explicit(&mut self, dirs: &[std::path::PathBuf]) {
        self.loaded_skills = Self::load_from_dirs(dirs);
    }

    #[cfg(test)]
    fn load_skills_subset_from_explicit(
        &mut self,
        dirs: &[std::path::PathBuf],
        allowed: &[String],
    ) {
        self.loaded_skills = Self::load_from_dirs_filtered(dirs, allowed);
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
    pub fn build_system_prompt(&self) -> String {
        let mut layers = vec![self.lex_imperialis.as_str()];

        for skill in &self.loaded_skills {
            layers.push(skill.as_str());
        }

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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn activate_persona_loads_mind_facts() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        let prompt = reg.build_system_prompt();
        let lex_pos = prompt.find("Lex Imperialis").unwrap();
        let persona_pos = prompt.find("Socratic Tutor").unwrap();
        assert!(lex_pos < persona_pos);
    }

    #[test]
    fn first_activation_returns_no_previous() {
        let mut reg = PluginRegistry::new(LEX.into());
        let result = reg.activate_persona(tutor_persona());
        assert!(result.previous_id.is_none());
    }

    // ── Persona deactivation ─────────────────────────────────

    #[test]
    fn deactivate_removes_directive_from_prompt() {
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.deactivate_persona();

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn deactivate_clears_persona_memory() {
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        assert!(!reg.memory().persona.is_empty());

        reg.deactivate_persona();
        assert!(reg.memory().persona.is_empty());
    }

    #[test]
    fn deactivate_preserves_project_memory() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        let result = reg.deactivate_persona();

        assert!(result.removed_id.as_deref().unwrap().contains("tutor"));
        assert!(result.facts_removed > 0);
    }

    #[test]
    fn deactivate_noop_when_none_active() {
        let mut reg = PluginRegistry::new(LEX.into());
        let result = reg.deactivate_persona();
        assert!(result.removed_id.is_none());
        assert_eq!(result.facts_removed, 0);
    }

    // ── Persona switching ────────────────────────────────────

    #[test]
    fn switch_replaces_directive() {
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        reg.activate_persona(engineer_persona());

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Socratic Tutor"));
        assert!(prompt.contains("Systems Engineer"));
    }

    #[test]
    fn switch_replaces_mind_facts() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());
        let result = reg.activate_persona(engineer_persona());
        assert!(result.previous_id.as_deref().unwrap().contains("tutor"));
    }

    #[test]
    fn switch_preserves_project_memory() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());

        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Alan Watts"));
        assert!(prompt.contains("Lex Imperialis"));
    }

    #[test]
    fn tone_switch_replaces() {
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());
        reg.activate_tone(concise_tone());

        let prompt = reg.build_system_prompt();
        assert!(!prompt.contains("Alan Watts"));
        assert!(prompt.contains("Concise"));
    }

    #[test]
    fn tone_deactivate_removes() {
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_tone(watts_tone());
        reg.deactivate_tone();
        assert!(!reg.build_system_prompt().contains("Alan Watts"));
    }

    // ── Memory layer isolation ───────────────────────────────

    #[test]
    fn query_all_merges_in_priority_order() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        reg.activate_persona(tutor_persona());

        assert!(!reg.memory().persona.is_empty());
        assert!(reg.memory().project.is_empty());
    }

    #[test]
    fn store_persona_fact_fails_without_active_persona() {
        let mut reg = PluginRegistry::new(LEX.into());
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
        let reg = PluginRegistry::new(LEX.into());
        let prompt = reg.build_system_prompt();
        assert!(prompt.contains("Lex Imperialis"));
        assert!(prompt.contains("Anti-Sycophancy"));
    }

    #[test]
    fn lex_survives_all_transitions() {
        let mut reg = PluginRegistry::new(LEX.into());

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
        let reg = PluginRegistry::new(LEX.into());
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

    #[test]
    fn load_skills_from_project_local_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# My Skill\nDo the thing.").unwrap();

        let mut reg = PluginRegistry::new(LEX.into());
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

        let mut reg = PluginRegistry::new(LEX.into());
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
    fn empty_skill_files_not_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("empty-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "   \n   ").unwrap();

        let mut reg = PluginRegistry::new(LEX.into());
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

        let mut reg = PluginRegistry::new(LEX.into());
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
        let mut reg = PluginRegistry::new(LEX.into());
        // Pass a nonexistent dir — should load nothing, not panic
        reg.load_skills_from_explicit(&[tmp.path().join("nonexistent").join("skills")]);
        assert_eq!(reg.skill_count(), 0);
    }
}
