//! # agentic-loop-eval
//!
//! Trajectory evaluation and self-improvement.
//! Metrics adapted from engram-evo: step_efficiency, tool_correctness,
//! plan_adherence, task_completion.

use agentic_loop_types::eval::{
    EvalReport, EvalScores, Trajectory, TurnScore,
};
use chrono::Utc;

pub mod improvement;

pub use improvement::{
    TrajectoryEvaluator, MemoryOptimizer, ImprovementLoop,
    DefaultImprovementLoop, HeuristicEvaluatorAdapter, HeuristicMemoryOptimizer,
    ImprovementConfig, ImprovementResult,
};

/// Evaluates trajectories using heuristic metrics.
pub struct HeuristicEvaluator;

impl HeuristicEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a trajectory with heuristic (no LLM) metrics.
    pub fn evaluate_heuristic(&self, trajectory: &Trajectory) -> EvalReport {
        let step_efficiency = Self::step_efficiency(trajectory);
        let tool_correctness = Self::tool_correctness(trajectory);
        let plan_adherence = Self::plan_adherence_heuristic(trajectory);
        let task_completion = Self::task_completion(trajectory);
        let composite = Self::composite(step_efficiency, tool_correctness, plan_adherence, task_completion);

        let scores = EvalScores {
            step_efficiency,
            tool_correctness,
            plan_adherence,
            task_completion,
            composite,
        };

        let turn_scores: Vec<TurnScore> = trajectory
            .turns
            .iter()
            .enumerate()
            .map(|(i, turn)| {
                let tool_err = turn
                    .tool_results
                    .iter()
                    .filter(|r| r.is_error)
                    .count() as f64;
                let tool_total = turn.tool_results.len().max(1) as f64;
                TurnScore {
                    turn_index: i,
                    step_efficiency: 1.0 - (tool_err / tool_total).min(1.0),
                    tool_correctness: if tool_err > 0.0 { 0.0 } else { 1.0 },
                    is_critical_failure: turn.stopped_reason
                        == agentic_loop_types::eval::StopReason::Error,
                }
            })
            .collect();

        let critical_failure_turn = turn_scores
            .iter()
            .find(|t| t.is_critical_failure)
            .map(|t| t.turn_index);

        let suggestions = Self::generate_suggestions(trajectory, &scores, critical_failure_turn);

        EvalReport {
            trajectory_id: trajectory.session_id.clone(),
            session_id: trajectory.session_id.clone(),
            scores,
            turn_scores,
            critical_failure_turn,
            improvement_suggestions: suggestions,
            evaluated_at: Utc::now(),
        }
    }

    /// Step efficiency: ratio of productive tool calls to total tool calls.
    /// A "productive" call is one without errors. Higher is better.
    fn step_efficiency(trajectory: &Trajectory) -> f64 {
        if trajectory.turns.is_empty() {
            return 1.0;
        }

        let total_calls: usize = trajectory
            .turns
            .iter()
            .map(|t| t.tool_calls.len())
            .sum();

        let error_calls: usize = trajectory
            .turns
            .iter()
            .flat_map(|t| t.tool_results.iter())
            .filter(|r| r.is_error)
            .count();

        if total_calls == 0 {
            return 1.0;
        }

        1.0 - (error_calls as f64 / total_calls as f64).min(1.0)
    }

    /// Tool correctness: percentage of tool calls that completed without errors.
    fn tool_correctness(trajectory: &Trajectory) -> f64 {
        let total: usize = trajectory
            .turns
            .iter()
            .flat_map(|t| t.tool_results.iter())
            .count();

        let errors: usize = trajectory
            .turns
            .iter()
            .flat_map(|t| t.tool_results.iter())
            .filter(|r| r.is_error)
            .count();

        if total == 0 {
            return 1.0;
        }

        (total - errors) as f64 / total as f64
    }

    /// Plan adherence heuristic: correlation between thinking and tool usage.
    /// If the agent thinks before acting, it gets a higher score.
    fn plan_adherence_heuristic(trajectory: &Trajectory) -> f64 {
        if trajectory.turns.is_empty() {
            return 1.0;
        }

        let has_thinking = trajectory
            .turns
            .iter()
            .any(|t| t.assistant_thinking.is_some());

        let has_repeated_failures = trajectory
            .turns
            .iter()
            .any(|t| {
                t.tool_results.iter().filter(|r| r.is_error).count() > 2
            });

        let mut score: f64 = 0.7; // base score

        if has_thinking {
            score += 0.2;
        }

        if has_repeated_failures {
            score -= 0.3;
        }

        // Penalize very long trajectories (> 20 turns)
        if trajectory.turns.len() > 20 {
            score -= 0.1;
        }

        score.clamp(0.0, 1.0)
    }

    /// Task completion: did the trajectory end with a stop reason?
    fn task_completion(trajectory: &Trajectory) -> f64 {
        if trajectory.turns.is_empty() {
            return 0.0;
        }

        match trajectory.turns.last().map(|t| t.stopped_reason) {
            Some(agentic_loop_types::eval::StopReason::Stop) => 1.0,
            Some(agentic_loop_types::eval::StopReason::ToolUse) => 0.5, // mid-task
            Some(agentic_loop_types::eval::StopReason::Error) => 0.1,
            Some(agentic_loop_types::eval::StopReason::Length) => 0.3,
            _ => 0.5,
        }
    }

    /// Weighted composite score.
    fn composite(efficiency: f64, correctness: f64, adherence: f64, completion: f64) -> f64 {
        let w_e = 0.25;
        let w_c = 0.30;
        let w_a = 0.20;
        let w_t = 0.25;

        (w_e * efficiency + w_c * correctness + w_a * adherence + w_t * completion)
            .clamp(0.0, 1.0)
    }

    /// Generate improvement suggestions based on scores.
    fn generate_suggestions(
        trajectory: &Trajectory,
        scores: &EvalScores,
        critical_turn: Option<usize>,
    ) -> Vec<String> {
        let mut suggestions = Vec::new();

        if scores.step_efficiency < 0.8 {
            suggestions.push(
                "Step efficiency is low. Consider planning tool calls more carefully to avoid retries.".to_string()
            );
        }

        if scores.tool_correctness < 0.9 {
            let error_tools: Vec<String> = trajectory
                .turns
                .iter()
                .flat_map(|t| t.tool_results.iter())
                .filter(|r| r.is_error)
                .map(|r| r.tool_name.clone())
                .collect();
            suggestions.push(format!(
                "Tool errors detected in: {}. Check argument schemas.",
                error_tools.join(", ")
            ));
        }

        if scores.plan_adherence < 0.6 {
            suggestions.push(
                "Plan adherence is low. Add thinking steps before tool calls.".to_string()
            );
        }

        if scores.task_completion < 0.8 {
            suggestions.push(
                "Task did not complete. Review the last step for blocking issues.".to_string()
            );
        }

        if let Some(turn_idx) = critical_turn {
            suggestions.push(format!(
                "Critical failure at turn {}. Investigate the error and add recovery logic.",
                turn_idx
            ));
        }

        if suggestions.is_empty() {
            suggestions.push("Trajectory looks good. No improvements needed.".to_string());
        }

        suggestions
    }
}

