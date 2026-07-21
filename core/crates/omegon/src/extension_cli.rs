//! Extension lifecycle management — install, list, remove, update, enable, disable.
//!
//! Extensions are native binaries or OCI containers installed into
//! `~/.omegon/extensions/<name>/`.  Each extension must have a
//! `manifest.toml` at the root.
//!
//! ## Install
//!
//! ```sh
//! omegon extension install https://github.com/user/my-extension
//! omegon extension install ./local/path/to/extension
//! omegon extension install https://example.com/my-extension-v1.0-aarch64-apple-darwin.tar.gz
//! ```
//!
//! Git URIs are cloned. Local paths are symlinked (development mode).
//! Tarball URLs (.tar.gz) are downloaded and extracted — no build step required.
//!
//! ## List
//!
//! ```sh
//! omegon extension list
//! ```
//!
//! ## Remove
//!
//! ```sh
//! omegon extension remove my-extension
//! ```
//!
//! ## Update
//!
//! ```sh
//! omegon extension update [name]
//! ```
//!
//! ## Enable / Disable
//!
//! ```sh
//! omegon extension enable my-extension
//! omegon extension disable my-extension
//! ```

use std::path::{Path, PathBuf};

use crate::extensions::manifest::ExtensionManifest;
use crate::extensions::state::ExtensionState;

/// Scaffold a new extension project.
pub fn init(name: &str) -> anyhow::Result<()> {
    // Validate name
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!("Extension name must be lowercase alphanumeric + hyphens (got '{name}')");
    }

    let dir = Path::new(name);
    if dir.exists() {
        anyhow::bail!("Directory '{name}' already exists");
    }

    std::fs::create_dir_all(dir.join("src"))?;

    // manifest.toml
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            r#"[extension]
name = "{name}"
version = "0.1.0"
description = "TODO: describe your extension"

[runtime]
type = "native"
binary = "target/release/{name}"

[startup]
ping_method = "get_tools"
timeout_ms = 5000

# [secrets]
# required = []
# optional = []

# [widgets.my-widget]
# label = "My Widget"
# kind = "stateful"
# renderer = "table"
"#
        ),
    )?;

    // Cargo.toml
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
omegon-extension = {{ git = "https://github.com/styrene-lab/omegon" }}
serde_json = "1"
tokio = {{ version = "1", features = ["rt", "macros", "io-util"] }}
async-trait = "0.1"
"#
        ),
    )?;

    // src/main.rs
    let struct_name = name
        .split('-')
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<String>();

    std::fs::write(
        dir.join("src").join("main.rs"),
        format!(
            r#"use omegon_extension::{{Extension, serve}};
use serde_json::{{json, Value}};
use async_trait::async_trait;

#[derive(Default)]
struct {struct_name};

#[async_trait]
impl Extension for {struct_name} {{
    fn name(&self) -> &str {{
        "{name}"
    }}

    fn version(&self) -> &str {{
        env!("CARGO_PKG_VERSION")
    }}

    async fn handle_rpc(
        &self,
        method: &str,
        params: Value,
    ) -> omegon_extension::Result<Value> {{
        match method {{
            "get_tools" => Ok(json!([
                {{
                    "name": "hello",
                    "label": "Hello",
                    "description": "A greeting tool - replace this with your own",
                    "parameters": {{
                        "type": "object",
                        "properties": {{
                            "name": {{"type": "string", "description": "Who to greet"}}
                        }},
                        "required": ["name"]
                    }}
                }}
            ])),
            "execute_tool" => {{
                let tool_name = params.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if tool_name != "hello" {{
                    return Err(omegon_extension::Error::method_not_found(tool_name));
                }}
                let args = params.get("args").cloned().unwrap_or_default();
                let who = args.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("World");
                Ok(json!({{
                    "content": [{{
                        "type": "text",
                        "text": format!("Hello, {{}}!", who)
                    }}]
                }}))
            }}
            _ => Err(omegon_extension::Error::method_not_found(method)),
        }}
    }}
}}

#[tokio::main]
async fn main() {{
    serve({struct_name}::default()).await.unwrap();
}}
"#
        ),
    )?;

    println!("Created extension '{name}' in ./{name}/");
    println!();
    println!("  Next steps:");
    println!("    cd {name}");
    println!("    cargo build --release");
    println!("    omegon extension install .");
    println!();
    println!("  Then restart omegon — your extension will be loaded automatically.");

    Ok(())
}

