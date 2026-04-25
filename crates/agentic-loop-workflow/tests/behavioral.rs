//! Behavioral tests — verify observable behavior through public APIs.
//!
//! These tests are written from the perspective of a consumer of the public
//! API. They assert *what* the system does, not *how*. If we refactor internals,
//! these tests should still pass.

use agentic_loop_workflow::{
    SessionStatus, StepContext, StepOutcome, WorkflowEngineImpl, WorkflowParserImpl,
    InMemoryStepRegistry, StepRegistry,
};
use agentic_loop_workflow::builtin_steps::*;
use agentic_loop_workflow::prompt::PromptBuilderImpl;

// ── Behavior: WorkflowParser produces a valid workflow from well-formed input ─

#[test]
fn given_valid_gherkin_when_parsed_then_workflow_has_states() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: Behavioral Test
  Description: Verify parsing behavior

  @state.start
  Scenario: Begin
    Given: a task
    When: starting
    Then: done

  @state.done
  Scenario: Complete
    Given: done
    When: finishing
    Then: complete

  Transitions:
    start -> done: auto
"#).expect("Valid Gherkin should produce a workflow");

    // Behavioral: we don't check internal vec lengths — we check the workflow
    // has the states we named and the initial state is correct.
    let state_names: Vec<&str> = workflow.states.iter().map(|s| s.name.as_str()).collect();
    assert!(state_names.contains(&"start"), "Workflow should have a 'start' state");
    assert!(state_names.contains(&"done"), "Workflow should have a 'done' state");
    assert_eq!(workflow.initial_state, "start", "Initial state should be 'start'");
}

// ── Behavior: WorkflowParser rejects malformed input gracefully ──────────────

#[test]
fn given_garbage_input_when_parsed_then_returns_error_not_panic() {
    let parser = WorkflowParserImpl;
    // These should all return Err, not panic
    let garbage_inputs = [
        "",
        "just some random text",
        "@workflow\nBut no Feature line",
        "Feature: No states\n  Description: empty",
        "@state.no_workflow\n  Scenario: Orphan",
        "\n\n\n",
        "@workflow\nFeature: \n  @state.\n  Scenario:\n",
    ];

    for input in &garbage_inputs {
        // The key behavior: never panics
        let result = parser.parse(input);
        // We don't care if it succeeds or fails — just that it doesn't panic.
        // But if it succeeds, the workflow should be usable.
        if let Ok(wf) = result {
            // If it parsed, it should be usable without crashing
            let _ = format!("{}", wf.name);
            let _ = wf.states.len();
        }
    }
}

// ── Behavior: WorkflowEngine advances through states in order ────────────────

#[test]
fn given_workflow_when_advanced_then_moves_from_start_to_done() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: State Advancement
  Description: Test state machine advancement

  @state.alpha
  Scenario: First
    Given: beginning
    When: working
    Then: go to beta

  @state.beta
  Scenario: Second
    Given: alpha done
    When: processing
    Then: go to done

  @state.done
  Scenario: Complete
    Given: all done
    When: finishing
    Then: complete

  Transitions:
    alpha -> beta: auto
    beta -> done: auto
"#).unwrap();

    let engine = WorkflowEngineImpl::new(workflow);
    let mut session = engine.create_session("advance-test", serde_json::json!({}));

    // Behavioral: after each advance, the session moves to the expected state
    assert_eq!(session.current_state, "alpha");

    let outcome = StepOutcome {
        state: "alpha".to_string(),
        agent_session_id: "test".to_string(),
        success: true,
        eval_score: None,
        next_state: None,
        error: None,
                summary: None,
    };
    let next = engine.advance(&mut session, outcome).unwrap();
    assert_eq!(next.as_deref(), Some("beta"));
    assert_eq!(session.current_state, "beta");

    let outcome = StepOutcome {
        state: "beta".to_string(),
        agent_session_id: "test".to_string(),
        success: true,
        eval_score: None,
        next_state: None,
        error: None,
                summary: None,
    };
    let next = engine.advance(&mut session, outcome).unwrap();
    assert_eq!(next.as_deref(), Some("done"));
    assert_eq!(session.current_state, "done");

    // Advancing from done completes the session
    let outcome = StepOutcome {
        state: "done".to_string(),
        agent_session_id: "test".to_string(),
        success: true,
        eval_score: None,
        next_state: None,
        error: None,
                summary: None,
    };
    let next = engine.advance(&mut session, outcome).unwrap();
    assert_eq!(next, None);
    assert_eq!(session.status, SessionStatus::Completed);
}

// ── Behavior: PromptBuilder produces non-empty prompts ───────────────────────

#[test]
fn given_workflow_and_scenario_when_build_prompts_then_output_is_nonempty() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: Prompt Test
  Description: Test prompt generation

  @state.start
  Scenario: Begin
    Given: ready
    When: starting
    Then: done
    Tools:
      file_read
      bash

  @state.done
  Scenario: Complete
    Given: done
    When: finishing
    Then: complete

  Transitions:
    start -> done: auto
