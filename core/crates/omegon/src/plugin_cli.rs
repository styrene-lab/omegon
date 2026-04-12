//! Plugin lifecycle management — install, list, remove, update.
//!
//! Plugins are git repositories cloned into `~/.omegon/plugins/<name>/`.
//! Each plugin must have a `plugin.toml` manifest at the root.
//!
//! ## Install
//!
//! ```sh
//! omegon plugin install https://github.com/user/my-plugin
//! omegon plugin install ./local/path/to/plugin
//! ```
//!
//! Git URIs are cloned. Local paths are symlinked (development mode).
//!
//! ## List
//!
//! ```sh
//! omegon plugin list
//! ```
//!
//! ## Remove
//!
//! ```sh
//! omegon plugin remove my-plugin
//! ```
//!
//! ## Update
//!
//! ```sh
//! omegon plugin update [name]
//! ```
//!
//! Runs `git pull` in the plugin directory. Without a name, updates all.

use std::path::{Path, PathBuf};

use crate::plugins::armory::ArmoryManifest;

/// Install a plugin from a git URI or local path.
pub fn install(uri: &str) -> anyhow::Result<()> {
    let plugins_dir = plugins_dir()?;
    std::fs::create_dir_all(&plugins_dir)?;

    let local_path = Path::new(uri);

    if local_path.exists() && local_path.join("plugin.toml").exists() {
        // Local path — symlink for development
        install_local(&plugins_dir, local_path)
    } else if uri.contains("://") || uri.contains("git@") || uri.ends_with(".git") {
        // Git URI — clone
        install_git(&plugins_dir, uri)
    } else {
        anyhow::bail!(
            "'{uri}' is not a valid plugin source.\n\
             Expected: a git URL or a local directory containing plugin.toml"
        );
    }
}

/// Render all installed plugins as terminal-friendly text.
pub fn list_summary() -> anyhow::Result<String> {
    let plugins_dir = plugins_dir()?;

    if !plugins_dir.exists() {
        return Ok("No plugins installed.\n  Install with: omegon plugin install <git-url>".into());
    }

    let entries: Vec<_> = std::fs::read_dir(&plugins_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() || e.path().is_symlink())
        .collect();

    if entries.is_empty() {
        return Ok("No plugins installed.".into());
    }

    let mut lines = vec![
        format!(
            "{:<20} {:<12} {:<10} DESCRIPTION",
            "NAME", "TYPE", "VERSION"
        ),
        "─".repeat(72),
    ];

    for entry in &entries {
        let dir = entry.path();
        // Follow symlinks
        let resolved = if dir.is_symlink() {
            std::fs::read_link(&dir).unwrap_or(dir.clone())
        } else {
            dir.clone()
        };

        let manifest_path = resolved.join("plugin.toml");
        if !manifest_path.exists() {
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            lines.push(format!(
                "{:<20} {:<12} {:<10} (no plugin.toml)",
                name, "?", "?"
            ));
            continue;
        }

        match load_manifest_summary(&manifest_path) {
            Ok(info) => {
                let symlink_marker = if dir.is_symlink() { " →" } else { "" };
                lines.push(format!(
                    "{:<20} {:<12} {:<10} {}{}",
                    info.name, info.plugin_type, info.version, info.description, symlink_marker
                ));
            }
            Err(e) => {
                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                lines.push(format!("{:<20} {:<12} {:<10} (error: {e})", name, "?", "?"));
            }
        }
    }

    let symlinks = entries.iter().filter(|e| e.path().is_symlink()).count();
    if symlinks > 0 {
        lines.push("\n  → = symlinked (development mode)".into());
    }

    Ok(lines.join("\n"))
}

/// List all installed plugins.
pub fn list() -> anyhow::Result<()> {
    println!("{}", list_summary()?);
    Ok(())
}

/// Remove an installed plugin by name.
pub fn remove(name: &str) -> anyhow::Result<()> {
    let plugins_dir = plugins_dir()?;
    let plugin_path = plugins_dir.join(name);

    if !plugin_path.exists() {
        anyhow::bail!("Plugin '{}' not found in {}", name, plugins_dir.display());
    }

    if plugin_path.is_symlink() {
        // Symlink — just remove the link
        std::fs::remove_file(&plugin_path)?;
        println!("Removed symlink: {name}");
    } else {
        // Cloned repo — remove directory
        std::fs::remove_dir_all(&plugin_path)?;
        println!("Removed plugin: {name}");
    }

    Ok(())
}

/// Update a plugin (or all plugins) by running `git pull`.
pub fn update(name: Option<&str>) -> anyhow::Result<()> {
    let plugins_dir = plugins_dir()?;

    if !plugins_dir.exists() {
        println!("No plugins installed.");
        return Ok(());
    }

    let dirs_to_update: Vec<PathBuf> = if let Some(name) = name {
        let path = plugins_dir.join(name);
        if !path.exists() {
            anyhow::bail!("Plugin '{}' not found", name);
        }
        vec![path]
    } else {
        std::fs::read_dir(&plugins_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !p.is_symlink())
            .collect()
    };

    if dirs_to_update.is_empty() {
        println!("No updatable plugins (symlinked plugins are managed externally).");
        return Ok(());
    }

    for dir in &dirs_to_update {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        let git_dir = dir.join(".git");

        if !git_dir.exists() {
            println!("  {name}: skipped (not a git repo)");
            continue;
        }

        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .arg("pull")
            .output()?;

        if output.status.success() {
            println!("  {name}: updated");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("  {name}: failed — {}", stderr.trim());
        }
    }

    Ok(())
}

