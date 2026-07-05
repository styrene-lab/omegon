//! Update checker — polls GitHub Releases API for new versions.
//!
//! At startup, spawns an async task that checks for newer releases.
//! Results are surfaced as a banner in the TUI footer.
//! The `/update` command triggers download + replace + exec restart.

use std::path::{Path, PathBuf};
use std::str;
use std::time::Duration;
use tokio::sync::watch;

/// Version comparison result.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub download_url: String,
    pub signature_url: String,
    pub certificate_url: String,
    pub release_notes: String,
    pub is_newer: bool,
}

impl UpdateInfo {
    pub fn has_downloadable_archive(&self) -> bool {
        !self.download_url.is_empty()
            && !self.signature_url.is_empty()
            && !self.certificate_url.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    Nightly,
}

impl UpdateChannel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "stable" => Some(Self::Stable),
            "rc" => Some(Self::Stable), // RC deprecated — redirect to stable
            "nightly" => Some(Self::Nightly),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }
}

/// Shared state for the update checker.
pub type UpdateReceiver = watch::Receiver<Option<UpdateInfo>>;
pub type UpdateSender = watch::Sender<Option<UpdateInfo>>;

/// Create the update channel.
pub fn channel() -> (UpdateSender, UpdateReceiver) {
    watch::channel(None)
}

/// GitHub release info (minimal subset).
#[derive(serde::Deserialize, Clone)]
pub(crate) struct GitHubRelease {
    pub tag_name: String,
    pub body: Option<String>,
    pub assets: Vec<GitHubAsset>,
    pub prerelease: bool,
}

#[derive(serde::Deserialize, Clone)]
pub(crate) struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
}

fn find_asset_url(assets: &[GitHubAsset], suffix: &str) -> String {
    assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .map(|a| a.browser_download_url.clone())
        .unwrap_or_default()
}

/// Path for the update check cache file.
fn cache_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".omegon/update-check.json"))
}

/// Read the cached update check result. Returns Some if the cache is
/// fresh (< 24 hours old) and matches the requested channel.
pub fn read_cache(channel: UpdateChannel) -> Option<UpdateInfo> {
    let path = cache_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let cached: serde_json::Value = serde_json::from_str(&content).ok()?;

    let cached_channel = cached["channel"].as_str()?;
    if cached_channel != channel.as_str() {
        return None;
    }

    let cached_at = cached["checked_at"].as_str()?;
    let checked_at = chrono::DateTime::parse_from_rfc3339(cached_at).ok()?;
    let age = chrono::Utc::now().signed_duration_since(checked_at);
    if age.num_hours() >= 24 {
        return None;
    }

    let latest = cached["latest"].as_str()?.to_string();
    let current = env!("CARGO_PKG_VERSION");
    if !is_newer(&latest, current) {
        return None;
    }

    let info = UpdateInfo {
        current: current.to_string(),
        latest,
        download_url: cached["download_url"].as_str().unwrap_or("").to_string(),
        signature_url: cached["signature_url"].as_str().unwrap_or("").to_string(),
        certificate_url: cached["certificate_url"].as_str().unwrap_or("").to_string(),
        release_notes: cached["release_notes"].as_str().unwrap_or("").to_string(),
        is_newer: true,
    };
    info.has_downloadable_archive().then_some(info)
}

/// Write the update check result to cache.
fn write_cache(info: &UpdateInfo, channel: UpdateChannel) {
    if !info.has_downloadable_archive() {
        clear_cache();
        return;
    }
    let Some(path) = cache_path() else { return };
    let cached = serde_json::json!({
        "channel": channel.as_str(),
        "latest": info.latest,
        "download_url": info.download_url,
        "signature_url": info.signature_url,
        "certificate_url": info.certificate_url,
        "release_notes": info.release_notes,
        "checked_at": chrono::Utc::now().to_rfc3339(),
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&cached).unwrap_or_default(),
    );
}

