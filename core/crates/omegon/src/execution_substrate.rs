//! Read-only detection of the substrate this Omegon process is running on.

use std::path::Path;

pub fn detect() -> omegon_traits::ExecutionSubstrate {
    let home = std::env::var("HOME").ok();
    let workspace = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "/workspace".to_string());
    let omegon_home = std::env::var("OMEGON_HOME")
        .ok()
        .or_else(|| home.as_ref().map(|h| format!("{h}/.omegon")))
        .unwrap_or_else(|| "/data/omegon".to_string());

    let kubernetes = is_kubernetes();
    let host_shim_oci = is_host_shim_oci();
    let generic_container = kubernetes || host_shim_oci || is_containerized();
    let has_host_runtime = has_host_runtime();

    let kind = if kubernetes {
        omegon_traits::ExecutionSubstrateKind::Kubernetes
    } else if host_shim_oci {
        omegon_traits::ExecutionSubstrateKind::HostShimOci
    } else if generic_container {
        omegon_traits::ExecutionSubstrateKind::OrchestratedContainer
    } else {
        omegon_traits::ExecutionSubstrateKind::HostNative
    };

    omegon_traits::ExecutionSubstrate {
        kind,
        container: generic_container.then(|| omegon_traits::ContainerSubstrate {
            detected: true,
            runtime: detect_container_runtime_hint(),
            image: std::env::var("OMEGON_OCI_IMAGE").ok(),
            image_digest: std::env::var("OMEGON_OCI_IMAGE_DIGEST").ok(),
            container_id: std::env::var("HOSTNAME").ok(),
            orchestrator: kubernetes
                .then_some(omegon_traits::ContainerOrchestratorKind::Kubernetes),
        }),
        paths: omegon_traits::ExecutionSubstratePaths {
            workspace,
            omegon_home,
            home,
        },
        capabilities: omegon_traits::ExecutionSubstrateCapabilities {
            can_launch_sibling_containers: !generic_container && has_host_runtime,
            can_mount_host_paths: !generic_container,
            can_write_workspace: true,
            has_host_runtime,
            has_kubernetes_service_account: has_kubernetes_service_account(),
        },
    }
}

fn is_host_shim_oci() -> bool {
    std::env::var("OMEGON_RUNTIME_CONTEXT").is_ok_and(|v| v == "host-shim-oci")
        || std::env::var("OMEGON_OCI_LAUNCHER").is_ok_and(|v| v == "omegon")
        || std::env::var("OMEGON_INSIDE_OCI").is_ok_and(|v| v == "1")
}

fn is_kubernetes() -> bool {
    std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() || has_kubernetes_service_account()
}

fn has_kubernetes_service_account() -> bool {
    Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists()
}

fn is_containerized() -> bool {
    Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|content| {
            content.contains("kubepods")
                || content.contains("containerd")
                || content.contains("docker")
                || content.contains("podman")
        })
}

fn detect_container_runtime_hint() -> Option<omegon_traits::ContainerRuntimeKind> {
    let content = std::fs::read_to_string("/proc/1/cgroup").unwrap_or_default();
    if content.contains("podman") {
        Some(omegon_traits::ContainerRuntimeKind::Podman)
    } else if content.contains("docker") {
        Some(omegon_traits::ContainerRuntimeKind::Docker)
    } else if content.contains("containerd") || content.contains("kubepods") {
        Some(omegon_traits::ContainerRuntimeKind::Containerd)
    } else {
        None
    }
}

fn has_host_runtime() -> bool {
    ["podman", "docker", "nerdctl"]
        .iter()
        .any(|runtime| command_available(runtime))
}

fn command_available(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn host_shim_oci_env_marks_host_shim_substrate() {
        let _guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::set_var("OMEGON_RUNTIME_CONTEXT", "host-shim-oci");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }

        let substrate = detect();
        assert_eq!(
            substrate.kind,
            omegon_traits::ExecutionSubstrateKind::HostShimOci
        );
        assert!(substrate.container.as_ref().unwrap().detected);
        assert!(!substrate.capabilities.can_mount_host_paths);

        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
        }
    }

    #[tokio::test]
    async fn kubernetes_env_marks_kubernetes_substrate() {
        let _guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::set_var("KUBERNETES_SERVICE_HOST", "10.0.0.1");
        }

        let substrate = detect();
        assert_eq!(
            substrate.kind,
            omegon_traits::ExecutionSubstrateKind::Kubernetes
        );
        assert_eq!(
            substrate.container.as_ref().unwrap().orchestrator,
            Some(omegon_traits::ContainerOrchestratorKind::Kubernetes)
        );

        unsafe {
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
    }
}
