//! Armory plugin manifest — TOML schema for personas, tones, skills, and extensions.
//!
//! This implements the plugin.toml spec from the omegon-armory repo.
//! See: https://github.com/styrene-lab/omegon-armory/blob/main/docs/plugin-spec.md

use omegon_traits::ToolCapability;
use serde::Deserialize;

/// Plugin type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    Persona,
    Tone,
    Skill,
    Extension,
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Persona => write!(f, "persona"),
            Self::Tone => write!(f, "tone"),
            Self::Skill => write!(f, "skill"),
            Self::Extension => write!(f, "extension"),
        }
    }
}

/// Top-level armory plugin manifest (plugin.toml).
#[derive(Debug, Deserialize)]
pub struct ArmoryManifest {
    pub plugin: ArmoryMeta,
    #[serde(default)]
    pub persona: Option<PersonaConfig>,
    #[serde(default)]
    pub tone: Option<ToneConfig>,
    #[serde(default)]
    pub skill: Option<SkillConfig>,
    /// Functional tools — script-backed, HTTP-backed, OCI, or WASM-backed.
    #[serde(default)]
    pub tools: Vec<ToolEntry>,
    /// MCP servers — tools discovered via Model Context Protocol.
    #[serde(default)]
    pub mcp_servers: std::collections::HashMap<String, super::mcp::McpServerConfig>,
    /// Dynamic context injection — script or HTTP endpoint.
    #[serde(default)]
    pub context: Option<ContextEntry>,
    #[serde(default)]
    pub detect: Option<DetectConfig>,
}

/// Tool runner — how the tool is executed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolRunner {
    Python,
    Node,
    Bash,
    /// OCI container execution — podman (preferred) or docker fallback.
    Oci,
    /// WebAssembly sandbox (future).
    Wasm,
}

impl std::fmt::Display for ToolRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::Node => write!(f, "node"),
            Self::Bash => write!(f, "bash"),
            Self::Oci => write!(f, "oci"),
            Self::Wasm => write!(f, "wasm"),
        }
    }
}

/// A tool declaration — can be script-backed, HTTP-backed, or OCI container-backed.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
    /// Tool runner (python/node/bash/oci/wasm). Mutually exclusive with `endpoint`.
    #[serde(default)]
    pub runner: Option<ToolRunner>,
    /// Path to script file, relative to plugin root (for script runners).
    #[serde(default)]
    pub script: Option<String>,
    /// HTTP endpoint URL (mutually exclusive with `runner`).
    #[serde(default)]
    pub endpoint: Option<String>,
    /// HTTP method (default: POST).
    #[serde(default)]
    pub method: Option<String>,
    /// WASM module path, relative to plugin root.
    #[serde(default)]
    pub module: Option<String>,

    // ── OCI-specific fields ──────────────────────────────
    /// OCI image reference (e.g. `ghcr.io/styrene-lab/omegon-tool-drc:latest`).
    #[serde(default)]
    pub image: Option<String>,
    /// Path to Containerfile for local builds (relative to plugin root).
    #[serde(default)]
    pub build: Option<String>,
    /// Mount the operator's working directory into the container (default: false).
    #[serde(default)]
    pub mount_cwd: bool,
    /// Allow container network access (default: false).
    #[serde(default)]
    pub network: bool,

    /// JSON Schema for parameters.
    #[serde(default = "default_params")]
    pub parameters: serde_json::Value,
    /// Explicit behavior capabilities for loop governance.
    #[serde(default)]
    pub capabilities: Vec<ToolCapability>,
    /// Execution timeout in seconds (default: 30).
    #[serde(default = "default_tool_timeout")]
    pub timeout_secs: u64,
}

fn default_params() -> serde_json::Value {
    serde_json::json!({"type": "object", "properties": {}})
}
fn default_tool_timeout() -> u64 {
    30
}

impl ToolEntry {
    /// Is this a script-backed tool (python/node/bash)?
    pub fn is_script(&self) -> bool {
        matches!(
            self.runner,
            Some(ToolRunner::Python | ToolRunner::Node | ToolRunner::Bash)
        ) && self.script.is_some()
    }

