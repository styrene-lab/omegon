//! OCI sandbox for code-act scripts.
//!
//! Runs generated Python scripts inside a container instead of bare
//! `python3`. The workspace and proxy socket are bind-mounted.

use std::path::Path;

use anyhow::Result;
use tokio::process::Command;

const DEFAULT_IMAGE: &str = "python:3.12-slim";

pub struct SandboxConfig {
    pub image: String,
    pub runtime: String,
}

impl SandboxConfig {
    pub fn detect() -> Option<Self> {
        let runtime = crate::nex::spawn::detect_container_runtime_public()?;
        let image = std::env::var("OMEGON_CODE_ACT_IMAGE")
            .unwrap_or_else(|_| DEFAULT_IMAGE.to_string());
        Some(Self { runtime, image })
    }
}

pub async fn execute_in_sandbox(
    config: &SandboxConfig,
    script_path: &Path,
    cwd: &Path,
    proxy_socket: Option<&Path>,
    timeout_secs: u64,
) -> Result<SandboxResult> {
    let mut cmd = Command::new(&config.runtime);
    cmd.arg("run");
    cmd.arg("--rm");
    cmd.arg("--network=none");

    cmd.arg(format!("-v={}:/work:rw", cwd.display()));
    cmd.arg("--workdir=/work");

    cmd.arg(format!(
        "-v={}:/script.py:ro",
        script_path.display()
    ));

    if let Some(sock) = proxy_socket {
        cmd.arg(format!(
            "-v={}:{}",
            sock.display(),
            sock.display()
        ));
    }

    cmd.arg("--read-only");
    cmd.arg("--tmpfs=/tmp:rw,nosuid,size=256m");
    cmd.arg("--cap-drop=ALL");
    cmd.arg("--security-opt=no-new-privileges");
    cmd.arg("--pids-limit=128");
    cmd.arg("--memory=1g");
    cmd.arg(format!("--stop-timeout={timeout_secs}"));

    cmd.arg(&config.image);
    cmd.arg("python3");
    cmd.arg("/script.py");

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs + 5),
        cmd.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("sandbox execution timed out after {timeout_secs}s"))?
    .map_err(|e| anyhow::anyhow!("failed to start container: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok(SandboxResult {
        stdout,
        stderr,
        exit_code,
    })
}

#[derive(Debug)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Check if a container runtime is available for code-act sandboxing.
pub fn sandbox_available() -> bool {
    SandboxConfig::detect().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_config_detect_returns_option() {
        let config = SandboxConfig::detect();
        // May or may not be available depending on the machine
        if let Some(c) = config {
            assert!(!c.runtime.is_empty());
            assert!(!c.image.is_empty());
        }
    }

    #[test]
    fn default_image_is_python_slim() {
        assert_eq!(DEFAULT_IMAGE, "python:3.12-slim");
    }
}
