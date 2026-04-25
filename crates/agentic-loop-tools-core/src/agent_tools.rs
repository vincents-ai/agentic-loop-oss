//! Agent-level tools for delegation, human interaction, memory, and self-diagnostics.
//!
//! These tools give the agent the ability to:
//! - Ask humans for input or decisions
//! - Delegate subtasks to specialized agents
//! - Check its own health
//! - Manage context/memory compaction
//! - Get project structure info
//! - List files with filtering

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

// ── AskHuman ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AskHumanTool;

#[async_trait]
impl Tool for AskHumanTool {
    agentic_loop_tools::impl_clone_box!(AskHumanTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "ask_human".to_string(),
            description: "Ask a human a question and wait for their response. Use when you need clarification, decisions, or information you can't find yourself.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question to ask" },
                    "context": { "type": "string", "description": "Optional context for the question" }
                },
                "required": ["question"]
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Args { question: String, #[serde(default)] context: Option<String> }
        let args: Args = serde_json::from_slice(args)?;
        Ok(serde_json::to_vec(&serde_json::json!({
            "type": "human_input_required",
            "question": args.question,
            "context": args.context,
            "instruction": "The agent is waiting for human input. Provide your response to continue."
        }))?)
    }
}

// ── DelegateTask ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DelegateTaskTool;

#[async_trait]
impl Tool for DelegateTaskTool {
    agentic_loop_tools::impl_clone_box!(DelegateTaskTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "delegate_task".to_string(),
            description: "Delegate a subtask to a specialized agent. Available agents: coding, review, research, security, planning.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The task to delegate" },
                    "agent_type": { "type": "string", "description": "Agent to use: coding, review, research, security, planning", "default": "coding" },
                    "context": { "type": "string", "description": "Additional context for the agent" },
                    "priority": { "type": "string", "description": "Priority: low, medium, high, critical" }
                },
                "required": ["task"]
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Args {
            task: String,
            #[serde(default = "default_agent")]
            agent_type: String,
            #[serde(default)]
            context: Option<String>,
            #[serde(default)]
            priority: Option<String>,
        }
        fn default_agent() -> String { "coding".to_string() }
        let args: Args = serde_json::from_slice(args)?;
        let valid_agents = ["coding", "review", "research", "security", "planning"];
        if !valid_agents.contains(&args.agent_type.as_str()) {
            anyhow::bail!("Unknown agent type '{}'. Valid: {:?}", args.agent_type, valid_agents);
        }
        Ok(serde_json::to_vec(&serde_json::json!({
            "delegated": true,
            "task": args.task,
            "agent_type": args.agent_type,
            "priority": args.priority,
            "status": "queued",
            "message": format!("Task delegated to {} agent", args.agent_type)
        }))?)
    }
}

// ── HealthCheck ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct HealthCheckTool;

#[async_trait]
impl Tool for HealthCheckTool {
    agentic_loop_tools::impl_clone_box!(HealthCheckTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "health_check".to_string(),
            description: "Check the agent's health: am I stuck? overspending? which tools are failing? Returns a diagnosis with recommendations.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "detailed": { "type": "boolean", "description": "Include per-tool breakdown", "default": false }
                }
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Args { #[serde(default)] detailed: bool }
        let _args: Args = serde_json::from_slice(args).unwrap_or(Args { detailed: false });
        Ok(serde_json::to_vec(&serde_json::json!({
            "type": "health_check_request",
            "message": "Health check requested. Runner will inject current diagnosis."
        }))?)
    }
}

// ── CompactContext ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CompactContextTool;

#[async_trait]
impl Tool for CompactContextTool {
    agentic_loop_tools::impl_clone_box!(CompactContextTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "compact_context".to_string(),
            description: "Request context compaction when approaching token limits. Summarizes conversation history to free up space.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "keep_recent": { "type": "integer", "description": "Number of recent messages to keep", "default": 10 },
                    "focus": { "type": "string", "description": "Topic to focus the summary on" }
                }
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default = "default_retention")]
            keep_recent: usize,
            #[serde(default)]
            focus: Option<String>,
        }
        fn default_retention() -> usize { 10 }
        let args: Args = serde_json::from_slice(args).unwrap_or(Args { keep_recent: default_retention(), focus: None });
        Ok(serde_json::to_vec(&serde_json::json!({
            "type": "compact_request",
            "keep_recent": args.keep_recent,
            "focus": args.focus,
            "message": "Context compaction requested. Runner will summarize older messages."
        }))?)
    }
}

// ── ProjectInfo ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ProjectInfoTool;