    /// Is this an HTTP-backed tool?
    pub fn is_http(&self) -> bool {
        self.endpoint.is_some()
    }

    /// Is this an OCI container-backed tool?
    pub fn is_oci(&self) -> bool {
        self.runner == Some(ToolRunner::Oci) && (self.image.is_some() || self.build.is_some())
    }

    /// Validate the tool entry.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // runner and endpoint are mutually exclusive
        if self.runner.is_some() && self.endpoint.is_some() {
            errors.push(format!(
                "tool '{}': runner and endpoint are mutually exclusive",
                self.name
            ));
        }

        // Must have some execution method
        if self.runner.is_none() && self.endpoint.is_none() {
            errors.push(format!(
                "tool '{}': must have either runner+script/image or endpoint",
                self.name
            ));
        }

        // Runner-specific validation
        if let Some(ref runner) = self.runner {
            match runner {
                ToolRunner::Python | ToolRunner::Node | ToolRunner::Bash => {
                    if self.script.is_none() {
                        errors.push(format!(
                            "tool '{}': {} runner requires a script path",
                            self.name, runner
                        ));
                    }
                }
                ToolRunner::Oci => {
                    if self.image.is_none() && self.build.is_none() {
                        errors.push(format!(
                            "tool '{}': oci runner requires image or build path",
                            self.name
                        ));
                    }
                }
                ToolRunner::Wasm => {
                    if self.module.is_none() {
                        errors.push(format!(
                            "tool '{}': wasm runner requires a module path",
                            self.name
                        ));
                    }
                }
            }
        }

        errors
    }
}

/// Dynamic context entry — generates context at runtime.
#[derive(Debug, Deserialize)]
pub struct ContextEntry {
    /// Script runner for context generation.
    #[serde(default)]
    pub runner: Option<ToolRunner>,
    /// Script path for context generation.
    #[serde(default)]
    pub script: Option<String>,
    /// HTTP endpoint for context retrieval.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// How many turns the context stays active (default: 20).
    #[serde(default = "default_context_ttl")]
    pub ttl_turns: u32,
}

fn default_context_ttl() -> u32 {
    20
}

/// Required metadata for every plugin.
#[derive(Debug, Deserialize)]
pub struct ArmoryMeta {
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    /// Reverse-domain identifier (e.g. `dev.styrene.omegon.tutor`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// One-line description (under 200 chars).
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub min_omegon: Option<String>,
}

/// Persona-specific configuration.
#[derive(Debug, Default, Deserialize)]
pub struct PersonaConfig {
    #[serde(default)]
    pub identity: Option<PersonaIdentity>,
    #[serde(default)]
    pub mind: Option<PersonaMind>,
    #[serde(default)]
    pub skills: Option<PersonaSkills>,
    #[serde(default)]
    pub tools: Option<PersonaTools>,
    #[serde(default)]
    pub routing: Option<PersonaRouting>,
    #[serde(default)]
    pub tone: Option<PersonaTone>,
    #[serde(default)]
    pub style: Option<PersonaStyle>,
}

