//! Nex capability resolver — read-only capability checks and recommendations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::registry::NexRegistry;

const CATALOG_TOML: &str = include_str!("../../../../../data/nex-capabilities.toml");

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CapabilityKey {
    pub kind: String,
    pub name: String,
}

impl CapabilityKey {
    pub fn parse(input: &str) -> Self {
        let trimmed = input.trim();
        if let Some((kind, name)) = trimmed.split_once(':') {
            return Self {
                kind: kind.trim().to_ascii_lowercase(),
                name: name.trim().to_ascii_lowercase(),
            };
        }
        let lowered = trimmed.to_ascii_lowercase();
        let kind = if lowered.starts_with("omegon-") || lowered == "scratchpad" {
            "extension"
        } else {
            "binary"
        };
        Self {
            kind: kind.to_string(),
            name: lowered,
        }
    }

    pub fn canonical(&self) -> String {
        format!("{}:{}", self.kind, self.name)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CapabilityCatalog {
    pub capabilities: BTreeMap<String, CatalogEntry>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CatalogEntry {
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub overlays: Vec<OverlayEntry>,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub armory: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OverlayEntry {
    pub base: String,
    pub profile_name: String,
    pub packages: Vec<String>,
}

impl CapabilityCatalog {
    pub fn bundled() -> anyhow::Result<Self> {
        toml::from_str(CATALOG_TOML).map_err(Into::into)
    }

    pub fn entry(&self, key: &CapabilityKey) -> Option<&CatalogEntry> {
        self.capabilities.get(&key.canonical())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Available,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityLocation {
    HostPath { path: String },
    NexProfile { profile: String },
    Extension { name: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CapabilityRecommendation {
    CreateProjectProfile {
        profile_name: String,
        base: String,
        packages: Vec<String>,
        manifest: String,
    },
    InstallExtension {
        name: String,
        armory: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityResolution {
    pub capability: CapabilityKey,
    pub status: CapabilityStatus,
    pub locations: Vec<CapabilityLocation>,
    pub recommendations: Vec<CapabilityRecommendation>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityContext {
    pub path: Option<String>,
    pub profile: Option<String>,
}

pub struct CapabilityResolver {
    catalog: CapabilityCatalog,
}

impl CapabilityResolver {
    pub fn bundled() -> anyhow::Result<Self> {
        Ok(Self {
            catalog: CapabilityCatalog::bundled()?,
        })
    }

    pub fn check(
        &self,
        capability: &str,
        context: CapabilityContext,
        registry: Option<&NexRegistry>,
    ) -> CapabilityResolution {
        let key = CapabilityKey::parse(capability);
        let mut locations = Vec::new();
        let mut diagnostics = Vec::new();
        let entry = self.catalog.entry(&key);

        if key.kind == "binary" {
            let commands = entry
                .map(|entry| entry.commands.as_slice())
                .unwrap_or_else(|| std::slice::from_ref(&key.name));
            for command in commands {
                if let Some(path) = find_on_path(command, context.path.as_deref()) {
                    locations.push(CapabilityLocation::HostPath {
                        path: path.display().to_string(),
                    });
                }
            }
        } else if key.kind == "extension" {
            diagnostics.push(
                "extension registry inspection is not wired yet; resolve can still recommend an install"
                    .to_string(),
            );
        } else {
            diagnostics.push(format!("unknown capability kind: {}", key.kind));
        }

        if let (Some(profile), Some(registry)) = (context.profile.as_deref(), registry)
            && registry.resolve(profile).is_some()
        {
            locations.push(CapabilityLocation::NexProfile {
                profile: profile.to_string(),
            });
        }

        let status = if !locations.is_empty() {
            CapabilityStatus::Available
        } else if entry.is_some() {
            CapabilityStatus::Missing
        } else {
            CapabilityStatus::Unknown
        };

        if entry.is_none() {
            diagnostics.push(format!("no catalog entry for {}", key.canonical()));
        }

        CapabilityResolution {
            capability: key,
            status,
            locations,
            recommendations: Vec::new(),
            diagnostics,
        }
    }

    pub fn resolve(
        &self,
        capability: &str,
        context: CapabilityContext,
        registry: Option<&NexRegistry>,
    ) -> CapabilityResolution {
        let mut resolution = self.check(capability, context, registry);
        let Some(entry) = self.catalog.entry(&resolution.capability) else {
            return resolution;
        };

        for overlay in &entry.overlays {
            resolution
                .recommendations
                .push(CapabilityRecommendation::CreateProjectProfile {
                    profile_name: overlay.profile_name.clone(),
                    base: overlay.base.clone(),
                    packages: overlay.packages.clone(),
                    manifest: overlay_manifest(overlay),
                });
        }

        if let Some(extension) = &entry.extension {
            resolution
                .recommendations
                .push(CapabilityRecommendation::InstallExtension {
                    name: extension.clone(),
                    armory: entry.armory,
                });
        }

        resolution
    }
}

fn overlay_manifest(overlay: &OverlayEntry) -> String {
    let packages = overlay
        .packages
        .iter()
        .map(|package| format!("\"{package}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[profile]\nname = \"{}\"\nbase = \"{}\"\n\n[overlays.capability]\npackages = [{}]\n",
        overlay.profile_name, overlay.base, packages
    )
}

fn find_on_path(command: &str, path_override: Option<&str>) -> Option<PathBuf> {
    let paths = path_override
        .map(std::env::split_paths)
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_else(|| std::env::var_os("PATH").map_or_else(Vec::new, |path| std::env::split_paths(&path).collect()));

    paths
        .into_iter()
        .map(|dir| dir.join(command))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads() {
        let catalog = CapabilityCatalog::bundled().unwrap();
        assert!(catalog.entry(&CapabilityKey::parse("binary:d2")).is_some());
        assert!(catalog.entry(&CapabilityKey::parse("scratchpad")).is_some());
    }

    #[test]
    fn capability_key_normalizes_short_forms() {
        assert_eq!(CapabilityKey::parse("d2").canonical(), "binary:d2");
        assert_eq!(
            CapabilityKey::parse("omegon-voice").canonical(),
            "extension:omegon-voice"
        );
    }

    #[test]
    fn resolve_missing_d2_suggests_overlay_profile() {
        let resolver = CapabilityResolver::bundled().unwrap();
        let resolution = resolver.resolve(
            "d2",
            CapabilityContext {
                path: Some(String::new()),
                profile: None,
            },
            None,
        );
        assert!(matches!(resolution.status, CapabilityStatus::Missing));
        assert!(matches!(
            resolution.recommendations.first(),
            Some(CapabilityRecommendation::CreateProjectProfile { profile_name, .. })
                if profile_name == "coding-d2"
        ));
    }

    #[test]
    fn unknown_capability_reports_diagnostic() {
        let resolver = CapabilityResolver::bundled().unwrap();
        let resolution = resolver.check(
            "binary:not-a-real-thing",
            CapabilityContext {
                path: Some(String::new()),
                profile: None,
            },
            None,
        );
        assert!(matches!(resolution.status, CapabilityStatus::Unknown));
        assert!(resolution.diagnostics.iter().any(|d| d.contains("no catalog entry")));
    }
}
