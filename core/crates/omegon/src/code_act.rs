//! Code-act execution mode — LLM generates executable scripts instead of
//! sequential tool calls. Phase 1: Python scripts with filesystem-local
//! tool proxies, executed via subprocess.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::tools::bash;
use omegon_traits::{ContentBlock, ToolResult};

const PYTHON_PRELUDE: &str = r#"
import json, os, subprocess, sys

def bash(command: str) -> str:
    """Run a shell command and return stdout. Stderr is printed."""
    result = subprocess.run(command, shell=True, capture_output=True, text=True)
    if result.stderr:
        print(result.stderr, file=sys.stderr)
    if result.returncode != 0:
        raise RuntimeError(f"command failed (exit {result.returncode}): {result.stderr.strip()}")
    return result.stdout

def read_file(path: str) -> str:
    """Read a file and return its contents."""
    with open(path) as f:
        return f.read()

def write_file(path: str, content: str) -> None:
    """Write content to a file, creating parent directories."""
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, 'w') as f:
        f.write(content)

def list_files(directory: str = ".", pattern: str = "**/*") -> list[str]:
    """List files matching a glob pattern."""
    import glob
    return sorted(glob.glob(os.path.join(directory, pattern), recursive=True))

"#;

const CODE_GEN_RULES: &str = "\
Generate a complete Python script that accomplishes the task. Rules:\n\
- Use the provided helper functions: bash(), read_file(), write_file(), list_files()\n\
- Print your final result to stdout — that's what the user sees\n\
- Use try/except for error handling where appropriate\n\
- You may use asyncio.gather() for parallel operations if beneficial\n\
- Do NOT use any packages that aren't in Python's standard library\n\
- Do NOT use input() or any interactive prompts\n\
- Wrap your main logic in a function and call it\n\
\n\
Respond with ONLY a Python code block (```python ... ```). No explanation before or after.";

pub struct CodeActExecutor {
    cwd: PathBuf,
    permitted: bool,
}

impl CodeActExecutor {
    pub fn new(cwd: PathBuf) -> Self {
        let bypass = std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok();
        let code_act = std::env::var("OMEGON_CODE_ACT")
            .map(|v| matches!(v.as_str(), "1" | "true"))
            .unwrap_or(false);
        Self { cwd, permitted: bypass || code_act }
    }

    pub fn build_prompt(&self, task: &str, context: Option<&str>) -> String {
        let mut prompt = String::with_capacity(2048);
        prompt.push_str("You are an autonomous coding agent operating in code-act mode.\n\n");

        prompt.push_str("## Available Functions\n\n");
        prompt.push_str("```python\n");
        prompt.push_str("def bash(command: str) -> str\n");
        prompt.push_str("def read_file(path: str) -> str\n");
        prompt.push_str("def write_file(path: str, content: str) -> None\n");
        prompt.push_str("def list_files(directory: str = '.', pattern: str = '**/*') -> list[str]\n");
        prompt.push_str("```\n\n");

        prompt.push_str(&format!("## Working Directory\n\n`{}`\n\n", self.cwd.display()));

        if let Some(ctx) = context {
            prompt.push_str(&format!("## Context\n\n{ctx}\n\n"));
        }

        prompt.push_str(&format!("## Task\n\n{task}\n\n"));
        prompt.push_str(&format!("## Instructions\n\n{CODE_GEN_RULES}\n"));
        prompt
    }

    pub fn extract_code(response: &str) -> Option<String> {
        if let Some(start) = response.find("```python") {
            let code_start = start + "```python".len();
            if let Some(end) = response[code_start..].find("```") {
                return Some(response[code_start..code_start + end].trim().to_string());
            }
        }
        if let Some(start) = response.find("```") {
            let code_start = start + "```".len();
            let code_start = if response[code_start..].starts_with('\n') {
                code_start + 1
            } else {
                code_start
            };
            if let Some(end) = response[code_start..].find("```") {
                return Some(response[code_start..code_start + end].trim().to_string());
            }
        }
        None
    }

    pub async fn execute_script(
        &self,
        script: &str,
        timeout_secs: Option<u64>,
        cancel: CancellationToken,
    ) -> Result<CodeActResult> {
        if !self.permitted {
            anyhow::bail!(
                "code-act execution requires explicit opt-in: set OMEGON_CODE_ACT=1 \
                 or use --dangerously-bypass-permissions"
            );
        }

        let full_script = format!("{PYTHON_PRELUDE}{script}");

        let run_id = uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("tmp").to_string();
        let script_path = self.cwd.join(".omegon").join(format!("code-act-{run_id}.py"));
        if let Some(parent) = script_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&script_path, &full_script)?;

        let command = format!("python3 {}", script_path.display());
        let result = bash::execute(&command, &self.cwd, timeout_secs, cancel).await?;

        let _ = std::fs::remove_file(&script_path);

        let output = result
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        let is_error = result
            .details
            .get("exitCode")
            .and_then(|v| v.as_i64())
            .is_some_and(|code| code != 0);

