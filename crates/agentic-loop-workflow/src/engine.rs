//! Workflow engine state machine.
//!
//! Drives agents through parsed .workflow files. Each state maps to a scenario
//! (Given/When/Then). The engine:
//! 1. Looks up the current state's scenario and assignee
//! 2. Builds a prompt via PromptBuilder
//! 3. Returns the prompt for the agent to execute
//! 4. On step completion, evaluates transitions to find the next state

use crate::{StepOutcome, SessionStatus};
use tracing::instrument;
use agentic_loop_types::workflow::{Scenario, Transition, Workflow};
use anyhow::Result;
use std::collections::HashMap;
use uuid::Uuid;

/// A running workflow session.
#[derive(Debug, Clone)]
pub struct WorkflowSession {
    pub id: String,
    pub task_id: String,
    pub workflow_name: String,
    pub current_state: String,
    pub status: SessionStatus,
    pub step_history: Vec<StepOutcome>,
    pub variables: serde_json::Value,
}

/// The workflow engine drives state machine execution.
pub struct WorkflowEngineImpl {
    /// Parsed workflow definition.
    workflow: Workflow,
    /// Scenarios indexed by state name.
    scenarios_by_state: HashMap<String, Scenario>,
    /// Transitions indexed by source state.
    transitions_from: HashMap<String, Vec<Transition>>,
    /// Wildcard transitions (from "*").
    wildcard_transitions: Vec<Transition>,
    /// Assignees indexed by state name (from config).
    assignees_by_state: HashMap<String, String>,
}

impl WorkflowEngineImpl {
    /// Create a new engine for a parsed workflow.
    pub fn new(workflow: Workflow) -> Self {
        let mut scenarios_by_state = HashMap::new();
        for scenario in &workflow.scenarios {
            scenarios_by_state.insert(scenario.state.clone(), scenario.clone());
        }

        let mut transitions_from: HashMap<String, Vec<Transition>> = HashMap::new();
        let mut wildcard_transitions = Vec::new();
        for t in &workflow.transitions {
            if t.from == "*" {
                wildcard_transitions.push(t.clone());
            } else {
                transitions_from
                    .entry(t.from.clone())
                    .or_default()
                    .push(t.clone());
            }
        }

        let mut assignees_by_state = HashMap::new();
        if let Some(assignee_lines) = &workflow.assignee {
            for line in assignee_lines {
                // Format: "state: agent"
                if let Some((state, agent)) = line.split_once(':') {
                    assignees_by_state.insert(state.trim().to_string(), agent.trim().to_string());
                }
            }
        }

        Self {
            workflow,
            scenarios_by_state,
            transitions_from,
            wildcard_transitions,
            assignees_by_state,
        }
    }

    /// Create a new session for a task.
    pub fn create_session(&self, task_id: &str, variables: serde_json::Value) -> WorkflowSession {
        WorkflowSession {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            workflow_name: self.workflow.name.clone(),
            current_state: self.workflow.initial_state.clone(),
            status: SessionStatus::Pending,
            step_history: Vec::new(),
            variables,
        }
    }

    /// Get the scenario for the current state.
    pub fn current_scenario(&self, session: &WorkflowSession) -> Option<&Scenario> {
        self.scenarios_by_state.get(&session.current_state)
    }

    /// Get the assignee for a state.
    pub fn assignee_for_state(&self, state: &str) -> Option<&str> {
        self.assignees_by_state.get(state).map(|s| s.as_str())
    }

    /// Get the assignee for the current state.
    pub fn current_assignee(&self, session: &WorkflowSession) -> Option<&str> {
        self.assignee_for_state(&session.current_state)
    }

    /// Record a step outcome and advance the state machine.
    #[instrument(skip(self, session), fields(state = %session.current_state))]
    pub fn advance(
        &self,
        session: &mut WorkflowSession,
        outcome: StepOutcome,
    ) -> Result<Option<String>> {
        session.status = SessionStatus::Running;

        let current = session.current_state.clone();
        session.step_history.push(outcome.clone());

        // Terminal states
        if current == "done" {
            session.status = SessionStatus::Completed;
            return Ok(None);
        }
        if current == "failed" {
            session.status = if outcome.success {
                SessionStatus::Completed
            } else {
                SessionStatus::Failed
            };
            return Ok(None);
        }

        // Find next state
        let next_state = self.resolve_next_state(&current, &outcome);
        match next_state {
            Some(next) => {
                session.current_state = next.clone();
                Ok(Some(next))
            }
            None => {
                // No transition found — stay in current state
                Ok(None)
            }
        }
    }

