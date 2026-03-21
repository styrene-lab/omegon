//! Delegate feature — spawn subagents with scoped tasks.
//!
//! Provides three tools for delegating tasks:
//! - `delegate`: spawn a background or synchronous subagent
//! - `delegate_result`: retrieve results from background delegates
//! - `delegate_status`: list all active/completed delegates
//!
//! The feature manages a result store for async delegates and emits notifications
//! when background tasks complete.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContextInjection, ContextSignals,
    ContentBlock, Feature, NotifyLevel, ToolDefinition, ToolResult,
};

/// Agent specification loaded from .omegon/agents/*.md
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub is_write_agent: bool,
}

/// Status of a delegate task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelegateTaskStatus {
    Running,
    Completed { success: bool },
    Failed { error: String },
}

/// A delegate task entry in the result store
#[derive(Debug, Clone)]
pub struct DelegateTask {
    pub task_id: String,
    pub agent_name: Option<String>,
    pub task_description: String,
    pub status: DelegateTaskStatus,
    pub result: Option<String>,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
}

/// Thread-safe store for delegate task results
#[derive(Debug, Clone)]
pub struct DelegateResultStore {
    tasks: Arc<Mutex<HashMap<String, DelegateTask>>>,
    next_id: Arc<Mutex<u32>>,
}

impl DelegateResultStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn generate_task_id(&self) -> String {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        format!("delegate_{}", id)
    }

    pub fn store_task(&self, task: DelegateTask) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task.task_id.clone(), task);
    }

    pub fn get_task(&self, task_id: &str) -> Option<DelegateTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(task_id).cloned()
    }

    pub fn update_task_status(&self, task_id: &str, status: DelegateTaskStatus, result: Option<String>) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status;
            task.result = result;
            if matches!(task.status, DelegateTaskStatus::Completed { .. } | DelegateTaskStatus::Failed { .. }) {
                task.completed_at = Some(SystemTime::now());
            }
        }
    }

    pub fn list_all_tasks(&self) -> Vec<DelegateTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().cloned().collect()
    }
}

/// Mock delegate runner for this implementation
/// In a real implementation, this would interface with the actual delegate engine
pub struct DelegateRunner {
    cwd: PathBuf,
    result_store: Arc<DelegateResultStore>,
}

impl DelegateRunner {
    pub fn new(cwd: PathBuf, result_store: Arc<DelegateResultStore>) -> Self {
        Self { cwd, result_store }
    }

    pub async fn spawn_delegate(
        &self,
        task_id: String,
        agent_name: Option<String>,
        task: String,
        _scope: Option<Vec<String>>,
        _model: Option<String>,
        _thinking_level: Option<String>,
        _facts: Option<Vec<String>>,
        _mind: Option<String>,
    ) -> anyhow::Result<()> {
        // For now, just simulate task execution
        let task_entry = DelegateTask {
            task_id: task_id.clone(),
            agent_name,
            task_description: task.clone(),
            status: DelegateTaskStatus::Running,
            result: None,
            started_at: SystemTime::now(),
            completed_at: None,
        };
        
        self.result_store.store_task(task_entry);

        // Spawn a background task that simulates work
        let store = self.result_store.clone();
        tokio::spawn(async move {
            // Simulate some work
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            
            // Mark as completed with mock result
            store.update_task_status(
                &task_id,
                DelegateTaskStatus::Completed { success: true },
                Some(format!("Task completed: {}", task)),
            );
        });

        Ok(())
    }

