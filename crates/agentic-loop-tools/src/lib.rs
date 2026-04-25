//! # agentic-loop-tools
//!
//! Tool trait definitions (WASM-safe, serialized args/results).

use agentic_loop_types::tool::ToolInfo;
use anyhow::Result;
use async_trait::async_trait;

/// A single tool that can be executed by an agent.
/// All args/results are serialized as `Vec<u8>` (serde_json) for WASM compatibility.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Execute the tool with serialized arguments, return serialized result.
    async fn execute(&self, args: &[u8]) -> Result<Vec<u8>>;

    /// Get tool metadata (name, description, parameters schema).
    fn info(&self) -> ToolInfo;

    /// Check if this tool is available in the current context.
    async fn is_available(&self) -> bool {
        true
    }

    /// Clone this tool into a boxed trait object.
    /// Required for tool registries that need to hand out copies.
    fn clone_box(&self) -> Box<dyn Tool + 'static>;
}

/// Macro to implement `clone_box` for a Tool that implements Clone.
/// Usage: `impl_clone_box!(MyTool);`
#[macro_export]
macro_rules! impl_clone_box {
    ($tool_type:ty) => {
        fn clone_box(&self) -> Box<dyn $crate::Tool + 'static> {
            Box::new(Clone::clone(self))
        }
    };
}

/// Provider of tools (e.g., built-in tools, WASM plugins).
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// List all tools provided by this provider.
    async fn list_tools(&self) -> Vec<ToolInfo>;

    /// Get a specific tool by name.
    async fn get_tool(&self, name: &str) -> Option<Box<dyn Tool>>;
}

/// Registry of all available tools.
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// Register a tool.
    async fn register(&self, tool: Box<dyn Tool>) -> Result<()>;

    /// Register all tools from a provider.
    async fn register_provider(&self, provider: Box<dyn ToolProvider>) -> Result<()>;

    /// Get a tool by name.
    async fn get(&self, name: &str) -> Option<Box<dyn Tool>>;

    /// List all registered tools.
    async fn list_all(&self) -> Vec<ToolInfo>;

    /// Check if a tool is registered.
    async fn has(&self, name: &str) -> bool;
}

/// Tool error types.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool unavailable: {0}")]
    Unavailable(String),

    #[error("Invalid arguments for tool {tool}: {reason}")]
    InvalidArguments { tool: String, reason: String },

    #[error("Tool execution failed: {tool}: {message}")]
    ExecutionFailed { tool: String, message: String },

    #[error("Tool timed out: {tool} after {timeout_ms}ms")]
    Timeout { tool: String, timeout_ms: u64 },

    #[error("Sandbox denied: {reason}")]
    SandboxDenied { reason: String },

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] serde_json::Error),

    #[error("Tool error: {0}")]
    Other(String),
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_tool_error_display() {
        let e = ToolError::NotFound("bash".to_string());
        assert!(e.to_string().contains("bash"));
    }

    #[test]
    fn test_tool_error_invalid_args() {
        let e = ToolError::InvalidArguments { tool: "file_read".to_string(), reason: "missing path".to_string() };
        assert!(e.to_string().contains("file_read"));
        assert!(e.to_string().contains("missing path"));
    }

    #[test]
    fn test_tool_error_execution_failed() {
        let e = ToolError::ExecutionFailed { tool: "bash".to_string(), message: "exit code 1".to_string() };
        assert!(e.to_string().contains("exit code 1"));
    }

    #[test]
    fn test_tool_error_sandbox_denied() {
        let e = ToolError::SandboxDenied { reason: "path outside workspace".to_string() };
        assert!(e.to_string().contains("path outside workspace"));
    }
}
