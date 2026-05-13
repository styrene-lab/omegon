//! Agent manifest loader — parses `agent.pkl` or `agent.toml` from a
//! catalog bundle directory and resolves file references.
//!
//! An agent manifest declares everything needed to deploy a purpose-built
//! agent: domain, persona, extensions, settings, workflow, secrets, and
//! triggers. Auspex reads these from a catalog to spawn agents.

use std::path::{Path, PathBuf};

use serde::Deserialize;

// ── Manifest structs ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct AgentManifest {
    pub agent: AgentMeta,
    pub persona: Option<PersonaConfig>,
    pub extensions: Option<Vec<ExtensionDep>>,
    pub settings: Option<SettingsConfig>,
    pub workflow: Option<WorkflowConfig>,
    pub secrets: Option<SecretsConfig>,
    pub triggers: Option<Vec<TriggerDef>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub domain: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonaConfig {
    pub directive: Option<String>,
    pub directive_inline: Option<String>,
    pub badge: Option<String>,
    /// One or more JSONL facts files (relative to bundle dir).
    /// Accepts both `mind_facts = "path"` (legacy TOML string) and
    /// `mind_facts = ["path1", "path2"]` (array / Pkl Listing).
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub mind_facts: Option<Vec<String>>,
    /// Additional persona directive files appended after `directive`.
    /// Used by user overlays to extend a base agent's persona without
    /// replacing it.
    #[serde(default)]
    pub directive_extend: Option<Vec<String>>,
    pub activated_skills: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionDep {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "*".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsConfig {
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowConfig {
    pub name: String,
    pub phases: Option<std::collections::HashMap<String, PhaseConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhaseConfig {
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub persona: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecretsConfig {
    pub required: Option<Vec<String>>,
    pub optional: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TriggerDef {
    pub name: String,
    pub schedule: Option<String>,
    pub interval: Option<String>,
    pub template: String,
}

/// Deserialize a field that may be either a single string or a list of strings.
/// Handles TOML `mind_facts = "path"` and Pkl `mind_facts { "path" }` equally.
fn deserialize_string_or_vec<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;

    struct StringOrVec;

    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Option<Vec<String>>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a string or list of strings")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            d.deserialize_any(StringOrVecInner)
        }
    }

    struct StringOrVecInner;

    impl<'de> Visitor<'de> for StringOrVecInner {
        type Value = Option<Vec<String>>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a string or list of strings")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(vec![v.to_owned()]))
        }

        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(vec![v]))
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            Ok(if items.is_empty() { None } else { Some(items) })
        }
    }

    d.deserialize_option(StringOrVec)
}

// ── Resolved manifest ────────────────────────────────────────────────────

/// A manifest with all file references resolved to absolute content.
#[derive(Debug, Clone)]
pub struct ResolvedManifest {
    pub manifest: AgentManifest,
    pub bundle_dir: PathBuf,
    /// Resolved persona directive text (from file or inline), with any
    /// `directive_extend` files appended after a separator.
    pub persona_directive: Option<String>,
    /// Resolved mind facts JSONL content — all `mind_facts` files
    /// concatenated in declaration order.
    pub mind_facts_content: Option<String>,
}

// ── Loading ──────────────────────────────────────────────────────────────

fn pkl_available() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::process::Command::new("pkl")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Load an agent manifest from a bundle directory.
///
/// Prefers `agent.pkl` when the `pkl` binary is available; falls back to
/// `agent.toml` otherwise. Resolves all file references before returning.
pub fn load(bundle_dir: &Path) -> anyhow::Result<ResolvedManifest> {
    let pkl_path = bundle_dir.join("agent.pkl");
    let toml_path = bundle_dir.join("agent.toml");

    let manifest: AgentManifest = if pkl_path.exists() && pkl_available() {
        rpkl::from_config_with_options(&pkl_path, crate::pkl_modules::omegon_eval_options())
            .map_err(|e| anyhow::anyhow!("agent.pkl: {e}"))?
    } else if toml_path.exists() {
        let content = std::fs::read_to_string(&toml_path)?;
        toml::from_str(&content)?
    } else if pkl_path.exists() {
        // pkl exists but binary unavailable — surface a clear error rather
        // than silently ignoring the intended format.
        anyhow::bail!(
            "agent.pkl found in {} but the pkl binary is not installed. \
             Install it (brew install pkl) or provide an agent.toml fallback.",
            bundle_dir.display()
        );
    } else {
        anyhow::bail!(
            "no agent manifest found in {}. Expected agent.pkl or agent.toml",
            bundle_dir.display()
        );
    };

    resolve(manifest, bundle_dir)
}

