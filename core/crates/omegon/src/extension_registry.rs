//! Armory extension registry — name-based discovery and install.
//!
//! Fetches the extension registry from the upstream armory, resolves
//! platform-specific pre-built binaries from GitHub Releases, and
//! delegates installation to [`crate::extension_cli::install_tarball`].
//!
//! ## Usage
//!
//! ```sh
//! omegon extension install codex              # install latest
//! omegon extension install codex --version 0.6.4  # pin version
//! omegon extension search knowledge           # search by keyword
//! omegon extension list --available           # show all armory extensions
//! ```

use std::collections::HashMap;

use crate::update::{GitHubAsset, GitHubRelease};

/// Base URL for the upstream armory.
const ARMORY_BASE: &str = "https://raw.githubusercontent.com/styrene-lab/omegon-armory/main";

/// Parsed entry from armory `registry.toml`.
#[derive(serde::Deserialize)]
pub(crate) struct RegistryEntry {
    pub repo: String,
    pub description: String,
    pub category: String,
    #[allow(dead_code)]
    pub maintainer: String,
    #[allow(dead_code)]
    pub license: String,
    #[allow(dead_code)]
    pub min_sdk: Option<String>,
    /// Prefix for release asset filenames (defaults to extension name).
    pub asset_prefix: Option<String>,
}

impl RegistryEntry {
    /// Extract `owner/repo` slug from the repo URL.
    fn github_slug(&self) -> Option<&str> {
        self.repo
            .strip_prefix("https://github.com/")
            .map(|s| s.trim_end_matches('/').trim_end_matches(".git"))
    }
}

/// Fetch the extension registry from the armory.
pub(crate) async fn fetch_registry(
    client: &reqwest::Client,
) -> anyhow::Result<HashMap<String, RegistryEntry>> {
    let url = format!("{ARMORY_BASE}/registry.toml");
    let text = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let registry: HashMap<String, RegistryEntry> = toml::from_str(&text)?;
    Ok(registry)
}

/// Find the best matching asset for the current platform.
fn find_platform_asset<'a>(assets: &'a [GitHubAsset], prefix: &str) -> Option<&'a GitHubAsset> {
    let target = crate::update::platform_archive_target();

    // Try exact platform match first
    let exact = assets.iter().find(|a| {
        a.name.starts_with(prefix)
            && a.name.contains(&target)
            && a.name.ends_with(".tar.gz")
    });
    if exact.is_some() {
        return exact;
    }

    // On macOS, try universal binary as fallback
    if cfg!(target_os = "macos") {
        let universal = assets.iter().find(|a| {
            a.name.starts_with(prefix)
                && a.name.contains("universal-apple-darwin")
                && a.name.ends_with(".tar.gz")
        });
        if universal.is_some() {
            return universal;
        }
    }

    None
}

/// Install an extension by name from the armory registry.
pub async fn install_by_name(name: &str, version: Option<&str>) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("omegon")
        .build()?;

    println!("  Resolving {name} from armory...");

    let registry = fetch_registry(&client).await.map_err(|e| {
        anyhow::anyhow!(
            "Could not reach the omegon armory ({e}).\n\
             Install directly: omegon extension install <url>"
        )
    })?;

    let entry = registry.get(name).ok_or_else(|| {
        let available: Vec<&str> = {
            let mut keys: Vec<&str> = registry.keys().map(|k| k.as_str()).collect();
            keys.sort();
            keys
        };
        anyhow::anyhow!(
            "Extension '{name}' not found in armory.\n\
             Available: {}",
            available.join(", ")
        )
    })?;

    let slug = entry.github_slug().ok_or_else(|| {
        anyhow::anyhow!(
            "Extension '{name}' has no GitHub repo configured.\n\
             Install from source: omegon extension install {}",
            entry.repo
        )
    })?;

    // Fetch release from GitHub API
    let release_url = match version {
        Some(v) => {
            let tag = if v.starts_with('v') {
                v.to_string()
            } else {
                format!("v{v}")
            };
            format!("https://api.github.com/repos/{slug}/releases/tags/{tag}")
        }
        None => format!("https://api.github.com/repos/{slug}/releases/latest"),
    };

    let release: GitHubRelease = client
        .get(&release_url)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| {
            if let Some(v) = version {
                anyhow::anyhow!("Release v{v} not found for {slug}: {e}")
            } else {
                anyhow::anyhow!(
                    "No releases found for '{name}' ({slug}).\n\
                     Install from source: omegon extension install {}",
                    entry.repo
                )
            }
        })?
        .json()
        .await?;

    let ver = release.tag_name.trim_start_matches('v');
    let prefix = entry.asset_prefix.as_deref().unwrap_or(name);

    let asset = find_platform_asset(&release.assets, prefix).ok_or_else(|| {
        let target = crate::update::platform_archive_target();
        anyhow::anyhow!(
            "Extension '{name}' v{ver} has no pre-built binary for {target}.\n\
             Install from source: omegon extension install {}",
            entry.repo
        )
    })?;

    println!("  Found {name} v{ver} ({slug})");

    // Delegate to existing tarball install
    let extensions_dir = crate::extension_cli::extensions_dir()?;
    std::fs::create_dir_all(&extensions_dir)?;
    crate::extension_cli::install_tarball(&extensions_dir, &asset.browser_download_url)
}

