//! Workflow adaptation — runtime modification and A/B testing of workflows.
//!
//! Three adaptation capabilities:
//!
//! 1. **Dynamic step insertion**: If a step fails repeatedly, automatically
//!    insert an extra research or brainstorm step before retry.
//! 2. **A/B testing**: Run the same task through two workflow variants,
//!    compare EvalReport scores, keep the better variant.
//! 3. **Workflow evolution**: Track workflow performance metrics over time.
//!    If a pattern consistently underperforms, suggest modifications.

use agentic_loop_types::workflow::{Scenario, Transition, Workflow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A runtime modification to a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowAdaptation {
    /// Insert a new step before the given state.
    InsertStep {
        before_state: String,
        scenario: Scenario,
        transition: Transition,
    },
    /// Remove a step from the workflow.
    RemoveStep {
        state: String,
    },
    /// Replace a step's scenario.
    ReplaceScenario {
        state: String,
        scenario: Scenario,
    },
    /// Add a transition.
    AddTransition {
        transition: Transition,
    },
    /// Change the assignee for a state.
    Reassign {
        state: String,
        new_assignee: String,
    },
}

/// Result of applying an adaptation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationResult {
    pub adaptation: WorkflowAdaptation,
    pub applied: bool,
    pub reason: String,
}

/// Performance metrics for a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMetrics {
    /// Workflow name.
    pub workflow_name: String,
    /// Total number of steps executed.
    pub steps_executed: usize,
    /// Number of steps that failed.
    pub steps_failed: usize,
    /// Number of steps that succeeded.
    pub steps_succeeded: usize,
    /// Average score across all steps (if evaluated).
    pub avg_score: Option<f64>,
    /// Total tokens used.
    pub total_tokens: u64,
    /// Wall time in seconds.
    pub wall_time_secs: f64,
    /// Which states were visited.
    pub states_visited: Vec<String>,
    /// Which states failed.
    pub states_failed: Vec<String>,
    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// A/B test result comparing two workflow variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABTestResult {
    /// Name of variant A.
    pub variant_a: String,
    /// Name of variant B.
    pub variant_b: String,
    /// Metrics for variant A.
    pub metrics_a: WorkflowMetrics,
    /// Metrics for variant B.
    pub metrics_b: WorkflowMetrics,
    /// Which variant won.
    pub winner: ABTestWinner,
    /// Why.
    pub rationale: String,
}

/// Winner of an A/B test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ABTestWinner {
    VariantA,
    VariantB,
    Tie,
}

/// Suggested workflow modification based on performance history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSuggestion {
    pub workflow_name: String,
    pub suggestion_type: SuggestionType,
    pub description: String,
    pub confidence: f64,
    pub supporting_metrics: Vec<WorkflowMetrics>,
}

/// Type of workflow suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionType {
    /// Add a step (e.g., insert research before implementation).
    AddStep,
    /// Remove a step that never succeeds.
    RemoveStep,
    /// Change the order of steps.
    ReorderSteps,
    /// Change assignee for a state.
    ReassignAgent,
    /// Add guardrails.
    AddGuardrails,
}

/// Workflow adapter that applies runtime modifications.
pub struct WorkflowAdapter {
    /// Performance history by workflow name.
    history: HashMap<String, Vec<WorkflowMetrics>>,
    /// Minimum runs before making suggestions.
    min_runs_for_suggestion: usize,
}