/// Resolve file references in a manifest.
fn resolve(manifest: AgentManifest, bundle_dir: &Path) -> anyhow::Result<ResolvedManifest> {
    let persona_directive = if let Some(ref persona) = manifest.persona {
        let base = if let Some(ref path) = persona.directive {
            let full = bundle_dir.join(path);
            Some(
                std::fs::read_to_string(&full)
                    .map_err(|e| anyhow::anyhow!("persona directive {}: {e}", full.display()))?,
            )
        } else {
            persona.directive_inline.clone()
        };

        // Append directive_extend files after the base directive.
        if let Some(ref extend_paths) = persona.directive_extend {
            let mut parts: Vec<String> = base.into_iter().collect();
            for path in extend_paths {
                let full = bundle_dir.join(path);
                let content = std::fs::read_to_string(&full)
                    .map_err(|e| anyhow::anyhow!("directive_extend {}: {e}", full.display()))?;
                parts.push(content);
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n\n---\n\n"))
            }
        } else {
            base
        }
    } else {
        None
    };

    let mind_facts_content = if let Some(ref persona) = manifest.persona {
        if let Some(ref paths) = persona.mind_facts {
            let mut all = String::new();
            for path in paths {
                let full = bundle_dir.join(path);
                let content = std::fs::read_to_string(&full)
                    .map_err(|e| anyhow::anyhow!("mind facts {}: {e}", full.display()))?;
                if !all.is_empty() && !all.ends_with('\n') {
                    all.push('\n');
                }
                all.push_str(&content);
            }
            if all.is_empty() { None } else { Some(all) }
        } else {
            None
        }
    } else {
        None
    };

    Ok(ResolvedManifest {
        manifest,
        bundle_dir: bundle_dir.to_path_buf(),
        persona_directive,
        mind_facts_content,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_toml_manifest() {
        let toml_str = r#"
[agent]
id = "test.coding-agent"
name = "Test Coder"
version = "1.0.0"
domain = "coding"

[persona]
directive_inline = "You are a test agent."
badge = "T"

[settings]
model = "anthropic:claude-sonnet-4-6"
thinking_level = "medium"
max_turns = 30

[secrets]
required = ["ANTHROPIC_API_KEY"]
optional = ["GITHUB_TOKEN"]

[[triggers]]
name = "hourly-check"
schedule = "hourly"
template = "Run status check."
"#;
        let manifest: AgentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.agent.id, "test.coding-agent");
        assert_eq!(manifest.agent.domain, "coding");
        assert_eq!(
            manifest
                .persona
                .as_ref()
                .unwrap()
                .directive_inline
                .as_deref(),
            Some("You are a test agent.")
        );
        assert_eq!(
            manifest
                .settings
                .as_ref()
                .unwrap()
                .thinking_level
                .as_deref(),
            Some("medium")
        );
        assert_eq!(manifest.triggers.as_ref().unwrap().len(), 1);
        assert_eq!(manifest.triggers.as_ref().unwrap()[0].name, "hourly-check");
    }

    #[test]
    fn parse_toml_with_workflow() {
        let toml_str = r#"
[agent]
id = "test.infra"
name = "Infra"
version = "0.1.0"
domain = "infra"

[workflow]
name = "ops-standard"

[workflow.phases.exploring]
model = "anthropic:claude-opus-4-6"
max_turns = 20
thinking_level = "high"

[workflow.phases.implementing]
model = "anthropic:claude-sonnet-4-6"
max_turns = 50
"#;
        let manifest: AgentManifest = toml::from_str(toml_str).unwrap();
        let wf = manifest.workflow.as_ref().unwrap();
        assert_eq!(wf.name, "ops-standard");
        let phases = wf.phases.as_ref().unwrap();
        assert_eq!(phases.len(), 2);
        assert_eq!(
            phases["exploring"].model.as_deref(),
            Some("anthropic:claude-opus-4-6")
        );
    }

    #[test]
    fn parse_toml_with_extensions() {
        let toml_str = r#"
[agent]
id = "test.ext"
name = "Ext"
version = "0.1.0"
domain = "coding"

[[extensions]]
name = "vox"
version = ">=0.3.0"

[[extensions]]
name = "scribe"
"#;
        let manifest: AgentManifest = toml::from_str(toml_str).unwrap();
        let exts = manifest.extensions.as_ref().unwrap();
        assert_eq!(exts.len(), 2);
        assert_eq!(exts[0].name, "vox");
        assert_eq!(exts[0].version, ">=0.3.0");
        assert_eq!(exts[1].name, "scribe");
        assert_eq!(exts[1].version, "*");
    }

    #[test]
    fn mind_facts_string_compat() {
        // Legacy TOML: single string value must deserialize to a one-element vec.
        let toml_str = r#"
[agent]
id = "test.agent"
name = "T"
version = "1.0.0"
domain = "coding"

[persona]
mind_facts = "mind/facts.jsonl"
"#;
        let manifest: AgentManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(
            manifest.persona.as_ref().unwrap().mind_facts.as_deref(),
            Some([String::from("mind/facts.jsonl")].as_slice())
        );
    }

    #[test]
    fn mind_facts_array() {
        let toml_str = r#"
[agent]
id = "test.agent"
name = "T"
version = "1.0.0"
domain = "coding"

[persona]
mind_facts = ["mind/base.jsonl", "mind/personal.jsonl"]
"#;
        let manifest: AgentManifest = toml::from_str(toml_str).unwrap();
        let facts = manifest
            .persona
            .as_ref()
            .unwrap()
            .mind_facts
            .as_ref()
            .unwrap();
        assert_eq!(facts, &["mind/base.jsonl", "mind/personal.jsonl"]);
    }

    #[test]
    fn load_from_bundle_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("agent.toml"),
            r#"
[agent]
id = "test.bundle"
name = "Bundle"
version = "1.0.0"
domain = "chat"

[persona]
directive = "PERSONA.md"
mind_facts = "mind/facts.jsonl"
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("PERSONA.md"), "You are helpful.").unwrap();
        std::fs::create_dir_all(dir.path().join("mind")).unwrap();
        std::fs::write(
            dir.path().join("mind/facts.jsonl"),
            r#"{"section":"test","content":"fact one"}"#,
        )
        .unwrap();

        let resolved = load(dir.path()).unwrap();
        assert_eq!(resolved.manifest.agent.id, "test.bundle");
        assert_eq!(
            resolved.persona_directive.as_deref(),
            Some("You are helpful.")
        );
        assert!(resolved.mind_facts_content.is_some());
    }

    #[test]
    fn load_multiple_mind_facts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("agent.toml"),
            r#"
