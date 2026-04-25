//! Shell execution tool (bash via tokio::process).

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Execute bash commands.
#[derive(Clone)]
pub struct BashTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

#[derive(Serialize)]
struct BashResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
    timed_out: bool,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "bash".into(),
                description: "Execute a bash command. Use for shell operations.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Bash command to execute"},
                        "timeout_secs": {"type": "integer", "description": "Timeout in seconds (default: 120)"}
                    },
                    "required": ["command"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    agentic_loop_tools::impl_clone_box!(BashTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: BashArgs = serde_json::from_slice(args)?;
        let timeout = Duration::from_secs(args.timeout_secs);

        let output = tokio::time::timeout(
            timeout,
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&args.command)
                .output(),
        )
        .await;

        match output {
            Ok(Ok(output)) => {
                let result = BashResult {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code().unwrap_or(-1),
                    timed_out: false,
                };
                Ok(serde_json::to_vec(&result)?)
            }
            Ok(Err(e)) => {
                let result = BashResult {
                    stdout: String::new(),
                    stderr: e.to_string(),
                    exit_code: -1,
                    timed_out: false,
                };
                Ok(serde_json::to_vec(&result)?)
            }
            Err(_) => {
                let result = BashResult {
                    stdout: String::new(),
                    stderr: format!("Command timed out after {}s", timeout.as_secs()),
                    exit_code: -1,
                    timed_out: true,
                };
                Ok(serde_json::to_vec(&result)?)
            }
        }
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bash_echo() {
        let tool = BashTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "command": "echo hello world"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["stdout"].as_str().unwrap().trim(), "hello world");
        assert_eq!(output["exit_code"], 0);
        assert_eq!(output["timed_out"], false);
    }

    #[tokio::test]
    async fn test_bash_failure() {
        let tool = BashTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "command": "exit 42"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["exit_code"], 42);
    }

    #[tokio::test]
    async fn test_bash_stderr() {
        let tool = BashTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "command": "echo error >&2"
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["stderr"].as_str().unwrap().trim(), "error");
    }

    #[tokio::test]
    async fn test_bash_timeout() {
        let tool = BashTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "command": "sleep 10",
            "timeout_secs": 1
        }))
        .unwrap();
        let result = tool.execute(&args).await.unwrap();
        let output: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(output["timed_out"], true);
    }
}
