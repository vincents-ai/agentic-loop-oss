//! Control flow tools (done, failed, escalate, retry).
//!
//! These tools signal workflow state transitions.

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::Serialize;

/// Mark a step as completed.
#[derive(Clone)]
pub struct DoneTool {
    info: ToolInfo,
}

#[derive(Serialize)]
struct ControlResult {
    action: String,
    success: bool,
    message: String,
}

impl DoneTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "done".into(),
                description: "Mark the current step as completed successfully.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID"},
                        "step": {"type": "string", "description": "Step name"},
                        "result": {"type": "string", "description": "Result description"}
                    },
                    "required": ["task_id"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for DoneTool {
    agentic_loop_tools::impl_clone_box!(DoneTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: serde_json::Value = serde_json::from_slice(args)?;
        let task_id = args["task_id"].as_str().unwrap_or("unknown");
        let step = args["step"].as_str().unwrap_or("");
        let result = args["result"].as_str().unwrap_or("Step completed");

        let output = ControlResult {
            action: "done".into(),
            success: true,
            message: format!("Task {} step '{}' done: {}", task_id, step, result),
        };

        Ok(serde_json::to_vec(&output)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// Mark a step as failed.
#[derive(Clone)]
pub struct FailedTool {
    info: ToolInfo,
}

impl FailedTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "failed".into(),
                description: "Mark the current step as failed.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID"},
                        "reason": {"type": "string", "description": "Failure reason"},
                        "escalate": {"type": "boolean", "description": "Whether to escalate"}
                    },
                    "required": ["task_id", "reason"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FailedTool {
    agentic_loop_tools::impl_clone_box!(FailedTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: serde_json::Value = serde_json::from_slice(args)?;
        let task_id = args["task_id"].as_str().unwrap_or("unknown");
        let reason = args["reason"].as_str().unwrap_or("unknown error");
        let escalate = args["escalate"].as_bool().unwrap_or(false);

        let mut msg = format!("Task {} failed: {}", task_id, reason);
        if escalate {
            msg.push_str(" [ESCALATED]");
        }

        let output = ControlResult {
            action: "failed".into(),
            success: false,
            message: msg,
        };

        Ok(serde_json::to_vec(&output)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// Escalate to a human.
#[derive(Clone)]
pub struct EscalateTool {
    info: ToolInfo,
}

impl EscalateTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "escalate".into(),
                description: "Escalate to a human reviewer.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID"},
                        "type": {"type": "string", "description": "Escalation type"},
                        "justification": {"type": "string", "description": "Why escalation is needed"}
                    },
                    "required": ["task_id", "type", "justification"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for EscalateTool {
    agentic_loop_tools::impl_clone_box!(EscalateTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: serde_json::Value = serde_json::from_slice(args)?;
        let task_id = args["task_id"].as_str().unwrap_or("unknown");
        let escalation_type = args["type"].as_str().unwrap_or("general");
        let justification = args["justification"].as_str().unwrap_or("");

        let output = ControlResult {
            action: "escalate".into(),
            success: true,
            message: format!(
                "Task {} escalated [{}]: {}",
                task_id, escalation_type, justification
            ),
        };

        Ok(serde_json::to_vec(&output)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// Retry from a specific state.
#[derive(Clone)]
pub struct RetryTool {
    info: ToolInfo,
}

impl RetryTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "retry".into(),
                description: "Retry from a specific workflow state.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "task_id": {"type": "string", "description": "Task ID"},
                        "from": {"type": "string", "description": "State to retry from"}
                    },
                    "required": ["task_id", "from"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for RetryTool {
    agentic_loop_tools::impl_clone_box!(RetryTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: serde_json::Value = serde_json::from_slice(args)?;
        let task_id = args["task_id"].as_str().unwrap_or("unknown");
        let from = args["from"].as_str().unwrap_or("start");

        let output = ControlResult {
            action: "retry".into(),
            success: true,
            message: format!("Task {} retrying from state: {}", task_id, from),
        };

        Ok(serde_json::to_vec(&output)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_done_tool() {
        let tool = DoneTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "task_id": "t-1",
            "step": "implement",
            "result": "Feature complete"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["action"], "done");
        assert_eq!(output["success"], true);
        assert!(output["message"].as_str().unwrap().contains("Feature complete"));
    }

    #[tokio::test]
    async fn test_failed_tool() {
        let tool = FailedTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "task_id": "t-1",
            "reason": "Build error",
            "escalate": true
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["action"], "failed");
        assert_eq!(output["success"], false);
        assert!(output["message"].as_str().unwrap().contains("ESCALATED"));
    }

    #[tokio::test]
    async fn test_escalate_tool() {
        let tool = EscalateTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "task_id": "t-1",
            "type": "security",
            "justification": "Potential SQL injection"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["action"], "escalate");
        assert!(output["message"].as_str().unwrap().contains("security"));
    }

    #[tokio::test]
    async fn test_retry_tool() {
        let tool = RetryTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "task_id": "t-1",
            "from": "implement"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["action"], "retry");
        assert!(output["message"].as_str().unwrap().contains("implement"));
    }
}