fn clear_cache() {
    if let Some(path) = cache_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn spawn_check_with_options(
    tx: UpdateSender,
    channel: UpdateChannel,
    delay: Duration,
    use_cache: bool,
) {
    // Check cache first — avoid a GitHub API call if we checked recently.
    if use_cache && let Some(cached) = read_cache(channel) {
        tracing::debug!(
            latest = %cached.latest,
            "update check: using cached result (< 24h old)"
        );
        let _ = tx.send(Some(cached));
        return;
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    crate::task_spawn::spawn_best_effort_result("update-check", async move {
        tokio::time::sleep(delay).await;

        match check_latest_for_channel(&current, channel).await {
            Ok(Some(info)) => {
                tracing::info!(
                    current = %info.current,
                    latest = %info.latest,
                    channel = channel.as_str(),
                    "new version available"
                );
                write_cache(&info, channel);
                let _ = tx.send(Some(info));
            }
            Ok(None) => {
                tracing::debug!(channel = channel.as_str(), "up to date");
                let _ = tx.send(None);
            }
            Err(e) => {
                tracing::debug!(
                    channel = channel.as_str(),
                    "update check failed (non-fatal): {e}"
                );
            }
        }
        Ok(())
    });
}

pub fn spawn_check_with_delay(tx: UpdateSender, channel: UpdateChannel, delay: Duration) {
    spawn_check_with_options(tx, channel, delay, true);
}

/// Spawn the background update check.
pub fn spawn_check(tx: UpdateSender, channel: UpdateChannel) {
    spawn_check_with_delay(tx, channel, Duration::from_secs(5));
}

/// Spawn an update check that bypasses the cache. Used for explicit operator
/// `/update` requests so a release first observed before assets were uploaded
/// cannot stay stuck as "not downloadable" for the cache TTL.
pub fn spawn_check_now(tx: UpdateSender, channel: UpdateChannel) {
    spawn_check_with_options(tx, channel, Duration::from_secs(0), false);
}

/// Poll for updates periodically so long-running TUI sessions notice new releases.
pub fn spawn_polling(tx: UpdateSender, settings: crate::settings::SharedSettings) {
    crate::task_spawn::spawn_best_effort("update-poller", async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            let channel = settings
                .lock()
                .ok()
                .and_then(|s| UpdateChannel::parse(&s.update_channel))
                .unwrap_or(UpdateChannel::Stable);
            spawn_check_with_delay(tx.clone(), channel, Duration::from_secs(0));
        }
    });
}

/// Check GitHub Releases for a newer version on the selected channel.
pub async fn check_latest_for_channel(
    current: &str,
    channel: UpdateChannel,
) -> anyhow::Result<Option<UpdateInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("omegon/{current}"))
        .build()?;

    let releases: Vec<GitHubRelease> = if matches!(channel, UpdateChannel::Stable) {
        vec![
            client
                .get("https://api.github.com/repos/styrene-lab/omegon/releases/latest")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?,
        ]
    } else {
        client
            .get("https://api.github.com/repos/styrene-lab/omegon/releases")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?
    };

    let target = platform_archive_target();
    let selected = releases.into_iter().find(|resp| {
        let latest = resp.tag_name.trim_start_matches('v');
        let latest = latest.to_lowercase();
        let channel_match = match channel {
            UpdateChannel::Stable => !resp.prerelease,
            UpdateChannel::Nightly => resp.prerelease && latest.contains("-nightly."),
        };
        channel_match && is_newer(&latest, current)
    });

    let Some(resp) = selected else {
        return Ok(None);
    };

    let latest = resp.tag_name.trim_start_matches('v').to_string();

    let archive_name = resp
        .assets
        .iter()
        .find(|a| a.name.contains(&target) && a.name.ends_with(".tar.gz"))
        .map(|a| a.name.clone())
        .unwrap_or_default();
    let download_url = find_asset_url(&resp.assets, &archive_name);
    let signature_url = if archive_name.is_empty() {
        String::new()
    } else {
        find_asset_url(&resp.assets, &format!("{archive_name}.sig"))
    };
    let certificate_url = if archive_name.is_empty() {
        String::new()
    } else {
        find_asset_url(&resp.assets, &format!("{archive_name}.pem"))
    };

    Ok(Some(UpdateInfo {
        current: current.to_string(),
        latest,
        download_url,
        signature_url,
        certificate_url,
        release_notes: resp.body.unwrap_or_default(),
        is_newer: true,
    }))
}