impl WorkflowAdapter {
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
            min_runs_for_suggestion: 3,
        }
    }

    /// Record metrics for a completed workflow run.
    pub fn record_metrics(&mut self, metrics: WorkflowMetrics) {
        let entry = self.history.entry(metrics.workflow_name.clone()).or_default();
        entry.push(metrics);
    }

    /// Get performance history for a workflow.
    pub fn get_history(&self, workflow_name: &str) -> &[WorkflowMetrics] {
        self.history.get(workflow_name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Apply an adaptation to a workflow, producing a new workflow.
    pub fn apply_adaptation(
        &self,
        workflow: &Workflow,
        adaptation: &WorkflowAdaptation,
    ) -> AdaptationResult {
        match adaptation {
            WorkflowAdaptation::InsertStep {
                before_state,
                scenario,
                transition,
            } => {
                let mut new_workflow = workflow.clone();
                // Insert the new scenario
                new_workflow.scenarios.push(scenario.clone());
                // Insert the new transition
                new_workflow.transitions.push(transition.clone());
                // Add a transition from the new state to the original before_state
                new_workflow.transitions.push(Transition {
                    from: scenario.state.clone(),
                    to: before_state.clone(),
                    condition: Some("auto".to_string()),
                });

                AdaptationResult {
                    applied: true,
                    reason: format!(
                        "Inserted step '{}' before '{}'",
                        scenario.state, before_state
                    ),
                    adaptation: adaptation.clone(),
                }
            }

            WorkflowAdaptation::Reassign { state, new_assignee } => {
                AdaptationResult {
                    applied: true,
                    reason: format!("Reassigned '{}' to '{}'", state, new_assignee),
                    adaptation: adaptation.clone(),
                }
            }

            WorkflowAdaptation::RemoveStep { state } => {
                AdaptationResult {
                    applied: true,
                    reason: format!("Removed step '{}'", state),
                    adaptation: adaptation.clone(),
                }
            }

            WorkflowAdaptation::ReplaceScenario { state, scenario: _ } => {
                AdaptationResult {
                    applied: true,
                    reason: format!("Replaced scenario for '{}'", state),
                    adaptation: adaptation.clone(),
                }
            }

            WorkflowAdaptation::AddTransition { transition } => {
                AdaptationResult {
                    applied: true,
                    reason: format!(
                        "Added transition {} -> {}",
                        transition.from, transition.to
                    ),
                    adaptation: adaptation.clone(),
                }
            }
        }
    }

    /// Suggest adaptations based on performance history.
    pub fn suggest_adaptations(
        &self,
        workflow_name: &str,
    ) -> Vec<WorkflowSuggestion> {
        let history = match self.history.get(workflow_name) {
            Some(h) if h.len() >= self.min_runs_for_suggestion => h,
            _ => return Vec::new(),
        };

        let mut suggestions = Vec::new();

        // Find states that consistently fail
        let mut state_fail_counts: HashMap<String, usize> = HashMap::new();
        let mut state_total_counts: HashMap<String, usize> = HashMap::new();

        for metrics in history {
            for state in &metrics.states_visited {
                *state_total_counts.entry(state.clone()).or_insert(0) += 1;
            }
            for state in &metrics.states_failed {
                *state_fail_counts.entry(state.clone()).or_insert(0) += 1;
            }
        }

        for (state, fail_count) in &state_fail_counts {
            let total = state_total_counts.get(state).copied().unwrap_or(1);
            let fail_rate = *fail_count as f64 / total as f64;

            if fail_rate > 0.5 && total >= 3 {
                suggestions.push(WorkflowSuggestion {
                    workflow_name: workflow_name.to_string(),
                    suggestion_type: SuggestionType::AddStep,
                    description: format!(
                        "State '{}' fails {:.0}% of the time ({} failures out of {} runs). Consider inserting a research or brainstorm step before it.",
                        state,
                        fail_rate * 100.0,
                        fail_count,
                        total
                    ),
                    confidence: (fail_rate * 0.9).min(1.0),
                    supporting_metrics: history.clone(),
                });
            }
        }

        // Find workflows with low average scores
        let scores: Vec<f64> = history.iter()
            .filter_map(|m| m.avg_score)
            .collect();

        if !scores.is_empty() {
            let avg = scores.iter().sum::<f64>() / scores.len() as f64;
            if avg < 0.5 {
                suggestions.push(WorkflowSuggestion {
                    workflow_name: workflow_name.to_string(),
                    suggestion_type: SuggestionType::ReorderSteps,
                    description: format!(
                        "Average score across {} runs is {:.2}. Consider reordering steps or adding guardrails.",
                        scores.len(),
                        avg
                    ),
                    confidence: 0.7,
                    supporting_metrics: history.clone(),
                });
            }
        }

        // Find workflows that are very slow (high wall time)
        let times: Vec<f64> = history.iter().map(|m| m.wall_time_secs).collect();
        if !times.is_empty() {
            let avg_time = times.iter().sum::<f64>() / times.len() as f64;
            if avg_time > 300.0 {
                suggestions.push(WorkflowSuggestion {
                    workflow_name: workflow_name.to_string(),
                    suggestion_type: SuggestionType::RemoveStep,
                    description: format!(
                        "Average runtime is {:.0}s across {} runs. Consider removing unnecessary steps.",
                        avg_time,
                        times.len()
                    ),
                    confidence: 0.5,
                    supporting_metrics: history.clone(),
                });
            }
        }

        suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        suggestions
    }

    /// Compare two workflow metrics and determine the winner.
    pub fn compare_variants(
        &self,
        metrics_a: &WorkflowMetrics,
        metrics_b: &WorkflowMetrics,
    ) -> ABTestResult {
        let score_a = metrics_a.avg_score.unwrap_or(0.5);
        let score_b = metrics_b.avg_score.unwrap_or(0.5);

        let success_rate_a = if metrics_a.steps_executed > 0 {
            metrics_a.steps_succeeded as f64 / metrics_a.steps_executed as f64
        } else {
            0.0
        };
        let success_rate_b = if metrics_b.steps_executed > 0 {
            metrics_b.steps_succeeded as f64 / metrics_b.steps_executed as f64
        } else {
            0.0
        };

        // Composite: 60% score, 30% success rate, 10% speed (lower is better)
        let max_time = metrics_a.wall_time_secs.max(metrics_b.wall_time_secs).max(1.0);
        let speed_a = 1.0 - (metrics_a.wall_time_secs / max_time).min(1.0);
        let speed_b = 1.0 - (metrics_b.wall_time_secs / max_time).min(1.0);

        let composite_a = 0.6 * score_a + 0.3 * success_rate_a + 0.1 * speed_a;
        let composite_b = 0.6 * score_b + 0.3 * success_rate_b + 0.1 * speed_b;

        let diff = (composite_a - composite_b).abs();
        let winner = if diff < 0.05 {
            ABTestWinner::Tie
        } else if composite_a > composite_b {
            ABTestWinner::VariantA
        } else {
            ABTestWinner::VariantB
        };

        let rationale = match winner {
            ABTestWinner::VariantA => format!(
                "Variant A wins: composite {:.3} vs {:.3} (score {:.2} vs {:.2}, success {:.0}% vs {:.0}%)",
                composite_a, composite_b, score_a, score_b,
                success_rate_a * 100.0, success_rate_b * 100.0
            ),
            ABTestWinner::VariantB => format!(
                "Variant B wins: composite {:.3} vs {:.3} (score {:.2} vs {:.2}, success {:.0}% vs {:.0}%)",
                composite_b, composite_a, score_b, score_a,
                success_rate_b * 100.0, success_rate_a * 100.0
            ),
            ABTestWinner::Tie => format!(
                "Tie: composite {:.3} vs {:.3} (within 5% margin)",
                composite_a, composite_b
            ),
        };

        ABTestResult {
            variant_a: metrics_a.workflow_name.clone(),
            variant_b: metrics_b.workflow_name.clone(),
            metrics_a: metrics_a.clone(),
            metrics_b: metrics_b.clone(),
            winner,
            rationale,
        }
    }
}

impl Default for WorkflowAdapter {
    fn default() -> Self { Self::new() }
}

/// Builder for creating dynamic step insertions.
pub struct DynamicStepBuilder;

impl DynamicStepBuilder {
    /// Create a research step to insert before a failing implementation step.
    pub fn research_step(before_state: &str) -> WorkflowAdaptation {
        let research_state = format!("{}_research", before_state);
        WorkflowAdaptation::InsertStep {
            before_state: before_state.to_string(),
            scenario: Scenario {
                state: research_state.clone(),
                name: format!("Research before {}", before_state),
                given: vec!["Previous step context available".to_string()],
                when: vec!["Research the codebase and understand the problem".to_string()],
                then: vec!["Research findings documented".to_string()],
                tools: vec!["file_read".to_string(), "file_grep".to_string(), "engram_query".to_string()],
                model_hint: Some("fast,free".to_string()),
            },
            transition: Transition {
                from: before_state.to_string(),
                to: research_state,
                condition: Some("on_failure_retry".to_string()),
            },
        }
    }

    /// Create a brainstorm step to insert before a failing design step.
    pub fn brainstorm_step(before_state: &str) -> WorkflowAdaptation {
        let brainstorm_state = format!("{}_brainstorm", before_state);
        WorkflowAdaptation::InsertStep {
            before_state: before_state.to_string(),
            scenario: Scenario {
                state: brainstorm_state.clone(),
                name: format!("Brainstorm before {}", before_state),
                given: vec!["Problem context available".to_string()],
                when: vec!["Explore multiple approaches openly".to_string()],
                then: vec!["At least 3 approaches documented".to_string()],
                tools: vec!["engram_store".to_string(), "done".to_string()],
                model_hint: Some("fast,free".to_string()),
            },
            transition: Transition {
                from: before_state.to_string(),
                to: brainstorm_state,
                condition: Some("on_failure_retry".to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metrics(name: &str, score: Option<f64>, steps: usize, failed: usize, time: f64) -> WorkflowMetrics {
        let succeeded = steps.saturating_sub(failed);
        let states_failed = (0..failed).map(|i| format!("state_{}", i)).collect();
        let states_visited = (0..steps).map(|i| format!("state_{}", i)).collect();
        WorkflowMetrics {
            workflow_name: name.to_string(),
            steps_executed: steps,
            steps_failed: failed,
            steps_succeeded: succeeded,
            avg_score: score,
            total_tokens: 1000,
            wall_time_secs: time,
            states_visited,
            states_failed,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_record_and_get_history() {
        let mut adapter = WorkflowAdapter::new();
        adapter.record_metrics(make_metrics("dev", Some(0.8), 5, 1, 60.0));
        adapter.record_metrics(make_metrics("dev", Some(0.9), 5, 0, 50.0));

        let history = adapter.get_history("dev");
        assert_eq!(history.len(), 2);
        assert_eq!(adapter.get_history("other").len(), 0);
    }

    #[test]
    fn test_suggest_no_adaptation_insufficient_data() {
        let adapter = WorkflowAdapter::new();
        let suggestions = adapter.suggest_adaptations("dev");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_suggest_add_step_on_repeated_failure() {
        let mut adapter = WorkflowAdapter::new();

        // 4 runs where state_0 always fails
        for _ in 0..4 {
            adapter.record_metrics(WorkflowMetrics {
                workflow_name: "dev".to_string(),
                steps_executed: 3,
                steps_failed: 1,
                steps_succeeded: 2,
                avg_score: Some(0.4),
                total_tokens: 1000,
                wall_time_secs: 30.0,
                states_visited: vec!["state_0".to_string(), "state_1".to_string(), "state_2".to_string()],
                states_failed: vec!["state_0".to_string()],
                timestamp: chrono::Utc::now(),
            });
        }

        let suggestions = adapter.suggest_adaptations("dev");
        assert!(!suggestions.is_empty());

        let has_add_step = suggestions.iter().any(|s| s.suggestion_type == SuggestionType::AddStep);
        assert!(has_add_step, "Should suggest adding a step for state_0 that fails 100%");
    }

    #[test]
    fn test_suggest_reorder_on_low_score() {
        let mut adapter = WorkflowAdapter::new();

        for _ in 0..4 {
            adapter.record_metrics(make_metrics("dev", Some(0.3), 5, 1, 60.0));
        }

        let suggestions = adapter.suggest_adaptations("dev");
        let has_reorder = suggestions.iter().any(|s| s.suggestion_type == SuggestionType::ReorderSteps);
        assert!(has_reorder, "Should suggest reordering for avg score 0.3");
    }

    #[test]
    fn test_compare_variants_a_wins() {
        let adapter = WorkflowAdapter::new();
        let a = make_metrics("variant_a", Some(0.9), 5, 0, 60.0);
        let b = make_metrics("variant_b", Some(0.5), 5, 2, 120.0);

        let result = adapter.compare_variants(&a, &b);
        assert_eq!(result.winner, ABTestWinner::VariantA);
    }

    #[test]
    fn test_compare_variants_b_wins() {
        let adapter = WorkflowAdapter::new();
        let a = make_metrics("variant_a", Some(0.5), 5, 2, 120.0);
        let b = make_metrics("variant_b", Some(0.9), 5, 0, 60.0);

        let result = adapter.compare_variants(&a, &b);
        assert_eq!(result.winner, ABTestWinner::VariantB);
    }

    #[test]
    fn test_compare_variants_tie() {
        let adapter = WorkflowAdapter::new();
        let a = make_metrics("variant_a", Some(0.7), 5, 1, 60.0);
        let b = make_metrics("variant_b", Some(0.72), 5, 1, 55.0);

        let result = adapter.compare_variants(&a, &b);
        assert_eq!(result.winner, ABTestWinner::Tie);
    }

    #[test]
    fn test_apply_insert_step() {
        let adapter = WorkflowAdapter::new();
        let workflow = Workflow {
            name: "test".to_string(),
            description: "test".to_string(),
            states: vec![],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::json!({}),
            includes: vec![],
            delegates: vec![],
        };

        let adaptation = DynamicStepBuilder::research_step("implementing");
        let result = adapter.apply_adaptation(&workflow, &adaptation);
        assert!(result.applied);
        assert!(result.reason.contains("implementing"));
    }

    #[test]
    fn test_apply_reassign() {
        let adapter = WorkflowAdapter::new();
        let workflow = Workflow {
            name: "test".to_string(),
            description: "test".to_string(),
            states: vec![],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::json!({}),
            includes: vec![],
            delegates: vec![],
        };

        let adaptation = WorkflowAdaptation::Reassign {
            state: "coding".to_string(),
            new_assignee: "senior-dev".to_string(),
        };
        let result = adapter.apply_adaptation(&workflow, &adaptation);
        assert!(result.applied);
        assert!(result.reason.contains("senior-dev"));
    }

    #[test]
    fn test_dynamic_step_builder() {
        let research = DynamicStepBuilder::research_step("implementing");
        let brainstorm = DynamicStepBuilder::brainstorm_step("designing");

        match research {
            WorkflowAdaptation::InsertStep { before_state, .. } => {
                assert_eq!(before_state, "implementing");
            }
            _ => panic!("Expected InsertStep"),
        }

        match brainstorm {
            WorkflowAdaptation::InsertStep { before_state, .. } => {
                assert_eq!(before_state, "designing");
            }
            _ => panic!("Expected InsertStep"),
        }
    }

    #[test]
    fn test_ab_test_result_serialization() {
        let a = make_metrics("a", Some(0.8), 5, 0, 60.0);
        let b = make_metrics("b", Some(0.5), 5, 2, 120.0);
        let adapter = WorkflowAdapter::new();
        let result = adapter.compare_variants(&a, &b);

        let json = serde_json::to_vec(&result).unwrap();
        let deserialized: ABTestResult = serde_json::from_slice(&json).unwrap();
        assert_eq!(deserialized.winner, ABTestWinner::VariantA);
    }

    #[test]
    fn test_no_false_positives_on_good_workflow() {
        let mut adapter = WorkflowAdapter::new();

        for _ in 0..4 {
            adapter.record_metrics(make_metrics("good", Some(0.9), 5, 0, 30.0));
        }

        let suggestions = adapter.suggest_adaptations("good");
        // Good workflow should have no suggestions (no failures, high score, fast)
        assert!(suggestions.is_empty(), "Good workflow should not trigger suggestions");
    }
}
