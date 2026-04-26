@workflow
Feature: Quick Task
  Description: Single-step task execution — do the work and call done.

  @state.working
  Scenario: Do the work
    Model: fast,free
    Given: Task description provided
    When: Agent completes the assigned task
    Then: Call done when finished
    Tools:
      file_read:
      file_write:
      file_edit:
      file_ls:
      bash:
      done:
      failed:

  @state.done
  Scenario: Complete
    Given: Task completed

  Assignee:
    working: coder

  Transitions:
    working -> done: auto