/// Semver comparison: is `latest` newer than `current`?
/// A stable release (0.15.2) is newer than its own prerelease variants.
fn is_newer(latest: &str, current: &str) -> bool {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum SuffixKind {
        Rc,
        Nightly,
        Stable,
    }

    let parse = |s: &str| -> (Vec<u32>, SuffixKind, u32) {
        let mut parts = s.splitn(2, '-');
        let base = parts.next().unwrap_or(s);
        let suffix = parts.next().unwrap_or("");
        let version: Vec<u32> = base.split('.').filter_map(|p| p.parse().ok()).collect();
        if let Some(num) = suffix.strip_prefix("rc.").and_then(|n| n.parse().ok()) {
            (version, SuffixKind::Rc, num)
        } else if let Some(num) = suffix.strip_prefix("nightly.").and_then(|n| n.parse().ok()) {
            (version, SuffixKind::Nightly, num)
        } else {
            (version, SuffixKind::Stable, 0)
        }
    };

    let (l_ver, l_kind, l_num) = parse(latest);
    let (c_ver, c_kind, c_num) = parse(current);
    match l_ver.cmp(&c_ver) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match l_kind.cmp(&c_kind) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => l_num > c_num,
        },
    }
}

/// Platform-specific asset name pattern.
pub(crate) fn platform_archive_target() -> String {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin".into()
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin".into()
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu".into()
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "x86_64-unknown-linux-gnu".into()
    } else {
        "unknown".into()
    }
}

async fn download_to_path(client: &reqwest::Client, url: &str, path: &Path) -> anyhow::Result<()> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::fs::write(path, &bytes).await?;
    Ok(())
}

fn verify_archive_signature(
    archive_path: &Path,
    sig_path: &Path,
    cert_path: &Path,
) -> anyhow::Result<()> {
    let blob = std::fs::read(archive_path)?;
    let signature = std::fs::read_to_string(sig_path)?;
    let cert_pem = std::fs::read_to_string(cert_path)?;

    <sigstore::cosign::Client as sigstore::cosign::CosignCapabilities>::verify_blob(
        &cert_pem,
        signature.trim(),
        &blob,
    )
    .map_err(|e| anyhow::anyhow!("blob signature verification failed: {e}"))?;

    verify_certificate_identity(&cert_pem)?;
    Ok(())
}

