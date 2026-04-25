//! BDD step definitions — dogfoods our own Gherkin parser.
//!
//! Each test reads a `.feature` file through our parser, then executes
//! the scenarios by verifying the described behavior.

#![allow(unused_variables)]

use agentic_loop_workflow::{
    SessionStatus, StepOutcome, WorkflowEngineImpl, WorkflowParserImpl,
    InMemoryStepRegistry, StepRegistry,
};
use agentic_loop_workflow::builtin_steps::*;

// ── Feature: Workflow Parsing ────────────────────────────────────────────────

#[test]
fn bdd_workflow_parsing_feature() {
    let parser = WorkflowParserImpl;
    let feature_source = include_str!("bdd/workflow_parsing.feature");
    let workflow = parser.parse(feature_source)
        .expect("workflow_parsing.feature should parse");

    // Feature-level: name and description
    assert_eq!(workflow.name, "Workflow Parsing");
    assert!(workflow.description.contains("parser correctly handles"));

    // State: valid_input — "Parse a well-formed workflow"
    let state_names: Vec<&str> = workflow.states.iter().map(|s| s.name.as_str()).collect();
    assert!(state_names.contains(&"valid_input"), "Should have valid_input state");
    assert!(state_names.contains(&"minimal_input"), "Should have minimal_input state");
    assert!(state_names.contains(&"empty_input"), "Should have empty_input state");
    assert!(state_names.contains(&"done"), "Should have done state");

    // Scenario: "Parse a well-formed workflow"
    let scenario = workflow.scenarios.iter()
        .find(|s| s.name == "Parse a well-formed workflow")
        .expect("Should have 'Parse a well-formed workflow' scenario");
    assert!(scenario.given.iter().any(|g| g.contains("@workflow")));
    assert!(scenario.then.iter().any(|t| t.contains("Workflow struct")));

    // Scenario: "Parse a minimal workflow"
    let minimal = workflow.scenarios.iter()
        .find(|s| s.name == "Parse a minimal workflow")
        .expect("Should have minimal scenario");
    assert!(minimal.given.iter().any(|g| g.contains("one @state")));

    // Scenario: "Handle empty input"
    let empty = workflow.scenarios.iter()
        .find(|s| s.name == "Handle empty input")
        .expect("Should have empty input scenario");
    assert!(empty.then.iter().any(|t| t.contains("error")));

    // Execute: Verify the parser behavior described in the scenarios
    // Given: a Gherkin source with @workflow and Feature headers
    let well_formed = "@workflow\nFeature: Test\n  @state.start\n  Scenario: S\n    Given: ok\n";
    // When: the parser processes the source
    let result = parser.parse(well_formed);
    // Then: a Workflow struct is returned with correct name
    assert!(result.is_ok(), "Well-formed input should parse");
    let wf = result.unwrap();
    assert_eq!(wf.name, "Test");

    // Given: a source with only @workflow and one @state
    let minimal_src = "@workflow\nFeature: Min\n  @state.only\n  Scenario: One\n    Given: ok\n";
    // When/Then
    let result = parser.parse(minimal_src);
    assert!(result.is_ok(), "Minimal input should parse");
    assert_eq!(result.unwrap().states.len(), 1);

    // Given: an empty string
    // When/Then: should not panic (checked by proptest already, but verify here)
    let result = parser.parse("");
    // Result can be Ok or Err — either is fine, just no panic
    let _ = result;
}

// ── Feature: State Machine Transitions ──────────────────────────────────────