/// Install an extension from a git URI or local path.
pub fn install(uri: &str) -> anyhow::Result<()> {
    let extensions_dir = extensions_dir()?;
    std::fs::create_dir_all(&extensions_dir)?;

    let local_path = Path::new(uri);

    if local_path.exists() && local_path.join("manifest.toml").exists() {
        install_local(&extensions_dir, local_path)
    } else if uri.ends_with(".tar.gz") || uri.ends_with(".tgz") {
        install_tarball(&extensions_dir, uri)
    } else if uri.contains("://") || uri.contains("git@") || uri.ends_with(".git") {
        install_git(&extensions_dir, uri)
    } else {
        anyhow::bail!(
            "'{uri}' is not a valid extension source.\n\
             Expected: a git URL, a tarball URL (.tar.gz), or a local directory containing manifest.toml"
        );
    }
}

/// Render all installed extensions as terminal-friendly text.
pub fn list_summary() -> anyhow::Result<String> {
    let extensions_dir = extensions_dir()?;

    if !extensions_dir.exists() {
        return Ok(
            "No extensions installed.\n  Install with: omegon extension install <git-url-or-path>"
                .into(),
        );
    }

    let entries: Vec<_> = std::fs::read_dir(&extensions_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() || e.path().is_symlink())
        .collect();

    if entries.is_empty() {
        return Ok("No extensions installed.".into());
    }

    let mut lines = vec![
        format!(
            "{:<20} {:<10} {:<10} {:<12} DESCRIPTION",
            "NAME", "VERSION", "RUNTIME", "STATUS"
        ),
        "─".repeat(80),
    ];

    for entry in &entries {
        let dir = entry.path();
        let resolved = if dir.is_symlink() {
            std::fs::read_link(&dir).unwrap_or(dir.clone())
        } else {
            dir.clone()
        };

        let manifest_path = resolved.join("manifest.toml");
        if !manifest_path.exists() {
            let name = dir.file_name().unwrap_or_default().to_string_lossy();
            lines.push(format!(
                "{:<20} {:<10} {:<10} {:<12} (no manifest.toml)",
                name, "?", "?", "?"
            ));
            continue;
        }

        match load_extension_summary(&resolved) {
            Ok(info) => {
                let symlink_marker = if dir.is_symlink() { " →" } else { "" };
                lines.push(format!(
                    "{:<20} {:<10} {:<10} {:<12} {}{}",
                    info.name,
                    info.version,
                    info.runtime,
                    info.status,
                    info.description,
                    symlink_marker
                ));
            }
            Err(e) => {
                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                lines.push(format!(
                    "{:<20} {:<10} {:<10} {:<12} (error: {e})",
                    name, "?", "?", "?"
                ));
            }
        }
    }

    let symlinks = entries.iter().filter(|e| e.path().is_symlink()).count();
    if symlinks > 0 {
        lines.push("\n  → = symlinked (development mode)".into());
    }

    Ok(lines.join("\n"))
}

/// List all installed extensions.
pub fn list() -> anyhow::Result<()> {
    println!("{}", list_summary()?);
    Ok(())
}

/// Remove an installed extension by name.
pub fn remove(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    let extensions_dir = extensions_dir()?;
    let ext_path = extensions_dir.join(name);

    if !ext_path.exists() && !ext_path.is_symlink() {
        anyhow::bail!(
            "Extension '{}' not found in {}",
            name,
            extensions_dir.display()
        );
    }

    if ext_path.is_symlink() {
        std::fs::remove_file(&ext_path)?;
        println!("Removed symlink: {name}");
    } else {
        std::fs::remove_dir_all(&ext_path)?;
        println!("Removed extension: {name}");
    }

    Ok(())
}

