@workflow
Feature: Tool Scoping
  Description: Scenarios control which tools are available

  @state.restricted_tools
  Scenario: Scenario restricts available tools
    Given: a scenario listing only file_read and bash as tools
    When: the runner loads the scenario
    Then: only file_read and bash are available (plus control tools)
    Tools:
      file_read
      bash

  @state.all_tools
  Scenario: Empty tools list means all tools available
    Given: a scenario with no tools specified
    When: the runner loads the scenario
    Then: all registered tools are available

  @state.done
  Scenario: Complete
    Given: all tool scoping scenarios pass
    When: finishing
    Then: tool scoping verified

  Transitions:
    restricted_tools -> all_tools: auto
    all_tools -> done: auto