fn verify_certificate_identity(cert_pem: &str) -> anyhow::Result<()> {
    use x509_parser::extensions::GeneralName;
    use x509_parser::pem::parse_x509_pem;
    use x509_parser::prelude::*;

    let (_, pem) = parse_x509_pem(cert_pem.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to parse PEM certificate: {e}"))?;
    let (_, cert) = X509Certificate::from_der(&pem.contents)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate DER: {e}"))?;

    let mut subject_uri: Option<String> = None;
    for ext in cert.extensions() {
        if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
            for name in &san.general_names {
                if let GeneralName::URI(uri) = name {
                    let uri_str = uri.to_string();
                    if uri_str.starts_with("https://github.com/") {
                        subject_uri = Some(uri_str);
                        break;
                    }
                }
            }
        }
    }

    let subject_uri =
        subject_uri.ok_or_else(|| anyhow::anyhow!("certificate missing GitHub Actions SAN URI"))?;
    if !subject_uri
        .starts_with("https://github.com/styrene-lab/omegon/.github/workflows/release.yml@")
    {
        anyhow::bail!("certificate SAN URI does not match release workflow policy: {subject_uri}");
    }

    let issuer_oid = "1.3.6.1.4.1.57264.1.1";
    let issuer = cert
        .extensions()
        .iter()
        .find(|ext| ext.oid.to_id_string() == issuer_oid)
        .map(|ext| {
            String::from_utf8_lossy(ext.value)
                .trim_matches(char::from(0))
                .to_string()
        })
        .unwrap_or_default();
    if issuer != "https://token.actions.githubusercontent.com" {
        anyhow::bail!("certificate issuer policy failed: {issuer}");
    }

    let repo_oid = "1.3.6.1.4.1.57264.1.5";
    let repo = cert
        .extensions()
        .iter()
        .find(|ext| ext.oid.to_id_string() == repo_oid)
        .map(|ext| {
            String::from_utf8_lossy(ext.value)
                .trim_matches(char::from(0))
                .to_string()
        })
        .unwrap_or_default();
    if repo != "styrene-lab/omegon" {
        anyhow::bail!("certificate repository policy failed: {repo}");
    }

    Ok(())
}

/// Detect whether the running binary is managed by Homebrew.
///
/// Homebrew installs to paths like:
///   /opt/homebrew/Cellar/omegon/<version>/bin/omegon   (macOS arm64)
///   /usr/local/Cellar/omegon/<version>/bin/omegon       (macOS x86_64)
///   /home/linuxbrew/.linuxbrew/Cellar/omegon/...        (Linux)
///
/// In-place upgrade of a Cellar-managed binary corrupts brew's tracking —
/// brew still reports the old version after the binary is replaced.
pub fn is_homebrew_managed(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str() == "Cellar" || c.as_os_str() == ".linuxbrew")
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct InstallReceipt {
    version: Option<String>,
    binary: Option<PathBuf>,
    version_dir: Option<PathBuf>,
    versioned_binary: Option<PathBuf>,
}

impl InstallReceipt {
    fn versioned_binary_path(&self) -> Option<PathBuf> {
        self.versioned_binary.clone().or_else(|| {
            self.version_dir
                .as_ref()
                .map(|version_dir| version_dir.join("omegon"))
        })
    }

    fn versions_root(&self) -> Option<PathBuf> {
        self.version_dir.as_ref()?.parent().map(Path::to_path_buf)
    }
}

fn install_receipt_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".config")
            .join("omegon")
            .join("install-receipt.json")
    })
}

fn read_install_receipt() -> anyhow::Result<InstallReceipt> {
    let path = install_receipt_path().ok_or_else(|| anyhow::anyhow!("home directory not found"))?;
    let content = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(unix)]
fn update_install_symlinks(binary_link: &Path, latest_binary: &Path) -> anyhow::Result<()> {
    if let Some(parent) = binary_link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    replace_symlink(binary_link, latest_binary)?;
    if let Some(parent) = binary_link.parent() {
        let om_link = parent.join("om");
        replace_symlink(&om_link, latest_binary)?;
    }
    Ok(())
}

