//! Docker Compose export — generate compose service definitions from Nex profiles.
//!
//! Ensures Nex profiles are deployable via standard tooling (docker-compose,
//! podman-compose, k8s kompose) without vendor lock-in to our spawn path.

use std::fmt::Write;

use super::profile::{NexNetworkPolicy, NexPortProtocol, NexProfile};

/// Generate a docker-compose service YAML fragment from a Nex profile.
///
/// The output is a complete `services:` block that can be used standalone
/// or merged into an existing docker-compose.yml.
///
/// # Deployment mode
///
/// Compose services run in `serve` mode (HTTP control port) by default,
/// not the interactive stdio protocol used by cleave/delegate child agents.
/// The control port is exposed at 7842.
///
/// # Egress filtering
///
/// When the profile specifies filtered egress, the entrypoint handles
/// iptables setup via the `OMEGON_EGRESS_FILTER` env var. This works
/// identically whether the container is started by our spawn path,
/// docker-compose, or any other OCI runtime.
pub fn to_compose_yaml(profile: &NexProfile, service_name: Option<&str>) -> String {
    let name = service_name.unwrap_or(&profile.name);
    let image = profile
        .image_ref
        .as_deref()
        .unwrap_or("ghcr.io/styrene-lab/omegon:latest");

    let mut yaml = String::new();
    let _ = writeln!(yaml, "services:");
    let _ = writeln!(yaml, "  {}:", name);
    let _ = writeln!(yaml, "    image: {}", image);

    // Resource limits
    let res = &profile.resource_limits;
    if res.memory_mb.is_some() || res.cpu_shares.is_some() || res.pids_limit.is_some() {
        let _ = writeln!(yaml, "    deploy:");
        let _ = writeln!(yaml, "      resources:");
        let _ = writeln!(yaml, "        limits:");
        if let Some(mem) = res.memory_mb {
            let _ = writeln!(yaml, "          memory: {}M", mem);
        }
        if let Some(pids) = res.pids_limit {
            let _ = writeln!(yaml, "    pids_limit: {}", pids);
        }
    }
    if let Some(cpu) = res.cpu_shares {
        let _ = writeln!(yaml, "    cpu_shares: {}", cpu);
    }

    // Read-only rootfs
    if res.readonly_rootfs {
        let _ = writeln!(yaml, "    read_only: true");
        let _ = writeln!(yaml, "    tmpfs:");
        let _ = writeln!(yaml, "      - /tmp:size=512m,rw,nosuid");
    }

    // Network policy
    let caps = &profile.capabilities;
    match &caps.network {
        NexNetworkPolicy::Isolated => {
            let _ = writeln!(yaml, "    network_mode: \"none\"");
        }
        NexNetworkPolicy::Egress { filter } => {
            // Bridge network — compose default. No network_mode override needed.
            let _ = writeln!(yaml, "    cap_add:");
            let _ = writeln!(yaml, "      - NET_ADMIN");
            if filter.is_some() {
                let _ = writeln!(
                    yaml,
                    "    # Egress filter applied by entrypoint via iptables"
                );
                let _ = writeln!(
                    yaml,
                    "    # OMEGON_EGRESS_FILTER is set in environment below"
                );
            }
        }
        NexNetworkPolicy::Bridge { ports } => {
            if !ports.is_empty() {
                let _ = writeln!(yaml, "    ports:");
                for pm in ports {
                    let proto = match pm.protocol {
                        NexPortProtocol::Tcp => "tcp",
                        NexPortProtocol::Udp => "udp",
                    };
                    let _ = writeln!(yaml, "      - \"{}:{}/{}\"", pm.host, pm.container, proto);
                }
            }
        }
        NexNetworkPolicy::Host => {
            let _ = writeln!(yaml, "    network_mode: \"host\"");
        }
        NexNetworkPolicy::Custom(net) => {
            let _ = writeln!(yaml, "    network_mode: \"{}\"", net);
        }
    }

    // Control port — always exposed for serve mode (unless isolated or host)
    match &caps.network {
        NexNetworkPolicy::Isolated => {}
        NexNetworkPolicy::Host => {} // host mode exposes all ports
        _ => {
            // Add control port if not already in bridge ports
            let has_control_port = matches!(&caps.network,
                NexNetworkPolicy::Bridge { ports } if ports.iter().any(|p| p.container == 7842)
            );
            if !has_control_port {
                let _ = writeln!(yaml, "    expose:");
                let _ = writeln!(yaml, "      - \"7842\"");
            }
        }
    }

    // Volumes
    let mut volumes = Vec::new();
    if caps.mount_cwd {
        let mount_flag = if caps.filesystem_write { "rw" } else { "ro" };
        volumes.push(format!(".:/work:{}", mount_flag));
    }
    for extra in &caps.mount_paths {
        let mount_flag = if caps.filesystem_write { "rw" } else { "ro" };
        let path_str = extra.display();
        volumes.push(format!("{}:{}:{}", path_str, path_str, mount_flag));
    }
    if !volumes.is_empty() {
        let _ = writeln!(yaml, "    volumes:");
        for v in &volumes {
            let _ = writeln!(yaml, "      - \"{}\"", v);
        }
    }

    // Working directory
    if caps.mount_cwd {
        let _ = writeln!(yaml, "    working_dir: /work");
    }

    // Environment
    let mut env_entries: Vec<(String, String)> = Vec::new();

    // Marker env vars
    env_entries.push(("OMEGON_CHILD".into(), "1".into()));
    env_entries.push(("OMEGON_NO_KEYRING".into(), "1".into()));

    // Egress filter JSON
    if let NexNetworkPolicy::Egress { filter: Some(f) } = &caps.network {
        let filter_json = serde_json::to_string(f).unwrap_or_default();
        env_entries.push(("OMEGON_EGRESS_FILTER".into(), filter_json));
    }

    // Passthrough env vars — compose uses ${VAR} syntax for host env
    for key in &caps.env_passthrough {
        env_entries.push((key.clone(), format!("${{{}}}", key)));
    }

    if !env_entries.is_empty() {
        let _ = writeln!(yaml, "    environment:");
        for (key, val) in &env_entries {
            let _ = writeln!(yaml, "      {}: \"{}\"", key, val.replace('"', "\\\""));
        }
    }

    // Labels
    let _ = writeln!(yaml, "    labels:");
    let _ = writeln!(
        yaml,
        "      sh.styrene.omegon.profile: \"{}\"",
        profile.name
    );
    let _ = writeln!(
        yaml,
        "      sh.styrene.omegon.hash: \"{}\"",
        profile.profile_hash
    );

    yaml
}