fn plugins_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".omegon").join("plugins"))
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))
}

fn install_local(plugins_dir: &Path, local_path: &Path) -> anyhow::Result<()> {
    let manifest = std::fs::read_to_string(local_path.join("plugin.toml"))?;
    let parsed = ArmoryManifest::parse(&manifest)?;
    let name = &parsed.plugin.name;

    let target = plugins_dir.join(name);
    if target.exists() || target.is_symlink() {
        std::fs::remove_file(&target).or_else(|_| std::fs::remove_dir_all(&target))?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(local_path, &target)?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(local_path, &target)?;

    println!("Linked local plugin '{}' → {}", name, local_path.display());
    Ok(())
}

fn install_git(plugins_dir: &Path, uri: &str) -> anyhow::Result<()> {
    let name = infer_plugin_name(uri)?;
    let target = plugins_dir.join(&name);

    if target.exists() {
        anyhow::bail!("Plugin '{}' already exists at {}", name, target.display());
    }

    let status = std::process::Command::new("git")
        .arg("clone")
        .arg(uri)
        .arg(&target)
        .status()?;

    if !status.success() {
        anyhow::bail!("git clone failed for {uri}");
    }

    let manifest_path = target.join("plugin.toml");
    if !manifest_path.exists() {
        std::fs::remove_dir_all(&target).ok();
        anyhow::bail!("cloned repository does not contain plugin.toml");
    }

    let manifest = std::fs::read_to_string(&manifest_path)?;
    let parsed = ArmoryManifest::parse(&manifest)?;
    if parsed.plugin.name != name {
        println!(
            "Note: repository inferred name '{}' but manifest declares '{}'.",
            name, parsed.plugin.name
        );
    }

    println!("Installed plugin '{}' from {uri}", parsed.plugin.name);
    Ok(())
}

fn infer_plugin_name(uri: &str) -> anyhow::Result<String> {
    let stripped = uri.trim_end_matches('/').trim_end_matches(".git");
    let name = stripped
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .or_else(|| stripped.rsplit_once(':').map(|(_, tail)| tail))
        .ok_or_else(|| anyhow::anyhow!("could not infer plugin name from URI: {uri}"))?;

    if name.is_empty() {
        anyhow::bail!("could not infer plugin name from URI: {uri}");
    }

    Ok(name.to_string())
}

struct ManifestSummary {
    name: String,
    plugin_type: String,
    version: String,
    description: String,
}

fn load_manifest_summary(path: &Path) -> anyhow::Result<ManifestSummary> {
    let content = std::fs::read_to_string(path)?;
    let manifest =
        ArmoryManifest::parse(&content).map_err(|e| anyhow::anyhow!("parse error: {e}"))?;

    Ok(ManifestSummary {
        name: manifest.plugin.name.clone(),
        plugin_type: manifest.plugin.plugin_type.to_string(),
        version: manifest.plugin.version.clone(),
        description: manifest.plugin.description.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_plugin_name_from_https_git() {
        let name = infer_plugin_name("https://github.com/user/my-plugin.git").unwrap();
        assert_eq!(name, "my-plugin");
    }

    #[test]
    fn infer_plugin_name_from_ssh_git() {
        let name = infer_plugin_name("git@github.com:user/my-plugin.git").unwrap();
        assert_eq!(name, "my-plugin");
    }

    #[test]
    fn infer_plugin_name_from_local_path() {
        let name = infer_plugin_name("./plugins/my-plugin").unwrap();
        assert_eq!(name, "my-plugin");
    }

    #[test]
    fn list_summary_reports_empty_installation() {
        let summary = list_summary().unwrap();
        assert!(summary.contains("No plugins installed") || summary.contains("DESCRIPTION"));
    }

    #[test]
    fn install_rejects_invalid_uri() {
        let err = install("not-a-uri").unwrap_err();
        assert!(err.to_string().contains("not a valid plugin source"));
    }

    #[test]
    fn install_local_symlinks_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin = tmp.path().join("example-plugin");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("plugin.toml"),
            r#"
[plugin]
type = "persona"
id = "dev.styrene.example-plugin"
name = "example-plugin"
version = "0.1.0"
description = "Example"

[persona.identity]
directive = "plugin.md"
"#,
        )
        .unwrap();
        std::fs::write(plugin.join("plugin.md"), "# Example").unwrap();

        let plugins = tempfile::tempdir().unwrap();
        install_local(plugins.path(), &plugin).unwrap();

        let link = plugins.path().join("example-plugin");
        assert!(link.exists(), "symlink should exist");
        assert!(link.is_symlink(), "should be a symlink");
    }
}
