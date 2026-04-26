@workflow
Feature: Content Discovery & Ideation Workflow
  As a content team
  We want to discover trends and generate ideas through defined states
  So that every step has clear ownership

  # ============================================================================
  # STATE: SCOUTING
  # Assigned to: Scout (monitors external APIs)
  # ============================================================================

  @state.scouting
  Scenario: Monitor external APIs
    Given: No active monitoring
    When: Scout monitors Tavily/SerpAPI for spikes
    Then: Trend data is captured
    Assignee: scout
    Transition: auto

  @state.scouting
  Scenario: Monitor GitHub trends
    Given: No active monitoring
    When: Scout checks trending repositories
    Then: Related projects captured
    Assignee: scout
    Transition: manual

  @state.scouting
  Scenario: Publish raw data
    Given: Trend data captured
    When: Scout pushes to engram context
    Then: Data is available for Librarian
    Assignee: scout
    Transition: auto

  # ============================================================================
  # STATE: DEDUPLICATING
  # Assigned to: Librarian (filters duplicates)
  # ============================================================================

  @state.deduplicating
  Scenario: Remove duplicates
    Given: Raw data from Scout
    When: Librarian searches engram for similar context
    Then: Redundant items filtered out
    Assignee: librarian
    Transition: auto

  @state.deduplicating
  Scenario: Identify decayed ideas
    Given: Existing context in engram
    When: Librarian identifies decayed trends
    Then: Decayed items flagged
    Assignee: librarian
    Transition: manual

  # ============================================================================
  # STATE: SYNTHESIZING
  # Assigned to: Synthesizer (cross-references)
  # ============================================================================

  @state.synthesizing
  Scenario: Cross-reference with Theory
    Given: Deduplicated data
    When: Synthesizer compares to project Theory
    Then: Why-to-How connections created
    Assignee: synthesizer
    Transition: manual

  @state.synthesizing
  Scenario: Create reasoning
    Given: Connections made
    When: Synthesizer documents reasoning
    Then: Reasoning stored in engram
    Assignee: synthesizer
    Transition: manual

  # ============================================================================
  # STATE: STRATEGIZING
  # Assigned to: Strategist (creates tasks)
  # ============================================================================

  @state.strategizing
  Scenario: Generate content hooks
    Given: Reasoning stored
    When: Strategist creates actionable briefs
    Then: Output to ideas.json
    Assignee: strategist
    Transition: manual

  @state.strategizing
  Scenario: Create engram tasks
    Given: Briefs generated
    When: Strategist creates tasks in engram
    Then: Tasks ready for execution
    Assignee: strategist
    Transition: auto

  # ============================================================================
  # STATE: AUDITING
  # Assigned to: Auditor (optional - checks alignment)
  # ============================================================================

  @state.auditing
  Scenario: Check cognitive dissonance
    Given: Content directions exist
    When: Auditor compares to audience performance
    Then: Misaligned items flagged
    Assignee: auditor
    Transition: manual

  @state.auditing
  Scenario: Generate report
    Given: Audit complete
    When: Auditor provides alignment report
    Then: Report stored in engram
    Assignee: auditor
    Transition: manual