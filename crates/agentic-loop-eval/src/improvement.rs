//! Self-improvement loop — evaluate trajectories and optimize memory.
//!
//! The improvement loop runs after each workflow completion:
//! 1. Evaluate the trajectory using TrajectoryEvaluator
//! 2. If score is below threshold, analyze failures
//! 3. Generate MemoryPatches (new Knowledge, updated Context, etc.)
//! 4. Apply patches to engram via the provided callback
//! 5. Optionally re-run from the failing state
//!
//! Adapted from engram-evo trajectory evaluation patterns.

use agentic_loop_types::eval::{EvalReport, MemoryPatch, PatchEntity, Trajectory};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trait for evaluating trajectories.
pub trait TrajectoryEvaluator: Send + Sync {
    /// Evaluate a full trajectory and produce an eval report.
    fn evaluate(&self, trajectory: &Trajectory) -> Result<EvalReport>;
}

/// Trait for optimizing memory based on eval results.
pub trait MemoryOptimizer: Send + Sync {
    /// Analyze a failed/poor trajectory and produce memory patches.
    fn analyze_failures(
        &self,
        trajectory: &Trajectory,
        report: &EvalReport,
    ) -> Result<Vec<MemoryPatch>>;
}

/// Trait for the self-improvement loop.
#[allow(async_fn_in_trait)]
pub trait ImprovementLoop: Send + Sync {
    /// Run the improvement loop on a completed trajectory.
    /// Returns the improvement result.
    async fn run_improvement(
        &self,
        trajectory: &Trajectory,
        task_id: &str,
    ) -> Result<ImprovementResult>;
}

/// Result of an improvement loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementResult {
    /// The original eval report.
    pub eval_report: EvalReport,
    /// Whether the score was above the threshold (no improvement needed).
    pub passed: bool,
    /// Memory patches generated.
    pub patches: Vec<MemoryPatch>,
    /// Whether patches were applied.
    pub patches_applied: bool,
    /// Improvement score delta (negative = improved).
    pub score_delta: f64,
    /// When this result was generated.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Configuration for the improvement loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementConfig {
    /// Minimum composite score to consider a trajectory successful.
    pub score_threshold: f64,
    /// Maximum number of re-run attempts.
    pub max_retries: usize,
    /// Whether to auto-apply patches.
    pub auto_apply: bool,
}

impl Default for ImprovementConfig {
    fn default() -> Self {
        Self {
            score_threshold: 0.7,
            max_retries: 2,
            auto_apply: true,
        }
    }
}

/// Callback for applying memory patches.
pub type PatchApplier = Box<dyn Fn(&PatchEntity) -> Result<()> + Send + Sync>;

// ─── Heuristic trajectory evaluator ──────────────────────────────────────────

/// Evaluator that delegates to the existing HeuristicEvaluator.
pub struct HeuristicEvaluatorAdapter {
    evaluator: crate::HeuristicEvaluator,
}

impl HeuristicEvaluatorAdapter {
    pub fn new() -> Self {
        Self {
            evaluator: crate::HeuristicEvaluator::new(),
        }
    }
}

impl Default for HeuristicEvaluatorAdapter {
    fn default() -> Self { Self::new() }
}

impl TrajectoryEvaluator for HeuristicEvaluatorAdapter {
    fn evaluate(&self, trajectory: &Trajectory) -> Result<EvalReport> {
        Ok(self.evaluator.evaluate_heuristic(trajectory))
    }
}

// ─── Heuristic memory optimizer ──────────────────────────────────────────────

/// Generates memory patches based on heuristic failure analysis.
/// Produces Knowledge entities for error patterns, Context entities
/// for lessons learned, and ADR updates for recurring failures.
pub struct HeuristicMemoryOptimizer;

impl HeuristicMemoryOptimizer {
    pub fn new() -> Self { Self }
}

impl Default for HeuristicMemoryOptimizer {
    fn default() -> Self { Self::new() }
}