[agent]
id = "test.multi-facts"
name = "Multi"
version = "1.0.0"
domain = "ops"

[persona]
mind_facts = ["mind/base.jsonl", "mind/personal.jsonl"]
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("mind")).unwrap();
        std::fs::write(
            dir.path().join("mind/base.jsonl"),
            "{\"section\":\"base\",\"content\":\"fact one\"}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("mind/personal.jsonl"),
            "{\"section\":\"personal\",\"content\":\"fact two\"}\n",
        )
        .unwrap();

        let resolved = load(dir.path()).unwrap();
        let facts = resolved.mind_facts_content.unwrap();
        assert!(facts.contains("fact one"));
        assert!(facts.contains("fact two"));
    }

    #[test]
    fn load_directive_extend() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("agent.toml"),
            r#"
[agent]
id = "test.extend"
name = "Extend"
version = "1.0.0"
domain = "ops"

[persona]
directive = "PERSONA.md"
directive_extend = ["PERSONA.personal.md"]
"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("PERSONA.md"), "Base persona.").unwrap();
        std::fs::write(
            dir.path().join("PERSONA.personal.md"),
            "Personal additions.",
        )
        .unwrap();

        let resolved = load(dir.path()).unwrap();
        let directive = resolved.persona_directive.unwrap();
        assert!(directive.contains("Base persona."));
        assert!(directive.contains("Personal additions."));
        assert!(directive.contains("---"));
    }

    fn has_pkl() -> bool {
        std::process::Command::new("pkl")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn load_pkl_manifest() {
        if !has_pkl() {
            eprintln!("skipping: pkl binary not found");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("agent.pkl"),
            r#"
agent {
  id = "test.pkl-agent"
  name = "Pkl Agent"
  version = "1.0.0"
  domain = "coding"
}

settings {
  model = "anthropic:claude-sonnet-4-6"
  thinking_level = "low"
}
"#,
        )
        .unwrap();

        let resolved = load(dir.path()).unwrap();
        assert_eq!(resolved.manifest.agent.id, "test.pkl-agent");
        assert_eq!(
            resolved
                .manifest
                .settings
                .as_ref()
                .unwrap()
                .model
                .as_deref(),
            Some("anthropic:claude-sonnet-4-6")
        );
    }
}