    pub async fn wait_for_result(
        &self,
        task_id: &str,
        _cancel: CancellationToken,
    ) -> anyhow::Result<String> {
        // Poll for completion (in real implementation, this would be more sophisticated)
        for _ in 0..30 { // 30 second timeout
            if let Some(task) = self.result_store.get_task(task_id) {
                match task.status {
                    DelegateTaskStatus::Completed { success: true } => {
                        return Ok(task.result.unwrap_or_else(|| "Task completed".to_string()));
                    }
                    DelegateTaskStatus::Completed { success: false } => {
                        return Err(anyhow::anyhow!("Task failed"));
                    }
                    DelegateTaskStatus::Failed { error } => {
                        return Err(anyhow::anyhow!("Task failed: {}", error));
                    }
                    DelegateTaskStatus::Running => {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            } else {
                return Err(anyhow::anyhow!("Task not found"));
            }
        }
        Err(anyhow::anyhow!("Task timed out"))
    }
}

/// The main delegate feature
pub struct DelegateFeature {
    result_store: Arc<DelegateResultStore>,
    available_agents: Vec<AgentSpec>,
    runner: Arc<DelegateRunner>,
}

impl DelegateFeature {
    pub fn new(cwd: &PathBuf, agents: Vec<AgentSpec>) -> Self {
        let result_store = Arc::new(DelegateResultStore::new());
        let runner = Arc::new(DelegateRunner::new(cwd.clone(), result_store.clone()));
        
        Self {
            result_store,
            available_agents: agents,
            runner,
        }
    }
}

#[async_trait]
impl Feature for DelegateFeature {
    fn name(&self) -> &str {
        "delegate"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "delegate".to_string(),
                label: "Delegate Task".to_string(),
                description: "Spawn a subagent to handle a specific task".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "Task description for the delegate"
                        },
                        "agent": {
                            "type": "string",
                            "description": "Optional specific agent to use"
                        },
                        "scope": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional file scope for the delegate"
                        },
                        "model": {
                            "type": "string",
                            "description": "Optional model override for the delegate"
                        },
                        "thinking_level": {
                            "type": "string",
                            "description": "Optional thinking level for the delegate"
                        },
                        "facts": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional facts to inject"
                        },
                        "mind": {
                            "type": "string",
                            "description": "Optional mind context for the delegate"
                        },
                        "background": {
                            "type": "boolean",
                            "description": "Run in background (default: true)",
                            "default": true
                        }
                    },
                    "required": ["task"]
                }),
            },
            ToolDefinition {
                name: "delegate_result".to_string(),
                label: "Get Delegate Result".to_string(),
                description: "Retrieve result from a background delegate task".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "The task ID to retrieve results for"
                        }
                    },
                    "required": ["task_id"]
                }),
            },
            ToolDefinition {
                name: "delegate_status".to_string(),
                label: "Delegate Status".to_string(),
                description: "List all delegate tasks and their status".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "delegate" => {
                let task: String = args.get("task")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task parameter is required"))?
                    .to_string();
                
                let agent = args.get("agent").and_then(|v| v.as_str()).map(|s| s.to_string());
                let scope = args.get("scope")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect());
                let model = args.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                let thinking_level = args.get("thinking_level").and_then(|v| v.as_str()).map(|s| s.to_string());
                let facts = args.get("facts")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect());
                let mind = args.get("mind").and_then(|v| v.as_str()).map(|s| s.to_string());
                let background = args.get("background").and_then(|v| v.as_bool()).unwrap_or(true);

                // Validate agent if specified
                if let Some(ref agent_name) = agent {
                    if !self.available_agents.iter().any(|a| a.name == *agent_name) {
                        return Err(anyhow::anyhow!("Unknown agent: {}", agent_name));
                    }
                }

                let task_id = self.result_store.generate_task_id();

                // Spawn the delegate
                self.runner.spawn_delegate(
                    task_id.clone(),
                    agent,
                    task,
                    scope,
                    model,
                    thinking_level,
                    facts,
                    mind,
                ).await?;

                if background {
                    // Return task ID for background execution
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("{{\"task_id\": \"{}\"}}", task_id),
                        }],
                        details: json!({ "task_id": task_id, "background": true }),
                    })
                } else {
                    // Wait for completion and return result
                    let result = self.runner.wait_for_result(&task_id, cancel).await?;
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: result }],
                        details: json!({ "task_id": task_id, "background": false }),
                    })
                }
            }

            "delegate_result" => {
                let task_id: String = args.get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task_id parameter is required"))?
                    .to_string();

                match self.result_store.get_task(&task_id) {
                    Some(task) => match task.status {
                        DelegateTaskStatus::Running => {
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: "Task still running".to_string(),
                                }],
                                details: json!({ "status": "running", "task_id": task_id }),
                            })
                        }
                        DelegateTaskStatus::Completed { success: true } => {
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: task.result.unwrap_or_else(|| "Task completed".to_string()),
                                }],
                                details: json!({ "status": "completed", "success": true, "task_id": task_id }),
                            })
                        }
                        DelegateTaskStatus::Completed { success: false } => {
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: "Task completed with failure".to_string(),
                                }],
                                details: json!({ "status": "completed", "success": false, "task_id": task_id }),
                            })
                        }
                        DelegateTaskStatus::Failed { error } => {
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: format!("Task failed: {}", error),
                                }],
                                details: json!({ "status": "failed", "error": error, "task_id": task_id }),
                            })
                        }
                    },
                    None => Err(anyhow::anyhow!("Task not found: {}", task_id)),
                }
            }

            "delegate_status" => {
                let tasks = self.result_store.list_all_tasks();
                let mut status_text = String::from("# Delegate Tasks\n\n| Task ID | Agent | Status | Description |\n|---------|-------|--------|-------------|\n");
                
                for task in tasks {
                    let agent = task.agent_name.as_deref().unwrap_or("default");
                    let status = match task.status {
                        DelegateTaskStatus::Running => "🔄 Running".to_string(),
                        DelegateTaskStatus::Completed { success: true } => "✅ Completed".to_string(),
                        DelegateTaskStatus::Completed { success: false } => "❌ Failed".to_string(),
                        DelegateTaskStatus::Failed { .. } => "❌ Error".to_string(),
                    };
                    let description = if task.task_description.len() > 50 {
                        format!("{}...", &task.task_description[..50])
                    } else {
                        task.task_description.clone()
                    };
                    
                    status_text.push_str(&format!("| {} | {} | {} | {} |\n", 
                        task.task_id, agent, status, description));
                }
                
                if self.result_store.list_all_tasks().is_empty() {
                    status_text.push_str("\nNo delegate tasks found.\n");
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: status_text }],
                    details: json!({ "task_count": self.result_store.list_all_tasks().len() }),
                })
            }

            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "delegate".to_string(),
            description: "delegate task management".to_string(),
            subcommands: vec!["status".to_string()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name == "delegate" {
            match args.trim() {
                "status" | "" => {
                    let tasks = self.result_store.list_all_tasks();
                    let mut result = format!("Delegate Tasks ({} total):\n\n", tasks.len());
                    
                    if tasks.is_empty() {
                        result.push_str("No delegate tasks found.\n");
                    } else {
                        for task in tasks {
                            let status = match task.status {
                                DelegateTaskStatus::Running => "🔄 Running",
                                DelegateTaskStatus::Completed { success: true } => "✅ Completed",
                                DelegateTaskStatus::Completed { success: false } => "❌ Failed",
                                DelegateTaskStatus::Failed { .. } => "❌ Error",
                            };
                            let agent = task.agent_name.as_deref().unwrap_or("default");
                            
                            result.push_str(&format!("  {} - {} - {} ({})\n", 
                                task.task_id, status, task.task_description, agent));
                        }
                    }
                    
                    CommandResult::Display(result)
                }
                _ => CommandResult::NotHandled,
            }
        } else {
            CommandResult::NotHandled
        }
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        if self.available_agents.is_empty() {
            return None;
        }

        let agents_list = self.available_agents
            .iter()
            .map(|agent| format!("  {} - {}", agent.name, agent.description))
            .collect::<Vec<_>>()
            .join("\n");

        Some(ContextInjection {
            source: "delegate".to_string(),
            content: format!("Available agents:\n{}", agents_list),
            priority: 5,
            ttl_turns: 10,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd { .. } => {
                // Check for completed background tasks and notify
                let tasks = self.result_store.list_all_tasks();
                let mut requests = Vec::new();

                for task in tasks {
                    if let DelegateTaskStatus::Completed { success } = task.status {
                        if let Some(completed_at) = task.completed_at {
                            // Only notify if completed recently (within last 5 seconds)
                            if completed_at.elapsed().unwrap_or_default().as_secs() < 5 {
                                let message = if success {
                                    format!("✅ Delegate {} completed: {}", task.task_id, task.task_description)
                                } else {
                                    format!("❌ Delegate {} failed: {}", task.task_id, task.task_description)
                                };
                                
                                requests.push(BusRequest::Notify {
                                    message,
                                    level: if success { NotifyLevel::Info } else { NotifyLevel::Warning },
                                });
                            }
                        }
                    }
                }

                requests
            }
            _ => vec![],
        }
    }
}