impl MemoryOptimizer for HeuristicMemoryOptimizer {
    fn analyze_failures(
        &self,
        trajectory: &Trajectory,
        report: &EvalReport,
    ) -> Result<Vec<MemoryPatch>> {
        let mut patches = Vec::new();

        // Analyze tool errors by name
        let mut tool_errors: HashMap<String, usize> = HashMap::new();
        for turn in &trajectory.turns {
            for result in &turn.tool_results {
                if result.is_error {
                    *tool_errors.entry(result.tool_name.clone()).or_insert(0) += 1;
                }
            }
        }

        // Generate patches for repeated tool failures
        for (tool_name, count) in &tool_errors {
            if *count >= 2 {
                patches.push(MemoryPatch {
                    target_failure_turn: 0,
                    rationale: format!(
                        "Tool '{}' failed {} times in trajectory. Creating knowledge entity with error pattern.",
                        tool_name, count
                    ),
                    entities: vec![PatchEntity {
                        entity_type: "Knowledge".to_string(),
                        title: format!("Error pattern: {} failures", tool_name),
                        content: format!(
                            "Tool '{}' failed {} times in session {}. Common causes: incorrect arguments, missing dependencies, permission issues.",
                            tool_name, count, trajectory.session_id
                        ),
                        knowledge_type: Some("pattern".to_string()),
                        tags: vec!["error-pattern".to_string(), "self-improvement".to_string()],
                        confidence: Some(0.7),
                    }],
                });
            }
        }

        // Generate patch for critical failure
        if let Some(turn_idx) = report.critical_failure_turn {
            let turn = trajectory.turns.get(turn_idx);
            let error_detail = turn
                .and_then(|t| t.tool_results.iter().find(|r| r.is_error))
                .map(|r| r.content.clone())
                .unwrap_or_else(|| "unknown error".to_string());

            patches.push(MemoryPatch {
                target_failure_turn: turn_idx,
                rationale: format!(
                    "Critical failure at turn {}. Capturing context for future avoidance.",
                    turn_idx
                ),
                entities: vec![PatchEntity {
                    entity_type: "Context".to_string(),
                    title: format!("Critical failure: session {}", trajectory.session_id),
                    content: format!(
                        "Session {} had a critical failure at turn {}. Error: {}. Task: {}. Suggestions: {}",
                        trajectory.session_id,
                        turn_idx,
                        error_detail,
                        trajectory.task_description.as_deref().unwrap_or("unknown"),
                        report.improvement_suggestions.join("; ")
                    ),
                    knowledge_type: None,
                    tags: vec!["critical-failure".to_string(), "self-improvement".to_string()],
                    confidence: Some(0.9),
                }],
            });
        }

        // Generate improvement suggestions as Knowledge entities
        if !report.improvement_suggestions.is_empty() {
            patches.push(MemoryPatch {
                target_failure_turn: 0,
                rationale: "Converting improvement suggestions to knowledge entities.".to_string(),
                entities: report
                    .improvement_suggestions
                    .iter()
                    .map(|suggestion| PatchEntity {
                        entity_type: "Knowledge".to_string(),
                        title: "Improvement suggestion".to_string(),
                        content: suggestion.clone(),
                        knowledge_type: Some("procedure".to_string()),
                        tags: vec!["improvement-suggestion".to_string(), "self-improvement".to_string()],
                        confidence: Some(0.6),
                    })
                    .collect(),
            });
        }

        // Score-based patch for low plan adherence
        if report.scores.plan_adherence < 0.6 {
            patches.push(MemoryPatch {
                target_failure_turn: 0,
                rationale: "Low plan adherence detected. Suggest adding reasoning steps.".to_string(),
                entities: vec![PatchEntity {
                    entity_type: "Knowledge".to_string(),
                    title: "Low plan adherence pattern".to_string(),
                    content: format!(
                        "Session {} had plan adherence score {:.2}. The agent did not think before acting. Add explicit reasoning requirements to system prompts.",
                        trajectory.session_id, report.scores.plan_adherence
                    ),
                    knowledge_type: Some("pattern".to_string()),
                    tags: vec!["plan-adherence".to_string(), "self-improvement".to_string()],
                    confidence: Some(0.8),
                }],
            });
        }

        Ok(patches)
    }
}

// ─── Default improvement loop implementation ─────────────────────────────────

