//! Agent catalog — discovers and lists available agent bundles.
//!
//! Agent bundles live in `$OMEGON_HOME/catalog/` as directories containing
//! an `agent.pkl` or `agent.toml` manifest. The catalog provides discovery
//! and resolution for the `--agent` CLI flag and Auspex spawn contracts.
//!
//! # Installation
//!
//! `cmd_install(offline)` fetches the upstream armory registry and downloads
//! each agent's files. When `offline = true` (or the network is unreachable),
//! it falls back to the copies embedded in the binary at compile time.

use std::path::{Path, PathBuf};

use crate::agent_manifest::{self, ResolvedManifest};

/// Base URL for the upstream armory catalog.
const ARMORY_BASE: &str =
    "https://raw.githubusercontent.com/styrene-lab/omegon-armory/main";

/// Parsed entry from `catalog-registry.toml`.
/// Only `files` is consumed; remaining fields are defined in the registry for
/// documentation and future use (toml deserialization ignores unknown fields by default).
#[derive(serde::Deserialize)]
struct ArmoryEntry {
    files: Vec<String>,
}

/// A catalog agent bundle with all files embedded at compile time.
struct BundledAgent {
    id: &'static str,
    /// TOML manifest — always present; used as fallback when pkl binary unavailable.
    agent_toml: &'static str,
    /// Pkl manifest — present for agents that have an agent.pkl.
    /// Enables `amends "omegon://catalog/<id>/agent.pkl"` inheritance for user overlays.
    agent_pkl: Option<&'static str>,
    persona_md: &'static str,
    mind_facts: Option<&'static str>,
}

const BUNDLED: &[BundledAgent] = &[
    BundledAgent {
        id: "styrene.bd-agent",
        agent_toml: include_str!("../../../../catalog/styrene.bd-agent/agent.toml"),
        agent_pkl: Some(include_str!("../../../../catalog/styrene.bd-agent/agent.pkl")),
        persona_md: include_str!("../../../../catalog/styrene.bd-agent/PERSONA.md"),
        mind_facts: Some(include_str!("../../../../catalog/styrene.bd-agent/mind/facts.jsonl")),
    },
    BundledAgent {
        id: "styrene.coding-agent",
        agent_toml: include_str!("../../../../catalog/styrene.coding-agent/agent.toml"),
        agent_pkl: None,
        persona_md: include_str!("../../../../catalog/styrene.coding-agent/PERSONA.md"),
        mind_facts: Some(include_str!("../../../../catalog/styrene.coding-agent/mind/facts.jsonl")),
    },
    BundledAgent {
        id: "styrene.community-agent",
        agent_toml: include_str!("../../../../catalog/styrene.community-agent/agent.toml"),
        agent_pkl: None,
        persona_md: include_str!("../../../../catalog/styrene.community-agent/PERSONA.md"),
        mind_facts: Some(include_str!("../../../../catalog/styrene.community-agent/mind/facts.jsonl")),
    },
    BundledAgent {
        id: "styrene.discord-agent",
        agent_toml: include_str!("../../../../catalog/styrene.discord-agent/agent.toml"),
        agent_pkl: Some(include_str!("../../../../catalog/styrene.discord-agent/agent.pkl")),
        persona_md: include_str!("../../../../catalog/styrene.discord-agent/PERSONA.md"),
        mind_facts: None,
    },
    BundledAgent {
        id: "styrene.infra-engineer",
        agent_toml: include_str!("../../../../catalog/styrene.infra-engineer/agent.toml"),
        agent_pkl: None,
        persona_md: include_str!("../../../../catalog/styrene.infra-engineer/PERSONA.md"),
        mind_facts: Some(include_str!("../../../../catalog/styrene.infra-engineer/mind/facts.jsonl")),
    },
    BundledAgent {
        id: "styrene.slack-agent",
        agent_toml: include_str!("../../../../catalog/styrene.slack-agent/agent.toml"),
        agent_pkl: Some(include_str!("../../../../catalog/styrene.slack-agent/agent.pkl")),
        persona_md: include_str!("../../../../catalog/styrene.slack-agent/PERSONA.md"),
        mind_facts: None,
    },
];

fn catalog_dir() -> Option<PathBuf> {
    crate::paths::omegon_home().ok().map(|h| h.join("catalog"))
}

/// List bundled agents and their installation status.
pub fn cmd_list() -> anyhow::Result<()> {
    let cat_dir = catalog_dir();
    println!("Bundled agents ({})\n", BUNDLED.len());
    for bundle in BUNDLED {
        let installed = cat_dir
            .as_ref()
            .map(|d| d.join(bundle.id).join("agent.toml").exists())
            .unwrap_or(false);
        let marker = if installed { "✓" } else { "○" };
        let (name, domain) = extract_agent_meta(bundle.agent_toml);
        println!("  {marker} {id:<30}  {name}  [{domain}]", id = bundle.id);
    }
    if let Some(dir) = &cat_dir {
        println!("\nInstall path: {}", dir.display());
    }
    Ok(())
}