/// Update an extension (or all extensions) by running `git pull`.
pub fn update(name: Option<&str>) -> anyhow::Result<()> {
    let extensions_dir = extensions_dir()?;

    if !extensions_dir.exists() {
        println!("No extensions installed.");
        return Ok(());
    }

    let dirs_to_update: Vec<PathBuf> = if let Some(name) = name {
        validate_name(name)?;
        let path = extensions_dir.join(name);
        if !path.exists() {
            anyhow::bail!("Extension '{}' not found", name);
        }
        vec![path]
    } else {
        std::fs::read_dir(&extensions_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !p.is_symlink())
            .collect()
    };

    if dirs_to_update.is_empty() {
        println!("No updatable extensions (symlinked extensions are managed externally).");
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
            let pull_out = String::from_utf8_lossy(&output.stdout);
            let already_up_to_date = pull_out.contains("Already up to date");

            // Rebuild native Cargo extensions if git pull brought changes
            if !already_up_to_date && dir.join("Cargo.toml").exists() {
                let manifest_path = dir.join("manifest.toml");
                let is_native = manifest_path
                    .exists()
                    .then(|| ExtensionManifest::from_file(&manifest_path).ok())
                    .flatten()
                    .is_some_and(|m| m.is_native());

                if is_native {
                    print!("  {name}: rebuilding... ");
                    let build = std::process::Command::new("cargo")
                        .arg("build")
                        .arg("--release")
                        .current_dir(dir)
                        .status();
                    match build {
                        Ok(s) if s.success() => println!("updated + rebuilt"),
                        _ => println!(
                            "updated but rebuild failed — run cargo build --release manually"
                        ),
                    }
                    continue;
                }
            }

            println!("  {name}: updated");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("  {name}: failed — {}", stderr.trim());
        }
    }

    Ok(())
}

/// Enable a disabled extension.
pub fn enable(name: &str) -> anyhow::Result<()> {
    let ext_dir = extension_dir(name)?;
    let mut state = ExtensionState::load(&ext_dir)?;

    if state.enabled {
        println!("Extension '{name}' is already enabled.");
        return Ok(());
    }

    state.mark_enabled();
    state.save(&ext_dir)?;
    println!("Enabled extension '{name}'.");
    Ok(())
}

/// Disable an extension (prevents spawning on next startup).
pub fn disable(name: &str) -> anyhow::Result<()> {
    let ext_dir = extension_dir(name)?;
    let mut state = ExtensionState::load(&ext_dir)?;

    if !state.enabled {
        println!("Extension '{name}' is already disabled.");
        return Ok(());
    }

    state.mark_disabled();
    state.save(&ext_dir)?;
    println!("Disabled extension '{name}'.");
    Ok(())
}

pub(crate) fn extensions_dir() -> anyhow::Result<PathBuf> {
    let base = crate::paths::omegon_home()?;
    Ok(base.join("extensions"))
}

/// Validate that an extension name is safe for use as a directory component.
/// Rejects path traversal attempts and any non-filesystem-safe characters.
fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("extension name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
        anyhow::bail!(
            "invalid extension name '{name}': must not contain '/', '\\', '..', or null bytes"
        );
    }
    // Reject absolute paths on Windows (e.g. "C:")
    if name.contains(':') {
        anyhow::bail!("invalid extension name '{name}': must not contain ':'");
    }
    Ok(())
}

fn extension_dir(name: &str) -> anyhow::Result<PathBuf> {
    validate_name(name)?;
    let dir = extensions_dir()?.join(name);
    if !dir.exists() {
        anyhow::bail!("Extension '{name}' not found at {}", dir.display());
    }
    Ok(dir)
}

fn install_local(extensions_dir: &Path, local_path: &Path) -> anyhow::Result<()> {
    let manifest = ExtensionManifest::from_file(&local_path.join("manifest.toml"))?;
    let name = &manifest.extension.name;

    // Verify binary exists for native extensions
    if manifest.is_native() {
        match manifest.native_binary_path(local_path) {
            Ok(_) => {}
            Err(_) => {
                println!(
                    "Warning: native binary not found. Build with `cargo build --release` before running."
                );
            }
        }
    }

    let target = extensions_dir.join(name);
    if target.exists() || target.is_symlink() {
        anyhow::bail!(
            "Extension '{}' already installed at {}. Remove first with: omegon extension remove {}",
            name,
            target.display(),
            name
        );
    }

    let canonical = std::fs::canonicalize(local_path)?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(&canonical, &target)?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&canonical, &target)?;

    println!("Linked extension '{}' → {}", name, canonical.display());

    print_secrets_hint(&manifest);

    Ok(())
}

