@workflow
Feature: Workflow Creation Workflow
  As a workflow author
  We want to create new workflows through defined states
  So that new processes can be defined

  # ============================================================================
  # STATE: DRAFT
  # Assigned to: Architect (creates draft)
  # ============================================================================

  @state.draft
  Scenario: Start workflow draft
    Given: A need for a new workflow
    When: Architect creates a draft
    Then: Draft is created in workflows directory
    Assignee: architect
    Transition: manual

  @state.draft
  Scenario: Add workflow metadata
    Given: Draft exists
    When: Architect adds name, description, version
    Then: Metadata is set
    Assignee: architect
    Transition: manual

  # ============================================================================
  # STATE: DEFINING
  # Assigned to: Deconstructor (defines states)
  # ============================================================================

  @state.defining
  Scenario: Define workflow states
    Given: Metadata is set
    When: Deconstructor defines states
    Then: States have clear transitions
    Assignee: deconstructor
    Transition: manual

  @state.defining
  Scenario: Define initial state
    Given: States are defined
    When: Deconstructor sets initial state
    Then: Workflow starts from correct state
    Assignee: deconstructor
    Transition: manual

  @state.defining
  Scenario: Define final states
    Given: States are defined
    When: Deconstructor marks final states
    Then: Workflow knows when complete
    Assignee: deconstructor
    Transition: manual

  # ============================================================================
  # STATE: SCENARIOS
  # Assigned to: Coder (writes scenarios)
  # ============================================================================

  @state.scenarios
  Scenario: Write feature scenarios
    Given: States are defined
    When: Coder writes Gherkin scenarios
    Then: Each scenario has Given/When/Then
    Assignee: coder
    Transition: manual

  @state.scenarios
  Scenario: Add agent assignments
    Given: Scenarios are written
    When: Coder adds Assignee per step
    Then: Each step has clear ownership
    Assignee: coder
    Transition: manual

  # ============================================================================
  # STATE: TESTING
  # Assigned to: Tester (validates workflow)
  # ============================================================================

  @state.testing
  Scenario: Parse workflow
    Given: Workflow is written
    When: Tester parses the Gherkin
    Then: No syntax errors
    Assignee: tester
    Transition: auto

  @state.testing
  Scenario: Validate transitions
    Given: Workflow parses
    When: Tester validates transitions
    Then: All transitions are valid
    Assignee: tester
    Transition: manual

  @state.testing
  Scenario: Test dry run
    Given: Workflow is valid
    When: Tester runs a dry run
    Then: Workflow executes correctly
    Assignee: tester
    Transition: manual

  # ============================================================================
  # STATE: PUBLISHED
  # Assigned to: Architect (publishes)
  # ============================================================================

  @state.published
  Scenario: Publish workflow
    Given: Testing passes
    When: Architect moves to published workflows
    Then: Workflow is available
    Assignee: architect
    Transition: manual