    /// Resolve the next state given current state and step outcome.
    fn resolve_next_state(&self, current: &str, outcome: &StepOutcome) -> Option<String> {
        // First try specific transitions from current state
        if let Some(transitions) = self.transitions_from.get(current) {
            for t in transitions {
                if self.matches_condition(&t.condition, outcome) {
                    return Some(t.to.clone());
                }
            }
        }

        // Fall back to wildcard transitions
        for t in &self.wildcard_transitions {
            if self.matches_condition(&t.condition, outcome) {
                return Some(t.to.clone());
            }
        }

        // If step succeeded and only one outgoing transition, take it
        if outcome.success {
            if let Some(transitions) = self.transitions_from.get(current) {
                if transitions.len() == 1 {
                    return Some(transitions[0].to.clone());
                }
            }
        }

        None
    }

    /// Check if a transition condition matches the step outcome.
    fn matches_condition(&self, condition: &Option<String>, outcome: &StepOutcome) -> bool {
        let Some(cond) = condition else {
            return true; // No condition = always matches
        };

        let cond_lower = cond.to_lowercase();

        // Failure transitions match when step failed (check before success conditions)
        if cond_lower.contains("fail") {
            return !outcome.success;
        }

        // Auto transitions always match on success
        if cond_lower.starts_with("auto") {
            return outcome.success;
        }

        // Manual transitions match when step completed
        if cond_lower.starts_with("manual") {
            return outcome.success;
        }

        // Guardrail transitions match on success
        if cond_lower.contains("guardrail") || cond_lower.contains("pass") {
            return outcome.success;
        }

        // Default: match on success
        outcome.success
    }

    /// Get all valid target states from the current state.
    pub fn valid_transitions(&self, state: &str) -> Vec<String> {
        let mut targets = Vec::new();

        if let Some(transitions) = self.transitions_from.get(state) {
            for t in transitions {
                targets.push(t.to.clone());
            }
        }

        for t in &self.wildcard_transitions {
            if !targets.contains(&t.to) {
                targets.push(t.to.clone());
            }
        }

        targets
    }

    /// Get the workflow definition.
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// Validate the workflow: check for orphan states, missing scenarios, etc.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // Check every state has a scenario
        for state in &self.workflow.states {
            if !self.scenarios_by_state.contains_key(&state.name) {
                warnings.push(format!(
                    "State '{}' has no Scenario definition",
                    state.name
                ));
            }
        }

        // Check every scenario has a corresponding state
        for (state_name, _) in &self.scenarios_by_state {
            if !self
                .workflow
                .states
                .iter()
                .any(|s| s.name == *state_name)
            {
                warnings.push(format!(
                    "Scenario for '{}' has no @state declaration",
                    state_name
                ));
            }
        }

        // Check transitions reference valid states
        for t in &self.workflow.transitions {
            if t.from != "*"
                && !self
                    .workflow
                    .states
                    .iter()
                    .any(|s| s.name == t.from)
            {
                warnings.push(format!(
                    "Transition from '{}' references unknown state",
                    t.from
                ));
            }
            if !self.workflow.states.iter().any(|s| s.name == t.to) {
                warnings.push(format!(
                    "Transition to '{}' references unknown state",
                    t.to
                ));
            }
        }

        // Check initial state exists
        if !self
            .workflow
            .states
            .iter()
            .any(|s| s.name == self.workflow.initial_state)
        {
            warnings.push(format!(
                "Initial state '{}' does not exist",
                self.workflow.initial_state
            ));
        }

        // Check for orphan states (no incoming transitions and not initial)
        for state in &self.workflow.states {
            if state.name == self.workflow.initial_state {
                continue;
            }
            let has_incoming = self
                .workflow
                .transitions
                .iter()
                .any(|t| t.to == state.name || (t.from == "*" && t.to == state.name));
            if !has_incoming {
                warnings.push(format!(
                    "State '{}' is orphaned (no incoming transitions)",
                    state.name
                ));
            }
        }

        warnings
    }

    /// Abort a session.
    pub fn abort(&self, session: &mut WorkflowSession) {
        session.status = SessionStatus::Aborted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkflowParserImpl;

    fn parse_and_create(content: &str) -> WorkflowEngineImpl {
        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();
        WorkflowEngineImpl::new(workflow)
    }

    #[test]
    fn test_create_session() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.end
  Scenario: End
    Given: Done
    When: Finish
    Then: Complete

  Transitions:
    start -> end: auto
"#,
        );

        let session = engine.create_session("task-1", serde_json::json!({}));
        assert_eq!(session.current_state, "start");
        assert_eq!(session.status, SessionStatus::Pending);
        assert_eq!(session.workflow_name, "Test");
    }

    #[test]
    fn test_advance_auto_transition() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Next

  @state.middle
  Scenario: Middle
    Given: Started
    When: Process
    Then: Done

  @state.end
  Scenario: End
    Given: Processed
    When: Finish
    Then: Complete

  Transitions:
    start -> middle: auto
    middle -> end: auto
