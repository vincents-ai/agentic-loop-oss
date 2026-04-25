//! Evaluation data types (adapted from engram-evo).

use serde::{Deserialize, Serialize};

/// A captured agent trajectory (sequence of turns).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub provider: String,
    pub turns: Vec<Turn>,
    pub task_description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub total_tokens: u64,
}

/// A single turn in a trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub index: usize,
    pub user_message: Option<String>,
    pub assistant_thinking: Option<String>,
    pub assistant_text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResultEntry>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub stopped_reason: StopReason,
}

/// Tool call within a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool result within a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEntry {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    Stop,
    Length,
    ToolUse,
    Error,
    Aborted,
    Unknown,
}

/// Evaluation report for a trajectory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub trajectory_id: String,
    pub session_id: String,
    pub scores: EvalScores,
    pub turn_scores: Vec<TurnScore>,
    pub critical_failure_turn: Option<usize>,
    pub improvement_suggestions: Vec<String>,
    pub evaluated_at: chrono::DateTime<chrono::Utc>,
}

/// Aggregate evaluation scores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalScores {
    pub step_efficiency: f64,
    pub tool_correctness: f64,
    pub plan_adherence: f64,
    pub task_completion: f64,
    pub composite: f64,
}

/// Per-turn evaluation score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnScore {
    pub turn_index: usize,
    pub step_efficiency: f64,
    pub tool_correctness: f64,
    pub is_critical_failure: bool,
}

/// A patch to memory/entities based on failure analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPatch {
    pub target_failure_turn: usize,
    pub rationale: String,
    pub entities: Vec<PatchEntity>,
}

/// An entity to create/modify as part of a memory patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchEntity {
    pub entity_type: String,
    pub title: String,
    pub content: String,
    pub knowledge_type: Option<String>,
    pub tags: Vec<String>,
    pub confidence: Option<f64>,
}