#[cfg(unix)]
fn replace_symlink(link: &Path, target: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(link) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || metadata.is_file() {
                std::fs::remove_file(link)?;
            } else {
                anyhow::bail!("refusing to replace non-file install target: {}", link.display());
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn update_install_symlinks(_binary_link: &Path, _latest_binary: &Path) -> anyhow::Result<()> {
    Ok(())
}

async fn update_install_receipt_for_replaced_binary(
    receipt: &Option<InstallReceipt>,
    latest: &str,
) -> anyhow::Result<()> {
    let Some(receipt) = receipt else {
        return Ok(());
    };
    let Some(versions_root) = receipt.versions_root() else {
        return Ok(());
    };
    let Some(receipt_path) = install_receipt_path() else {
        return Ok(());
    };

    let current_exe = std::env::current_exe()?;
    let latest_dir = versions_root.join(latest);
    tokio::fs::create_dir_all(&latest_dir).await?;
    let latest_binary = latest_dir.join("omegon");
    if !paths_refer_to_same_file(&latest_binary, &current_exe) {
        tokio::fs::copy(&current_exe, &latest_binary).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&latest_binary, std::fs::Permissions::from_mode(0o755))
                .await?;
        }
    }

    let mut value: serde_json::Value = match tokio::fs::read_to_string(&receipt_path).await {
        Ok(content) => serde_json::from_str(&content)?,
        Err(_) => serde_json::json!({}),
    };
    if let Some(binary_link) = receipt.binary.as_ref() {
        update_install_symlinks(binary_link, &latest_binary)?;
    }

    value["version"] = serde_json::Value::String(latest.to_string());
    value["binary"] = receipt
        .binary
        .as_ref()
        .map(|path| serde_json::Value::String(path.display().to_string()))
        .unwrap_or_else(|| serde_json::Value::String(latest_binary.display().to_string()));
    value["version_dir"] = serde_json::Value::String(latest_dir.display().to_string());
    value["versioned_binary"] = serde_json::Value::String(latest_binary.display().to_string());
    value["installed_at"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
    tokio::fs::write(&receipt_path, serde_json::to_string_pretty(&value)? + "\n").await?;
    Ok(())
}

/// Download, verify, and replace the current binary, then exec() into it.
/// Returns the path to the new binary on success (caller does the exec).
pub async fn download_and_replace(info: &UpdateInfo) -> anyhow::Result<PathBuf> {
    if info.download_url.is_empty() {
        anyhow::bail!("No download URL for this platform");
    }
    if info.signature_url.is_empty() || info.certificate_url.is_empty() {
        anyhow::bail!("Release is missing signature sidecars; refusing unverified install");
    }

    let current_exe = std::env::current_exe()?;

    if is_homebrew_managed(&current_exe) {
        let formula = "omegon";
        anyhow::bail!(
            "This binary is managed by Homebrew — in-place upgrade would corrupt brew's \
             version tracking.\n\nTo upgrade, run:\n  brew upgrade {formula}"
        );
    }
    let install_receipt = read_install_receipt().ok();
    let managed_install = install_receipt
        .as_ref()
        .and_then(InstallReceipt::versioned_binary_path)
        .is_some_and(|path| paths_refer_to_same_file(&path, &current_exe));

    let tmp_path = current_exe.with_extension("new");
    let archive_path = current_exe.with_extension("tar.gz");
    let signature_path = current_exe.with_extension("tar.gz.sig");
    let certificate_path = current_exe.with_extension("tar.gz.pem");
    let backup_path = current_exe.with_extension("bak");

    tracing::info!(url = %info.download_url, "downloading update archive");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("omegon/{}", info.current))
        .build()?;

    download_to_path(&client, &info.download_url, &archive_path).await?;
    download_to_path(&client, &info.signature_url, &signature_path).await?;
    download_to_path(&client, &info.certificate_url, &certificate_path).await?;

    verify_archive_signature(&archive_path, &signature_path, &certificate_path)?;

    let archive_path_clone = archive_path.clone();
    let tmp_path_clone = tmp_path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(&archive_path_clone)?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        let mut extracted = false;
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            if path.file_name().and_then(|n| n.to_str()) == Some("omegon") {
                let mut out = std::fs::File::create(&tmp_path_clone)?;
                std::io::copy(&mut entry, &mut out)?;
                extracted = true;
                break;
            }
        }
        if !extracted {
            anyhow::bail!("Downloaded archive did not contain omegon binary");
        }
        Ok(())
    })
    .await??;

    tokio::fs::remove_file(&archive_path).await.ok();
    tokio::fs::remove_file(&signature_path).await.ok();
    tokio::fs::remove_file(&certificate_path).await.ok();

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)).await?;
    }

    // Verify the new binary runs
    let output = tokio::process::Command::new(&tmp_path)
        .arg("--version")
        .output()
        .await?;

    if !output.status.success() {
        tokio::fs::remove_file(&tmp_path).await.ok();
        anyhow::bail!("Downloaded binary failed --version check");
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    if !version_output.contains(&info.latest) {
        tokio::fs::remove_file(&tmp_path).await.ok();
        anyhow::bail!(
            "Version mismatch: expected {}, got {}",
            info.latest,
            version_output.trim()
        );
    }

    // Atomic replace: current → backup, new → current
    if backup_path.exists() {
        tokio::fs::remove_file(&backup_path).await.ok();
    }
    tokio::fs::rename(&current_exe, &backup_path).await?;
    tokio::fs::rename(&tmp_path, &current_exe).await?;

    if managed_install {
        update_install_receipt_for_replaced_binary(&install_receipt, &info.latest).await?;
    }

    tracing::info!("binary replaced: {} → {}", info.current, info.latest);
    Ok(current_exe)
}

