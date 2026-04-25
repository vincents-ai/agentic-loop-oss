//! Benchmarks for agentic-loop-workflow.
//!
//! Run with: cargo bench --package agentic-loop-workflow

use agentic_loop_workflow::parser::WorkflowParserImpl;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const SAMPLE_WORKFLOW: &str = r#"
@workflow
Feature: development
  Description: Standard development workflow

  @state.start
  Scenario: Begin development task
    Given: task is defined
    When: developer starts work
    Then: Move to "brainstorm" state
    Tools:
      file_read
      bash

  @state.brainstorm
  Scenario: Explore ideas and plan
    Given: task requirements are clear
    When: brainstorming and planning
    Then: Move to "research" state
    Tools:
      file_read
      bash
      web_fetch

  @state.research
  Scenario: Research existing solutions
    Given: brainstorm is complete
    When: researching solutions
    Then: Move to "implement" state
    Tools:
      file_read
      file_grep
      web_fetch
      bash

  @state.implement
  Scenario: Write code
    Given: research is complete
    When: implementing solution
    Then: Move to "test" state
    Tools:
      file_read
      file_write
      file_edit
      bash

  @state.test
  Scenario: Verify implementation
    Given: implementation is complete
    When: running tests
    Then: Move to "review" state
    Tools:
      bash
      file_read

  @state.review
  Scenario: Code review
    Given: tests pass
    When: reviewing code
    Then: done
    Tools:
      file_read

  Config:
    max_retries: 3

  Transitions:
    start -> brainstorm: auto
    brainstorm -> research: auto
    research -> implement: auto
    implement -> test: auto
    test -> review: auto
    review -> done: manual (done)
"#;

fn bench_parse_workflow(c: &mut Criterion) {
    c.bench_function("parse_development_workflow", |b| {
        b.iter(|| {
            let _ = WorkflowParserImpl::parse(black_box(SAMPLE_WORKFLOW));
        })
    });
}

fn bench_parse_minimal(c: &mut Criterion) {
    let minimal = "@workflow\nFeature: test\n  @state.start\n  Scenario: Begin\n    Given: ok\n";
    c.bench_function("parse_minimal_workflow", |b| {
        b.iter(|| {
            let _ = WorkflowParserImpl::parse(black_box(minimal));
        })
    });
}

fn bench_parse_large(c: &mut Criterion) {
    let mut large = String::from("@workflow\nFeature: large-workflow\n\n");
    for i in 0..100 {
        large.push_str(&format!(
            "  @state.state{}\n  Scenario: Step {}\n    Given: prev done\n    When: executing\n    Then: next\n    Tools:\n      file_read\n      bash\n\n",
            i, i
        ));
    }
    large.push_str("  Transitions:\n");
    for i in 0..99 {
        large.push_str(&format!("    state{} -> state{}: auto\n", i, i + 1));
    }
    large.push_str("    state99 -> done: manual (done)\n");

    c.bench_function("parse_100_state_workflow", |b| {
        b.iter(|| {
            let _ = WorkflowParserImpl::parse(black_box(&large));
        })
    });
}

criterion_group!(benches, bench_parse_workflow, bench_parse_minimal, bench_parse_large);
criterion_main!(benches);
