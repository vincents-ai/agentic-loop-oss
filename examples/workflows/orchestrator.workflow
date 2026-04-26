@workflow
Feature: Orchestrator Workflow
  Description: Multi-repo orchestration with skill-aware agent selection

  # ============================================================================
  # STATES
  # ============================================================================

  @state.analyze
  Scenario: Analyze request across repos
    Given: User request with optional repo list
    When: Analyze which repos are involved
    Then: Store repo list in context
    Then: Move to "select_skills" state

  @state.select_skills
  Scenario: Find relevant skills across all repos
    Given: Request analyzed
    When: Search skills in:
      - Central skills repo
      - Each target repo's .agentic-loop/skills/
      - Engram skills entities
    Then: Create SkillSelection entity
    Then: Move to "select_model" state

  @state.select_model
  Scenario: Select optimal model
    Given: Skills selected
    When: Select model based on:
      - Task complexity
      - Provider rate limits
      - Budget constraints
    Then: Store ModelSelection entity
    Then: Move to "decompose" state

  @state.decompose
  Scenario: Decompose into repo-specific tasks
    Given: Skills and model selected
    When: Break into tasks per repo
    Then: Create Task entities per repo
    Then: Move to "execute" state

  @state.execute
  Scenario: Execute across repos
    Given: Tasks per repo
    When: Execute in parallel across repos
    Then: Track progress per repo
    Then: Move to "monitor" state

  @state.complete
  Scenario: Aggregate results
    Given: All repos complete
    When: Aggregate reasoning from all repos
    Then: Create final summary
    Then: Move to "complete" state

  # ============================================================================
  # ASSIGNEE
  # ============================================================================
  Assignee:
    analyze: architect
    select_skills: skill-selector
    select_model: model-selector
    decomposing: deconstructor
    execute: dynamic (per repo)
    monitor: monetiser
    complete: mcp

  # ============================================================================
  # REFERENCES
  # ============================================================================
  References:
    - entity: Task
      action: create
      scope: multi-repo
    - entity: Context
      action: create
      type: repo_context
    - entity: Reasoning
      action: create
      type: multi_repo_analysis
    - entity: SkillSelection
      action: create
      scope: central + per-repo

  # ============================================================================
  # TOOLS - Multi-repo + Skills
  # ============================================================================
  Tools:
    # Existing tools
    done:
      usage: done --task-id {{task_id}} --result "summary"
    failed:
      usage: failed --task-id {{task_id}} --reason "reason"
    escalate:
      usage: escalate --task-id {{task_id}} --reason "reason"
      creates: EscalationRequest

    # NEW: Multi-repo tools
    repo_list:
      usage: repo_list --all
      description: List all configured repositories

    repo_context:
      usage: repo_context --repo {{repo_name}}
      description: Get engram context from specific repo

    repo_switch:
      usage: repo_switch --repo {{repo_name}}
      description: Switch working context to another repo

    skill_search:
      usage: skill_search --query "search terms" --category optional
      description: Search skills across repos and central store

    skill_list:
      usage: skill_list --repo {{repo_name}} --category optional
      description: List available skills

    skill_use:
      usage: skill_use --skill {{skill_name}}
      description: Include skill instructions in prompt

  # ============================================================================
  # TRANSITIONS
  # ============================================================================
  Transitions:
    analyze -> select_skills: auto
    select_skills -> select_model: auto
    select_model -> decompose: auto
    decompose -> execute: auto
    execute -> complete: all_done