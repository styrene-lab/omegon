//! Container materialization — resolve a NexProfile to a runnable container command.

use std::path::Path;
use std::process::Command;

use super::profile::NexProfile;

/// Validate an environment variable key — alphanumeric + underscore only.
/// Prevents injection of container flags via crafted env var names.
fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Build a container runtime command from a Nex profile.
///
/// The returned `Command` is ready to spawn via `tokio::process::Command::from(std)`.
/// Applies resource limits, network policy, mount policy, and env passthrough
/// from the profile.
pub fn materialize_container(
    profile: &NexProfile,
    runtime: &str,
    cwd: &Path,
    prompt_file: &Path,
    agent_args: &[String],
    env: &[(String, String)],
) -> Command {
    let image = profile
        .image_ref
        .as_deref()
        .unwrap_or("ghcr.io/styrene-lab/omegon:latest");

    let mut cmd = Command::new(runtime);
    cmd.arg("run");
    cmd.arg("--rm");
    cmd.arg("-i"); // interactive — stdin piped for stdio protocol

    // Resource limits
    if let Some(mem) = profile.resource_limits.memory_mb {
        cmd.arg(format!("--memory={}m", mem));
    }
    if let Some(cpu) = profile.resource_limits.cpu_shares {
        cmd.arg(format!("--cpu-shares={}", cpu));
    }
    if let Some(pids) = profile.resource_limits.pids_limit {
        cmd.arg(format!("--pids-limit={}", pids));
    }
    if profile.resource_limits.readonly_rootfs {
        cmd.arg("--read-only");
        // tmpfs for /tmp — exec allowed because coding domains compile there
        cmd.arg("--tmpfs=/tmp:rw,nosuid,size=512m");
    }

    // Network policy — graduated isolation from the capabilities
    let network_policy = &profile.capabilities.network;
    cmd.arg(format!("--network={}", network_policy.network_flag()));

    // Port mappings for bridge mode
    if let super::profile::NexNetworkPolicy::Bridge { ports } = network_policy {
        for mapping in ports {
            let proto = match mapping.protocol {
                super::profile::NexPortProtocol::Tcp => "tcp",
                super::profile::NexPortProtocol::Udp => "udp",
            };
            cmd.arg(format!(
                "--publish={}:{}/{}",
                mapping.host, mapping.container, proto
            ));
        }
    }

    // Filtered egress — inject iptables rules via entrypoint wrapper.
    // Requires NET_ADMIN capability inside the container (scoped to the
    // container's own network namespace, not the host).
    if let super::profile::NexNetworkPolicy::Egress {
        filter: Some(filter),
    } = network_policy
    {
        cmd.arg("--cap-add=NET_ADMIN");
        // The iptables init script is passed via OMEGON_EGRESS_FILTER env var
        // and executed by the container entrypoint before starting the agent.
        let filter_json = serde_json::to_string(filter).unwrap_or_default();
        cmd.arg("-e");
        cmd.arg(format!("OMEGON_EGRESS_FILTER={}", filter_json));
    }

    // Mount policy — use canonical paths to prevent traversal (M2 fix)
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    if profile.capabilities.mount_cwd {
        let cwd_str = canonical_cwd.display();
        if profile.capabilities.filesystem_write {
            cmd.arg(format!("-v={}:/work", cwd_str));
        } else {
            cmd.arg(format!("-v={}:/work:ro", cwd_str));
        }
        cmd.arg("--workdir=/work");
    }

    // Additional mount paths — canonicalize each
    for extra_path in &profile.capabilities.mount_paths {
        let canonical =
            std::fs::canonicalize(extra_path).unwrap_or_else(|_| extra_path.to_path_buf());
        let path_str = canonical.display();
        if profile.capabilities.filesystem_write {
            cmd.arg(format!("-v={}:{}:rw", path_str, path_str));
        } else {
            cmd.arg(format!("-v={}:{}:ro", path_str, path_str));
        }
    }

    // Prompt file — if outside the cwd mount, mount it separately
    let canonical_prompt =
        std::fs::canonicalize(prompt_file).unwrap_or_else(|_| prompt_file.to_path_buf());
    if !canonical_prompt.starts_with(&canonical_cwd) {
        cmd.arg(format!("-v={}:/prompt:ro", canonical_prompt.display()));
    }

    // Environment passthrough — validate keys to prevent injection (H1 fix)
    for (key, value) in env {
        if is_valid_env_key(key) {
            cmd.arg("-e");
            cmd.arg(format!("{}={}", key, value));
        } else {
            tracing::warn!(key = %key, "skipping env var with invalid key in nex container");
        }
    }
    for key in &profile.capabilities.env_passthrough {
        if !is_valid_env_key(key) {
            tracing::warn!(key = %key, "skipping invalid env_passthrough key in nex profile");
            continue;
        }
        if let Ok(value) = std::env::var(key) {
            cmd.arg("-e");
            cmd.arg(format!("{}={}", key, value));
        }
    }

    // Child agent marker env vars (B2 fix)
    cmd.arg("-e");
    cmd.arg("OMEGON_CHILD=1");
    cmd.arg("-e");
    cmd.arg("OMEGON_NO_KEYRING=1");

    // Labels for tracking (M4 partial — add session-scoped name)
    cmd.arg(format!(
        "--label=sh.styrene.omegon.profile={}",
        profile.name
    ));
    cmd.arg(format!(
        "--label=sh.styrene.omegon.hash={}",
        profile.profile_hash
    ));

    // Image
    cmd.arg(image);

    // Agent entrypoint args (passed after the image)
    for arg in agent_args {
        cmd.arg(arg);
    }

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nex::manifest::NexManifest;
    use std::path::PathBuf;

    #[test]
    fn env_key_validation() {
        assert!(is_valid_env_key("FOO_BAR"));
        assert!(is_valid_env_key("ANTHROPIC_API_KEY"));
        assert!(!is_valid_env_key(""));
        assert!(!is_valid_env_key("FOO=BAR"));
        assert!(!is_valid_env_key("FOO BAR"));
        assert!(!is_valid_env_key("FOO\nBAR"));
    }

    #[test]
    fn materialize_basic_container() {
        let toml = r#"
[profile]
name = "test"
base = "coding"
image = "ghcr.io/styrene-lab/omegon:0.17.6"

[resources]
memory_mb = 1024

[network]
policy = "isolated"

[capabilities]
mount_cwd = true
filesystem_write = true
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cwd = PathBuf::from("/tmp/test-project");
        let prompt = cwd.join(".cleave-prompt.md");

        let cmd = materialize_container(
            &profile,
            "podman",
            &cwd,
            &prompt,
            &["--prompt-file=/work/.cleave-prompt.md".into()],
            &[("ANTHROPIC_API_KEY".into(), "sk-test".into())],
        );

        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"--memory=1024m".to_string()));
        assert!(args.contains(&"--network=none".to_string()));
        assert!(args.contains(&"ghcr.io/styrene-lab/omegon:0.17.6".to_string()));
        // B2 fix — child marker env vars present
        assert!(args.contains(&"OMEGON_CHILD=1".to_string()));
        assert!(args.contains(&"OMEGON_NO_KEYRING=1".to_string()));
    }

    #[test]
    fn egress_produces_bridge_network() {
        let toml = r#"
[profile]
name = "net-test"
base = "coding"

[network]
policy = "egress"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--network=bridge".to_string()));
        // Unfiltered egress — no NET_ADMIN cap needed
        assert!(!args.iter().any(|a| a.contains("NET_ADMIN")));
    }

    #[test]
    fn filtered_egress_adds_cap_and_filter_env() {
        let toml = r#"
[profile]
name = "filtered"
base = "coding"

[network]
policy = "egress"

[network.egress]
allow_hosts = ["api.anthropic.com"]
allow_ports = [443]
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--network=bridge".to_string()));
        assert!(args.contains(&"--cap-add=NET_ADMIN".to_string()));
        assert!(args.iter().any(|a| a.starts_with("OMEGON_EGRESS_FILTER=")));
    }

    #[test]
    fn bridge_with_port_mappings() {
        let toml = r#"
[profile]
name = "dev"
base = "coding-node"

[network]
policy = "bridge"

[[network.ports]]
host = 3000
container = 3000

[[network.ports]]
host = 8080
container = 80
protocol = "tcp"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--network=bridge".to_string()));
        assert!(args.contains(&"--publish=3000:3000/tcp".to_string()));
        assert!(args.contains(&"--publish=8080:80/tcp".to_string()));
    }

    #[test]
    fn isolated_forces_none() {
        let toml = r#"
[profile]
name = "locked"
base = "coding"

[network]
policy = "isolated"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--network=none".to_string()));
    }

    #[test]
    fn host_network() {
        let toml = r#"
[profile]
name = "infra"
base = "infra"

[network]
policy = "host"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"--network=host".to_string()));
    }

    #[test]
    fn invalid_env_keys_skipped() {
        let toml = r#"
[profile]
name = "test"
base = "coding"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let cmd = materialize_container(
            &profile,
            "podman",
            Path::new("/tmp"),
            Path::new("/tmp/p.md"),
            &[],
            &[
                ("GOOD_KEY".into(), "val".into()),
                ("BAD=KEY".into(), "val".into()),
                ("BAD KEY".into(), "val".into()),
            ],
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"GOOD_KEY=val".to_string()));
        assert!(!args.iter().any(|a| a.contains("BAD")));
    }
}
