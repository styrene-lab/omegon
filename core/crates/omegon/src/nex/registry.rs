//! Nex profile registry — built-in domain profiles + user-defined custom profiles.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use tracing;

use super::manifest::NexManifest;
use super::profile::{NexCapabilities, NexDomain, NexProfile, NexResourceLimits};

const DEFAULT_REGISTRY: &str = "ghcr.io/styrene-lab";

/// Registry of available Nex profiles.
pub struct NexRegistry {
    profiles: HashMap<String, NexProfile>,
}

impl NexRegistry {
    /// Load profiles from built-in domains + custom TOML files.
    ///
    /// Custom profiles are loaded from:
    /// - `~/.config/omegon/nex/*.toml` (global)
    /// - `<project_root>/.omegon/nex/*.toml` (project-local)
    pub fn load(omegon_home: &Path, project_root: Option<&Path>) -> Result<Self> {
        let mut profiles = HashMap::new();

        // Register built-in domain profiles
        let version = env!("CARGO_PKG_VERSION");
        let mut builtin_names = Vec::new();
        for domain in builtin_domains() {
            let name = domain.to_string();
            let image_ref = domain.default_image_ref(DEFAULT_REGISTRY, version);
            builtin_names.push(name.clone());
            profiles.insert(
                name.clone(),
                NexProfile {
                    name,
                    profile_hash: format!("builtin:{domain}:{version}"),
                    base_domain: domain,
                    overlays: Vec::new(),
                    resource_limits: NexResourceLimits::default(),
                    capabilities: NexCapabilities::default(),
                    image_ref: Some(image_ref),
                    signed_by: None,
                },
            );
        }

        // Load custom profiles from global config
        let global_dir = omegon_home.join("nex");
        load_custom_profiles(&global_dir, &mut profiles, &builtin_names);

        // Load custom profiles from project
        if let Some(root) = project_root {
            let project_dir = root.join(".omegon").join("nex");
            load_custom_profiles(&project_dir, &mut profiles, &builtin_names);
        }

        Ok(Self { profiles })
    }

    /// Resolve a profile by name or hash prefix.
    pub fn resolve(&self, name_or_hash: &str) -> Option<&NexProfile> {
        // Direct name match
        if let Some(p) = self.profiles.get(name_or_hash) {
            return Some(p);
        }

        // Hash prefix match
        self.profiles
            .values()
            .find(|p| p.profile_hash.starts_with(name_or_hash))
    }

    /// Get the default profile for a given domain.
    pub fn resolve_for_domain(&self, domain: &NexDomain) -> Option<&NexProfile> {
        self.profiles
            .values()
            .find(|p| &p.base_domain == domain && p.overlays.is_empty())
    }

    /// List all registered profiles.
    pub fn list(&self) -> Vec<&NexProfile> {
        let mut profiles: Vec<_> = self.profiles.values().collect();
        profiles.sort_by_key(|p| &p.name);
        profiles
    }

    /// Number of registered profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

fn builtin_domains() -> Vec<NexDomain> {
    vec![
        NexDomain::Chat,
        NexDomain::Coding,
        NexDomain::CodingPython,
        NexDomain::CodingNode,
        NexDomain::CodingRust,
        NexDomain::Infra,
        NexDomain::Full,
    ]
}

fn load_custom_profiles(
    dir: &Path,
    profiles: &mut HashMap<String, NexProfile>,
    builtins: &[String],
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return, // directory doesn't exist — not an error
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            match NexManifest::from_file(&path) {
                Ok(manifest) => {
                    let profile = manifest.into_profile();

                    // Prevent project-local manifests from silently overriding
                    // built-in profiles (H4 security fix). A malicious repo
                    // could ship .omegon/nex/coding.toml to hijack the
                    // built-in coding profile with a custom image.
                    if builtins.contains(&profile.name) {
                        tracing::warn!(
                            name = %profile.name,
                            path = %path.display(),
                            "custom nex profile shadows a built-in — ignoring \
                             (use a different name to avoid collision)"
                        );
                        continue;
                    }

                    tracing::debug!(
                        name = %profile.name,
                        hash = %profile.profile_hash,
                        path = %path.display(),
                        "loaded custom nex profile"
                    );
                    profiles.insert(profile.name.clone(), profile);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to load nex profile"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_registered() {
        let registry = NexRegistry::load(Path::new("/nonexistent"), None).unwrap();
        assert_eq!(registry.len(), 7);
        assert!(registry.resolve("coding").is_some());
        assert!(registry.resolve("coding-python").is_some());
        assert!(registry.resolve("infra").is_some());
        assert!(registry.resolve("nonexistent").is_none());
    }

    #[test]
    fn resolve_by_domain() {
        let registry = NexRegistry::load(Path::new("/nonexistent"), None).unwrap();
        let profile = registry.resolve_for_domain(&NexDomain::CodingRust).unwrap();
        assert_eq!(profile.name, "coding-rust");
        assert!(
            profile
                .image_ref
                .as_ref()
                .unwrap()
                .contains("omegon-coding-rust")
        );
    }
}