fn install_git(extensions_dir: &Path, uri: &str) -> anyhow::Result<()> {
    let name = infer_extension_name(uri)?;
    let target = extensions_dir.join(&name);

    if target.exists() {
        anyhow::bail!(
            "Extension '{}' already exists at {}",
            name,
            target.display()
        );
    }

    let status = std::process::Command::new("git")
        .arg("clone")
        .arg(uri)
        .arg(&target)
        .status()?;

    if !status.success() {
        anyhow::bail!(
            "git clone failed for {uri}\n  \
             Check: URL is correct, you have network access, and git credentials are configured."
        );
    }

    let manifest_path = target.join("manifest.toml");
    if !manifest_path.exists() {
        std::fs::remove_dir_all(&target).ok();
        anyhow::bail!("cloned repository does not contain manifest.toml");
    }

    let manifest = ExtensionManifest::from_file(&manifest_path)?;
    if manifest.extension.name != name {
        println!(
            "Note: inferred name '{}' but manifest declares '{}'.",
            name, manifest.extension.name
        );
    }

    // Build native extensions that have a Cargo.toml
    if manifest.is_native() && target.join("Cargo.toml").exists() {
        println!("Building extension '{}'...", manifest.extension.name);
        let build = std::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .current_dir(&target)
            .status();

        match build {
            Ok(s) if s.success() => {
                println!("Build succeeded.");
            }
            Ok(s) => {
                std::fs::remove_dir_all(&target).ok();
                anyhow::bail!(
                    "cargo build --release failed (exit {}) for extension '{}'",
                    s.code().unwrap_or(-1),
                    manifest.extension.name
                );
            }
            Err(e) => {
                std::fs::remove_dir_all(&target).ok();
                anyhow::bail!(
                    "failed to run cargo build for extension '{}': {e}",
                    manifest.extension.name
                );
            }
        }
    } else if manifest.is_native() {
        match manifest.native_binary_path(&target) {
            Ok(_) => {}
            Err(_) => {
                println!(
                    "Warning: native binary not found. Build manually in the extension directory."
                );
            }
        }
    }

    println!(
        "Installed extension '{}' from {uri}",
        manifest.extension.name
    );
    print_secrets_hint(&manifest);

    Ok(())
}

/// Install a pre-built extension from a tarball URL or local .tar.gz file.
///
/// The tarball must contain a `manifest.toml` and (for native extensions) the
/// pre-built binary.  No build step is performed — this is the path for users
/// without a Rust toolchain.
pub(crate) fn install_tarball(extensions_dir: &Path, uri: &str) -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "omegon-ext-install-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp)?;
    let archive_path = tmp.join("extension.tar.gz");

    if uri.starts_with("http://") || uri.starts_with("https://") {
        // Download
        println!("Downloading {uri}...");
        let status = std::process::Command::new("curl")
            .args(["-fSL", "-o"])
            .arg(&archive_path)
            .arg(uri)
            .status()?;
        if !status.success() {
            anyhow::bail!("download failed for {uri}");
        }
    } else {
        // Local tarball
        let local = Path::new(uri);
        if !local.exists() {
            anyhow::bail!("tarball not found: {uri}");
        }
        std::fs::copy(local, &archive_path)?;
    }

    // Extract
    let extract_dir = tmp.join("extracted");
    std::fs::create_dir_all(&extract_dir)?;
    let status = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(&archive_path)
        .arg("-C")
        .arg(&extract_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("failed to extract tarball");
    }

    // Find manifest.toml — may be at root or one level deep
    let manifest_path = if extract_dir.join("manifest.toml").exists() {
        extract_dir.join("manifest.toml")
    } else {
        // Check one level deep (tarball may have a top-level directory)
        let mut found = None;
        for entry in std::fs::read_dir(&extract_dir)? {
            let entry = entry?;
            if entry.path().is_dir() && entry.path().join("manifest.toml").exists() {
                found = Some(entry.path().join("manifest.toml"));
                break;
            }
        }
        found.ok_or_else(|| anyhow::anyhow!("tarball does not contain manifest.toml"))?
    };

    let ext_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid manifest path"))?;
    let manifest = ExtensionManifest::from_file(&manifest_path)?;
    let name = &manifest.extension.name;

    let target = extensions_dir.join(name);
    if target.exists() || target.is_symlink() {
        anyhow::bail!(
            "Extension '{}' already installed at {}. Remove first with: omegon extension remove {}",
            name,
            target.display(),
            name
        );
    }

    // Copy extracted contents to extensions dir (not symlink — this is a real install)
    copy_dir_recursive(ext_root, &target)?;

    // Make native binary executable
    if manifest.is_native() {
        if let Ok(bin_path) = manifest.native_binary_path(&target) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&bin_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&bin_path, perms)?;
            }
            println!(
                "Installed extension '{}' v{} (pre-built binary)",
                name, manifest.extension.version
            );
        } else {
            println!(
                "Installed extension '{}' v{} (warning: native binary not found in tarball)",
                name, manifest.extension.version
            );
        }
    } else {
        println!(
            "Installed extension '{}' v{}",
            name, manifest.extension.version
        );
    }

    print_secrets_hint(&manifest);

    // Clean up temp dir
    std::fs::remove_dir_all(&tmp).ok();

    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

