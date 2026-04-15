//! Agent catalog — discovers and lists available agent bundles.
//!
//! Agent bundles live in `$OMEGON_HOME/catalog/` as directories containing
//! an `agent.pkl` or `agent.toml` manifest. The catalog provides discovery
//! and resolution for the `--agent` CLI flag and Auspex spawn contracts.

use std::path::{Path, PathBuf};

use crate::agent_manifest::{self, ResolvedManifest};

/// Summary of an available agent in the catalog.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub domain: String,
    pub bundle_dir: PathBuf,
}

/// Discover all agent bundles in the catalog directory.
pub fn list(omegon_home: &Path) -> Vec<CatalogEntry> {
    let catalog_dir = omegon_home.join("catalog");
    if !catalog_dir.is_dir() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&catalog_dir) else {
        return Vec::new();
    };

    let mut catalog = Vec::new();

    for entry in entries.flatten() {
        let bundle_dir = entry.path();
        if !bundle_dir.is_dir() {
            continue;
        }

        // Check for manifest file
        let has_manifest = bundle_dir.join("agent.pkl").exists()
            || bundle_dir.join("agent.toml").exists();
        if !has_manifest {
            continue;
        }

        match agent_manifest::load(&bundle_dir) {
            Ok(resolved) => {
                let m = &resolved.manifest;
                catalog.push(CatalogEntry {
                    id: m.agent.id.clone(),
                    name: m.agent.name.clone(),
                    version: m.agent.version.clone(),
                    description: m.agent.description.clone(),
                    domain: m.agent.domain.clone(),
                    bundle_dir,
                });
            }
            Err(e) => {
                tracing::warn!(
                    path = %bundle_dir.display(),
                    error = %e,
                    "skipping invalid agent bundle"
                );
            }
        }
    }

    catalog.sort_by(|a, b| a.id.cmp(&b.id));
    catalog
}

/// Resolve an agent by ID from the catalog. Searches `$OMEGON_HOME/catalog/`
/// and also accepts a direct path to a bundle directory.
pub fn resolve(omegon_home: &Path, agent_id: &str) -> anyhow::Result<ResolvedManifest> {
    // First, check if agent_id is a direct path
    let as_path = Path::new(agent_id);
    if as_path.is_dir() {
        return agent_manifest::load(as_path);
    }

    // Search catalog by directory name or agent id
    let catalog_dir = omegon_home.join("catalog");
    if !catalog_dir.is_dir() {
        anyhow::bail!(
            "catalog directory not found: {}. Create it or pass a direct path.",
            catalog_dir.display()
        );
    }

    // Try exact directory match first
    let exact = catalog_dir.join(agent_id);
    if exact.is_dir() {
        return agent_manifest::load(&exact);
    }

    // Scan all bundles and match by agent.id
    let Ok(entries) = std::fs::read_dir(&catalog_dir) else {
        anyhow::bail!("cannot read catalog directory: {}", catalog_dir.display());
    };

    for entry in entries.flatten() {
        let bundle_dir = entry.path();
        if !bundle_dir.is_dir() {
            continue;
        }

        if let Ok(resolved) = agent_manifest::load(&bundle_dir) {
            if resolved.manifest.agent.id == agent_id {
                return Ok(resolved);
            }
        }
    }

    anyhow::bail!(
        "agent '{}' not found in catalog at {}",
        agent_id,
        catalog_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_empty_catalog() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("catalog")).unwrap();
        let entries = list(dir.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn list_discovers_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("catalog/test-agent");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("agent.toml"),
            r#"
[agent]
id = "test.agent"
name = "Test"
version = "1.0.0"
domain = "chat"
"#,
        )
        .unwrap();

        let entries = list(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "test.agent");
        assert_eq!(entries[0].domain, "chat");
    }

    #[test]
    fn resolve_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("catalog/my-agent");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("agent.toml"),
            r#"
[agent]
id = "org.my-agent"
name = "My Agent"
version = "1.0.0"
domain = "coding"
"#,
        )
        .unwrap();

        let resolved = resolve(dir.path(), "org.my-agent").unwrap();
        assert_eq!(resolved.manifest.agent.id, "org.my-agent");
    }

    #[test]
    fn resolve_by_dir_name() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("catalog/my-agent");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("agent.toml"),
            r#"
[agent]
id = "org.my-agent"
name = "My Agent"
version = "1.0.0"
domain = "coding"
"#,
        )
        .unwrap();

        let resolved = resolve(dir.path(), "my-agent").unwrap();
        assert_eq!(resolved.manifest.agent.id, "org.my-agent");
    }

    #[test]
    fn resolve_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("catalog")).unwrap();
        assert!(resolve(dir.path(), "nonexistent").is_err());
    }
}