/// Generate a complete docker-compose.yml with version header.
pub fn to_compose_file(profile: &NexProfile, service_name: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("# Generated by omegon nex compose\n");
    out.push_str("# Deploy with: docker compose up -d\n");
    out.push_str("#\n");
    out.push_str(&format!(
        "# Profile: {} ({})\n",
        profile.name,
        profile.capabilities.network.display_label()
    ));
    out.push_str(&format!("# Hash:    {}\n", profile.profile_hash));
    out.push('\n');
    out.push_str(&to_compose_yaml(profile, service_name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nex::manifest::NexManifest;

    #[test]
    fn isolated_profile_produces_network_none() {
        let toml = r#"
[profile]
name = "test"
base = "coding"
image = "ghcr.io/styrene-lab/omegon:0.17.7"

[network]
policy = "isolated"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("network_mode: \"none\""));
        assert!(yaml.contains("image: ghcr.io/styrene-lab/omegon:0.17.7"));
        assert!(yaml.contains("read_only: true"));
    }

    #[test]
    fn egress_filtered_produces_cap_and_env() {
        let toml = r#"
[profile]
name = "api-agent"
base = "coding"

[network]
policy = "egress"

[network.egress]
allow_hosts = ["api.anthropic.com"]
allow_ports = [443]
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("cap_add:"));
        assert!(yaml.contains("NET_ADMIN"));
        assert!(yaml.contains("OMEGON_EGRESS_FILTER"));
        assert!(yaml.contains("api.anthropic.com"));
    }

    #[test]
    fn bridge_with_ports_produces_publish() {
        let toml = r#"
[profile]
name = "dev-server"
base = "coding-node"

[network]
policy = "bridge"

[[network.ports]]
host = 3000
container = 3000

[[network.ports]]
host = 8080
container = 80
protocol = "udp"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("\"3000:3000/tcp\""));
        assert!(yaml.contains("\"8080:80/udp\""));
    }

    #[test]
    fn host_network_uses_network_mode() {
        let toml = r#"
[profile]
name = "infra"
base = "infra"

[network]
policy = "host"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("network_mode: \"host\""));
    }

    #[test]
    fn volumes_mount_cwd_and_extras() {
        let toml = r#"
[profile]
name = "test"
base = "coding"

[capabilities]
mount_cwd = true
filesystem_write = false
mount_paths = ["/data/shared"]
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains(".:/work:ro"));
        assert!(yaml.contains("/data/shared:/data/shared:ro"));
        assert!(yaml.contains("working_dir: /work"));
    }

    #[test]
    fn env_passthrough_uses_compose_syntax() {
        let toml = r#"
[profile]
name = "test"
base = "coding"

[capabilities]
env_passthrough = ["DATABASE_URL", "REDIS_URL"]
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("DATABASE_URL: \"${DATABASE_URL}\""));
        assert!(yaml.contains("REDIS_URL: \"${REDIS_URL}\""));
    }

    #[test]
    fn resource_limits_in_deploy_section() {
        let toml = r#"
[profile]
name = "constrained"
base = "coding"

[resources]
memory_mb = 2048
pids_limit = 256
cpu_shares = 512
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, None);
        assert!(yaml.contains("memory: 2048M"));
        assert!(yaml.contains("pids_limit: 256"));
        assert!(yaml.contains("cpu_shares: 512"));
    }

    #[test]
    fn custom_service_name() {
        let toml = r#"
[profile]
name = "test"
base = "coding"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let yaml = to_compose_yaml(&profile, Some("my-agent"));
        assert!(yaml.contains("  my-agent:"));
    }

    #[test]
    fn compose_file_has_header() {
        let toml = r#"
[profile]
name = "test"
base = "coding"
"#;
        let profile = NexManifest::from_toml(toml).unwrap().into_profile();
        let file = to_compose_file(&profile, None);
        assert!(file.starts_with("# Generated by omegon nex compose"));
        assert!(file.contains("docker compose up -d"));
        assert!(file.contains("Profile: test"));
    }
}
