//! Custom Gherkin parser for .workflow files.
//!
//! Two-phase parser:
//! 1. **Lexer**: Tokenizes raw text into a stream of tokens
//! 2. **AST Builder**: Constructs a Workflow AST from tokens
//!
//! Supports: @workflow, Feature, Description, @state, Scenario,
//! Given/When/Then/And, Assignee, References, Guardrails, Tools,
//! Transitions, Config, HumanCheckpoint, IncludeWorkflow, Delegate.

use agentic_loop_types::workflow::{Scenario, State, Transition, Workflow};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// Parser for .workflow files.
pub struct WorkflowParserImpl;

impl WorkflowParserImpl {
    pub fn new() -> Self {
        Self
    }

    /// Parse a .workflow template from text.
    pub fn parse(&self, content: &str) -> Result<Workflow> {
        let lines: Vec<&str> = content.lines().collect();
        let mut parser = InnerParser::new(&lines);

        parser.parse_workflow()
    }
}

impl Default for WorkflowParserImpl {
    fn default() -> Self {
        Self::new()
    }
}

/// Inner parser state.
struct InnerParser<'a> {
    lines: &'a [&'a str],
    pos: usize,
}

impl<'a> InnerParser<'a> {
    fn new(lines: &'a [&str]) -> Self {
        Self { lines, pos: 0 }
    }

