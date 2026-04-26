@workflow
Feature: Code Review Workflow
  As a review team
  We want to review code through defined states
  So that every review has clear ownership

  # ============================================================================
  # STATE: PENDING REVIEW
  # Assigned to: Reviewer (picks up review)
  # ============================================================================

  @state.pending
  Scenario: Pick up review task
    Given: A task is pending review
    When: Reviewer loads the changes
    Then: Changes are loaded
    And: Task moves to "reviewing" state
    Assignee: reviewer
    Transition: auto

  # ============================================================================
  # STATE: REVIEWING
  # Assigned to: Reviewer (performs review)
  # ============================================================================

  @state.reviewing
  Scenario: Check code quality
    Given: Changes are loaded
    When: Reviewer checks for code quality
    Then: Quality findings are documented
    Assignee: reviewer

  @state.reviewing
  Scenario: Check security
    Given: Changes are loaded
    When: Reviewer checks for security issues
    Then: Security findings are documented
    Assignee: reviewer

  @state.reviewing
  Scenario: Check patterns
    Given: Changes are loaded
    When: Reviewer checks adherence to patterns
    Then: Pattern findings are documented
    Assignee: reviewer

  @state.reviewing
  Scenario: Check tests
    Given: Changes are loaded
    When: Reviewer verifies test coverage
    Then: Test findings are documented
    Assignee: reviewer

  # ============================================================================
  # STATE: COMPLETE
  # Assigned to: Reviewer (summarizes)
  # ============================================================================

  @state.complete
  Scenario: Complete review
    Given: All checks are done
    When: Reviewer provides summary
    Then: Task moves to "approved" or "changes_requested"
    And: Review is stored in engram
    Assignee: reviewer
    Transition: manual