/// Install agents to `~/.omegon/catalog/`.
///
/// Fetches from the upstream armory unless `offline` is `true` or the network
/// is unavailable, in which case it falls back to the copies embedded in the
/// binary at compile time.
pub async fn cmd_install(offline: bool) -> anyhow::Result<()> {
    let cat_dir =
        catalog_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    std::fs::create_dir_all(&cat_dir)?;

    if !offline {
        match install_from_upstream(&cat_dir).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("  ! upstream fetch failed ({e}), falling back to bundled");
            }
        }
    }

    install_from_bundled(&cat_dir)
}

fn print_install_summary(installed: usize, updated: usize, cat_dir: &Path) {
    println!(
        "\n{installed} agent(s) installed, {updated} updated → {}",
        cat_dir.display()
    );
    println!("Agents are active immediately in new sessions.");
}

/// Download all agents listed in the armory `catalog-registry.toml`.
/// Files within each agent bundle are fetched concurrently.
async fn install_from_upstream(cat_dir: &Path) -> anyhow::Result<()> {
    use futures::future::try_join_all;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let registry_url = format!("{ARMORY_BASE}/catalog-registry.toml");
    let registry_text = client
        .get(&registry_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let registry: std::collections::HashMap<String, ArmoryEntry> =
        toml::from_str(&registry_text)?;

    // Sort for stable output order.
    let mut ids: Vec<&String> = registry.keys().collect();
    ids.sort();

    let mut installed = 0usize;
    let mut updated = 0usize;

    for id in ids {
        let entry = &registry[id];
        let bundle_dir = cat_dir.join(id);
        std::fs::create_dir_all(&bundle_dir)?;

        let already_exists = bundle_dir.join("agent.toml").exists();

        // Fetch all files for this agent concurrently.
        let fetches: Vec<_> = entry
            .files
            .iter()
            .map(|file| {
                let url = format!("{ARMORY_BASE}/catalog/{id}/{file}");
                let client = client.clone();
                async move {
                    let bytes = client
                        .get(&url)
                        .send()
                        .await?
                        .error_for_status()?
                        .bytes()
                        .await?;
                    Ok::<(String, Vec<u8>), anyhow::Error>((file.clone(), bytes.to_vec()))
                }
            })
            .collect();

        let results = try_join_all(fetches).await?;

        let mut any_changed = false;
        for (file, bytes) in results {
            let dest = bundle_dir.join(&file);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let existing = std::fs::read(&dest).ok();
            if existing.as_deref() != Some(bytes.as_slice()) {
                std::fs::write(&dest, &bytes)?;
                any_changed = true;
            }
        }

        if !already_exists {
            println!("  + {id}");
            installed += 1;
        } else if any_changed {
            println!("  ↑ {id}  (updated)");
            updated += 1;
        } else {
            println!("  ✓ {id}  (unchanged)");
        }
    }

    print_install_summary(installed, updated, cat_dir);
    Ok(())
}

/// Write the compile-time bundled agents to disk.
fn install_from_bundled(cat_dir: &Path) -> anyhow::Result<()> {
    let mut installed = 0usize;
    let mut updated = 0usize;

    for bundle in BUNDLED {
        let bundle_dir = cat_dir.join(bundle.id);
        std::fs::create_dir_all(&bundle_dir)?;

        let toml_path = bundle_dir.join("agent.toml");
        let old_content = std::fs::read_to_string(&toml_path).ok();
        let already_exists = old_content.is_some();
        let changed = old_content.as_deref() != Some(bundle.agent_toml);

        std::fs::write(&toml_path, bundle.agent_toml)?;
        if let Some(pkl) = bundle.agent_pkl {
            std::fs::write(bundle_dir.join("agent.pkl"), pkl)?;
        }
        std::fs::write(bundle_dir.join("PERSONA.md"), bundle.persona_md)?;
        if let Some(facts) = bundle.mind_facts {
            let mind_dir = bundle_dir.join("mind");
            std::fs::create_dir_all(&mind_dir)?;
            std::fs::write(mind_dir.join("facts.jsonl"), facts)?;
        }

        if !already_exists {
            println!("  + {}", bundle.id);
            installed += 1;
        } else if changed {
            println!("  ↑ {}  (updated)", bundle.id);
            updated += 1;
        } else {
            println!("  ✓ {}  (unchanged)", bundle.id);
        }
    }

    print_install_summary(installed, updated, cat_dir);
    Ok(())
}

/// Parse name and domain from an embedded agent.toml string.
fn extract_agent_meta(toml_src: &str) -> (String, String) {
    #[derive(serde::Deserialize)]
    struct AgentSection {
        name: Option<String>,
        domain: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Outer {
        agent: Option<AgentSection>,
    }
    let parsed: Outer = toml::from_str(toml_src).unwrap_or(Outer { agent: None });
    let section = parsed.agent.unwrap_or(AgentSection { name: None, domain: None });
    (
        section.name.unwrap_or_default(),
        section.domain.unwrap_or_default(),
    )
}

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
        let has_manifest =
            bundle_dir.join("agent.pkl").exists() || bundle_dir.join("agent.toml").exists();
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

        if let Ok(resolved) = agent_manifest::load(&bundle_dir)
            && resolved.manifest.agent.id == agent_id
        {
            return Ok(resolved);
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