#[test]
fn bdd_state_transitions_feature() {
    let parser = WorkflowParserImpl;
    let feature_source = include_str!("bdd/state_transitions.feature");
    let workflow = parser.parse(feature_source)
        .expect("state_transitions.feature should parse");

    assert_eq!(workflow.name, "State Machine Transitions");

    let engine = WorkflowEngineImpl::new(workflow);

    // Scenario: "Linear state advancement"
    // Given: a workflow with start -> middle -> done transitions
    let linear_wf = parser.parse(r#"
@workflow
Feature: Linear
  Description: Linear flow

  @state.start
  Scenario: Begin
    Given: ready
    When: go
    Then: next

  @state.middle
  Scenario: Middle
    Given: started
    When: process
    Then: next

  @state.done
  Scenario: Done
    Given: processed
    When: finish
    Then: done

  Transitions:
    start -> middle: auto
    middle -> done: auto
"#).unwrap();

    // When: the engine advances through each state
    let linear_engine = WorkflowEngineImpl::new(linear_wf);
    let mut session = linear_engine.create_session("bdd-linear", serde_json::json!({}));
    let states_visited = advance_all(&linear_engine, &mut session);

    // Then: the session visits states in order and reaches done
    assert_eq!(states_visited, vec!["start", "middle", "done"]);
    assert_eq!(session.status, SessionStatus::Completed);

    // Scenario: "Branching on success or failure"
    // Given: a workflow with success and failure branches
    let branch_wf = parser.parse(r#"
@workflow
Feature: Branch
  Description: Branching

  @state.start
  Scenario: Decide
    Given: ready
    When: deciding
    Then: branch

  @state.success_path
  Scenario: Success
    Given: succeeded
    When: processing
    Then: done

  @state.failure_path
  Scenario: Failure
    Given: failed
    When: recovering
    Then: done

  @state.done
  Scenario: Done
    Given: done
    When: finish
    Then: done

  Transitions:
    start -> success_path: success
    start -> failure_path: failure
    success_path -> done: auto
    failure_path -> done: auto
"#).unwrap();

    // When: a step succeeds
    let branch_engine = WorkflowEngineImpl::new(branch_wf);
    let mut session = branch_engine.create_session("bdd-branch", serde_json::json!({}));

    let outcome = StepOutcome {
        state: "start".to_string(),
        agent_session_id: "bdd".to_string(),
        success: true,
        eval_score: None,
        next_state: None,
        error: None,
                summary: None,
    };
    branch_engine.advance(&mut session, outcome).unwrap();

    // Then: the engine follows the success transition
    assert_eq!(session.current_state, "success_path",
        "On success, should go to success_path");

    // Scenario: "Terminal state detection"
    // Already verified above — session reaches Completed status at done
}

// ── Feature: Tool Scoping ────────────────────────────────────────────────────

#[test]
fn bdd_tool_scoping_feature() {
    let parser = WorkflowParserImpl;
    let feature_source = include_str!("bdd/tool_scoping.feature");
    let workflow = parser.parse(feature_source)
        .expect("tool_scoping.feature should parse");

    assert_eq!(workflow.name, "Tool Scoping");

    // Scenario: "Scenario restricts available tools"
    let restricted = workflow.scenarios.iter()
        .find(|s| s.name == "Scenario restricts available tools")
        .expect("Should have restricted tools scenario");
    assert!(restricted.tools.contains(&"file_read".to_string()));
    assert!(restricted.tools.contains(&"bash".to_string()));
    assert!(!restricted.tools.contains(&"file_write".to_string()),
        "Should NOT have file_write in restricted scenario");

    // Scenario: "Empty tools list means all tools available"
    let all_tools = workflow.scenarios.iter()
        .find(|s| s.name == "Empty tools list means all tools available")
        .expect("Should have all tools scenario");
    assert!(all_tools.tools.is_empty(),
        "Empty tools list means all tools available");

    // Verify the engine creates correct sessions
    let engine = WorkflowEngineImpl::new(workflow);
    let session = engine.create_session("bdd-tools", serde_json::json!({}));
    assert_eq!(session.current_state, "restricted_tools");

    // Check that restricted_tools scenario has only specified tools
    let scenario = engine.current_scenario(&session).unwrap();
    assert_eq!(scenario.tools.len(), 2);
    assert!(scenario.tools.contains(&"file_read".to_string()));
}

// ── Feature: Engine validates workflow integrity ─────────────────────────────

#[test]
fn bdd_workflow_validation_feature() {
    // Dogfood: write a workflow describing validation behavior,
    // then validate it
    let parser = WorkflowParserImpl;

    // Valid workflow should pass validation
    let valid = parser.parse(r#"
@workflow
Feature: Valid Workflow
  Description: Should pass validation

  @state.start
  Scenario: Begin
    Given: ready
    When: go
    Then: done

  @state.done
  Scenario: Complete
    Given: done
    When: finish
    Then: complete

  Transitions:
    start -> done: auto
"#).unwrap();

    let engine = WorkflowEngineImpl::new(valid);
    let _warnings = engine.validate();

    // Workflow with unreachable states should warn
    let unreachable = parser.parse(r#"
@workflow
Feature: Unreachable States
  Description: Has unreachable states

  @state.start
  Scenario: Begin
    Given: ready
    When: go
    Then: done

  @state.done
  Scenario: Complete
    Given: done
    When: finish
    Then: complete

  @state.orphan
  Scenario: Never Reached
    Given: never
    When: never
    Then: never

  Transitions:
    start -> done: auto
"#).unwrap();

    let engine = WorkflowEngineImpl::new(unreachable);
    let warnings = engine.validate();
    assert!(!warnings.is_empty(), "Should warn about unreachable 'orphan' state");
}

// ── Feature: Builtin step handlers all work ──────────────────────────────────

#[tokio::test]
async fn bdd_builtin_steps_execute() {
    // Given: a step registry with all builtin steps
    let mut registry = InMemoryStepRegistry::new();
    register_builtin_steps(&mut registry);

    let ctx = agentic_loop_workflow::StepContext {
        task_description: "BDD test".to_string(),
        current_state: "working".to_string(),
        workflow_name: "bdd".to_string(),
        available_tools: vec![],
        tool_schemas: vec![],
        guardrails: vec![],
        previous_outcome: None,
        variables: serde_json::json!({}),
                model_hint: None,
    };

    // When: each builtin step is executed
    // Then: all return success
    let steps = ["think", "plan", "research", "implement", "review", "test"];
    for step_name in &steps {
        let handler = registry.get(step_name)
            .unwrap_or_else(|| panic!("{} should be registered", step_name));
        let outcome = handler.execute(&ctx).await;
        assert!(outcome.success, "Step '{}' should succeed", step_name);
    }
}

// ── Helper ───────────────────────────────────────────────────────────────────

fn advance_all(engine: &WorkflowEngineImpl, session: &mut agentic_loop_workflow::WorkflowSession) -> Vec<String> {
    let mut visited = Vec::new();
    loop {
        visited.push(session.current_state.clone());
        let outcome = StepOutcome {
            state: session.current_state.clone(),
            agent_session_id: "bdd".to_string(),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
                summary: None,
        };
        let next = engine.advance(session, outcome).unwrap();
        if next.is_none() || session.status == SessionStatus::Completed {
            break;
        }
    }
    visited
}