/// Perform an exec() restart — replaces the current process with the new binary.
/// This preserves no state — the session will need to be resumed from disk.
#[cfg(unix)]
pub fn exec_restart(binary: &Path, args: &[String]) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(binary).args(args).exec();
    // exec() only returns on error
    Err(err.into())
}

#[cfg(not(unix))]
pub fn exec_restart(binary: &Path, args: &[String]) -> anyhow::Result<()> {
    std::process::Command::new(binary).args(args).spawn()?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homebrew_managed_detection() {
        assert!(is_homebrew_managed(Path::new(
            "/opt/homebrew/Cellar/omegon/0.15.4/bin/omegon"
        )));
        assert!(is_homebrew_managed(Path::new(
            "/usr/local/Cellar/omegon/0.15.4/bin/omegon"
        )));
        assert!(is_homebrew_managed(Path::new(
            "/home/linuxbrew/.linuxbrew/Cellar/omegon/0.15.4/bin/omegon"
        )));
        assert!(!is_homebrew_managed(Path::new("/usr/local/bin/omegon")));
        assert!(!is_homebrew_managed(Path::new(
            "/Users/cwilson/.local/bin/omegon"
        )));
        assert!(!is_homebrew_managed(Path::new(
            "/tmp/omegon-release-ws/core/target/release/omegon"
        )));
    }

    #[test]
    fn install_receipt_uses_installer_receipt_layout() {
        let receipt = InstallReceipt {
            version: Some("0.27.0".into()),
            binary: Some(PathBuf::from("/usr/local/bin/omegon")),
            version_dir: Some(PathBuf::from("/home/me/.omegon/versions/0.27.0")),
            versioned_binary: Some(PathBuf::from(
                "/home/me/.omegon/versions/0.27.0/omegon",
            )),
        };

        assert_eq!(
            receipt.versioned_binary_path().as_deref(),
            Some(Path::new("/home/me/.omegon/versions/0.27.0/omegon"))
        );
        assert_eq!(
            receipt.versions_root().as_deref(),
            Some(Path::new("/home/me/.omegon/versions"))
        );
    }

    #[test]
    fn install_receipt_derives_binary_from_version_dir_when_needed() {
        let receipt = InstallReceipt {
            version: Some("0.27.0".into()),
            binary: None,
            version_dir: Some(PathBuf::from("/home/me/.omegon/versions/0.27.0")),
            versioned_binary: None,
        };

        assert_eq!(
            receipt.versioned_binary_path().as_deref(),
            Some(Path::new("/home/me/.omegon/versions/0.27.0/omegon"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn replace_symlink_repoints_installer_launcher() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_target = temp.path().join("old");
        let new_target = temp.path().join("new");
        let link = temp.path().join("omegon");
        std::fs::write(&old_target, "old").expect("old target");
        std::fs::write(&new_target, "new").expect("new target");
        std::os::unix::fs::symlink(&old_target, &link).expect("initial symlink");

        replace_symlink(&link, &new_target).expect("replace symlink");

        assert_eq!(std::fs::read_link(&link).expect("read link"), new_target);
    }

    #[cfg(unix)]
    #[test]
    fn replace_symlink_rejects_directories() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("new");
        let link = temp.path().join("omegon");
        std::fs::write(&target, "new").expect("target");
        std::fs::create_dir(&link).expect("directory at install target");

        let err = replace_symlink(&link, &target).expect_err("directory should be rejected");
        assert!(
            err.to_string().contains("refusing to replace non-file"),
            "{err}"
        );
    }

    #[test]
    fn update_info_requires_signed_archive_sidecars() {
        let mut info = UpdateInfo {
            current: "0.22.2".into(),
            latest: "0.22.3".into(),
            download_url: "https://example.invalid/omegon.tar.gz".into(),
            signature_url: "https://example.invalid/omegon.tar.gz.sig".into(),
            certificate_url: "https://example.invalid/omegon.tar.gz.pem".into(),
            release_notes: String::new(),
            is_newer: true,
        };
        assert!(info.has_downloadable_archive());

        info.signature_url.clear();
        assert!(!info.has_downloadable_archive());
    }

    #[test]
    fn rc_channel_parses_distinct_from_nightly() {
        // RC is deprecated — parses to Stable for backward compatibility
        assert_eq!(UpdateChannel::parse("rc"), Some(UpdateChannel::Stable));
        assert_eq!(
            UpdateChannel::parse("nightly"),
            Some(UpdateChannel::Nightly)
        );
        assert_ne!(UpdateChannel::parse("rc"), UpdateChannel::parse("nightly"));
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.15.2", "0.15.1"));
        assert!(is_newer("0.16.0", "0.15.2"));
        assert!(is_newer("1.0.0", "0.15.2"));
        assert!(!is_newer("0.15.1", "0.15.2"));
        assert!(!is_newer("0.15.2", "0.15.2"));
        assert!(is_newer("0.15.2", "0.15.2-rc.3"));
        assert!(!is_newer("0.15.1", "0.15.2-rc.3"));
        assert!(is_newer("0.15.3-rc.7", "0.15.2"));
        assert!(is_newer("0.15.3-nightly.20260326", "0.15.3-rc.7"));
        assert!(is_newer(
            "0.15.3-nightly.20260327",
            "0.15.3-nightly.20260326"
        ));
        assert!(is_newer("0.15.3", "0.15.3-nightly.20260327"));
    }

    #[test]
    fn platform_archive_target_is_valid() {
        let name = platform_archive_target();
        assert!(
            name.contains("darwin") || name.contains("linux"),
            "got: {name}"
        );
        assert!(
            name.contains("aarch64") || name.contains("x86_64"),
            "got: {name}"
        );
    }

    #[test]
    fn find_asset_url_matches_exact_suffix() {
        let assets = vec![
            GitHubAsset {
                name: "omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz".into(),
                browser_download_url: "https://example.invalid/archive".into(),
            },
            GitHubAsset {
                name: "omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz.sig".into(),
                browser_download_url: "https://example.invalid/archive.sig".into(),
            },
        ];
        assert_eq!(
            find_asset_url(
                &assets,
                "omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz.sig"
            ),
            "https://example.invalid/archive.sig"
        );
    }

    #[test]
    fn certificate_identity_requires_repo_workflow_prefix() {
        let cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----";
        let err = verify_certificate_identity(cert).expect_err("invalid cert should fail");
        assert!(
            err.to_string().contains("parse PEM certificate")
                || err.to_string().contains("parse certificate DER")
        );
    }
}