#[async_trait]
impl Tool for ProjectInfoTool {
    agentic_loop_tools::impl_clone_box!(ProjectInfoTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "project_info".to_string(),
            description: "Get project structure: directory tree, file counts by type, total size. Useful for understanding unfamiliar codebases.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Root directory path" },
                    "max_depth": { "type": "integer", "description": "Maximum directory depth to scan", "default": 3 }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            #[serde(default = "default_depth")]
            max_depth: usize,
        }
        fn default_depth() -> usize { 3 }
        let args: Args = serde_json::from_slice(args)?;
        let root = PathBuf::from(&args.path);
        if !root.exists() { anyhow::bail!("Path does not exist: {}", args.path); }
        if !root.is_dir() { anyhow::bail!("Path is not a directory: {}", args.path); }

        let mut file_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut total_size: u64 = 0;
        let mut total_files: usize = 0;
        let mut total_dirs: usize = 0;

        walk_dir(&root, 0, args.max_depth, &mut file_counts, &mut total_size, &mut total_files, &mut total_dirs);

        let mut extensions: Vec<(String, usize)> = file_counts.into_iter().collect();
        extensions.sort_by(|a, b| b.1.cmp(&a.1));
        extensions.truncate(15);

        Ok(serde_json::to_vec(&serde_json::json!({
            "path": args.path,
            "total_files": total_files,
            "total_dirs": total_dirs,
            "total_size_bytes": total_size,
            "total_size_human": format_size(total_size),
            "extensions": extensions,
            "max_depth": args.max_depth,
        }))?)
    }
}

// ── ListFiles ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ListFilesTool;

#[async_trait]
impl Tool for ListFilesTool {
    agentic_loop_tools::impl_clone_box!(ListFilesTool);
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "list_files".to_string(),
            description: "Recursively list files in a directory, optionally filtering by extension or glob pattern.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to list" },
                    "extension": { "type": "string", "description": "Filter by file extension (e.g. 'rs')" },
                    "pattern": { "type": "string", "description": "Glob pattern filter (e.g. '**/*.rs')" },
                    "max_depth": { "type": "integer", "description": "Maximum depth", "default": 3 }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            #[serde(default)]
            pattern: Option<String>,
            #[serde(default)]
            extension: Option<String>,
            #[serde(default = "default_depth")]
            max_depth: usize,
        }
        fn default_depth() -> usize { 3 }
        let args: Args = serde_json::from_slice(args)?;
        let root = PathBuf::from(&args.path);
        if !root.exists() { anyhow::bail!("Path does not exist: {}", args.path); }

        let mut files: Vec<serde_json::Value> = Vec::new();
        collect_files(&root, 0, args.max_depth, &args.extension, &mut files, &root);

        if let Some(ref pattern) = args.pattern {
            if let Ok(glob) = glob::Pattern::new(pattern) {
                files.retain(|f| {
                    let p = f["path"].as_str().unwrap_or("");
                    glob.matches(p)
                });
            }
        }

        files.sort_by(|a, b| a["path"].as_str().unwrap_or("").cmp(b["path"].as_str().unwrap_or("")));
        files.truncate(500);

        Ok(serde_json::to_vec(&serde_json::json!({
            "path": args.path,
            "count": files.len(),
            "files": files,
        }))?)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

const SKIP_DIRS: &[&str] = &["target", "node_modules", "__pycache__", ".git", "build", "dist", ".next", ".venv"];

fn should_skip(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

fn walk_dir(
    dir: &std::path::Path, depth: usize, max_depth: usize,
    file_counts: &mut std::collections::HashMap<String, usize>,
    total_size: &mut u64, total_files: &mut usize, total_dirs: &mut usize,
) {
    if depth > max_depth { return; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if should_skip(&name) { continue; }
            if path.is_dir() {
                *total_dirs += 1;
                walk_dir(&path, depth + 1, max_depth, file_counts, total_size, total_files, total_dirs);
            } else {
                *total_files += 1;
                if let Ok(meta) = std::fs::metadata(&path) { *total_size += meta.len(); }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("(no ext)").to_string();
                *file_counts.entry(ext).or_insert(0) += 1;
            }
        }
    }
}

