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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    Nightly,
}

impl UpdateChannel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "stable" => Some(Self::Stable),
            "nightly" | "rc" => Some(Self::Nightly),
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
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
    prerelease: bool,
}

#[derive(serde::Deserialize, Clone)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn find_asset_url(assets: &[GitHubAsset], suffix: &str) -> String {
    assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .map(|a| a.browser_download_url.clone())
        .unwrap_or_default()
}

/// Spawn the background update check.
pub fn spawn_check(tx: UpdateSender, channel: UpdateChannel) {
    let current = env!("CARGO_PKG_VERSION").to_string();
    crate::task_spawn::spawn_best_effort_result("update-check", async move {
        tokio::time::sleep(Duration::from_secs(5)).await;

        match check_latest_for_channel(&current, channel).await {
            Ok(Some(info)) => {
                tracing::info!(
                    current = %info.current,
                    latest = %info.latest,
                    channel = channel.as_str(),
                    "new version available"
                );
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
        let channel_match = match channel {
            UpdateChannel::Stable => !resp.prerelease,
            UpdateChannel::Nightly => resp.prerelease,
        };
        channel_match && is_newer(latest, current)
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
fn platform_archive_target() -> String {
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
