@workflow
Feature: Implementation Workflow
  Description: Implements code changes with workspace isolation and auto-merge

  # ============================================================================
  # WORKSPACE INTEGRATION - Git worktree per task
  # ============================================================================

  @state.todo
  Scenario: Pick up implementation task
    Given: Engram Task exists with status "todo"
    When: Architect reads requirements from engram context
    Then: Move to "analyzing" state

  # ============================================================================
  # WORKSPACE: CREATE
  # ============================================================================

  @state.creating_workspace
  Scenario: Create isolated workspace
    Given: Task is being worked on
    When: Create git worktree for this task:
      - Clone from repo_url
      - Create new branch: tasks/{{task_id}}
      - Path: {{path_strategy}}/{{task_id}}
    Then: Workspace created in "ready" state
    Then: Move to "analyzing" state
    Tools:
      workspace_create:
        Usage: workspace_create --repo-url {{repo_url}} --task-id {{task_id}} --base-branch {{branch}}
      bash:

  # ============================================================================
  # STATES
  # ============================================================================

  @state.analyzing
  Scenario: Analyze codebase
    Given: Workspace is ready
    When: Architect examines code in workspace
    Then: Store analysis as Reasoning entity
    Then: Move to "planning" state
    Tools:
      file_read:
      file_ls:
      bash:
      done:
      failed:

  @state.planning
  Scenario: Create implementation plan
    Given: Analysis complete
    When: Deconstructor creates subtask list
    Then: Create Task entities as children
    Then: Move to "implementing" state
    Tools:
      file_read:
      file_ls:
      bash:
      done:
      failed:

  @state.implementing
  Scenario: Write code changes in workspace
    Given: Plan defined
    When: Builder modifies files in workspace
    Then: Run build guardrail
    Then: Move to "testing" or "escalated" state
    Tools:
      file_read:
      file_write:
      file_edit:
      file_ls:
      bash:
      done:
      failed:

  @state.testing
  Scenario: Run tests in workspace
    Given: Code is implemented
    When: Run test command in workspace:
      - cd {{workspace_path}}
      - cargo test
    Then: Capture test results
    Then: If pass, move to "reviewing"
    Then: If fail, move back to "implementing"
    Tools:
      bash:
      workspace_test:
        Usage: workspace_test --task-id {{task_id}}
      done:
      failed:

  Guardrails:
    testing:
      Command: cd {{workspace_path}} && cargo test
      Creates: ExecutionResult
      Valid: exit_code == 0

  @state.reviewing
  Scenario: Review code changes
    Given: Tests pass
    When: Reviewer examines workspace changes
    Then: Create review Reasoning
    Then: Move to "merging" state
    Tools:
      file_read:
      file_ls:
      bash:
      done:
      failed:

  # ============================================================================
  # WORKSPACE: MERGE
  # ============================================================================

  @state.merging
  Scenario: Merge changes to main
    Given: Review approved
    When: Auto-merge or manual merge:
      - If auto_merge: run tests again
      - Push branch
      - Create PR/MR
    Then: If success, move to "done"
    Then: If failure, rollback and move to "failed"
    Tools:
      bash:
      workspace_merge:
        Usage: workspace_merge --task-id {{task_id}} --auto true
      done:
      failed:

  Guardrails:
    merging:
      Command: cd {{workspace_path}} && cargo test && git push
      Creates: ExecutionResult
      Valid: exit_code == 0
      Error: "Merge failed, rolling back"

  # ============================================================================
  # WORKSPACE: CLEANUP
  # ============================================================================

  @state.done
  Scenario: Complete implementation
    Given: Merge successful
    When: Cleanup workspace:
      - Delete worktree
      - Delete branch (optional)
    Then: Store final Reasoning
    Then: Task complete
    Tools:
      bash:
      workspace_cleanup:
        Usage: workspace_cleanup --task-id {{task_id}}
      done:
      failed:

  @state.failed
  Scenario: Implementation failed
    Given: Max retries exhausted or unrecoverable error
    When: Cleanup workspace (keep branch for debugging)
    Then: Store failure reasoning
    Then: Move to "failed" state
    Tools:
      bash:
      failed:

  # ============================================================================
  # ASSIGNEE
  # ============================================================================
  Assignee:
    analyzing: architect
    planning: deconstructor
    implementing: builder
    testing: tester
    reviewing: reviewer
    merging: architect
    done: architect
    failed: architect

  # ============================================================================
  # REFERENCES
  # ============================================================================
  References:
    - entity: Task
      Required: true
    - entity: Workspace
      Action: create
      Type: git_worktree
    - entity: Context
      Required: true
    - entity: Reasoning
      Action: create
    - entity: ExecutionResult
      Action: create
    - entity: EscalationRequest
      Action: create

  # ============================================================================
  # WORKSPACE PATH STRATEGIES
  # ============================================================================
  Config:
    path_strategy: local | system | custom
    # local: ./workspaces/{{task_id}}
    # system: /var/lib/agentic-loop/workspaces/{{task_id}}
    # custom: {{base_path}}/{{task_id}}
    auto_merge: boolean
    test_command: string
    merge_target: string

  # ============================================================================
  # TRANSITIONS
  # ============================================================================
  Transitions:
    todo -> creating_workspace: auto
    creating_workspace -> analyzing: auto
    analyzing -> planning: manual (done)
    planning -> implementing: manual (done)
    implementing -> testing: auto (guardrail)
    testing -> implementing: auto (tests_fail)
    testing -> reviewing: auto (tests_pass)
    reviewing -> merging: manual (done)
    merging -> done: auto (merge_success)
    merging -> failed: auto (merge_fail)
    "*" -> failed: manual (failed)