fn infer_extension_name(uri: &str) -> anyhow::Result<String> {
    let stripped = uri.trim_end_matches('/').trim_end_matches(".git");
    let name = stripped
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .or_else(|| stripped.rsplit_once(':').map(|(_, tail)| tail))
        .ok_or_else(|| anyhow::anyhow!("could not infer extension name from URI: {uri}"))?;

    if name.is_empty() {
        anyhow::bail!("could not infer extension name from URI: {uri}");
    }

    Ok(name.to_string())
}

fn print_secrets_hint(manifest: &ExtensionManifest) {
    let all_secrets: Vec<&String> = manifest
        .secrets
        .required
        .iter()
        .chain(manifest.secrets.optional.iter())
        .collect();

    if all_secrets.is_empty() {
        return;
    }

    println!();
    if !manifest.secrets.required.is_empty() {
        println!("Required secrets:");
        for s in &manifest.secrets.required {
            println!("  omegon secret set {s} <value>");
        }
    }
    if !manifest.secrets.optional.is_empty() {
        println!("Optional secrets (for additional connectors):");
        for s in &manifest.secrets.optional {
            println!("  omegon secret set {s} <value>");
        }
    }
}

struct ExtensionSummary {
    name: String,
    version: String,
    runtime: String,
    status: String,
    description: String,
}

