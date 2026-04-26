@workflow
Feature: Research Workflow
  Description: Research topics and gather information

  @state.receiving
  Scenario: Receive research topic
    Given: Research topic provided
    When: Load topic into context
    Then: Move to "searching" state

  @state.searching
  Scenario: Search for information
    Given: Topic loaded
    When: Search external sources
    Then: Collect results
    Then: Move to "analyzing" state

  @state.analyzing
  Scenario: Analyze findings
    Given: Search results
    When: Deduplicate against existing context
    Then: Filter redundant items
    Then: Move to "synthesizing" state

  @state.synthesizing
  Scenario: Synthesize findings
    Given: Deduplicated findings
    When: Cross-reference with project Theory
    Then: Create Reasoning entity
    Then: Move to "complete" state

  @state.complete
  Scenario: Complete research
    Given: Synthesis complete
    When: Create final Reasoning
    Then: Move to "complete" state

  Assignee:
    receiving: researcher
    searching: scout
    analyzing: librarian
    synthesizing: synthesizer
    complete: researcher

  References:
    - entity: Context
      action: create
    - entity: Reasoning
      action: create
      type: research_findings
    - entity: Relationship
      action: create

  Tools:
    done:
      usage: done --task-id {{task_id}} --result "summary"
    failed:
      usage: failed --task-id {{task_id}} --reason "reason"
    escalate:
      usage: escalate --task-id {{task_id}} --reason "reason"

  Transitions:
    receiving -> searching: auto
    searching -> analyzing: auto
    analyzing -> synthesizing: auto
    synthesizing -> complete: auto