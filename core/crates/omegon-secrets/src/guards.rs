//! Tool guards — block or flag tool calls that access sensitive paths.

use serde_json::Value;

/// Guard decision for a tool call.
#[derive(Debug, Clone)]
pub enum GuardDecision {
    /// Tool call accesses a sensitive path — block it entirely.
    Block { reason: String, path: String },
    /// Tool call accesses a sensitive path — warn but allow.
    Warn { reason: String, path: String },
}

impl GuardDecision {
    pub fn is_block(&self) -> bool {
        matches!(self, Self::Block { .. })
    }
}

/// Sensitive path patterns and their guard actions.
struct SensitivePattern {
    /// Glob-like suffix or exact match
    pattern: &'static str,
    description: &'static str,
    action: Action,
}

#[derive(Clone, Copy)]
enum Action {
    Block,
    Warn,
}

const SENSITIVE_PATTERNS: &[SensitivePattern] = &[
    // Credentials & secrets
    SensitivePattern { pattern: ".env", description: "Environment variables file", action: Action::Warn },
    SensitivePattern { pattern: ".env.local", description: "Local environment overrides", action: Action::Warn },
    SensitivePattern { pattern: ".env.production", description: "Production environment", action: Action::Block },
    SensitivePattern { pattern: ".netrc", description: "Network credentials", action: Action::Block },
    SensitivePattern { pattern: ".npmrc", description: "npm credentials", action: Action::Warn },
    SensitivePattern { pattern: ".pypirc", description: "PyPI credentials", action: Action::Block },
    // SSH & GPG
    SensitivePattern { pattern: ".ssh/id_", description: "SSH private key", action: Action::Block },
    SensitivePattern { pattern: ".ssh/config", description: "SSH config", action: Action::Warn },
    SensitivePattern { pattern: ".gnupg/", description: "GPG keyring", action: Action::Block },
    // Git internals
    SensitivePattern { pattern: ".git/config", description: "Git config (may contain tokens)", action: Action::Warn },
    // Cloud credentials
    SensitivePattern { pattern: ".aws/credentials", description: "AWS credentials", action: Action::Block },
    SensitivePattern { pattern: ".kube/config", description: "Kubernetes config", action: Action::Warn },
    SensitivePattern { pattern: ".docker/config.json", description: "Docker credentials", action: Action::Warn },
    // Secrets files
    SensitivePattern { pattern: "secrets.json", description: "Secrets configuration", action: Action::Warn },
    SensitivePattern { pattern: "secrets.yaml", description: "Secrets configuration", action: Action::Warn },
    SensitivePattern { pattern: "secrets.yml", description: "Secrets configuration", action: Action::Warn },
    // Vault/keystore
    SensitivePattern { pattern: "vault.json", description: "Vault configuration (may contain auth)", action: Action::Block },
    SensitivePattern { pattern: ".vault-token", description: "Vault token", action: Action::Block },
    SensitivePattern { pattern: "keystore.jks", description: "Java keystore", action: Action::Block },
    SensitivePattern { pattern: ".p12", description: "PKCS#12 certificate", action: Action::Block },
    SensitivePattern { pattern: ".pem", description: "PEM certificate/key", action: Action::Warn },
];

/// Path guard — checks tool arguments for sensitive file paths.
pub struct PathGuard;

impl PathGuard {
    pub fn new() -> Self {
        Self
    }

    /// Check if a tool call targets a sensitive path.
    pub fn check(&self, tool_name: &str, args: &Value) -> Option<GuardDecision> {
        let path = match tool_name {
            "read" | "write" | "edit" | "view" => args.get("path")?.as_str()?,
            "bash" => {
                // Check if the command references sensitive paths
                let cmd = args.get("command")?.as_str()?;
                return self.check_bash_command(cmd);
            }
            _ => return None,
        };

        self.match_path(path)
    }

    fn match_path(&self, path: &str) -> Option<GuardDecision> {
        for pattern in SENSITIVE_PATTERNS {
            if path.contains(pattern.pattern) || path.ends_with(pattern.pattern) {
                return Some(match pattern.action {
                    Action::Block => GuardDecision::Block {
                        reason: pattern.description.to_string(),
                        path: path.to_string(),
                    },
                    Action::Warn => GuardDecision::Warn {
                        reason: pattern.description.to_string(),
                        path: path.to_string(),
                    },
                });
            }
        }
        None
    }

    fn check_bash_command(&self, cmd: &str) -> Option<GuardDecision> {
        // Check for commands that commonly access secret stores
        let secret_commands = [
            ("security find-generic-password", "macOS Keychain access"),
            ("security find-internet-password", "macOS Keychain access"),
            ("pass show", "password-store access"),
            ("gpg --decrypt", "GPG decryption"),
            ("vault read", "HashiCorp Vault read"),
            ("aws secretsmanager", "AWS Secrets Manager"),
            ("gcloud secrets", "GCP Secret Manager"),
        ];

        for (pattern, desc) in &secret_commands {
            if cmd.contains(pattern) {
                return Some(GuardDecision::Warn {
                    reason: desc.to_string(),
                    path: cmd.to_string(),
                });
            }
        }

        // Also check for file paths in the command
        for pattern in SENSITIVE_PATTERNS {
            if cmd.contains(pattern.pattern) {
                return Some(match pattern.action {
                    Action::Block => GuardDecision::Block {
                        reason: format!("Command references {}", pattern.description),
                        path: cmd.to_string(),
                    },
                    Action::Warn => GuardDecision::Warn {
                        reason: format!("Command references {}", pattern.description),
                        path: cmd.to_string(),
                    },
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn block_ssh_key_read() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": "/home/user/.ssh/id_rsa"}));
        assert!(decision.is_some());
        assert!(decision.unwrap().is_block());
    }

    #[test]
    fn warn_env_file() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": ".env"}));
        assert!(decision.is_some());
        assert!(!decision.unwrap().is_block()); // warn, not block
    }

    #[test]
    fn allow_normal_file() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": "src/main.rs"}));
        assert!(decision.is_none());
    }

    #[test]
    fn warn_bash_keychain_access() {
        let guard = PathGuard::new();
        let decision = guard.check("bash", &json!({"command": "security find-generic-password -s myapp -w"}));
        assert!(decision.is_some());
    }

    #[test]
    fn block_vault_json() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": "/home/user/.omegon/vault.json"}));
        assert!(decision.is_some());
        assert!(decision.unwrap().is_block());
    }

    #[test]
    fn block_vault_token() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": "/home/user/.vault-token"}));
        assert!(decision.is_some());
        assert!(decision.unwrap().is_block());
    }

    #[test]
    fn block_aws_credentials() {
        let guard = PathGuard::new();
        let decision = guard.check("read", &json!({"path": "/home/user/.aws/credentials"}));
        assert!(decision.is_some());
        assert!(decision.unwrap().is_block());
    }
}