fn load_extension_summary(dir: &Path) -> anyhow::Result<ExtensionSummary> {
    let manifest = ExtensionManifest::from_extension_dir(dir)?;
    let state = ExtensionState::load(dir)?;

    let runtime = if manifest.is_native() {
        "native"
    } else {
        "oci"
    };

    Ok(ExtensionSummary {
        name: manifest.extension.name,
        version: manifest.extension.version,
        runtime: runtime.to_string(),
        status: state.status_text(),
        description: manifest.extension.description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_extension_name_from_https() {
        let name = infer_extension_name("https://github.com/styrene-lab/vox.git").unwrap();
        assert_eq!(name, "vox");
    }

    #[test]
    fn infer_extension_name_from_ssh() {
        let name = infer_extension_name("git@github.com:styrene-lab/vox.git").unwrap();
        assert_eq!(name, "vox");
    }

    #[test]
    fn infer_extension_name_from_local() {
        let name = infer_extension_name("./extensions/vox").unwrap();
        assert_eq!(name, "vox");
    }

    #[test]
    fn install_rejects_invalid_uri() {
        let err = install("not-a-uri").unwrap_err();
        assert!(err.to_string().contains("not a valid extension source"));
    }

    #[test]
    fn list_summary_handles_missing_dir() {
        let summary = list_summary().unwrap();
        // Either reports extensions or says none installed
        assert!(summary.contains("extension") || summary.contains("DESCRIPTION"));
    }

    #[test]
    fn remove_rejects_path_traversal() {
        let err = remove("../../.ssh").unwrap_err();
        assert!(err.to_string().contains("must not contain"));
    }

    #[test]
    fn remove_rejects_slash_in_name() {
        let err = remove("foo/bar").unwrap_err();
        assert!(err.to_string().contains("must not contain"));
    }

    #[test]
    fn validate_name_rejects_empty() {
        let err = validate_name("").unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn validate_name_accepts_normal_names() {
        validate_name("vox").unwrap();
        validate_name("scribe-rpc").unwrap();
        validate_name("my_extension.v2").unwrap();
    }

    #[test]
    fn enable_disable_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let ext = tmp.path().join("test-ext");
        std::fs::create_dir_all(ext.join(".omegon")).unwrap();
        std::fs::write(
            ext.join("manifest.toml"),
            r#"
[extension]
name = "test-ext"
version = "0.1.0"
description = "Test"

[runtime]
type = "native"
binary = "bin/test"
"#,
        )
        .unwrap();

        // Start enabled (default)
        let state = ExtensionState::load(&ext).unwrap();
        assert!(state.enabled);

        // Disable
        let mut state = ExtensionState::load(&ext).unwrap();
        state.mark_disabled();
        state.save(&ext).unwrap();

        let state = ExtensionState::load(&ext).unwrap();
        assert!(!state.enabled);
        assert_eq!(state.status_text(), "disabled");

        // Re-enable
        let mut state = ExtensionState::load(&ext).unwrap();
        state.mark_enabled();
        state.save(&ext).unwrap();

        let state = ExtensionState::load(&ext).unwrap();
        assert!(state.enabled);
        assert_eq!(state.status_text(), "enabled");
    }

    #[test]
    fn install_local_symlinks_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let ext = tmp.path().join("test-ext");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::write(
            ext.join("manifest.toml"),
            r#"
[extension]
name = "test-ext"
version = "0.1.0"
description = "Test extension"

[runtime]
type = "native"
binary = "target/release/test-ext"
"#,
        )
        .unwrap();

        let ext_dir = tempfile::tempdir().unwrap();
        install_local(ext_dir.path(), &ext).unwrap();

        let link = ext_dir.path().join("test-ext");
        assert!(link.exists(), "symlink should exist");
        assert!(link.is_symlink(), "should be a symlink");
    }

    #[test]
    fn copy_dir_recursive_copies_files_and_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "hello").unwrap();
        std::fs::write(src.join("sub/b.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
            "world"
        );
    }

    #[test]
    fn install_tarball_from_local_file() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a tarball containing manifest.toml + a fake binary
        let staging = tmp.path().join("my-ext");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(
            staging.join("manifest.toml"),
            r#"
[extension]
name = "my-ext"
version = "1.0.0"
description = "Test tarball extension"

[runtime]
type = "native"
binary = "my-ext"
"#,
        )
        .unwrap();
        // Write a fake binary (just needs to exist)
        std::fs::write(staging.join("my-ext"), "#!/bin/sh\necho ok").unwrap();

        // Create .tar.gz
        let tarball = tmp.path().join("my-ext.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["czf"])
            .arg(&tarball)
            .arg("-C")
            .arg(tmp.path())
            .arg("my-ext")
            .status()
            .unwrap();
        assert!(status.success(), "tar should succeed");

        // Install into a fresh extensions dir
        let ext_dir = tempfile::tempdir().unwrap();
        install_tarball(ext_dir.path(), tarball.to_str().unwrap()).unwrap();

        let installed = ext_dir.path().join("my-ext");
        assert!(installed.exists(), "extension dir should exist");
        assert!(
            installed.join("manifest.toml").exists(),
            "manifest should exist"
        );
        assert!(installed.join("my-ext").exists(), "binary should exist");

        // Verify it's a real copy, not a symlink
        assert!(!installed.is_symlink(), "should not be a symlink");
    }

    #[test]
    fn install_tarball_rejects_missing_manifest() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a tarball with no manifest.toml
        let staging = tmp.path().join("bad-ext");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("README.md"), "no manifest here").unwrap();

        let tarball = tmp.path().join("bad-ext.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["czf"])
            .arg(&tarball)
            .arg("-C")
            .arg(tmp.path())
            .arg("bad-ext")
            .status()
            .unwrap();
        assert!(status.success());

        let ext_dir = tempfile::tempdir().unwrap();
        let err = install_tarball(ext_dir.path(), tarball.to_str().unwrap()).unwrap_err();
        assert!(
            err.to_string().contains("manifest.toml"),
            "should mention missing manifest: {}",
            err
        );
    }

    #[test]
    fn install_tarball_rejects_duplicate() {
        let tmp = tempfile::tempdir().unwrap();

        let staging = tmp.path().join("dup-ext");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(
            staging.join("manifest.toml"),
            r#"
[extension]
name = "dup-ext"
version = "1.0.0"
description = "Duplicate test"

[runtime]
type = "native"
binary = "dup-ext"
"#,
        )
        .unwrap();
        std::fs::write(staging.join("dup-ext"), "fake").unwrap();

        let tarball = tmp.path().join("dup-ext.tar.gz");
        std::process::Command::new("tar")
            .args(["czf"])
            .arg(&tarball)
            .arg("-C")
            .arg(tmp.path())
            .arg("dup-ext")
            .status()
            .unwrap();

        let ext_dir = tempfile::tempdir().unwrap();

        // First install succeeds
        install_tarball(ext_dir.path(), tarball.to_str().unwrap()).unwrap();

        // Second install fails with "already installed"
        let err = install_tarball(ext_dir.path(), tarball.to_str().unwrap()).unwrap_err();
        assert!(
            err.to_string().contains("already installed"),
            "should reject duplicate: {}",
            err
        );
    }

    #[test]
    fn install_routes_tarball_url() {
        // Verify the install() dispatcher recognizes .tar.gz URLs
        // (will fail on network, but should not fall through to git or "invalid source")
        let err = install("https://example.com/ext-1.0.tar.gz").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("not a valid extension source"),
            "should route to tarball path, not reject: {}",
            msg
        );
    }
}
