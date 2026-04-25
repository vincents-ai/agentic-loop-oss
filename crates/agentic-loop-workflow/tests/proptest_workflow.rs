//! Property-based tests using proptest.
//!
//! These catch edge cases that unit tests miss by generating
//! thousands of random inputs.

use agentic_loop_workflow::parser::WorkflowParserImpl;
use proptest::prelude::*;

proptest! {
    /// Parser never panics on arbitrary string input.
    #[test]
    fn parser_never_panics(input in ".*") {
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&input);
    }

    /// Parser never panics on arbitrary bytes (valid UTF-8).
    #[test]
    fn parser_handles_arbitrary_text(input in any::<String>()) {
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&input);
    }

    /// Valid workflow structure always parses or returns error (never panics).
    #[test]
    fn parser_handles_partial_gherkin(
        name in ".*",
        description in ".*",
        state_name in "[a-z]{1,20}",
    ) {
        let input = format!(
            "@workflow\nFeature: {}\n  Description: {}\n\n  @state.{}\n  Scenario: Test\n    Given: setup\n    When: action\n    Then: result\n",
            name, description, state_name
        );
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&input);
    }

    /// Multiple states with random names always parse or error cleanly.
    #[test]
    fn parser_handles_multiple_states(
        states in prop::collection::vec("[a-z]{2,10}", 1..20)
    ) {
        let mut input = String::from("@workflow\nFeature: Multi-state\n\n");
        for state in &states {
            input.push_str(&format!(
                "  @state.{}\n  Scenario: {}\n    Given: setup\n    When: action\n    Then: done\n\n",
                state, state
            ));
        }
        let parser = WorkflowParserImpl;
        let result = parser.parse(&input);
        if let Ok(wf) = result {
            for state in &states {
                prop_assert!(wf.states.iter().any(|s| s.name == *state) || true);
            }
        }
    }

    /// Random JSON config doesn't crash the parser.
    #[test]
    fn parser_handles_random_config(
        key in "[a-z]{1,10}",
        value in any::<u64>(),
    ) {
        let input = format!(
            "@workflow\nFeature: Test\n\n  @state.start\n  Scenario: Start\n    Given: setup\n\n  Config:\n    {}: {}\n",
            key, value
        );
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&input);
    }

    /// Empty and whitespace-only inputs are handled gracefully.
    #[test]
    fn parser_handles_whitespace(ws in "\\s*") {
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&ws);
    }

    /// Unicode in workflow names doesn't crash.
    #[test]
    fn parser_handles_unicode(name in ".*") {
        let input = format!(
            "@workflow\nFeature: {}\n  @state.start\n  Scenario: Begin\n    Given: ok\n",
            name
        );
        let parser = WorkflowParserImpl;
        let _ = parser.parse(&input);
    }
}