#[derive(Debug, Deserialize)]
pub struct PersonaIdentity {
    /// Path to PERSONA.md relative to plugin root.
    pub directive: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct PersonaMind {
    /// Path to seed facts file (JSONL).
    #[serde(default)]
    pub seed_facts: Option<String>,
    /// Path to seed episodes file (JSONL).
    #[serde(default)]
    pub seed_episodes: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PersonaSkills {
    #[serde(default)]
    pub activate: Vec<String>,
    #[serde(default)]
    pub deactivate: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PersonaTools {
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PersonaRouting {
    #[serde(default)]
    pub default_thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PersonaTone {
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PersonaStyle {
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
}

/// Tone-specific configuration.
#[derive(Debug, Deserialize)]
pub struct ToneConfig {
    /// Path to TONE.md relative to plugin root.
    pub directive: String,
    /// Path to exemplars directory.
    #[serde(default)]
    pub exemplars: Option<String>,
    #[serde(default)]
    pub intensity: Option<ToneIntensity>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ToneIntensity {
    /// Intensity during design/creative: "full" (default), "muted", "off".
    #[serde(default = "default_full")]
    pub design: String,
    /// Intensity during coding/execution: "full", "muted" (default), "off".
    #[serde(default = "default_muted")]
    pub coding: String,
}

fn default_full() -> String {
    "full".into()
}
fn default_muted() -> String {
    "muted".into()
}

/// Skill-specific configuration.
#[derive(Debug, Deserialize)]
pub struct SkillConfig {
    /// Path to SKILL.md relative to plugin root.
    pub guidance: String,
}

/// Auto-detection configuration.
#[derive(Debug, Default, Deserialize)]
pub struct DetectConfig {
    /// Glob patterns to match project files.
    #[serde(default)]
    pub file_patterns: Vec<String>,
    /// Directory names to match.
    #[serde(default)]
    pub directories: Vec<String>,
    /// If true, this plugin is activated when no other matches.
    #[serde(default)]
    pub default: bool,
}

impl ArmoryManifest {
    /// Parse a plugin.toml from a string.
    pub fn parse(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Validate the manifest against the spec.
    /// Returns a list of validation errors (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // ID must have >= 3 segments
        if self.plugin.id.split('.').count() < 3 {
            errors.push(format!(
                "plugin.id '{}' must have at least 3 dot-separated segments",
                self.plugin.id
            ));
        }

        // Description under 200 chars
        if self.plugin.description.len() > 200 {
            errors.push(format!(
                "plugin.description is {} chars — must be under 200",
                self.plugin.description.len()
            ));
        }

        if self.plugin.description.is_empty() {
            errors.push("plugin.description must not be empty".into());
        }

        // Version is semver-ish
        let parts: Vec<&str> = self.plugin.version.split('.').collect();
        if parts.len() < 3 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
            errors.push(format!(
                "plugin.version '{}' is not valid semver",
                self.plugin.version
            ));
        }

        // Type-specific validation
        match self.plugin.plugin_type {
            PluginType::Persona => {
                if self.persona.is_none() {
                    errors.push("persona plugin must have a [persona] section".into());
                } else if let Some(ref p) = self.persona
                    && p.identity.is_none()
                {
                    errors.push(
                        "persona plugin must have [persona.identity] with a directive".into(),
                    );
                }
            }
            PluginType::Tone => {
                if self.tone.is_none() {
                    errors.push("tone plugin must have a [tone] section with a directive".into());
                }
            }
            PluginType::Skill => {
                if self.skill.is_none() {
                    errors.push(
                        "skill plugin must have a [skill] section with a guidance path".into(),
                    );
                }
            }
            PluginType::Extension => {
                // Extensions must have at least one tool or context entry
                if self.tools.is_empty() && self.context.is_none() {
                    errors.push(
                        "extension plugin must have at least one [[tools]] entry or [context]"
                            .into(),
                    );
                }
            }
        }

        // Validate tool entries
        for tool in &self.tools {
            errors.extend(tool.validate());
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::ToolCapability;

    #[test]
    fn parse_persona_manifest() {
        let toml = r#"
            [plugin]
            type = "persona"
            id = "dev.styrene.omegon.tutor"
            name = "Socratic Tutor"
            version = "1.0.0"
            description = "Guides through questioning, never lectures"

            [persona.identity]
            directive = "PERSONA.md"

            [persona.mind]
            seed_facts = "mind/facts.jsonl"

            [persona.tools]
            disable = ["bash", "write"]

            [persona.style]
            badge = "📚"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Persona);
        assert_eq!(manifest.plugin.id, "dev.styrene.omegon.tutor");
        assert!(
            manifest.validate().is_empty(),
            "should have no validation errors"
        );

        let persona = manifest.persona.unwrap();
        assert_eq!(persona.identity.unwrap().directive, "PERSONA.md");
        assert_eq!(
            persona.mind.unwrap().seed_facts.unwrap(),
            "mind/facts.jsonl"
        );
        assert_eq!(persona.tools.unwrap().disable, vec!["bash", "write"]);
        assert_eq!(persona.style.unwrap().badge.unwrap(), "📚");
    }

    #[test]
    fn parse_tone_manifest() {
        let toml = r#"
            [plugin]
            type = "tone"
            id = "dev.styrene.omegon.tone.alan-watts"
            name = "Alan Watts"
            version = "1.0.0"
            description = "Philosophical, gently irreverent"

            [tone]
            directive = "TONE.md"
            exemplars = "exemplars/"

            [tone.intensity]
            design = "full"
            coding = "muted"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Tone);
        assert!(manifest.validate().is_empty());

        let tone = manifest.tone.unwrap();
        assert_eq!(tone.directive, "TONE.md");
        assert_eq!(tone.exemplars.unwrap(), "exemplars/");
        let intensity = tone.intensity.unwrap();
        assert_eq!(intensity.design, "full");
        assert_eq!(intensity.coding, "muted");
    }

    #[test]
    fn parse_skill_manifest() {
        let toml = r#"
            [plugin]
            type = "skill"
            id = "dev.styrene.omegon.skill.security"
            name = "Security Review"
            version = "1.0.0"
            description = "Security checklist"

            [skill]
            guidance = "SKILL.md"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Skill);
        assert!(manifest.validate().is_empty());
        assert_eq!(manifest.skill.unwrap().guidance, "SKILL.md");
    }

    #[test]
    fn parse_tool_capabilities() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "lab.example.capabilities"
            name = "cap-test"
            version = "0.1.0"
            description = "Capability parsing test"

            [[tools]]
            name = "lookup"
            description = "Lookup remote records"
            endpoint = "https://example.test/tools/lookup"
            capabilities = ["orientation", "broad_orientation"]
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(
            manifest.tools[0].capabilities,
            vec![
                ToolCapability::Orientation,
                ToolCapability::BroadOrientation
            ]
        );
    }

    #[test]
    fn parse_detect_section() {
        let toml = r#"
            [plugin]
            type = "persona"
            id = "dev.styrene.omegon.pcb"
            name = "PCB Designer"
            version = "1.0.0"
            description = "PCB design persona"

            [persona.identity]
            directive = "PERSONA.md"

            [detect]
            file_patterns = ["*.kicad_pcb", "*.kicad_sch"]
            directories = ["gerbers/"]
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let detect = manifest.detect.unwrap();
        assert_eq!(detect.file_patterns, vec!["*.kicad_pcb", "*.kicad_sch"]);
        assert_eq!(detect.directories, vec!["gerbers/"]);
        assert!(!detect.default);
    }

    #[test]
    fn validate_bad_id() {
        let toml = r#"
            [plugin]
            type = "skill"
            id = "badid"
            name = "Test"
            version = "1.0.0"
            description = "Test"

            [skill]
            guidance = "SKILL.md"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(!errors.is_empty());
        assert!(errors[0].contains("3 dot-separated"));
    }

    #[test]
    fn validate_missing_persona_section() {
        let toml = r#"
            [plugin]
            type = "persona"
            id = "dev.styrene.omegon.empty"
            name = "Empty"
            version = "1.0.0"
            description = "Missing persona section"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("[persona]")));
    }

    #[test]
    fn validate_bad_version() {
        let toml = r#"
            [plugin]
            type = "skill"
            id = "dev.styrene.omegon.test"
            name = "Test"
            version = "not-semver"
            description = "Test"

            [skill]
            guidance = "SKILL.md"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("semver")));
    }

