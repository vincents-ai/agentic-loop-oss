@workflow
Feature: Planning Workflow
  As a planning team
  We want to create plans through defined states
  So that every planning step has clear ownership

  # ============================================================================
  # STATE: RECEIVING
  # Assigned to: Architect (receives requirements)
  # ============================================================================

  @state.receiving
  Scenario: Receive requirements
    Given: Requirements are provided
    When: Architect loads them
    Then: Requirements are in context
    And: Task moves to "analyzing" state
    Assignee: architect
    Transition: auto

  # ============================================================================
  # STATE: ANALYZING
  # Assigned to: Architect (analyzes scope)
  # ============================================================================

  @state.analyzing
  Scenario: Analyze scope
    Given: Requirements loaded
    When: Architect analyzes scope and complexity
    Then: Scope is understood
    Assignee: architect
    Transition: manual

  @state.analyzing
  Scenario: Identify constraints
    Given: Requirements analyzed
    When: Architect identifies constraints
    Then: Constraints are documented
    Assignee: architect
    Transition: manual

  # ============================================================================
  # STATE: DECOMPOSING
  # Assigned to: Deconstructor (breaks into tasks)
  # ============================================================================

  @state.decomposing
  Scenario: Decompose into tasks
    Given: Analysis complete
    When: Deconstructor breaks into atomic tasks
    Then: Each task has acceptance criteria
    Assignee: deconstructor
    Transition: manual

  @state.decomposing
  Scenario: Identify dependencies
    Given: Tasks are defined
    When: Deconstructor identifies dependencies
    Then: Tasks are ordered correctly
    Assignee: deconstructor
    Transition: manual

  # ============================================================================
  # STATE: CREATING
  # Assigned to: TaskPlanner (creates engram tasks)
  # ============================================================================

  @state.creating
  Scenario: Create engram tasks
    Given: Tasks are ordered
    When: TaskPlanner creates each task in engram
    Then: Tasks are linked to parent
    And: Each task has assignee
    Assignee: task-planner
    Transition: auto

  @state.creating
  Scenario: Document reasoning
    Given: Tasks are created
    When: TaskPlanner documents decisions
    Then: Reasoning stored in engram
    Assignee: task-planner
    Transition: manual

  # ============================================================================
  # STATE: COMPLETE
  # Assigned to: Architect (signs off)
  # ============================================================================

  @state.complete
  Scenario: Complete planning
    Given: Tasks are created
    When: Architect reviews the plan
    Then: Plan moves to "ready" state
    And: Summary provided
    Assignee: architect
    Transition: manual