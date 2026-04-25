//! # agentic-loop-tools-core
//!
//! Built-in tool implementations for agentic-loop:
//! - File operations via gix (read, write, edit, ls)
//! - Shell execution via tokio::process (bash)
//! - Engram entity tools (via Storage trait)
//! - Control tools (done, failed, escalate, retry)

mod file_ops;
mod shell;
mod control;
mod search;
pub mod git_ops;
pub mod web;
pub mod engram_tools;
pub mod agent_tools;

use agentic_loop_tools::{Tool, ToolProvider};
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use std::collections::HashMap;

pub use file_ops::{FileReadTool, FileWriteTool, FileEditTool, FileLsTool};
pub use shell::BashTool;
pub use control::{DoneTool, FailedTool, EscalateTool, RetryTool};
pub use search::{FileGrepTool, FileGlobTool};
pub use git_ops::{GitStatusTool, GitLogTool, GitDiffTool, GitShowTool};
pub use web::WebFetchTool;
pub use engram_tools::{
    EngramStorage, EngramStoreTool, EngramQueryTool,
    EngramSearchTool, EngramGetTool, EngramRelationshipTool,
};

/// Registry of built-in tools.
pub struct CoreToolProvider {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl CoreToolProvider {
    /// Create a provider with all built-in tools.
    pub fn new() -> Self {
        let mut tools: HashMap<String, Box<dyn Tool>> = HashMap::new();

        // File operations
        tools.insert("file_read".into(), Box::new(FileReadTool::new()));
        tools.insert("file_write".into(), Box::new(FileWriteTool::new()));
        tools.insert("file_edit".into(), Box::new(FileEditTool::new()));
        tools.insert("file_ls".into(), Box::new(FileLsTool::new()));

        // Shell
        tools.insert("bash".into(), Box::new(BashTool::new()));

        // Control
        tools.insert("done".into(), Box::new(DoneTool::new()));
        tools.insert("failed".into(), Box::new(FailedTool::new()));
        tools.insert("escalate".into(), Box::new(EscalateTool::new()));
        tools.insert("retry".into(), Box::new(RetryTool::new()));

        // Search
        tools.insert("file_grep".into(), Box::new(FileGrepTool::new()));
        tools.insert("file_glob".into(), Box::new(FileGlobTool::new()));

        // Git
        tools.insert("git_status".into(), Box::new(GitStatusTool::new()));
        tools.insert("git_log".into(), Box::new(GitLogTool::new()));
        tools.insert("git_diff".into(), Box::new(GitDiffTool::new()));
        tools.insert("git_show".into(), Box::new(GitShowTool::new()));

        // Web
        tools.insert("web_fetch".into(), Box::new(WebFetchTool::new()));

        Self { tools }
    }

    /// Register a custom tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.info().name.clone();
        self.tools.insert(name, tool);
    }
}

impl Default for CoreToolProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for CoreToolProvider {
    async fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools.values().map(|t| t.info()).collect()
    }

    async fn get_tool(&self, name: &str) -> Option<Box<dyn Tool>> {
        // We can't clone Box<dyn Tool>, so we return a new instance
        // This is a limitation of the current trait design
        match name {
            "file_read" => Some(Box::new(FileReadTool::new())),
            "file_write" => Some(Box::new(FileWriteTool::new())),
            "file_edit" => Some(Box::new(FileEditTool::new())),
            "file_ls" => Some(Box::new(FileLsTool::new())),
            "bash" => Some(Box::new(BashTool::new())),
            "done" => Some(Box::new(DoneTool::new())),
            "failed" => Some(Box::new(FailedTool::new())),
            "escalate" => Some(Box::new(EscalateTool::new())),
            "retry" => Some(Box::new(RetryTool::new())),
            "file_grep" => Some(Box::new(FileGrepTool::new())),
            "file_glob" => Some(Box::new(FileGlobTool::new())),
            "git_status" => Some(Box::new(GitStatusTool::new())),
            "git_log" => Some(Box::new(GitLogTool::new())),
            "git_diff" => Some(Box::new(GitDiffTool::new())),
            "git_show" => Some(Box::new(GitShowTool::new())),
            "web_fetch" => Some(Box::new(WebFetchTool::new())),
            _ => None,
        }
    }
}
