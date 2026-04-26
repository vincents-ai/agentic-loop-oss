@workflow
Feature: Testing Workflow
  Description: Run tests and verify code quality

  @state.pending
  Scenario: Pick up test task
    Given: Test task exists
    When: Load test files
    Then: Move to "testing" state

  @state.testing
  Scenario: Run test suite
    Given: Test files loaded
    When: Execute test command
    Then: Capture results
    Then: Move to "analyzing" state

  @state.analyzing
  Scenario: Analyze results
    Given: Test results
    When: Check pass/fail status
    Then: Check coverage threshold
    Then: Move to "complete" or "fixing" state

  @state.fixing
  Scenario: Fix test failures
    Given: Tests failed
    When: Identify fix needed
    Then: Move to implementing (external)
    Then: Wait for fix

  @state.complete
  Scenario: Complete testing
    Given: Tests passing
    When: Create Reasoning with summary
    Then: Move to "complete" state

  Assignee:
    pending: tester
    testing: tester
    analyzing: tester
    fixing: builder
    complete: tester

  Guardrails:
    testing:
      command: cargo test --summary
      valid: tests_passed > 0
    analyzing:
      command: cargo llvm-cov --summary
      valid: coverage >= 80

  References:
    - entity: ExecutionResult
      action: create
    - entity: Reasoning
      action: create

  Tools:
    done:
      usage: done --task-id {{task_id}} --result "summary"
    failed:
      usage: failed --task-id {{task_id}} --reason "reason"

  Transitions:
    pending -> testing: auto
    testing -> analyzing: auto
    analyzing -> complete: tests_passed AND coverage_met
    analyzing -> fixing: tests_failed OR coverage_low
    fixing -> testing: fix_complete
    "*" -> complete: done called