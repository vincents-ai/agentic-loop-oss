@workflow
Feature: Development Workflow
  Description: Canonical development workflow for agentic-loop. Every feature follows this process.
  This workflow dogfoods agentic-loop to build agentic-loop.

  # ============================================================================
  # STATE: BRAINSTORM
  # Open exploration, no constraints, generate ideas
  # ============================================================================

  @state.brainstorm
  Scenario: Explore ideas and possibilities
    Model: fast,free
    Given: A feature request, bug report, or improvement idea
    When: Architect explores the problem space openly
      - Consider multiple approaches
      - Identify risks and unknowns
      - Note dependencies and constraints
      - Record all ideas in engram as Context entities
    Then: Ideas captured in engram with brainstorm tag
    Then: Move to "refine" state
    Tools:
      file_read:
      file_ls:
      bash:
      done:

  Assignee:
    brainstorm: architect

  References:
    - entity: Context
      Action: create
      Type: brainstorm
      Tags: brainstorm,ideas
    - entity: Relationship
      Action: create
      LinkedTo: "{{task_id}}"

  # ============================================================================
  # STATE: REFINE
  # Narrow down to specific, actionable scope
  # ============================================================================

  @state.refine
  Scenario: Narrow scope and define acceptance criteria
    Model: smart
    Given: Brainstorm ideas in engram
    When: Deconstructor refines into:
      - Specific scope boundaries (what is in/out)
      - Acceptance criteria (measurable outcomes)
      - Affected crates identified
      - Risk assessment
    Then: Refined scope stored as Reasoning entity
    Then: Move to "research" state

  Assignee:
    refine: deconstructor

  References:
    - entity: Reasoning
      Action: create
      Type: scope_refinement
      LinkedTo: "{{task_id}}"
    - entity: Context
      Required: true
      Error: "No brainstorm context found in engram"

  Tools:
    file_read:
    file_ls:
    bash:
    done:

  # ============================================================================
  # STATE: RESEARCH
  # Investigate existing code, dependencies, patterns
  # ============================================================================

  @state.research
  Scenario: Research existing code and patterns
    Model: fast,free
    Given: Refined scope in engram
    When: Researcher investigates:
      - Read relevant source files in affected crates
      - Check existing trait definitions
      - Review dependency APIs (engram, vincents-llm, etc.)
      - Search for similar patterns in the codebase
      - Identify reusable code
      - Check for breaking changes
    Then: Research findings stored as Context entities
    Then: Move to "plan" state

  Assignee:
    research: researcher

  References:
    - entity: Context
      Action: create
      Type: research_findings
      LinkedTo: "{{task_id}}"
    - entity: Reasoning
      Action: create
      Type: research_analysis

  Tools:
    file_read:
    file_ls:
    bash:
    done:
    failed:

  # ============================================================================
  # STATE: PLAN
  # Create implementation plan with tasks
  # ============================================================================

  @state.plan
  Scenario: Create implementation plan
    Model: smart
    Given: Research findings in engram
    When: Deconstructor creates:
      - Ordered list of implementation steps
      - Each step has acceptance criteria
      - Child tasks created in engram per step
      - Dependencies between steps documented
      - Affected files listed per step
    Then: Plan stored as Reasoning with child Task entities
    Then: Move to "bdd_red" state

  Assignee:
    plan: deconstructor

  References:
    - entity: Task
      Action: create
      Type: implementation_step
      LinkedTo: "{{task_id}}"
    - entity: Reasoning
      Action: create
      Type: implementation_plan

  Tools:
    file_read:
    file_ls:
    bash:
    done:
    failed:

  # ============================================================================
  # STATE: BDD RED
  # Write failing tests first (Given/When/Then scenarios)
  # ============================================================================

  @state.bdd_red
  Scenario: Write failing BDD tests
    Model: coding
    Given: Implementation plan in engram
    When: Tester writes:
      - Gherkin scenarios for each acceptance criterion
      - Unit test stubs that fail
      - Integration test stubs that fail
      - Property test invariants
      - All tests MUST fail at this stage (RED)
    Then: Tests committed, all failing
    Then: Test files listed in Reasoning entity
    Then: Move to "implement" state

  Assignee:
    bdd_red: tester

  Guardrails:
    bdd_red:
      Command: cargo test --workspace 2>&1 | tail -5
      Creates: ExecutionResult
      Valid: exit_code != 0
      Error: "Tests must FAIL at RED stage - did you write assertions?"

  References:
    - entity: ExecutionResult
      Action: create
      Type: bdd_red_results
    - entity: Reasoning
      Action: create
      Type: test_coverage_plan

  Tools:
    file_read:
    file_write:
    file_edit:
    file_ls:
    bash:
    done:
    failed:

  # ============================================================================
  # STATE: IMPLEMENT
  # Write code to make tests pass
  # ============================================================================

  @state.implement
  Scenario: Implement the feature
    Model: coding
    Given: Failing tests from BDD RED stage
    When: Builder implements:
      - Follow the plan from engram
      - Write minimum code to make tests pass
      - Use trait-first approach (define trait, then impl)
      - All file operations via gix/git2 (never subprocess)
      - All engram operations via Storage trait (never CLI)
      - Dependencies via cargo commands (never edit Cargo.toml)
    Then: Run build guardrail
    Then: Move to "bdd_green" state

  Assignee:
    implement: builder

  Guardrails:
    implement:
      Command: cargo check --workspace --message-format short
      Creates: ExecutionResult
      Valid: exit_code == 0
      Error: "Build failed - fix before proceeding"

  References:
    - entity: ExecutionResult
      Action: create
      Type: build_result
    - entity: Reasoning
      Action: create
      Type: implementation_notes

  Tools:
    file_read:
    file_write:
    file_edit:
    file_ls:
    bash:
    done:
    failed:

  # ============================================================================
  # STATE: BDD GREEN
  # All tests passing, guardrails satisfied
  # ============================================================================

  @state.bdd_green
  Scenario: Verify all tests pass
    Model: coding
    Given: Implementation complete
    When: Tester runs full test suite:
      - All BDD scenarios pass
      - All unit tests pass
      - All integration tests pass
      - All property tests pass
      - Clippy passes with no warnings
    Then: All tests GREEN
    Then: Move to "document" state

  Assignee:
    bdd_green: tester

  Guardrails:
    bdd_green:
      Command: cargo test --workspace 2>&1 | tail -5
      Creates: ExecutionResult
      Valid: exit_code == 0
      Error: "Tests still failing - return to implement"
    bdd_green_clippy:
      Command: cargo clippy --workspace -- -D warnings 2>&1 | tail -5
      Creates: ExecutionResult
      Valid: exit_code == 0
      Error: "Clippy warnings found"

  References:
    - entity: ExecutionResult
      Action: create
      Type: bdd_green_results

  Tools:
    file_read:
    file_ls:
    bash:
    done:
    failed:

  Transitions:
    bdd_green -> implement: auto (tests_fail)
    bdd_green -> document: auto (tests_pass)

  # ============================================================================
  # STATE: DOCUMENT
  # Update documentation, record reasoning
  # ============================================================================

  @state.document
  Scenario: Update documentation
    Model: fast,free
    Given: All tests passing
    When: Architect updates:
      - Update crate documentation (doc comments)
      - Update SPEC.md if architecture changed
      - Update IMPLEMENTATION_ROADMAP.md if tasks changed
      - Update STORAGE.md if storage changed
      - Update AGENTS.md if dev workflow changed
      - Update .pi/SYSTEM_APPEND.md if bootstrap changed
      - Record implementation reasoning in engram
      - Create/update knowledge entries for patterns learned
    Then: Documentation complete
    Then: Move to "done" state

  Assignee:
    document: architect

  References:
    - entity: Reasoning
      Action: create
      Type: implementation_summary
      LinkedTo: "{{task_id}}"
    - entity: Knowledge
      Action: create
      Type: patterns_learned

  Tools:
    file_read:
    file_write:
    file_edit:
    file_ls:
    bash:
    done:

  # ============================================================================
  # STATE: DONE
  # ============================================================================

  @state.done
  Scenario: Feature complete
    Given: All stages passed
    When: Final verification:
      - All tests GREEN
      - Build passes
      - Documentation updated
      - Engram entities up to date
    Then: Task marked as done
    Tools:
      file_read:
      file_ls:
      bash:
      done:

  # ============================================================================
  # GLOBAL CONFIG
  # ============================================================================

  Config:
    max_retries: 3
    retry_from: implement
    # If BDD_GREEN fails 3 times, escalate to human

  Transitions:
    brainstorm -> refine: auto
    refine -> research: manual (done)
    research -> plan: manual (done)
    plan -> bdd_red: manual (done)
    bdd_red -> implement: auto (red_tests_confirmed)
    implement -> bdd_green: auto (build_passes)
    bdd_green -> implement: auto (tests_fail)
    bdd_green -> document: auto (tests_pass)
    document -> done: manual (done)
    "*" -> failed: manual (failed)
    failed -> brainstorm: manual (retry)