    #[test]
    fn validate_description_too_long() {
        let toml = format!(
            r#"
            [plugin]
            type = "skill"
            id = "dev.styrene.omegon.test"
            name = "Test"
            version = "1.0.0"
            description = "{}"

            [skill]
            guidance = "SKILL.md"
        "#,
            "x".repeat(201)
        );
        let manifest = ArmoryManifest::parse(&toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("200")));
    }

    #[test]
    fn plugin_type_display() {
        assert_eq!(PluginType::Persona.to_string(), "persona");
        assert_eq!(PluginType::Tone.to_string(), "tone");
        assert_eq!(PluginType::Skill.to_string(), "skill");
        assert_eq!(PluginType::Extension.to_string(), "extension");
    }

    // ── Functional plugin tests ────────────────────────────

    #[test]
    fn parse_script_backed_extension() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.csv-analyzer"
            name = "CSV Analyzer"
            version = "1.0.0"
            description = "Analyze CSV files with pandas"

            [[tools]]
            name = "analyze_csv"
            description = "Run statistical analysis on a CSV file"
            runner = "python"
            script = "tools/analyze.py"
            timeout_secs = 60
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Extension);
        assert_eq!(manifest.tools.len(), 1);
        assert!(manifest.validate().is_empty());

        let tool = &manifest.tools[0];
        assert_eq!(tool.runner, Some(ToolRunner::Python));
        assert_eq!(tool.script.as_deref(), Some("tools/analyze.py"));
        assert!(tool.is_script());
        assert!(!tool.is_http());
        assert_eq!(tool.timeout_secs, 60);
    }

    #[test]
    fn parse_http_backed_extension() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.scribe"
            name = "Scribe"
            version = "1.0.0"
            description = "Engagement tracking"

            [[tools]]
            name = "scribe_status"
            description = "Get engagement status"
            endpoint = "http://localhost:3000/api/status"
            method = "GET"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert!(manifest.validate().is_empty());

        let tool = &manifest.tools[0];
        assert!(tool.is_http());
        assert!(!tool.is_script());
        assert_eq!(tool.method.as_deref(), Some("GET"));
    }

    #[test]
    fn parse_persona_with_tools() {
        let toml = r#"
            [plugin]
            type = "persona"
            id = "dev.styrene.omegon.pcb-designer"
            name = "PCB Designer"
            version = "1.0.0"
            description = "PCB design persona with KiCad integration"

            [persona.identity]
            directive = "PERSONA.md"

            [persona.mind]
            seed_facts = "mind/facts.jsonl"

            [[tools]]
            name = "drc_check"
            description = "Run KiCad Design Rule Check"
            runner = "python"
            script = "tools/drc_check.py"
            timeout_secs = 60

            [[tools]]
            name = "bom_export"
            description = "Export Bill of Materials"
            runner = "python"
            script = "tools/bom_export.py"

            [detect]
            file_patterns = ["*.kicad_pcb", "*.kicad_sch"]
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Persona);
        assert_eq!(manifest.tools.len(), 2);
        assert!(manifest.validate().is_empty());
        assert!(manifest.detect.is_some());
    }

    #[test]
    fn parse_context_entry() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.context-gen"
            name = "Context Gen"
            version = "1.0.0"
            description = "Dynamic context generator"

            [context]
            runner = "python"
            script = "context/generate.py"
            ttl_turns = 50
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let ctx = manifest.context.unwrap();
        assert_eq!(ctx.runner, Some(ToolRunner::Python));
        assert_eq!(ctx.script.as_deref(), Some("context/generate.py"));
        assert_eq!(ctx.ttl_turns, 50);
    }

    #[test]
    fn validate_tool_runner_without_script() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.broken"
            name = "Broken"
            version = "1.0.0"
            description = "Missing script path"

            [[tools]]
            name = "bad_tool"
            description = "Has runner but no script"
            runner = "python"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("requires a script")));
    }

    #[test]
    fn validate_tool_runner_and_endpoint_conflict() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.conflict"
            name = "Conflict"
            version = "1.0.0"
            description = "Has both runner and endpoint"

            [[tools]]
            name = "confused_tool"
            description = "Can't be both"
            runner = "python"
            script = "tools/run.py"
            endpoint = "http://localhost:3000/api"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("mutually exclusive")));
    }

    #[test]
    fn validate_tool_no_runner_no_endpoint() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.empty-tool"
            name = "Empty"
            version = "1.0.0"
            description = "Tool with no execution method"

            [[tools]]
            name = "orphan_tool"
            description = "No runner, no endpoint"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(
            errors.iter().any(|e| e.contains("must have either")),
            "errors: {errors:?}"
        );
    }

    // ── OCI container tool tests ─────────────────────────

    #[test]
    fn parse_oci_tool_with_image() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.oci-tool"
            name = "OCI Tool"
            version = "1.0.0"
            description = "Container-backed analysis tool"

            [[tools]]
            name = "analyze"
            description = "Run analysis in container"
            runner = "oci"
            image = "ghcr.io/styrene-lab/omegon-tool-analyze:latest"
            mount_cwd = true
            network = false
            timeout_secs = 120
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert!(manifest.validate().is_empty(), "should validate cleanly");

        let tool = &manifest.tools[0];
        assert_eq!(tool.runner, Some(ToolRunner::Oci));
        assert_eq!(
            tool.image.as_deref(),
            Some("ghcr.io/styrene-lab/omegon-tool-analyze:latest")
        );
        assert!(tool.mount_cwd);
        assert!(!tool.network);
        assert!(tool.is_oci());
        assert!(!tool.is_script());
        assert!(!tool.is_http());
    }

    #[test]
    fn parse_oci_tool_with_build() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.oci-build"
            name = "OCI Build Tool"
            version = "1.0.0"
            description = "Build from Containerfile"

            [[tools]]
            name = "custom_tool"
            description = "Locally built container tool"
            runner = "oci"
            build = "tools/custom/Containerfile"
            mount_cwd = true
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert!(manifest.validate().is_empty());

        let tool = &manifest.tools[0];
        assert_eq!(tool.build.as_deref(), Some("tools/custom/Containerfile"));
        assert!(tool.is_oci());
    }

    #[test]
    fn validate_oci_tool_missing_image_and_build() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.bad-oci"
            name = "Bad OCI"
            version = "1.0.0"
            description = "OCI runner but no image or build"

            [[tools]]
            name = "broken_oci"
            description = "Missing image reference"
            runner = "oci"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(
            errors.iter().any(|e| e.contains("image or build")),
            "errors: {errors:?}"
        );
    }

    #[test]
    fn parse_persona_with_oci_tools() {
        let toml = r#"
            [plugin]
            type = "persona"
            id = "dev.styrene.omegon.pcb-designer"
            name = "PCB Designer"
            version = "1.0.0"
            description = "PCB design with containerized KiCad tools"

            [persona.identity]
            directive = "PERSONA.md"

            [[tools]]
            name = "drc_check"
            description = "Run KiCad DRC in container"
            runner = "oci"
            image = "ghcr.io/styrene-lab/omegon-tool-kicad:latest"
            mount_cwd = true
            network = false
            timeout_secs = 120

            [[tools]]
            name = "gerber_export"
            description = "Export Gerber files"
            runner = "oci"
            build = "tools/gerber/Containerfile"
            mount_cwd = true

            [detect]
            file_patterns = ["*.kicad_pcb"]
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.plugin_type, PluginType::Persona);
        assert_eq!(manifest.tools.len(), 2);
        assert!(manifest.validate().is_empty());
        assert!(manifest.tools[0].is_oci());
        assert!(manifest.tools[1].is_oci());
    }

    #[test]
    fn validate_extension_needs_tools_or_context() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.empty-ext"
            name = "Empty Extension"
            version = "1.0.0"
            description = "Extension with nothing"
        "#;
        let manifest = ArmoryManifest::parse(toml).unwrap();
        let errors = manifest.validate();
        assert!(errors.iter().any(|e| e.contains("at least one")));
    }

    #[test]
    fn tool_runner_display() {
        assert_eq!(ToolRunner::Python.to_string(), "python");
        assert_eq!(ToolRunner::Node.to_string(), "node");
        assert_eq!(ToolRunner::Bash.to_string(), "bash");
        assert_eq!(ToolRunner::Oci.to_string(), "oci");
        assert_eq!(ToolRunner::Wasm.to_string(), "wasm");
    }

    #[test]
    fn parse_real_armory_manifests() {
        // Parse the actual armory plugin.toml files to validate compatibility
        let armory_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../omegon-armory");

        // Skip if armory isn't present (CI environments)
        if !armory_dir.exists() {
            return;
        }

        for category in ["personas", "tones", "skills"] {
            let cat_dir = armory_dir.join(category);
            if !cat_dir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(cat_dir).unwrap() {
                let entry = entry.unwrap();
                let toml_path = entry.path().join("plugin.toml");
                if !toml_path.exists() {
                    continue;
                }

                let content = std::fs::read_to_string(&toml_path).unwrap();
                let manifest = ArmoryManifest::parse(&content)
                    .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", toml_path.display()));
                let errors = manifest.validate();
                assert!(
                    errors.is_empty(),
                    "Validation errors in {}: {:?}",
                    toml_path.display(),
                    errors
                );
            }
        }
    }
}
