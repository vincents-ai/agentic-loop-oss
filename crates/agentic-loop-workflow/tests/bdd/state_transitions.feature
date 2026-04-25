@workflow
Feature: State Machine Transitions
  Description: The engine correctly advances through states

  @state.linear_flow
  Scenario: Linear state advancement
    Given: a workflow with start -> middle -> done transitions
    When: the engine advances through each state
    Then: the session visits states in order and reaches done

  @state.branching_flow
  Scenario: Branching on success or failure
    Given: a workflow with success and failure branches
    When: a step succeeds
    Then: the engine follows the success transition

  @state.terminal_state
  Scenario: Terminal state detection
    Given: a session in the done state
    When: the engine advances
    Then: the session status becomes Completed

  @state.done
  Scenario: Complete
    Given: all transition scenarios pass
    When: finishing
    Then: transition behavior verified

  Transitions:
    linear_flow -> branching_flow: auto
    branching_flow -> terminal_state: auto
    terminal_state -> done: auto
