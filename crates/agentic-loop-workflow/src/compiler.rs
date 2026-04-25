//! Natural language to workflow compilation.
//!
//! Compiles natural language process descriptions into `.workflow` files.
//! Includes workflow versioning, A/B testing, pattern synthesis, and
//! dynamic assembly from known good states.

use agentic_loop_types::workflow::{Scenario, State, Transition, Workflow};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A prompt template for generating workflows from natural language.
pub const NL_TO_WORKFLOW_PROMPT: &str = r#"
You are a workflow compiler. Convert the following natural language process description
into a Gherkin-like .workflow format.

Rules:
- Start with @workflow and Feature: header
- Each step becomes a @state.name with a Scenario
- Each Scenario has Given/When/Then sections
- List tools each state needs
- Add Transitions: section at the end showing state flow
- Add Config: section with max_retries

Example output:
```
@workflow
Feature: {name}
  Description: {description}

  @state.start
  Scenario: {first step}
    Given: {precondition}
    When: {action}
    Then: {outcome}
    Then: Move to "{next_state}" state
    Tools:
      {tool_list}

  Config:
    max_retries: 3

  Transitions:
    start -> next: auto
    next -> done: manual (done)
```

Process description:
{description}
"#;

/// A compiled workflow from natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledWorkflow {
    /// The compiled workflow.
    pub workflow: Workflow,
    /// Source natural language description.
    pub source_description: String,
    /// Compilation confidence (0.0–1.0).
    pub confidence: f64,
    /// Whether human review is recommended.
    pub needs_review: bool,
}

/// Workflow version for A/B testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVersion {
    /// Version identifier (e.g. "v1", "v2").
    pub version: String,
    /// The workflow definition.
    pub workflow: Workflow,
    /// Number of times this version has been run.
    pub run_count: u64,
    /// Average eval score across runs.
    pub avg_eval_score: f64,
    /// Success rate (0.0–1.0).
    pub success_rate: f64,
}

/// A/B test result comparing two workflow versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABTestResult {
    /// Version A identifier.
    pub version_a: String,
    /// Version B identifier.
    pub version_b: String,
    /// Total runs in the test.
    pub total_runs: u64,
    /// Winner (if statistically significant).
    pub winner: Option<String>,
    /// Score difference (positive = B better).
    pub score_delta: f64,
    /// Whether the result is statistically significant.
    pub significant: bool,
}

/// A recognized pattern that can be synthesized into a workflow template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPattern {
    /// Pattern name (e.g. "feature-development", "bug-fix").
    pub name: String,
    /// States in the pattern.
    pub states: Vec<String>,
    /// Typical transitions.
    pub transitions: Vec<(String, String)>,
    /// Number of times this pattern was observed.
    pub observation_count: u64,
    /// Average success rate when using this pattern.
    pub avg_success_rate: f64,
}

/// Workflow compiler — converts NL descriptions to workflows.
pub struct WorkflowCompiler;

impl WorkflowCompiler {
    /// Create a new compiler.
    pub fn new() -> Self {
        Self
    }

    /// Build an LLM prompt for compiling a workflow from natural language.
    pub fn build_prompt(description: &str, name: &str) -> String {
        NL_TO_WORKFLOW_PROMPT
            .replace("{name}", name)
            .replace("{description}", description)
    }

    /// Compile a simple sequential workflow from state names.
    /// This is a rule-based fallback when no LLM is available.
    pub fn compile_sequential(
        name: &str,
        description: &str,
        states: Vec<&str>,
        tools_per_state: HashMap<&str, Vec<&str>>,
    ) -> Result<CompiledWorkflow> {
        let workflow_states: Vec<State> = states
            .iter()
            .map(|s| State {
                name: s.to_string(),
                description: format!("State: {}", s),
                config: serde_json::Value::Null,
            })
            .collect();

        let scenarios: Vec<Scenario> = states
            .iter()
            .map(|s| Scenario {
                name: format!("Execute {}", s),
                state: s.to_string(),
                given: vec!["Previous state completed".to_string()],
                when: vec![format!("Perform {} operations", s)],
                then: vec![format!("{} completed", s)],
                tools: tools_per_state
                    .get(s)
                    .map(|t| t.iter().map(|x| x.to_string()).collect())
                    .unwrap_or_default(),
                model_hint: None,
            })
            .collect();

        let transitions: Vec<Transition> = states
            .windows(2)
            .map(|w| Transition {
                from: w[0].to_string(),
                to: w[1].to_string(),
                condition: Some("auto".to_string()),
            })
            .chain(
                states
                    .last()
                    .map(|last| Transition {
                        from: last.to_string(),
                        to: "done".to_string(),
                        condition: Some("manual (done)".to_string()),
                    })
                    .into_iter(),
            )
            .collect();

        let initial = states.first().map(|s| s.to_string()).unwrap_or_default();

        let workflow = Workflow {
            name: name.to_string(),
            description: description.to_string(),
            states: workflow_states,
            initial_state: initial,
            scenarios,
            transitions,
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::json!({"max_retries": 3}),
            includes: vec![],
            delegates: vec![],
        };

        Ok(CompiledWorkflow {
            workflow,
            source_description: description.to_string(),
            confidence: 0.7, // Rule-based is less confident than LLM
            needs_review: true,
        })
    }
}