        Ok(CodeActResult {
            output,
            is_error,
            exit_code: result
                .details
                .get("exitCode")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1) as i32,
        })
    }

    pub fn build_retry_prompt(&self, task: &str, code: &str, error: &str) -> String {
        let mut prompt = self.build_prompt(task, None);
        prompt.push_str("\n## Previous Attempt (Failed)\n\n");
        prompt.push_str(&format!("```python\n{code}\n```\n\n"));
        prompt.push_str(&format!("## Error\n\n```\n{error}\n```\n\n"));
        prompt.push_str("Fix the error and generate a corrected script. Same rules apply.\n");
        prompt
    }
}

#[derive(Debug)]
pub struct CodeActResult {
    pub output: String,
    pub is_error: bool,
    pub exit_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_python_fenced_block() {
        let response = "Here's the code:\n\n```python\nprint('hello')\n```\n\nDone.";
        let code = CodeActExecutor::extract_code(response).unwrap();
        assert_eq!(code, "print('hello')");
    }

    #[test]
    fn extract_unfenced_block() {
        let response = "```\nimport os\nprint(os.getcwd())\n```";
        let code = CodeActExecutor::extract_code(response).unwrap();
        assert_eq!(code, "import os\nprint(os.getcwd())");
    }

    #[test]
    fn extract_no_code_block() {
        let response = "I can't generate code for this.";
        assert!(CodeActExecutor::extract_code(response).is_none());
    }

    #[test]
    fn build_prompt_includes_task() {
        let exec = CodeActExecutor::new(PathBuf::from("/tmp/test"));
        let prompt = exec.build_prompt("Fix the tests", None);
        assert!(prompt.contains("Fix the tests"));
        assert!(prompt.contains("bash(command: str)"));
        assert!(prompt.contains("/tmp/test"));
    }

    #[test]
    fn build_prompt_includes_context() {
        let exec = CodeActExecutor::new(PathBuf::from("/tmp"));
        let prompt = exec.build_prompt("task", Some("This is a Rust project"));
        assert!(prompt.contains("This is a Rust project"));
    }

    #[test]
    fn retry_prompt_includes_error() {
        let exec = CodeActExecutor::new(PathBuf::from("/tmp"));
        let prompt = exec.build_retry_prompt("task", "print(x)", "NameError: x is not defined");
        assert!(prompt.contains("NameError"));
        assert!(prompt.contains("print(x)"));
        assert!(prompt.contains("Previous Attempt"));
    }

    fn permitted_executor(cwd: PathBuf) -> CodeActExecutor {
        CodeActExecutor { cwd, permitted: true }
    }

    #[tokio::test]
    async fn execute_rejects_without_opt_in() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = CodeActExecutor { cwd: tmp.path().to_path_buf(), permitted: false };
        let cancel = CancellationToken::new();
        let result = exec.execute_script("print('nope')", Some(5), cancel).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("code-act execution requires"));
    }

    #[tokio::test]
    async fn execute_simple_script() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = permitted_executor(tmp.path().to_path_buf());
        let cancel = CancellationToken::new();

        let result = exec
            .execute_script("print('hello from code-act')", Some(10), cancel)
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.exit_code, 0);
        assert!(result.output.contains("hello from code-act"));
    }

    #[tokio::test]
    async fn execute_script_with_file_ops() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = permitted_executor(tmp.path().to_path_buf());
        let cancel = CancellationToken::new();

        let script = r#"
write_file("test_output.txt", "generated content")
content = read_file("test_output.txt")
print(f"Read back: {content}")
"#;

        let result = exec.execute_script(script, Some(10), cancel).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("Read back: generated content"));
        assert!(tmp.path().join("test_output.txt").exists());
    }

    #[tokio::test]
    async fn execute_script_error_captured() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = permitted_executor(tmp.path().to_path_buf());
        let cancel = CancellationToken::new();

        let result = exec
            .execute_script("raise ValueError('intentional')", Some(10), cancel)
            .await
            .unwrap();

        assert!(result.is_error);
        assert_ne!(result.exit_code, 0);
        assert!(result.output.contains("ValueError") || result.output.contains("intentional"));
    }

    #[tokio::test]
    async fn execute_script_with_bash_proxy() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = permitted_executor(tmp.path().to_path_buf());
        let cancel = CancellationToken::new();

        let script = r#"
result = bash("echo 'subprocess works'")
print(f"Got: {result.strip()}")
"#;

        let result = exec.execute_script(script, Some(10), cancel).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("Got: subprocess works"));
    }

    #[tokio::test]
    async fn execute_script_cleanup_removes_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let exec = permitted_executor(tmp.path().to_path_buf());
        let cancel = CancellationToken::new();

        exec.execute_script("print('clean')", Some(10), cancel)
            .await
            .unwrap();

        let leftover: Vec<_> = std::fs::read_dir(tmp.path().join(".omegon"))
            .into_iter()
            .flat_map(|d| d.filter_map(|e| e.ok()))
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "py"))
            .collect();
        assert!(leftover.is_empty(), "temp script should be cleaned up, found: {leftover:?}");
    }
}