    fn current_line(&self) -> Option<&'a str> {
        self.lines.get(self.pos).copied()
    }

    /// Get current line, panicking with a clear message if past end.
    /// Should only be called after verifying !is_at_end().
    fn current_line_checked(&self) -> Result<&'a str> {
        self.lines.get(self.pos).copied()
            .ok_or_else(|| anyhow::anyhow!("Unexpected end of input at line {}", self.pos))
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.lines.len()
    }

    /// Trimmed current line, skipping empty lines.
    fn next_meaningful_line(&mut self) -> Option<&'a str> {
        while !self.is_at_end() {
            let line = self.current_line_checked().ok()?.trim();
            if !line.is_empty() && !line.starts_with('#') {
                return Some(line);
            }
            self.advance();
        }
        None
    }

    /// Collect all indented lines following the current position until
    /// we hit a line at the same or lesser indentation (or a top-level keyword).
    fn collect_indented_block(&mut self, base_indent: usize) -> Vec<String> {
        let mut block = Vec::new();
        while !self.is_at_end() {
            let raw = self.current_line().unwrap_or("");
            let trimmed = raw.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                self.advance();
                continue;
            }

            // Stop at new section markers regardless of indentation
            if trimmed.starts_with("@state.") || trimmed == "Assignee:" || trimmed == "References:" || trimmed == "Guardrails:" || trimmed == "Tools:" || trimmed == "Transitions:" || trimmed == "Config:" {
                break;
            }

            let indent = raw.len() - raw.trim_start().len();
            if indent <= base_indent {
                break;
            }

            block.push(trimmed.to_string());
            self.advance();
        }
        block
    }

    /// Parse the full workflow.
    fn parse_workflow(&mut self) -> Result<Workflow> {
        // Parse @workflow tag
        let first = self.next_meaningful_line().context("Empty workflow file")?;
        if first != "@workflow" {
            bail!("Expected @workflow tag at start, got: {}", first);
        }
        self.advance();

        // Parse Feature line
        let feature_line = self.next_meaningful_line().context("Missing Feature line")?;
        let name = feature_line
            .strip_prefix("Feature:")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| feature_line.to_string());
        self.advance();

        // Parse Description (indented lines after Feature)
        let description = self.parse_description()?;

        // Parse body sections
        let mut states = Vec::new();
        let mut scenarios = Vec::new();
        let mut transitions = Vec::new();
        let mut guardrails = Vec::new();
        let mut assignee: Option<HashMap<String, String>> = None;
        let mut references: Vec<String> = Vec::new();
        let mut config = serde_json::Value::Null;
        let mut initial_state: Option<String> = None;

        while !self.is_at_end() {
            let line = match self.current_line() {
                Some(l) => l.trim(),
                None => break,
            };

            if line.is_empty() || line.starts_with('#') {
                self.advance();
                continue;
            }

            if line.starts_with("@state.") {
                let (state, scenario) = self.parse_state_block()?;
                if initial_state.is_none() {
                    initial_state = Some(state.name.clone());
                }
                states.push(state);
                scenarios.push(scenario);
            } else if line == "Assignee:" {
                self.advance();
                assignee = Some(self.parse_assignee_block());
            } else if line == "References:" {
                self.advance();
                references = self.parse_references_block();
            } else if line == "Guardrails:" {
                self.advance();
                guardrails = self.parse_guardrails_block();
            } else if line == "Tools:" {
                self.advance();
                // Scenario-level tools — attach to most recent scenario
                let tool_names = self.parse_scenario_tools_block();
                if let Some(last_scenario) = scenarios.last_mut() {
                    last_scenario.tools.extend(tool_names);
                }
            } else if line == "Transitions:" {
                self.advance();
                transitions = self.parse_transitions_block()?;
            } else if line == "Config:" {
                self.advance();
                config = self.parse_config_block()?;
            } else {
                // Skip unknown lines
                self.advance();
            }
        }

        // Resolve assignees from global block into states
        if let Some(ref a) = assignee {
            for state in &mut states {
                if let Some(agent) = a.get(&state.name) {
                    // Store assignee in state config
                    if state.config.is_null() {
                        state.config = serde_json::json!({});
                    }
                    if let Some(obj) = state.config.as_object_mut() {
                        obj.insert("assignee".to_string(), serde_json::Value::String(agent.clone()));
                    }
                }
            }
        }

        Ok(Workflow {
            name,
            description,
            initial_state: initial_state.unwrap_or_default(),
            states,
            scenarios,
            transitions,
            guardrails,
            assignee: assignee.map(|m| {
                m.into_iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect()
            }),
            references,
            config,
            includes: vec![],
            delegates: vec![],
        })
    }

    /// Parse the description block (indented lines after Feature).
    fn parse_description(&mut self) -> Result<String> {
        let mut lines = Vec::new();
        while !self.is_at_end() {
            let raw = self.current_line_checked()?;
            let trimmed = raw.trim();

            if trimmed.is_empty() {
                self.advance();
                // Empty line may be paragraph separator
                if !lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            }

            // Check if this is indented (part of description)
            let indent = raw.len() - raw.trim_start().len();
            if indent == 0 {
                break;
            }

            // Stop if it's a top-level keyword
            if trimmed.starts_with('@') || trimmed == "Feature:" || trimmed.starts_with("Feature:") {
                break;
            }

            lines.push(trimmed.to_string());
            self.advance();
        }

        // Remove trailing empty lines
        while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }

        Ok(lines.join("\n"))
    }

    /// Parse a @state.<name> block and its Scenario.
    fn parse_state_block(&mut self) -> Result<(State, Scenario)> {
        let line = self.current_line_checked()?.trim();
        let state_name = line
            .strip_prefix("@state.")
            .context("Expected @state.<name>")?
            .to_string();
        self.advance();

        // Parse Scenario line
        let scenario_line = self.next_meaningful_line().context("Missing Scenario after @state")?;
        let scenario_name = scenario_line
            .strip_prefix("Scenario:")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| scenario_line.to_string());
        self.advance();

        // Parse Model: hint (optional)
        let mut model_hint: Option<String> = None;
        if !self.is_at_end() {
            if let Some(next) = self.next_meaningful_line() {
                if let Some(mh) = next.trim().strip_prefix("Model:") {
                    model_hint = Some(mh.trim().to_string());
                    self.advance();
                }
            }
        }

        // Parse Given/When/Then/And steps + per-scenario Tools
        let mut given = Vec::new();
        let mut when_steps = Vec::new();
        let mut then_steps = Vec::new();
        let mut scenario_tools: Vec<String> = Vec::new();
        let _state_config = serde_json::Value::Null;
        let mut in_scenario_tools = false;

        while !self.is_at_end() {
            let raw = self.current_line_checked()?;
            let trimmed = raw.trim();
            let indent = raw.len() - raw.trim_start().len();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                self.advance();
                continue;
            }

            // Stop if we hit a new major section (Tools: only at top-level, not scenario-level)
            let is_major_section = trimmed.starts_with("@state.") || trimmed == "Assignee:" || trimmed == "References:" || trimmed == "Guardrails:" || trimmed == "Transitions:" || trimmed == "Config:";
            if is_major_section {
                break;
            }

            // Tools: — scenario-level if indented (inside scenario), workflow-level if at indent 0
            if trimmed == "Tools:" {
                if indent > 0 {
                    // Scenario-level tools
                    in_scenario_tools = true;
                    self.advance();
                    continue;
                } else {
                    // Workflow-level tools — stop scenario parsing
                    break;
                }
            }

            if in_scenario_tools {
                // Stop collecting tools at section markers or step keywords
                if trimmed.starts_with("@state.") || trimmed == "Assignee:" || trimmed == "References:" || trimmed == "Guardrails:" || trimmed == "Transitions:" || trimmed == "Config:" {
                    in_scenario_tools = false;
                    continue;
                }
                // Stop if we hit Given/When/Then (new scenario steps)
                if trimmed.starts_with("Given:") || trimmed.starts_with("When:") || trimmed.starts_with("Then:") {
                    in_scenario_tools = false;
                    // Fall through to parse as step
                } else {
                    // Skip non-tool lines (Usage, Creates, Description, etc.)
                    let is_metadata = trimmed.starts_with("Usage:") || trimmed.starts_with("Creates") || trimmed.starts_with("Option:") || trimmed.starts_with("Description:") || trimmed.starts_with('#');
                    if !is_metadata {
                        // Tool definition line: "bash:" or "tool_name:"
                        let tool_name = trimmed.strip_suffix(':').unwrap_or(trimmed);
                        let tool_name = tool_name.split_whitespace().next().unwrap_or("");
                        if !tool_name.is_empty() && !scenario_tools.contains(&tool_name.to_string()) {
                            scenario_tools.push(tool_name.to_string());
                        }
                    }
                    self.advance();
                    continue;
                }
            }

            // Stop if unindented and not a step keyword
            if indent == 0 && !trimmed.starts_with("Given") && !trimmed.starts_with("When") && !trimmed.starts_with("Then") && !trimmed.starts_with("And") && !trimmed.starts_with("-") {
                break;
            }

            if let Some(rest) = trimmed.strip_prefix("Given:") {
                given.push(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("When:") {
                when_steps.push(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("Then:") {
                then_steps.push(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("And:") {
                // And continues the last section
                then_steps.push(rest.trim().to_string());
            } else if trimmed.starts_with("- ") {
                // Continuation of previous step (bullet point)
                if !then_steps.is_empty() {
                    let idx = then_steps.len() - 1;
                    then_steps[idx].push('\n');
                    then_steps[idx].push_str(trimmed);
                } else if !when_steps.is_empty() {
                    let idx = when_steps.len() - 1;
                    when_steps[idx].push('\n');
                    when_steps[idx].push_str(trimmed);
                } else if !given.is_empty() {
                    let idx = given.len() - 1;
                    given[idx].push('\n');
                    given[idx].push_str(trimmed);
                }
            }

            self.advance();
        }

        let state = State {
            name: state_name.clone(),
            description: scenario_name.clone(),
            config: _state_config,
        };

        let scenario = Scenario {
            state: state_name,
            name: scenario_name,
            given,
            when: when_steps,
            then: then_steps,
            tools: scenario_tools,
            model_hint,
        };

        Ok((state, scenario))
    }

    /// Parse Assignee block.
    fn parse_assignee_block(&mut self) -> HashMap<String, String> {
        let mut assignee = HashMap::new();
        let block = self.collect_indented_block(0);

        for line in &block {
            // Format: "state: agent_name"
            if let Some((state, agent)) = line.split_once(':') {
                assignee.insert(
                    state.trim().to_string(),
                    agent.trim().to_string(),
                );
            }
        }

        assignee
    }

    /// Parse References block.
    fn parse_references_block(&mut self) -> Vec<String> {
        let block = self.collect_indented_block(0);
        // Collect each "- entity: ..." line as a reference
        block
            .iter()
            .filter(|l| l.starts_with("- "))
            .map(|l| l.to_string())
            .collect()
    }

    /// Parse Guardrails block (simplified - collect as strings).
    fn parse_guardrails_block(&mut self) -> Vec<String> {
        let block = self.collect_indented_block(0);
        let mut guardrails = Vec::new();
        let mut current_name = String::new();

        for line in &block {
            if !line.starts_with(' ') && !line.starts_with('-') && line.contains(':') {
                // New guardrail definition: "name:"
                if !current_name.is_empty() {
                    guardrails.push(current_name.clone());
                }
                current_name = line.to_string();
            } else {
                current_name.push('\n');
                current_name.push_str(line);
            }
        }
        if !current_name.is_empty() {
            guardrails.push(current_name);
        }

        guardrails
    }

    /// Parse a scenario-level Tools block (tool names only, not definitions).
    fn parse_scenario_tools_block(&mut self) -> Vec<String> {
        let block = self.collect_indented_block(0);
        let mut tools = Vec::new();
        for line in &block {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
            // Skip metadata lines
            if trimmed.starts_with("Usage:") || trimmed.starts_with("Creates") || trimmed.starts_with("Option:") || trimmed.starts_with("Description:") || trimmed.starts_with("Review") { continue; }
            // Tool definition line: "tool_name:" or "tool_name"
            let tool_name = trimmed.strip_suffix(':').unwrap_or(trimmed);
            let tool_name = tool_name.split_whitespace().next().unwrap_or("");
            if !tool_name.is_empty() && !tools.contains(&tool_name.to_string()) {
                tools.push(tool_name.to_string());
            }
        }
        tools
    }

    /// Parse Transitions block.
    fn parse_transitions_block(&mut self) -> Result<Vec<Transition>> {
        let block = self.collect_indented_block(0);
        let mut transitions = Vec::new();

        for line in &block {
            // Format: "from -> to: trigger"
            if let Some((path, trigger)) = line.split_once(':') {
                let trigger = trigger.trim().to_string();
                if let Some((from, to)) = path.trim().split_once("->") {
                    let from = from.trim().trim_matches('"').to_string();
                    let to = to.trim().trim_matches('"').to_string();
                    transitions.push(Transition {
                        from,
                        to,
                        condition: Some(trigger),
                    });
                }
            }
        }

        Ok(transitions)
    }

    /// Parse Config block as JSON value.
    fn parse_config_block(&mut self) -> Result<serde_json::Value> {
        let block = self.collect_indented_block(0);
        let mut config = serde_json::Map::new();

        for line in &block {
            // Format: "key: value" or "key: type1 | type2"
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                // Try to parse as number, boolean, or keep as string
                let json_val = if value == "true" {
                    serde_json::Value::Bool(true)
                } else if value == "false" {
                    serde_json::Value::Bool(false)
                } else if let Ok(n) = value.parse::<i64>() {
                    serde_json::Value::Number(n.into())
                } else if let Ok(n) = value.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::Value::String(value.to_string())
                };

                config.insert(key.to_string(), json_val);
            }
        }

        Ok(serde_json::Value::Object(config))
    }
} // impl InnerParser

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_workflow() {
        let content = r#"@workflow
Feature: Test Workflow
  Description: A minimal test workflow

  @state.start
  Scenario: Begin
    Given: Nothing
    When: Start
    Then: Done

  @state.done
  Scenario: Complete
    Given: Started
    When: Finish
    Then: End
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.name, "Test Workflow");
        assert_eq!(workflow.states.len(), 2);
        assert_eq!(workflow.initial_state, "start");
        assert_eq!(workflow.states[0].name, "start");
        assert_eq!(workflow.states[1].name, "done");
    }

    #[test]
    fn test_parse_assignee() {
        let content = r#"@workflow
Feature: Assigned Workflow
  Description: Test assignee parsing

  @state.analyzing
  Scenario: Analyze
    Given: Input
    When: Process
    Then: Output

  Assignee:
    analyzing: architect
    planning: deconstructor
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert!(workflow.assignee.is_some());
        let Some(assignee_list) = &workflow.assignee else {
            panic!("Expected assignee to be Some");
        };
        assert!(assignee_list.iter().any(|a| a.contains("analyzing: architect")));
        assert!(assignee_list.iter().any(|a| a.contains("planning: deconstructor")));
    }

    #[test]
    fn test_parse_transitions() {
        let content = r#"@workflow
Feature: Transition Test
  Description: Test transitions

  @state.start
  Scenario: Start
    Given: Ready
    When: Go
    Then: Next

  @state.middle
  Scenario: Middle
    Given: Started
    When: Process
    Then: Done

  @state.end
  Scenario: End
    Given: Processed
    When: Finish
    Then: Complete

  Transitions:
    start -> middle: auto
    middle -> end: manual (done)
    "*" -> end: manual (abort)
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.transitions.len(), 3);
        assert_eq!(workflow.transitions[0].from, "start");
        assert_eq!(workflow.transitions[0].to, "middle");
        assert_eq!(workflow.transitions[0].condition, Some("auto".to_string()));
        assert_eq!(workflow.transitions[2].from, "*");
    }

    #[test]
    fn test_parse_config() {
        let content = r#"@workflow
Feature: Config Test
  Description: Test config parsing

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  Config:
    max_retries: 3
    auto_merge: true
    path_strategy: local
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.config["max_retries"], 3);
        assert_eq!(workflow.config["auto_merge"], true);
        assert_eq!(workflow.config["path_strategy"], "local");
    }

    #[test]
    fn test_parse_scenario_tools() {
        let content = r#"@workflow
Feature: Scenario Tools Test
  Description: Test per-scenario tools

  @state.research
  Scenario: Research
    Given: Task assigned
    When: Read codebase
    Then: Findings stored
    Tools:
      file_read:
        Usage: file_read --path /foo
      file_ls:
        Usage: file_ls --path /bar
      grep:
        Usage: grep --pattern foo

  @state.implement
  Scenario: Implement
    Given: Research done
    When: Write code
    Then: Tests pass
    Tools:
      file_read:
        Usage: file_read --path /foo
      file_write:
        Usage: file_write --path /bar
      file_edit:
        Usage: file_edit --path /baz
      bash:
        Usage: bash --command "cargo test"
      done:
        Usage: done

  Assignee:
    research: researcher
    implement: coder
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.scenarios.len(), 2);

        let research = &workflow.scenarios[0];
        assert_eq!(research.state, "research");
        assert_eq!(research.tools, vec!["file_read", "file_ls", "grep"]);

        let implement = &workflow.scenarios[1];
        assert_eq!(implement.state, "implement");
        assert_eq!(implement.tools, vec!["file_read", "file_write", "file_edit", "bash", "done"]);
    }

    #[test]
    fn test_parse_references() {
        let content = r#"@workflow
Feature: Refs Test
  Description: Test references

  @state.start
  Scenario: Begin
    Given: Ready
    When: Go
    Then: Done

  References:
    - entity: Task
      Required: true
    - entity: Context
      Action: create
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.references.len(), 2);
        assert!(workflow.references[0].contains("Task"));
        assert!(workflow.references[1].contains("Context"));
    }

    #[test]
    fn test_parse_scenario_steps() {
        let content = r#"@workflow
Feature: Steps Test
  Description: Test step parsing

  @state.research
  Scenario: Research codebase
    Given: Task has requirements
    When: Investigate existing patterns
      - Check crate APIs
      - Review trait definitions
    Then: Store findings
    Then: Move to "plan" state
"#;

        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.scenarios.len(), 1);
        let scenario = &workflow.scenarios[0];
        assert_eq!(scenario.state, "research");
        assert_eq!(scenario.given.len(), 1);
        assert_eq!(scenario.when.len(), 1);
        assert!(scenario.when[0].contains("Check crate APIs"));
        assert_eq!(scenario.then.len(), 2);
    }

    #[test]
    fn test_parse_implementation_workflow() {
        // Parse the actual implementation.workflow file
        let content = include_str!("../../../examples/workflows/implementation.workflow");
        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.name, "Implementation Workflow");
        assert!(workflow.states.len() >= 8, "Got {} states", workflow.states.len());
        assert!(workflow.transitions.len() >= 5, "Got {} transitions", workflow.transitions.len());
        assert!(workflow.assignee.is_some());
        assert!(!workflow.references.is_empty());
    }

    #[test]
    fn test_parse_development_workflow() {
        // Parse the actual development.workflow file
        let content = include_str!("../../../examples/workflows/development.workflow");
        let parser = WorkflowParserImpl::new();
        let workflow = parser.parse(content).unwrap();

        assert_eq!(workflow.name, "Development Workflow");
        assert!(workflow.states.len() >= 8);
        assert_eq!(workflow.initial_state, "brainstorm");
    }

    #[test]
    fn test_empty_file_fails() {
        let parser = WorkflowParserImpl::new();
        assert!(parser.parse("").is_err());
    }

    #[test]
    fn test_missing_workflow_tag_fails() {
        let parser = WorkflowParserImpl::new();
        assert!(parser.parse("Feature: Test\n  Description: Test").is_err());
    }
}