/// Workflow version manager for A/B testing.
pub struct WorkflowVersionManager {
    versions: HashMap<String, Vec<WorkflowVersion>>,
}

impl WorkflowVersionManager {
    pub fn new() -> Self {
        Self {
            versions: HashMap::new(),
        }
    }

    /// Register a new workflow version.
    pub fn register(&mut self, workflow_name: &str, version: WorkflowVersion) {
        self.versions
            .entry(workflow_name.to_string())
            .or_default()
            .push(version);
    }

    /// Record a run result for a specific version.
    pub fn record_run(
        &mut self,
        workflow_name: &str,
        version_id: &str,
        eval_score: f64,
        success: bool,
    ) {
        if let Some(versions) = self.versions.get_mut(workflow_name) {
            if let Some(v) = versions.iter_mut().find(|v| v.version == version_id) {
                let total_score = v.avg_eval_score * v.run_count as f64 + eval_score;
                let total_success = v.success_rate * v.run_count as f64 + if success { 1.0 } else { 0.0 };
                v.run_count += 1;
                v.avg_eval_score = total_score / v.run_count as f64;
                v.success_rate = total_success / v.run_count as f64;
            }
        }
    }

    /// Compare two versions for A/B testing.
    pub fn compare(
        &self,
        workflow_name: &str,
        version_a: &str,
        version_b: &str,
    ) -> Option<ABTestResult> {
        let versions = self.versions.get(workflow_name)?;
        let a = versions.iter().find(|v| v.version == version_a)?;
        let b = versions.iter().find(|v| v.version == version_b)?;

        // Require at least 3 runs per version for significance
        let significant = a.run_count >= 3 && b.run_count >= 3;
        let delta = b.avg_eval_score - a.avg_eval_score;

        let winner = if significant && delta.abs() > 0.05 {
            Some(if delta > 0.0 {
                version_b.to_string()
            } else {
                version_a.to_string()
            })
        } else {
            None
        };

        Some(ABTestResult {
            version_a: version_a.to_string(),
            version_b: version_b.to_string(),
            total_runs: a.run_count + b.run_count,
            winner,
            score_delta: delta,
            significant,
        })
    }

