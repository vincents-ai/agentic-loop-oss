//! Session data types (pi-compatible JSONL + custom workflow sessions).

use serde::{Deserialize, Serialize};

/// Result of running a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub success: bool,
    pub summary: String,
    pub steps: Vec<String>,
    pub tool_calls: Vec<String>,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

impl Default for TaskResult {
    fn default() -> Self {
        Self {
            success: false,
            summary: String::new(),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 0,
            cost_usd: 0.0,
            duration_ms: 0,
        }
    }
}

impl TaskResult {
    pub fn success(summary: impl Into<String>) -> Self {
        Self {
            success: true,
            summary: summary.into(),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 0,
            cost_usd: 0.0,
            duration_ms: 0,
        }
    }

    pub fn failure(summary: impl Into<String>) -> Self {
        Self {
            success: false,
            summary: summary.into(),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 0,
            cost_usd: 0.0,
            duration_ms: 0,
        }
    }
}

/// Workflow session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Running,
    Completed,
    Failed,
    Aborted,
}

/// Step status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}