fn collect_files(
    dir: &std::path::Path, depth: usize, max_depth: usize,
    extension: &Option<String>, files: &mut Vec<serde_json::Value>, base: &std::path::Path,
) {
    if depth > max_depth { return; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if should_skip(&name) { continue; }
            if path.is_dir() {
                collect_files(&path, depth + 1, max_depth, extension, files, base);
            } else {
                let ext_match = match extension {
                    Some(ext) => path.extension().and_then(|e| e.to_str()).map(|e| e == ext).unwrap_or(false),
                    None => true,
                };
                if ext_match {
                    let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    files.push(serde_json::json!({ "path": rel, "size": size }));
                }
            }
        }
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB { format!("{:.1} GB", bytes as f64 / GB as f64) }
    else if bytes >= MB { format!("{:.1} MB", bytes as f64 / MB as f64) }
    else if bytes >= KB { format!("{:.1} KB", bytes as f64 / KB as f64) }
    else { format!("{} B", bytes) }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tool_args(json: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(json).unwrap()
    }

    #[tokio::test]
    async fn test_ask_human_returns_prompt() {
        let tool = AskHumanTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"question": "Should I proceed?"}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["type"], "human_input_required");
        assert_eq!(v["question"], "Should I proceed?");
    }

    #[tokio::test]
    async fn test_delegate_valid_coding() {
        let tool = DelegateTaskTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"task": "Fix auth bug", "agent_type": "coding"}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert!(v["delegated"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_delegate_invalid_agent() {
        let tool = DelegateTaskTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"task": "stuff", "agent_type": "nonexistent"}))).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delegate_default_agent() {
        let tool = DelegateTaskTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"task": "Write tests"}))).await;
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["agent_type"], "coding");
    }

    #[tokio::test]
    async fn test_delegate_all_valid_agents() {
        let tool = DelegateTaskTool;
        for agent in &["coding", "review", "research", "security", "planning"] {
            let result = tool.execute(&tool_args(&serde_json::json!({"task": "test", "agent_type": agent}))).await;
            assert!(result.is_ok(), "Agent '{}' should be valid", agent);
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let tool = HealthCheckTool;
        let result = tool.execute(&tool_args(&serde_json::json!({}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["type"], "health_check_request");
    }

    #[tokio::test]
    async fn test_compact_context() {
        let tool = CompactContextTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"keep_recent": 5}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["type"], "compact_request");
        assert_eq!(v["keep_recent"], 5);
    }

    #[tokio::test]
    async fn test_project_info_nonexistent() {
        let tool = ProjectInfoTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"path": "/nonexistent"}))).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_project_info_counts() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "pub fn lib() {}").unwrap();
        std::fs::write(tmp.path().join("README.md"), "# Test").unwrap();

        let tool = ProjectInfoTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"path": tmp.path().to_str().unwrap()}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["total_files"], 3);
    }

    #[tokio::test]
    async fn test_project_info_extension_counts() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "").unwrap();
        std::fs::write(tmp.path().join("c.toml"), "").unwrap();

        let tool = ProjectInfoTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"path": tmp.path().to_str().unwrap()}))).await;
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        let exts = v["extensions"].as_array().unwrap();
        let rs_count = exts.iter().find(|e| e[0] == "rs").map(|e| e[1].as_u64().unwrap()).unwrap();
        assert_eq!(rs_count, 2);
    }

    #[tokio::test]
    async fn test_list_files_basic() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "fn b() {}").unwrap();
        std::fs::write(tmp.path().join("c.txt"), "text").unwrap();

        let tool = ListFilesTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"path": tmp.path().to_str().unwrap()}))).await;
        assert!(result.is_ok());
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["count"], 3);
    }

    #[tokio::test]
    async fn test_list_files_extension_filter() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "").unwrap();
        std::fs::write(tmp.path().join("c.txt"), "").unwrap();

        let tool = ListFilesTool;
        let result = tool.execute(&tool_args(&serde_json::json!({
            "path": tmp.path().to_str().unwrap(),
            "extension": "rs"
        }))).await;
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["count"], 2);
    }

    #[tokio::test]
    async fn test_list_files_skips_hidden() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("visible.txt"), "yes").unwrap();
        std::fs::create_dir(tmp.path().join(".hidden")).unwrap();
        std::fs::write(tmp.path().join(".hidden/secret.txt"), "no").unwrap();

        let tool = ListFilesTool;
        let result = tool.execute(&tool_args(&serde_json::json!({"path": tmp.path().to_str().unwrap()}))).await;
        let v: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(v["count"], 1);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_tool_info_names() {
        assert_eq!(AskHumanTool.info().name, "ask_human");
        assert_eq!(DelegateTaskTool.info().name, "delegate_task");
        assert_eq!(HealthCheckTool.info().name, "health_check");
        assert_eq!(CompactContextTool.info().name, "compact_context");
        assert_eq!(ProjectInfoTool.info().name, "project_info");
        assert_eq!(ListFilesTool.info().name, "list_files");
    }

    #[test]
    fn test_should_skip() {
        assert!(should_skip(".git"));
        assert!(should_skip("target"));
        assert!(should_skip("node_modules"));
        assert!(!should_skip("src"));
        assert!(!should_skip("main.rs"));
    }
}