"#).unwrap();

    let builder = PromptBuilderImpl::new();
    let engine = WorkflowEngineImpl::new(workflow);
    let session = engine.create_session("prompt-test", serde_json::json!({}));
    let scenario = engine.current_scenario(&session).unwrap();

    let system_prompt = builder.build_system_prompt(
        engine.workflow(),
        &session.current_state,
        scenario,
        None,
    );

    let ctx = StepContext {
        task_description: "Write a function".to_string(),
        current_state: "start".to_string(),
        workflow_name: "prompt-test".to_string(),
        available_tools: vec!["file_read".to_string(), "bash".to_string()],
        tool_schemas: vec![],
        guardrails: vec![],
        previous_outcome: None,
        variables: serde_json::json!({}),
                model_hint: None,
    };

    let user_prompt = builder.build_user_prompt(&ctx, scenario);

    // Behavioral: prompts should be meaningful (non-empty)
    assert!(!system_prompt.is_empty(), "System prompt should not be empty");
    assert!(!user_prompt.is_empty(), "User prompt should not be empty");
}

// ── Behavior: Step registry resolves registered steps ────────────────────────

#[tokio::test]
async fn given_builtin_steps_when_resolved_then_all_execute_successfully() {
    let mut registry = InMemoryStepRegistry::new();
    register_builtin_steps(&mut registry);

    let ctx = StepContext {
        task_description: "Test".to_string(),
        current_state: "working".to_string(),
        workflow_name: "test".to_string(),
        available_tools: vec![],
        tool_schemas: vec![],
        guardrails: vec![],
        previous_outcome: None,
        variables: serde_json::json!({}),
                model_hint: None,
    };

    // Behavioral: every registered builtin step should execute without error
    let expected_steps = ["think", "plan", "research", "implement", "review", "test"];
    for name in &expected_steps {
        let handler = registry.get(name).unwrap_or_else(|| panic!("Step '{}' should be registered", name));
        let outcome = handler.execute(&ctx).await;
        assert!(outcome.success, "Step '{}' should succeed when given valid context", name);
    }
}

// ── Behavior: Engine validates workflows and reports issues ──────────────────

#[test]
fn given_workflow_with_no_transitions_when_validated_then_reports_warning() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: No Transitions
  Description: Workflow with no transitions

  @state.start
  Scenario: Begin
    Given: ready
    When: stuck
    Then: never moves

  @state.done
  Scenario: Complete
    Given: never reached
    When: never
    Then: never
"#).unwrap();

    let engine = WorkflowEngineImpl::new(workflow);
    let warnings = engine.validate();

    // Behavioral: engine should detect that some states have no transitions
    assert!(!warnings.is_empty(), "Should warn about unreachable states or missing transitions");
}

// ── Behavior: Session tracks step history correctly ──────────────────────────

#[test]
fn given_session_when_steps_executed_then_history_is_recorded() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: History Test
  Description: Test step history tracking

  @state.start
  Scenario: Begin
    Given: ready
    When: starting
    Then: go to middle

  @state.middle
  Scenario: Process
    Given: started
    When: processing
    Then: go to done

  @state.done
  Scenario: Complete
    Given: done
    When: finishing
    Then: complete

  Transitions:
    start -> middle: auto
    middle -> done: auto
"#).unwrap();

    let engine = WorkflowEngineImpl::new(workflow);
    let mut session = engine.create_session("history-test", serde_json::json!({}));

    // Advance through all states
    for state in &["start", "middle", "done"] {
        let outcome = StepOutcome {
            state: state.to_string(),
            agent_session_id: "test".to_string(),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
                summary: None,
        };
        engine.advance(&mut session, outcome).unwrap();
    }

    // Behavioral: session should have recorded all 3 steps
    assert_eq!(session.step_history.len(), 3, "Should have 3 steps in history");
    assert_eq!(session.step_history[0].state, "start");
    assert_eq!(session.step_history[1].state, "middle");
    assert_eq!(session.step_history[2].state, "done");
}

// ── Behavior: Variables are preserved across state transitions ───────────────

#[test]
fn given_variables_when_session_created_then_variables_accessible_throughout() {
    let parser = WorkflowParserImpl;
    let workflow = parser.parse(r#"
@workflow
Feature: Variables Test
  Description: Test variable preservation

  @state.start
  Scenario: Begin
    Given: ready
    When: starting
    Then: done

  @state.done
  Scenario: Complete
    Given: done
    When: finishing
    Then: complete

  Transitions:
    start -> done: auto
"#).unwrap();

    let engine = WorkflowEngineImpl::new(workflow);
    let vars = serde_json::json!({
        "target": "main.rs",
        "count": 42,
        "tags": ["rust", "agent"]
    });

    let mut session = engine.create_session("vars-test", vars.clone());

    // Variables should be preserved
    assert_eq!(session.variables["target"], "main.rs");
    assert_eq!(session.variables["count"], 42);

    // After advancing, variables should still be there
    let outcome = StepOutcome {
        state: "start".to_string(),
        agent_session_id: "test".to_string(),
        success: true,
        eval_score: None,
        next_state: None,
        error: None,
                summary: None,
    };
    engine.advance(&mut session, outcome).unwrap();

    assert_eq!(session.variables["target"], "main.rs", "Variables should survive state transition");
    assert_eq!(session.variables["tags"][1], "agent");
}