"#,
        );

        let mut session = engine.create_session("task-1", serde_json::json!({}));
        assert_eq!(session.current_state, "start");

        let outcome = StepOutcome {
            state: "start".into(),
            agent_session_id: "agent-1".into(),
            success: true,
            eval_score: Some(0.9),
            next_state: None,
            error: None,
                summary: None,
        };

        let next = engine.advance(&mut session, outcome).unwrap();
        assert_eq!(next, Some("middle".to_string()));
        assert_eq!(session.current_state, "middle");

        let outcome2 = StepOutcome {
            state: "middle".into(),
            agent_session_id: "agent-2".into(),
            success: true,
            eval_score: Some(0.85),
            next_state: None,
            error: None,
                summary: None,
        };

        let next2 = engine.advance(&mut session, outcome2).unwrap();
        assert_eq!(next2, Some("end".to_string()));
        assert_eq!(session.current_state, "end");
    }

    #[test]
    fn test_terminal_state_done() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.done
  Scenario: Complete
    Given: Done
    When: Finish
    Then: End
"#,
        );

        let mut session = engine.create_session("task-1", serde_json::json!({}));
        // Advance to done
        session.current_state = "done".to_string();

        let outcome = StepOutcome {
            state: "done".into(),
            agent_session_id: "agent-1".into(),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
                summary: None,
        };

        let next = engine.advance(&mut session, outcome).unwrap();
        assert!(next.is_none());
        assert_eq!(session.status, SessionStatus::Completed);
    }

    #[test]
    fn test_wildcard_transition() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.done
  Scenario: Done
    Given: Completed
    When: Finish
    Then: End

  @state.failed
  Scenario: Failed
    Given: Error
    When: Report
    Then: End

  Transitions:
    start -> done: auto
    "*" -> failed: manual (failed)
"#,
        );

        let mut session = engine.create_session("task-1", serde_json::json!({}));

        let failed_outcome = StepOutcome {
            state: "start".into(),
            agent_session_id: "agent-1".into(),
            success: false,
            eval_score: None,
            next_state: None,
            error: Some("Something went wrong".into()),
                summary: None,
        };

        let next = engine.advance(&mut session, failed_outcome).unwrap();
        assert_eq!(next, Some("failed".to_string()));
    }

    #[test]
    fn test_assignee_lookup() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.analyzing
  Scenario: Analyze
    Given: Input
    When: Process
    Then: Output

  @state.implementing
  Scenario: Implement
    Given: Plan
    When: Code
    Then: Done

  Assignee:
    analyzing: architect
    implementing: builder
"#,
        );

        let session = engine.create_session("task-1", serde_json::json!({}));
        assert_eq!(engine.current_assignee(&session), Some("architect"));

        let scenario = engine.current_scenario(&session).unwrap();
        assert_eq!(scenario.state, "analyzing");
    }

    #[test]
    fn test_valid_transitions() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.middle
  Scenario: Middle
    Given: Started
    When: Process
    Then: Done

  @state.end
  Scenario: End
    Given: Done
    When: Finish
    Then: Complete

  Transitions:
    start -> middle: auto
    start -> end: manual (skip)
    "*" -> end: manual (abort)
"#,
        );

        let transitions = engine.valid_transitions("start");
        assert!(transitions.contains(&"middle".to_string()));
        assert!(transitions.contains(&"end".to_string()));
    }

    #[test]
    fn test_validation_detects_orphans() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.orphan
  Scenario: Orphan
    Given: Nothing
    When: Nothing
    Then: Nothing

  @state.end
  Scenario: End
    Given: Done
    When: Finish
    Then: Complete

  Transitions:
    start -> end: auto
"#,
        );

        let warnings = engine.validate();
        assert!(
            warnings.iter().any(|w| w.contains("orphan")),
            "Expected orphan warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_step_history() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  @state.end
  Scenario: End
    Given: Done
    When: Finish
    Then: Complete

  Transitions:
    start -> end: auto
"#,
        );

        let mut session = engine.create_session("task-1", serde_json::json!({}));

        let outcome = StepOutcome {
            state: "start".into(),
            agent_session_id: "agent-1".into(),
            success: true,
            eval_score: Some(0.95),
            next_state: None,
            error: None,
                summary: None,
        };

        engine.advance(&mut session, outcome).unwrap();
        assert_eq!(session.step_history.len(), 1);
        assert_eq!(session.step_history[0].eval_score, Some(0.95));
    }

    #[test]
    fn test_abort_session() {
        let engine = parse_and_create(
            r#"@workflow
Feature: Test
  Description: Test

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done
"#,
        );

        let mut session = engine.create_session("task-1", serde_json::json!({}));
        engine.abort(&mut session);
        assert_eq!(session.status, SessionStatus::Aborted);
    }
}