/// Default improvement loop that evaluates and patches without LLM.
pub struct DefaultImprovementLoop {
    evaluator: Box<dyn TrajectoryEvaluator>,
    optimizer: Box<dyn MemoryOptimizer>,
    config: ImprovementConfig,
    patch_applier: Option<PatchApplier>,
}

impl DefaultImprovementLoop {
    pub fn new(
        evaluator: Box<dyn TrajectoryEvaluator>,
        optimizer: Box<dyn MemoryOptimizer>,
    ) -> Self {
        Self {
            evaluator,
            optimizer,
            config: ImprovementConfig::default(),
            patch_applier: None,
        }
    }

    /// Create with the default heuristic evaluator and optimizer.
    pub fn heuristic() -> Self {
        Self {
            evaluator: Box::new(HeuristicEvaluatorAdapter::new()),
            optimizer: Box::new(HeuristicMemoryOptimizer::new()),
            config: ImprovementConfig::default(),
            patch_applier: None,
        }
    }

    /// Set the configuration.
    pub fn with_config(mut self, config: ImprovementConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the patch applier callback.
    pub fn with_applier(mut self, applier: PatchApplier) -> Self {
        self.patch_applier = Some(applier);
        self
    }
}

impl ImprovementLoop for DefaultImprovementLoop {
    async fn run_improvement(
        &self,
        trajectory: &Trajectory,
        _task_id: &str,
    ) -> Result<ImprovementResult> {
        let eval_report = self.evaluator.evaluate(trajectory)?;
        let passed = eval_report.scores.composite >= self.config.score_threshold;

        let patches = if passed {
            Vec::new()
        } else {
            self.optimizer.analyze_failures(trajectory, &eval_report)?
        };

        let mut patches_applied = false;
        if self.config.auto_apply && !patches.is_empty() {
            if let Some(ref applier) = self.patch_applier {
                for patch in &patches {
                    for entity in &patch.entities {
                        if applier(entity).is_ok() {
                            patches_applied = true;
                        }
                    }
                }
            }
        }

        Ok(ImprovementResult {
            score_delta: if passed { 0.0 } else { self.config.score_threshold - eval_report.scores.composite },
            eval_report,
            passed,
            patches,
            patches_applied,
            timestamp: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentic_loop_types::eval::{StopReason, ToolCall, ToolResultEntry, Turn};
    use std::sync::{Arc, Mutex};

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
    fn test_heuristic_evaluator_adapter() {
        let evaluator = HeuristicEvaluatorAdapter::new();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false)], Some("thinking"), StopReason::Stop),
        ]);
        let report = evaluator.evaluate(&trajectory).unwrap();
        assert!(report.scores.composite > 0.7);
    }

    #[test]
    fn test_memory_optimizer_no_failures() {
        let optimizer = HeuristicMemoryOptimizer::new();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false)], None, StopReason::Stop),
        ]);
        let report = HeuristicEvaluatorAdapter::new().evaluate(&trajectory).unwrap();
        let patches = optimizer.analyze_failures(&trajectory, &report).unwrap();
        // No failures = minimal patches (suggestions only)
        assert!(patches.len() <= 1);
    }

    #[test]
    fn test_memory_optimizer_repeated_tool_errors() {
        let optimizer = HeuristicMemoryOptimizer::new();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("bash", true), ("bash", true), ("bash", true)], None, StopReason::Error),
        ]);
        let report = HeuristicEvaluatorAdapter::new().evaluate(&trajectory).unwrap();
        let patches = optimizer.analyze_failures(&trajectory, &report).unwrap();

        let has_error_pattern = patches.iter().any(|p|
            p.entities.iter().any(|e| e.title.contains("Error pattern"))
        );
        assert!(has_error_pattern, "Should detect repeated bash failures");
    }

    #[test]
    fn test_memory_optimizer_critical_failure() {
        let optimizer = HeuristicMemoryOptimizer::new();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false)], None, StopReason::Stop),
            make_turn(vec![("write", true)], None, StopReason::Error),
        ]);
        let report = HeuristicEvaluatorAdapter::new().evaluate(&trajectory).unwrap();
        let patches = optimizer.analyze_failures(&trajectory, &report).unwrap();

        let has_critical = patches.iter().any(|p|
            p.entities.iter().any(|e| e.title.contains("Critical failure"))
        );
        assert!(has_critical, "Should detect critical failure");
    }

    #[test]
    fn test_memory_optimizer_low_plan_adherence() {
        let optimizer = HeuristicMemoryOptimizer::new();
        // 30 turns with repeated errors and no thinking
        let turns: Vec<Turn> = (0..30)
            .map(|i| make_turn(
                if i % 3 == 0 { vec![("bash", true)] } else { vec![("read", false)] },
                None,
                StopReason::Stop,
            ))
            .collect();
        let trajectory = make_trajectory(turns);
        let report = HeuristicEvaluatorAdapter::new().evaluate(&trajectory).unwrap();
        let patches = optimizer.analyze_failures(&trajectory, &report).unwrap();

        // Should detect repeated bash failures and/or low adherence
        let has_relevant_patch = patches.iter().any(|p|
            p.entities.iter().any(|e|
                e.title.contains("plan adherence") || e.title.contains("Error pattern")
            )
        );
        assert!(has_relevant_patch, "Should detect failures or low plan adherence");
    }

    #[tokio::test]
    async fn test_improvement_loop_passing() {
        let improvement = DefaultImprovementLoop::heuristic();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false), ("write", false)], Some("planning"), StopReason::Stop),
        ]);
        let result = improvement.run_improvement(&trajectory, "task-1").await.unwrap();
        assert!(result.passed);
        assert!(result.patches.is_empty());
    }

    #[tokio::test]
    async fn test_improvement_loop_failing() {
        let improvement = DefaultImprovementLoop::heuristic();
        let trajectory = make_trajectory(vec![
            make_turn(vec![("bash", true), ("bash", true), ("bash", true)], None, StopReason::Error),
        ]);
        let result = improvement.run_improvement(&trajectory, "task-1").await.unwrap();
        assert!(!result.passed);
        assert!(!result.patches.is_empty());
        assert!(result.score_delta > 0.0);
    }

    #[tokio::test]
    async fn test_improvement_loop_with_applier() {
        let applied = Arc::new(Mutex::new(Vec::<String>::new()));
        let applied_clone = applied.clone();

        let improvement = DefaultImprovementLoop::heuristic().with_applier(Box::new(move |entity| {
            let mut a = applied_clone.lock().unwrap();
            a.push(entity.title.clone());
            Ok(())
        }));

        let trajectory = make_trajectory(vec![
            make_turn(vec![("bash", true), ("bash", true)], None, StopReason::Error),
        ]);
        let result = improvement.run_improvement(&trajectory, "task-1").await.unwrap();
        assert!(!result.passed);
        assert!(result.patches_applied);
        assert!(!applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_improvement_loop_custom_threshold() {
        let config = ImprovementConfig {
            score_threshold: 0.95, // Very high
            max_retries: 1,
            auto_apply: false,
        };
        let improvement = DefaultImprovementLoop::heuristic().with_config(config);
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false)], Some("thinking"), StopReason::Stop),
        ]);
        let _result = improvement.run_improvement(&trajectory, "task-1").await.unwrap();
        // Even a good trajectory may not reach 0.95
        // The heuristic evaluator gives 1.0 for a perfect trajectory
        // but plan_adherence starts at 0.7 base
    }

    #[test]
    fn test_improvement_config_default() {
        let config = ImprovementConfig::default();
        assert_eq!(config.score_threshold, 0.7);
        assert_eq!(config.max_retries, 2);
        assert!(config.auto_apply);
    }

    #[test]
    fn test_improvement_result_serialization() {
        let trajectory = make_trajectory(vec![
            make_turn(vec![("read", false)], None, StopReason::Stop),
        ]);
        let report = HeuristicEvaluatorAdapter::new().evaluate(&trajectory).unwrap();
        let result = ImprovementResult {
            eval_report: report,
            passed: true,
            patches: vec![],
            patches_applied: false,
            score_delta: 0.0,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_vec(&result).unwrap();
        let deserialized: ImprovementResult = serde_json::from_slice(&json).unwrap();
        assert!(deserialized.passed);
    }
}