    /// Get all versions for a workflow.
    pub fn get_versions(&self, workflow_name: &str) -> Vec<&WorkflowVersion> {
        self.versions
            .get(workflow_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get the best performing version.
    pub fn best_version(&self, workflow_name: &str) -> Option<&WorkflowVersion> {
        self.versions
            .get(workflow_name)?
            .iter()
            .max_by(|a, b| {
                let score_a = a.avg_eval_score * 0.6 + a.success_rate * 0.4;
                let score_b = b.avg_eval_score * 0.6 + b.success_rate * 0.4;
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// Pattern synthesizer — detects repeated patterns and creates templates.
pub struct PatternSynthesizer {
    patterns: Vec<WorkflowPattern>,
}

impl PatternSynthesizer {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    /// Register a completed workflow run as a potential pattern.
    /// Returns the pattern name.
    pub fn observe(
        &mut self,
        states: &[String],
        success: bool,
    ) -> String {
        let state_key: String = states.join(" -> ");
        let transitions: Vec<(String, String)> = states
            .windows(2)
            .map(|w| (w[0].clone(), w[1].clone()))
            .collect();

        for pattern in &mut self.patterns {
            let pattern_key = pattern.states.join(" -> ");
            if pattern_key == state_key {
                pattern.observation_count += 1;
                let total = pattern.avg_success_rate * (pattern.observation_count - 1) as f64
                    + if success { 1.0 } else { 0.0 };
                pattern.avg_success_rate = total / pattern.observation_count as f64;
                return pattern.name.clone();
            }
        }

        // New pattern
        let name = format!("pattern-{}", self.patterns.len() + 1);
        let pattern = WorkflowPattern {
            name: name.clone(),
            states: states.to_vec(),
            transitions,
            observation_count: 1,
            avg_success_rate: if success { 1.0 } else { 0.0 },
        };
        self.patterns.push(pattern);
        name
    }

    /// Find patterns with at least `min_observations` and `min_success_rate`.
    pub fn find_reliable(
        &self,
        min_observations: u64,
        min_success_rate: f64,
    ) -> Vec<&WorkflowPattern> {
        self.patterns
            .iter()
            .filter(|p| p.observation_count >= min_observations && p.avg_success_rate >= min_success_rate)
            .collect()
    }

    /// Synthesize a workflow from a reliable pattern.
    pub fn synthesize(
        &self,
        pattern_name: &str,
        workflow_name: &str,
        description: &str,
    ) -> Option<Workflow> {
        let pattern = self.patterns.iter().find(|p| p.name == pattern_name)?;

        let states: Vec<State> = pattern
            .states
            .iter()
            .map(|s| State {
                name: s.clone(),
                description: format!("Auto-synthesized state: {}", s),
                config: serde_json::Value::Null,
            })
            .collect();

        let scenarios: Vec<Scenario> = pattern
            .states
            .iter()
            .map(|s| Scenario {
                name: format!("Execute {}", s),
                state: s.clone(),
                given: vec!["Previous state completed".to_string()],
                when: vec![format!("Perform {} operations", s)],
                then: vec![format!("{} completed", s)],
                tools: vec![],
                model_hint: None,
            })
            .collect();

        let transitions: Vec<Transition> = pattern
            .transitions
            .iter()
            .map(|(from, to)| Transition {
                from: from.clone(),
                to: to.clone(),
                condition: Some("auto".to_string()),
            })
            .collect();

        Some(Workflow {
            name: workflow_name.to_string(),
            description: description.to_string(),
            initial_state: pattern.states.first().cloned()?,
            states,
            scenarios,
            transitions,
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::json!({"synthesized": true, "source_pattern": pattern_name}),
            includes: vec![],
            delegates: vec![],
        })
    }

    /// Get all known patterns.
    pub fn patterns(&self) -> &[WorkflowPattern] {
        &self.patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_sequential() {
        let mut tools = HashMap::new();
        tools.insert("research", vec!["file_read", "bash"]);
        tools.insert("implement", vec!["file_write", "file_edit", "bash"]);

        let compiled = WorkflowCompiler::compile_sequential(
            "test-workflow",
            "A test workflow",
            vec!["research", "implement", "test", "done"],
            tools,
        ).unwrap();

        assert_eq!(compiled.workflow.name, "test-workflow");
        assert_eq!(compiled.workflow.states.len(), 4);
        assert_eq!(compiled.workflow.initial_state, "research");
        assert_eq!(compiled.workflow.scenarios.len(), 4);
        assert!(compiled.needs_review);
        assert_eq!(compiled.confidence, 0.7);
    }

    #[test]
    fn test_compile_sequential_transitions() {
        let compiled = WorkflowCompiler::compile_sequential(
            "simple",
            "desc",
            vec!["start", "middle", "end"],
            HashMap::new(),
        ).unwrap();

        assert_eq!(compiled.workflow.transitions.len(), 3);
        assert_eq!(compiled.workflow.transitions[0].from, "start");
        assert_eq!(compiled.workflow.transitions[0].to, "middle");
        assert_eq!(compiled.workflow.transitions[1].from, "middle");
        assert_eq!(compiled.workflow.transitions[1].to, "end");
        assert_eq!(compiled.workflow.transitions[2].from, "end");
        assert_eq!(compiled.workflow.transitions[2].to, "done");
    }

    #[test]
    fn test_build_prompt() {
        let prompt = WorkflowCompiler::build_prompt("research and implement", "my-workflow");
        assert!(prompt.contains("my-workflow"));
        assert!(prompt.contains("research and implement"));
        assert!(prompt.contains("@workflow"));
    }

    #[test]
    fn test_version_manager_register() {
        let mut mgr = WorkflowVersionManager::new();
        mgr.register("dev-workflow", WorkflowVersion {
            version: "v1".to_string(),
            workflow: make_test_workflow("dev-v1"),
            run_count: 0,
            avg_eval_score: 0.0,
            success_rate: 0.0,
        });
        assert_eq!(mgr.get_versions("dev-workflow").len(), 1);
    }

    #[test]
    fn test_version_manager_record_runs() {
        let mut mgr = WorkflowVersionManager::new();
        mgr.register("wf", WorkflowVersion {
            version: "v1".to_string(),
            workflow: make_test_workflow("wf-v1"),
            run_count: 0,
            avg_eval_score: 0.0,
            success_rate: 0.0,
        });

        mgr.record_run("wf", "v1", 0.8, true);
        mgr.record_run("wf", "v1", 0.9, true);
        mgr.record_run("wf", "v1", 0.7, false);

        let v = mgr.get_versions("wf")[0];
        assert_eq!(v.run_count, 3);
        assert!((v.avg_eval_score - 0.8).abs() < 0.01);
        assert!((v.success_rate - 0.667).abs() < 0.01);
    }

    #[test]
    fn test_version_manager_ab_test() {
        let mut mgr = WorkflowVersionManager::new();
        mgr.register("wf", WorkflowVersion {
            version: "v1".to_string(),
            workflow: make_test_workflow("v1"),
            run_count: 5,
            avg_eval_score: 0.7,
            success_rate: 0.6,
        });
        mgr.register("wf", WorkflowVersion {
            version: "v2".to_string(),
            workflow: make_test_workflow("v2"),
            run_count: 5,
            avg_eval_score: 0.9,
            success_rate: 0.8,
        });

        let result = mgr.compare("wf", "v1", "v2").unwrap();
        assert_eq!(result.total_runs, 10);
        assert!(result.significant);
        assert_eq!(result.winner.as_deref(), Some("v2"));
        assert!(result.score_delta > 0.1);
    }

    #[test]
    fn test_version_manager_ab_test_not_significant() {
        let mut mgr = WorkflowVersionManager::new();
        mgr.register("wf", WorkflowVersion {
            version: "v1".to_string(),
            workflow: make_test_workflow("v1"),
            run_count: 2,
            avg_eval_score: 0.8,
            success_rate: 0.5,
        });
        mgr.register("wf", WorkflowVersion {
            version: "v2".to_string(),
            workflow: make_test_workflow("v2"),
            run_count: 2,
            avg_eval_score: 0.85,
            success_rate: 0.75,
        });

        let result = mgr.compare("wf", "v1", "v2").unwrap();
        assert!(!result.significant);
        assert!(result.winner.is_none());
    }

    #[test]
    fn test_version_manager_best_version() {
        let mut mgr = WorkflowVersionManager::new();
        mgr.register("wf", WorkflowVersion {
            version: "v1".to_string(),
            workflow: make_test_workflow("v1"),
            run_count: 5,
            avg_eval_score: 0.6,
            success_rate: 0.5,
        });
        mgr.register("wf", WorkflowVersion {
            version: "v2".to_string(),
            workflow: make_test_workflow("v2"),
            run_count: 5,
            avg_eval_score: 0.9,
            success_rate: 0.9,
        });

        let best = mgr.best_version("wf").unwrap();
        assert_eq!(best.version, "v2");
    }

    #[test]
    fn test_pattern_synthesizer_observe() {
        let mut synth = PatternSynthesizer::new();
        let name = synth.observe(
            &["research".to_string(), "implement".to_string(), "test".to_string()],
            true,
        );
        assert!(!name.is_empty());
        let p = synth.patterns().iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.observation_count, 1);
        assert_eq!(p.avg_success_rate, 1.0);
    }

    #[test]
    fn test_pattern_synthesizer_repeated() {
        let mut synth = PatternSynthesizer::new();
        let states = vec!["start".to_string(), "done".to_string()];
        synth.observe(&states, true);
        synth.observe(&states, true);
        synth.observe(&states, false);
        let name = synth.observe(&states, true);
        assert!(!name.is_empty());
        let p = synth.patterns().iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.observation_count, 4);
        assert!((p.avg_success_rate - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_pattern_synthesizer_find_reliable() {
        let mut synth = PatternSynthesizer::new();
        let good = vec!["research".to_string(), "implement".to_string()];
        let bad = vec!["skip".to_string(), "fail".to_string()];
        for _ in 0..5 { synth.observe(&good, true); }
        synth.observe(&bad, false);
        synth.observe(&bad, false);

        let reliable = synth.find_reliable(3, 0.8);
        assert_eq!(reliable.len(), 1);
        assert!(reliable[0].states.starts_with(&["research".to_string()]));
    }

    #[test]
    fn test_pattern_synthesizer_synthesize() {
        let mut synth = PatternSynthesizer::new();
        let pattern_name = synth.observe(
            &["plan".to_string(), "build".to_string(), "verify".to_string()],
            true,
        );

        let wf = synth.synthesize(&pattern_name, "synth-workflow", "Synthesized").unwrap();
        assert_eq!(wf.name, "synth-workflow");
        assert_eq!(wf.states.len(), 3);
        assert_eq!(wf.initial_state, "plan");
        assert!(wf.config["synthesized"].as_bool().unwrap());
    }

    fn make_test_workflow(name: &str) -> Workflow {
        Workflow {
            name: name.to_string(),
            description: String::new(),
            states: vec![State { name: "start".to_string(), description: "Start".to_string(), config: serde_json::Value::Null }],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![],
            delegates: vec![],
        }
    }
}
