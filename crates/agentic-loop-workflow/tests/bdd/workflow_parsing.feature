@workflow
Feature: Workflow Parsing
  Description: The parser correctly handles various Gherkin inputs

  @state.valid_input
  Scenario: Parse a well-formed workflow
    Given: a Gherkin source with @workflow and Feature headers
    When: the parser processes the source
    Then: a Workflow struct is returned with correct name, states, and transitions

  @state.minimal_input
  Scenario: Parse a minimal workflow
    Given: a source with only @workflow and one @state
    When: the parser processes it
    Then: a Workflow with one state is returned

  @state.empty_input
  Scenario: Handle empty input
    Given: an empty string
    When: the parser processes it
    Then: an error is returned without panicking

  @state.done
  Scenario: Complete
    Given: all parse scenarios pass
    When: finishing
    Then: parsing behavior verified

  Transitions:
    valid_input -> minimal_input: auto
    minimal_input -> empty_input: auto
    empty_input -> done: auto