/// List all extensions available in the armory, marking installed ones.
pub async fn list_available() -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("omegon")
        .build()?;

    let registry = fetch_registry(&client).await.map_err(|e| {
        anyhow::anyhow!(
            "Could not reach the omegon armory ({e}).\n\
             Check your network connection."
        )
    })?;

    // Check which are already installed
    let extensions_dir = crate::extension_cli::extensions_dir()?;
    let installed: std::collections::HashSet<String> = if extensions_dir.exists() {
        std::fs::read_dir(&extensions_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir() || e.path().is_symlink())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let mut entries: Vec<(&String, &RegistryEntry)> = registry.iter().collect();
    entries.sort_by_key(|(k, _)| k.as_str());

    println!("\nAvailable extensions (armory)\n");

    for (name, entry) in &entries {
        let marker = if installed.contains(name.as_str()) {
            "\x1b[32m  \u{2713}\x1b[0m"
        } else {
            "  \u{25cb}"
        };
        // Truncate description to fit
        let desc = if entry.description.len() > 60 {
            format!("{}...", &entry.description[..57])
        } else {
            entry.description.clone()
        };
        println!(
            "{} {:<12} {:<12} {}",
            marker, name, entry.category, desc
        );
    }

    println!("\n  Install: omegon extension install <name>\n");

    Ok(())
}

/// Search the armory registry by keyword.
pub async fn search(query: Option<&str>) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("omegon")
        .build()?;

    let registry = fetch_registry(&client).await.map_err(|e| {
        anyhow::anyhow!("Could not reach the omegon armory ({e}).")
    })?;

    let mut entries: Vec<(&String, &RegistryEntry)> = match query {
        Some(q) => {
            let q_lower = q.to_lowercase();
            registry
                .iter()
                .filter(|(name, entry)| {
                    name.to_lowercase().contains(&q_lower)
                        || entry.description.to_lowercase().contains(&q_lower)
                        || entry.category.to_lowercase().contains(&q_lower)
                })
                .collect()
        }
        None => registry.iter().collect(),
    };

    entries.sort_by_key(|(k, _)| k.as_str());

    if entries.is_empty() {
        println!("No extensions match '{}'.", query.unwrap_or(""));
        return Ok(());
    }

    println!(
        "\n{:<12} {:<12} DESCRIPTION",
        "NAME", "CATEGORY"
    );
    println!("{}", "\u{2500}".repeat(70));

    for (name, entry) in &entries {
        let desc = if entry.description.len() > 46 {
            format!("{}...", &entry.description[..43])
        } else {
            entry.description.clone()
        };
        println!("{:<12} {:<12} {}", name, entry.category, desc);
    }

    println!();
    Ok(())
}

/// Check whether a string looks like a bare extension name (not a URL or path).
pub fn is_bare_name(uri: &str) -> bool {
    !uri.contains('/')
        && !uri.contains("://")
        && !uri.ends_with(".tar.gz")
        && !uri.ends_with(".tgz")
        && !uri.ends_with(".git")
        && !std::path::Path::new(uri).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_bare_name_accepts_simple_names() {
        assert!(is_bare_name("codex"));
        assert!(is_bare_name("scribe"));
        assert!(is_bare_name("my-extension"));
    }

    #[test]
    fn is_bare_name_rejects_urls_and_paths() {
        assert!(!is_bare_name("https://github.com/foo/bar"));
        assert!(!is_bare_name("git@github.com:foo/bar.git"));
        assert!(!is_bare_name("./local/path"));
        assert!(!is_bare_name("https://example.com/ext.tar.gz"));
        assert!(!is_bare_name("/absolute/path"));
    }

    #[test]
    fn github_slug_extracts_owner_repo() {
        let entry = RegistryEntry {
            repo: "https://github.com/styrene-lab/flynt".into(),
            description: String::new(),
            category: String::new(),
            maintainer: String::new(),
            license: String::new(),
            min_sdk: None,
            asset_prefix: None,
        };
        assert_eq!(entry.github_slug(), Some("styrene-lab/flynt"));
    }

    #[test]
    fn github_slug_handles_trailing_slash() {
        let entry = RegistryEntry {
            repo: "https://github.com/styrene-lab/vox/".into(),
            description: String::new(),
            category: String::new(),
            maintainer: String::new(),
            license: String::new(),
            min_sdk: None,
            asset_prefix: None,
        };
        assert_eq!(entry.github_slug(), Some("styrene-lab/vox"));
    }

    #[test]
    fn github_slug_returns_none_for_non_github() {
        let entry = RegistryEntry {
            repo: "https://gitlab.com/foo/bar".into(),
            description: String::new(),
            category: String::new(),
            maintainer: String::new(),
            license: String::new(),
            min_sdk: None,
            asset_prefix: None,
        };
        assert_eq!(entry.github_slug(), None);
    }

    #[test]
    fn find_platform_asset_matches_target() {
        let assets = vec![
            GitHubAsset {
                name: "codex-agent-0.6.4-x86_64-unknown-linux-gnu.tar.gz".into(),
                browser_download_url: "https://example.com/linux.tar.gz".into(),
            },
            GitHubAsset {
                name: "codex-agent-0.6.4-universal-apple-darwin.tar.gz".into(),
                browser_download_url: "https://example.com/mac.tar.gz".into(),
            },
            GitHubAsset {
                name: "Codyx-0.6.4-macos.dmg".into(),
                browser_download_url: "https://example.com/app.dmg".into(),
            },
        ];

        let result = find_platform_asset(&assets, "codex-agent");
        assert!(result.is_some(), "should find a matching asset");
        let asset = result.unwrap();
        assert!(asset.name.ends_with(".tar.gz"));
        assert!(asset.name.starts_with("codex-agent"));
    }

    #[test]
    fn find_platform_asset_ignores_non_tarballs() {
        let assets = vec![GitHubAsset {
            name: "Codyx-0.6.4-macos.dmg".into(),
            browser_download_url: "https://example.com/app.dmg".into(),
        }];

        assert!(find_platform_asset(&assets, "codex-agent").is_none());
    }
}