impl Default for HeuristicEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentic_loop_types::eval::{StopReason, ToolCall, ToolResultEntry, Turn};

    fn make_trajectory(turns: Vec<Turn>) -> Trajectory {
        Trajectory {
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            model: "test-model".into(),
            provider: "test".into(),
            turns,
            task_description: Some("test task".into()),
            created_at: Utc::now(),
            total_tokens: 100,
        }
    }

    fn make_turn(
        tool_calls: Vec<(&str, bool)>,
        thinking: Option<&str>,
        stop: StopReason,
    ) -> Turn {
        Turn {
            index: 0,
            user_message: Some("do something".into()),
            assistant_thinking: thinking.map(|s| s.to_string()),
            assistant_text: Some("result".into()),
            tool_calls: tool_calls
                .iter()
                .enumerate()
                .map(|(i, (name, _))| ToolCall {
                    id: format!("call-{}", i),
                    name: name.to_string(),
                    arguments: serde_json::json!({}),
                })
                .collect(),
            tool_results: tool_calls
                .iter()
                .enumerate()
                .map(|(i, (name, is_err))| ToolResultEntry {
                    tool_call_id: format!("call-{}", i),
                    tool_name: name.to_string(),
                    content: if *is_err { "error".into() } else { "ok".into() },
                    is_error: *is_err,
                })
                .collect(),
            timestamp: Utc::now(),
            stopped_reason: stop,
        }
    }

    #[test]
    fn test_perfect_trajectory() {
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false), ("write", false)], Some("planning..."), StopReason::Stop),
        ]);

        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);

        assert!(report.scores.step_efficiency >= 0.9);
        assert!(report.scores.tool_correctness >= 0.9);
        assert!(report.scores.task_completion >= 0.9);
        assert!(report.scores.composite >= 0.8);
    }

    #[test]
    fn test_error_trajectory() {
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", true), ("read", true), ("write", true)], None, StopReason::Error),
        ]);

        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);

        assert!(report.scores.tool_correctness < 0.5);
        assert!(report.scores.task_completion < 0.5);
        assert!(report.scores.composite < 0.5);
        assert!(report.critical_failure_turn.is_some());
        assert!(report.improvement_suggestions.len() >= 2);
    }

    #[test]
    fn test_empty_trajectory() {
        let trajectory = make_trajectory(vec![]);
        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);

        assert_eq!(report.scores.step_efficiency, 1.0);
        assert_eq!(report.scores.task_completion, 0.0);
    }

    #[test]
    fn test_partial_success() {
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false), ("write", false)], Some("thinking"), StopReason::ToolUse),
        ]);

        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);

        assert!(report.scores.tool_correctness >= 0.9);
        assert!(report.scores.plan_adherence >= 0.8);
        assert_eq!(report.scores.task_completion, 0.5); // ToolUse = mid-task
    }

    #[test]
    fn test_long_trajectory_penalized() {
        let turns: Vec<Turn> = (0..25)
            .map(|_| make_turn(vec![("read", false)], None, StopReason::Stop))
            .collect();
        let trajectory = make_trajectory(turns);

        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);
        assert!(report.scores.plan_adherence < 0.7);
    }

    #[test]
    fn test_suggestions_generated() {
        let trajectory = make_trajectory(vec![
            make_turn(vec![("tool", true)], None, StopReason::Error),
        ]);

        let report = HeuristicEvaluator::new().evaluate_heuristic(&trajectory);
        assert!(!report.improvement_suggestions.is_empty());
        assert!(report.improvement_suggestions.iter().any(|s| s.contains("error") || s.contains("Error")));
    }
}

/// Eval error types.
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("Trajectory not found: {0}")]
    TrajectoryNotFound(String),

    #[error("No turns to evaluate")]
    EmptyTrajectory,

    #[error("Invalid eval score: {0}")]
    InvalidScore(f64),

    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Eval error: {0}")]
    Other(String),
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_eval_error_not_found() {
        let e = EvalError::TrajectoryNotFound("s1".to_string());
        assert!(e.to_string().contains("s1"));
    }

    #[test]
    fn test_eval_error_empty() {
        let e = EvalError::EmptyTrajectory;
        assert!(e.to_string().contains("No turns"));
    }

    #[test]
    fn test_eval_error_invalid_score() {
        let e = EvalError::InvalidScore(1.5);
        assert!(e.to_string().contains("1.5"));
    }
}