/// Agent loader - scans .omegon/agents/*.md files for agent specifications
pub fn scan_agents(cwd: &PathBuf) -> Vec<AgentSpec> {
    let agents_dir = cwd.join(".omegon").join("agents");
    let mut agents = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            if let Some(extension) = entry.path().extension() {
                if extension == "md" {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Some(agent) = parse_agent_spec(&content) {
                            agents.push(agent);
                        }
                    }
                }
            }
        }
    }

    // Add default agents if no custom agents found
    if agents.is_empty() {
        agents.push(AgentSpec {
            name: "general".to_string(),
            description: "General-purpose assistant agent".to_string(),
            is_write_agent: true,
        });
        agents.push(AgentSpec {
            name: "analyzer".to_string(),
            description: "Read-only analysis and research agent".to_string(),
            is_write_agent: false,
        });
    }

    agents
}

/// Parse agent specification from markdown content
fn parse_agent_spec(content: &str) -> Option<AgentSpec> {
    let lines: Vec<&str> = content.lines().collect();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut is_write_agent = false;

    for line in lines {
        let line = line.trim();
        if let Some(title) = line.strip_prefix("# ") {
            name = Some(title.to_string());
        } else if let Some(desc) = line.strip_prefix("> ") {
            description = Some(desc.to_string());
        } else if line.contains("write") && (line.contains("agent") || line.contains("mode")) {
            is_write_agent = true;
        }
    }

    if let (Some(name), Some(description)) = (name, description) {
        Some(AgentSpec {
            name,
            description,
            is_write_agent,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_delegate_feature_tools() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![AgentSpec {
            name: "test_agent".to_string(),
            description: "Test agent".to_string(),
            is_write_agent: true,
        }];
        
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);
        let tools = feature.tools();
        
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "delegate"));
        assert!(tools.iter().any(|t| t.name == "delegate_result"));
        assert!(tools.iter().any(|t| t.name == "delegate_status"));
    }

    #[tokio::test]
    async fn test_sync_delegate_unknown_agent() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![];
        
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);
        
        let args = json!({
            "task": "test task",
            "agent": "unknown_agent",
            "background": false
        });
        
        let result = feature.execute("delegate", "test_call", args, CancellationToken::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delegate_result_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), vec![]);
        
        let args = json!({ "task_id": "nonexistent_task" });
        
        let result = feature.execute("delegate_result", "test_call", args, CancellationToken::new()).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_context_lists_agents() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![
            AgentSpec {
                name: "agent1".to_string(),
                description: "First agent".to_string(),
                is_write_agent: true,
            },
            AgentSpec {
                name: "agent2".to_string(),
                description: "Second agent".to_string(),
                is_write_agent: false,
            },
        ];
        
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);
        
        let signals = ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 1000,
        };
        
        let context = feature.provide_context(&signals);
        assert!(context.is_some());
        
        let context = context.unwrap();
        assert!(context.content.contains("agent1"));
        assert!(context.content.contains("agent2"));
        assert!(context.content.contains("First agent"));
        assert!(context.content.contains("Second agent"));
    }

    #[test]
    fn test_parse_agent_spec() {
        let content = r#"# TestAgent

> A test agent for testing purposes

This agent runs in write mode and can modify files.
"#;
        
        let spec = parse_agent_spec(content);
        assert!(spec.is_some());
        
        let spec = spec.unwrap();
        assert_eq!(spec.name, "TestAgent");
        assert_eq!(spec.description, "A test agent for testing purposes");
        assert!(spec.is_write_agent);
    }

    #[test]
    fn test_scan_agents() {
        let temp_dir = TempDir::new().unwrap();
        let agents_dir = temp_dir.path().join(".omegon").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        
        // Create test agent files
        std::fs::write(
            agents_dir.join("test1.md"),
            "# TestAgent1\n\n> Test agent 1\n\nwrite agent capabilities"
        ).unwrap();
        
        std::fs::write(
            agents_dir.join("test2.md"), 
            "# TestAgent2\n\n> Test agent 2\n\nread-only analysis"
        ).unwrap();
        
        let agents = scan_agents(&temp_dir.path().to_path_buf());
        assert_eq!(agents.len(), 2);
        
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"TestAgent1"));
        assert!(names.contains(&"TestAgent2"));
        
        // Check write agent detection
        let write_agent = agents.iter().find(|a| a.name == "TestAgent1").unwrap();
        assert!(write_agent.is_write_agent);
        
        let read_agent = agents.iter().find(|a| a.name == "TestAgent2").unwrap();
        assert!(!read_agent.is_write_agent);
    }
}
