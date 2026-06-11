use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArmoryProfileSummary {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub source_path: String,
    pub defaults: ArmoryProfileDefaults,
    pub export: ArmoryProfileExport,
    pub dependencies: Vec<ArmoryProfileDependency>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArmoryProfileDefaults {
    pub posture: Option<String>,
    pub thinking_level: Option<String>,
    pub max_turns: Option<u32>,
    pub persona: Option<String>,
    pub tone: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArmoryProfileExport {
    pub default_format: Option<String>,
    pub include_optional: bool,
    pub include_native_notes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArmoryProfileDependency {
    pub kind: String,
    pub id: String,
    pub version: Option<String>,
    pub required: bool,
    pub activate: String,
}

#[derive(Debug, Deserialize)]
struct ProfileDocument {
    profile: ProfileMeta,
    #[serde(default)]
    defaults: ProfileDefaultsDocument,
    #[serde(default)]
    export: ProfileExportDocument,
    #[serde(default)]
    dependencies: Vec<ProfileDependencyDocument>,
}

#[derive(Debug, Deserialize)]
struct ProfileMeta {
    id: String,
    slug: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
}

#[derive(Debug, Default, Deserialize)]
struct ProfileDefaultsDocument {
    posture: Option<String>,
    thinking_level: Option<String>,
    max_turns: Option<u32>,
    persona: Option<String>,
    tone: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ProfileExportDocument {
    default_format: Option<String>,
    #[serde(default)]
    include_optional: bool,
    #[serde(default)]
    include_native_notes: bool,
}

#[derive(Debug, Deserialize)]
struct ProfileDependencyDocument {
    kind: String,
    id: String,
    version: Option<String>,
    #[serde(default)]
    required: bool,
    #[serde(default = "default_activate")]
    activate: String,
}

fn default_activate() -> String {
    "manual".to_string()
}

pub fn list_armory_profiles_from_root(
    armory_root: &Path,
) -> anyhow::Result<Vec<ArmoryProfileSummary>> {
    let profiles_dir = armory_root.join("profiles");
    if !profiles_dir.exists() {
        return Ok(Vec::new());
    }

    let mut profiles = Vec::new();
    for entry in std::fs::read_dir(&profiles_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let profile_path = path.join("profile.toml");
        if !profile_path.exists() {
            continue;
        }
        profiles.push(armory_profile_summary_from_file(&profile_path)?);
    }
    profiles.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(profiles)
}

pub fn armory_profile_summary_from_file(profile_path: &Path) -> anyhow::Result<ArmoryProfileSummary> {
    let content = std::fs::read_to_string(profile_path)?;
    let doc: ProfileDocument = toml::from_str(&content).map_err(|err| {
        anyhow::anyhow!(
            "failed to parse Armory profile at {}: {err}",
            profile_path.display()
        )
    })?;

    let mut dependencies: Vec<_> = doc
        .dependencies
        .into_iter()
        .map(|dep| ArmoryProfileDependency {
            kind: dep.kind,
            id: dep.id,
            version: dep.version,
            required: dep.required,
            activate: dep.activate,
        })
        .collect();
    dependencies.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.activate.cmp(&b.activate))
    });

    Ok(ArmoryProfileSummary {
        id: doc.profile.id,
        slug: doc.profile.slug,
        name: doc.profile.name,
        version: doc.profile.version,
        description: doc.profile.description,
        category: doc.profile.category,
        source_path: profile_path.display().to_string(),
        defaults: ArmoryProfileDefaults {
            posture: doc.defaults.posture,
            thinking_level: doc.defaults.thinking_level,
            max_turns: doc.defaults.max_turns,
            persona: doc.defaults.persona,
            tone: doc.defaults.tone,
        },
        export: ArmoryProfileExport {
            default_format: doc.export.default_format,
            include_optional: doc.export.include_optional,
            include_native_notes: doc.export.include_native_notes,
        },
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_armory_profiles_with_dependency_policy() {
        let temp = tempfile::tempdir().unwrap();
        let profile_dir = temp.path().join("profiles").join("rust-shop");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[profile]
schema = "dev.styrene.omegon.profile.v1"
id = "dev.styrene.omegon.profile.rust-shop"
slug = "rust-shop"
name = "Rust Shop"
version = "1.0.0"
description = "Curated Rust stack"
category = "engineering"

[defaults]
posture = "architect"
thinking_level = "medium"
max_turns = 50
persona = "systems-engineer"
tone = "concise"

[export]
default_format = "generic-markdown"
include_optional = false
include_native_notes = true

[[dependencies]]
kind = "skill"
id = "rust"
version = ">=1.0.0"
required = true
activate = "always"

[[dependencies]]
kind = "extension"
id = "shuttle"
version = ">=0.19"
required = false
activate = "manual"
"#,
        )
        .unwrap();

        let profiles = list_armory_profiles_from_root(temp.path()).unwrap();

        assert_eq!(profiles.len(), 1);
        let profile = &profiles[0];
        assert_eq!(profile.slug, "rust-shop");
        assert_eq!(profile.defaults.persona.as_deref(), Some("systems-engineer"));
        assert_eq!(profile.export.default_format.as_deref(), Some("generic-markdown"));
        assert_eq!(profile.dependencies.len(), 2);
        assert!(profile.dependencies.iter().any(|dep| {
            dep.kind == "skill" && dep.id == "rust" && dep.required && dep.activate == "always"
        }));
        assert!(profile.dependencies.iter().any(|dep| {
            dep.kind == "extension"
                && dep.id == "shuttle"
                && !dep.required
                && dep.activate == "manual"
        }));
    }

    #[test]
    fn missing_profiles_dir_is_empty_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let profiles = list_armory_profiles_from_root(temp.path()).unwrap();
        assert!(profiles.is_empty());
    }
}
