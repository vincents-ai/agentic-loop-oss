//! Prompt builder — generates rich agent prompts pre-loaded with engram context.
//!
//! Design principle: pre-load everything the agent needs into the prompt.
//! The LLM should NOT need to discover Context, Reasoning, Knowledge, ADRs,
//! or Relationships via tool calls — they are injected directly.
//!
//! System prompt structure (ADR-020):
//!   1. Persona instructions from YAML (instructions field)
//!   2. fap_table (WHO/WHAT/WHY/HOW/WHEN behavioral framing)
//!   3. ov_requirements (operational constraints)
//!   4. ADRs relevant to the current task
//!   5. Project Knowledge entities tagged to task context
//!   6. Control + work tools for this scenario
//!
//! User prompt structure:
//!   1. Task (full description + priority + status)
//!   2. Context entities linked to task (pre-fetched from References)
//!   3. Given (preconditions from scenario + loaded context)
//!   4. Do (actions from scenario)
//!   5. Expected (outcomes from scenario)
//!   6. Previous reasoning (steps + conclusion from prior state)
//!   7. Guardrails (rendered explicitly)
//!   8. Tool parameter schemas

use agentic_loop_types::workflow::{Scenario, Workflow};
use crate::StepContext;
use tracing::instrument;

// ─── Engram context provider trait ────────────────────────────────────────────

/// Provider of engram context for prompt enrichment.
///
/// Abstracts over engram's Storage trait so the prompt builder
/// doesn't depend on engram directly (avoids git2 version conflicts).
pub trait EngramContextProvider: Send + Sync {
    /// Load persona instructions for the given assignee slug.
    /// Returns (instructions, fap_table, ov_requirements).
    fn load_persona(&self, assignee: &str) -> Option<PersonaData>;

    /// Load context entities linked to a task.
    /// Returns (title, content) pairs.
    fn load_task_context(&self, task_id: &str, tags: &[String]) -> Vec<ContextEntry>;

    /// Load reasoning for a task + state.
    fn load_reasoning(&self, task_id: &str, state: &str) -> Option<ReasoningData>;

    /// Load knowledge entities relevant to a task.
    fn load_knowledge(&self, query: &str, limit: usize) -> Vec<KnowledgeEntry>;

    /// Load ADRs relevant to the current work.
    fn load_adrs(&self, tags: &[String]) -> Vec<AdrEntry>;
}

/// Persona data loaded from engram.
#[derive(Debug, Clone)]
pub struct PersonaData {
    pub instructions: String,
    pub fap_table: Vec<(String, String)>,
    pub ov_requirements: Vec<String>,
}

/// A context entry loaded from engram.
#[derive(Debug, Clone)]
pub struct ContextEntry {
    pub title: String,
    pub content: String,
    pub relevance: String,
}

/// Reasoning data loaded from engram.
#[derive(Debug, Clone)]
pub struct ReasoningData {
    pub steps: Vec<String>,
    pub conclusion: String,
    pub confidence: f64,
}

/// A knowledge entry loaded from engram.
#[derive(Debug, Clone)]
pub struct KnowledgeEntry {
    pub title: String,
    pub content: String,
    pub knowledge_type: String,
    pub confidence: f64,
}

/// An ADR entry loaded from engram.
#[derive(Debug, Clone)]
pub struct AdrEntry {
    pub title: String,
    pub context: String,
    pub decision: String,
    pub consequences: String,
}

// ─── No-op provider (default, no engram) ──────────────────────────────────────

/// Provider that returns nothing — used when engram is not available.
pub struct NoopContextProvider;

impl EngramContextProvider for NoopContextProvider {
    fn load_persona(&self, _assignee: &str) -> Option<PersonaData> { None }
    fn load_task_context(&self, _task_id: &str, _tags: &[String]) -> Vec<ContextEntry> { Vec::new() }
    fn load_reasoning(&self, _task_id: &str, _state: &str) -> Option<ReasoningData> { None }
    fn load_knowledge(&self, _query: &str, _limit: usize) -> Vec<KnowledgeEntry> { Vec::new() }
    fn load_adrs(&self, _tags: &[String]) -> Vec<AdrEntry> { Vec::new() }
}

// ─── Prompt builder ────────────────────────────────────────────────────────────

/// Builds rich prompts pre-loaded with engram context.
pub struct PromptBuilderImpl {
    system_prefix: String,
    context_provider: Box<dyn EngramContextProvider>,
}

impl PromptBuilderImpl {
    /// Create with default (no-op) context provider.
    pub fn new() -> Self {
        Self {
            system_prefix: DEFAULT_SYSTEM_PREFIX.to_string(),
            context_provider: Box::new(NoopContextProvider),
        }
    }

    /// Create with a custom engram context provider.
    pub fn with_provider(provider: Box<dyn EngramContextProvider>) -> Self {
        Self {
            system_prefix: DEFAULT_SYSTEM_PREFIX.to_string(),
            context_provider: provider,
        }
    }

    /// Build the system prompt — enriched with persona, ADRs, knowledge.
    #[instrument(skip_all, fields(state = %_state_name, tools = scenario.tools.len()))]
    pub fn build_system_prompt(
        &self,
        workflow: &Workflow,
        _state_name: &str,
        scenario: &Scenario,
        assignee: Option<&str>,
    ) -> String {
        let mut parts = Vec::new();

        // ── 1. Persona instructions ───────────────────────────────────────────
        if let Some(agent) = assignee {
            if let Some(persona) = self.context_provider.load_persona(agent) {
                // Persona instructions as the PRIMARY system prompt
                if !persona.instructions.is_empty() {
                    parts.push(persona.instructions);
                }

                // fap_table as behavioral framing
                if !persona.fap_table.is_empty() {
                    parts.push("## Behavioral Framework".to_string());
                    for (key, value) in &persona.fap_table {
                        parts.push(format!("**{}**: {}", key, value));
                    }
                    parts.push(String::new());
                }

                // ov_requirements as operational constraints
                if !persona.ov_requirements.is_empty() {
                    parts.push("## Operational Requirements".to_string());
                    for req in &persona.ov_requirements {
                        parts.push(format!("- {}", req));
                    }
                    parts.push(String::new());
                }
            } else {
                parts.push(format!("You are acting as: **{}**.\n", agent));
            }
        }

        // ── 2. Workflow description (if available) ────────────────────────────
        if !workflow.description.is_empty() {
            parts.push(format!("## Workflow\n{}\n", workflow.description));
        }

        // ── 3. ADRs as architectural constraints ──────────────────────────────
        let adr_tags: Vec<String> = workflow.references.iter()
            .filter(|r| r.contains("adr"))
            .cloned()
            .collect();
        let adrs = self.context_provider.load_adrs(&adr_tags);
        if !adrs.is_empty() {
            parts.push("## Architecture Decisions".to_string());
            for adr in &adrs {
                parts.push(format!("### {}", adr.title));
                parts.push(format!("**Decision**: {}", adr.decision));
                if !adr.consequences.is_empty() {
                    parts.push(format!("**Consequences**: {}", adr.consequences));
                }
                parts.push(String::new());
            }
        }

        // ── 4. Project Knowledge ───────────────────────────────────────────────
        let knowledge = self.context_provider.load_knowledge(
            &workflow.description,
            5,
        );
        if !knowledge.is_empty() {
            parts.push("## Project Knowledge".to_string());
            for k in &knowledge {
                parts.push(format!("- **{}** ({}): {}", k.title, k.knowledge_type, k.content));
            }
            parts.push(String::new());
        }

        // ── 5. Rules (generic, always present) ─────────────────────────────────
        parts.push(self.system_prefix.clone());

        // ── 6. Tools ───────────────────────────────────────────────────────────
        parts.push("## Tools\n".to_string());
        parts.push("Signal completion/failure:".to_string());
        for tool in CONTROL_TOOLS {
            parts.push(format!("  - {}", tool));
        }
        parts.push(String::new());

        if !scenario.tools.is_empty() {
            parts.push("Work tools for this step:".to_string());
            for tool_name in &scenario.tools {
                parts.push(format!("  - {}", tool_name));
            }
        } else {
            parts.push("Work tools: all available (no restrictions).".to_string());
        }

        parts.join("\n")
    }

    /// Build the user prompt — enriched with context, reasoning, guardrails.
    #[instrument(skip_all, fields(state = %context.current_state))]
    pub fn build_user_prompt(&self, context: &StepContext, scenario: &Scenario) -> String {
        let mut parts = Vec::new();

        // ── 0. Model hint ─────────────────────────────────────────────────────────
        if let Some(ref hint) = context.model_hint {
            parts.push(format!("## Model Preference\nThis step prefers a {} model.\n", hint));
        }

        // ── 1. Task ─────────────────────────────────────────────────────────────
        parts.push(format!("## Task\n{}\n", context.task_description));

        // ── 2. Pre-loaded context entities (from References) ────────────────────
        let ctx_entries = self.context_provider.load_task_context(
            &context.task_description,
            &context.available_tools,
        );
        if !ctx_entries.is_empty() {
            parts.push("## Context".to_string());
            for entry in &ctx_entries {
                parts.push(format!("**{}** [{}]: {}", entry.title, entry.relevance, entry.content));
            }
            parts.push(String::new());
        }

        // ── 3. Given (preconditions + loaded context) ──────────────────────────
        if !scenario.given.is_empty() {
            parts.push("## Given\n".to_string());
            for given in &scenario.given {
                parts.push(format!("- {}\n", substitute_vars(given, &context.variables)));
            }
        }

        // ── 4. Do (actions) ────────────────────────────────────────────────────
        if !scenario.when.is_empty() {
            parts.push("## Do\n".to_string());
            for when in &scenario.when {
                let rendered = substitute_vars(when, &context.variables);
                // Strip lines that reference tools not in the available set
                // (e.g. "Record in engram" when no engram tools are available)
                let available_lower: Vec<&str> = context.available_tools.iter().map(|t| t.as_str()).collect();
                let should_include = if rendered.to_lowercase().contains("engram")
                    && !available_lower.iter().any(|t| t.contains("engram"))
                {
                    false
                } else if rendered.trim().starts_with('-') && rendered.trim().len() <= 2 {
                    // Skip empty bullet points
                    false
                } else {
                    true
                };
                if should_include {
                    parts.push(format!("- {}\n", rendered));
                }
            }
        }

        // ── 5. Expected (outcomes) ──────────────────────────────────────────────
        if !scenario.then.is_empty() {
            parts.push("## Expected\n".to_string());
            for then in &scenario.then {
                // Filter out state transition instructions — the engine handles those
                let rendered = substitute_vars(then, &context.variables);
                if !rendered.to_lowercase().starts_with("move to")
                    && !rendered.to_lowercase().contains("move to \"")
                    && !rendered.to_lowercase().contains("go to")
                    && !rendered.to_lowercase().contains("state")
                {
                    parts.push(format!("- {}\n", rendered));
                }
            }
        }

        // ── 6. Previous step context ──────────────────────────────────────────
        if let Some(ref prev) = context.previous_outcome {
            // Use summary from previous step if available
            if let Some(ref summary) = prev.summary {
                if !summary.is_empty() {
                    parts.push(format!(
                        "## Previous Step ({})\n{}\n",
                        prev.state, summary
                    ));
                }
            } else if !prev.success {
                // Failure info takes priority over reasoning
                parts.push(format!(
                    "Previous step ({}): failed — {}\n",
                    prev.state,
                    prev.error.as_deref().unwrap_or("unknown")
                ));
                parts.push("Consider a different approach to avoid the same failure.\n".to_string());
            } else if let Some(reasoning) = self.context_provider.load_reasoning(
                &context.task_description,
                &prev.state,
            ) {
                // Fallback: load reasoning from context provider (engram path)
                parts.push(format!(
                    "## Previous Step ({})\n",
                    prev.state
                ));
                parts.push(format!("**Conclusion** (confidence {:.0}%): {}",
                    reasoning.confidence * 100.0,
                    reasoning.conclusion));
                if !reasoning.steps.is_empty() {
                    parts.push("\nSteps:".to_string());
                    for step in &reasoning.steps {
                        parts.push(format!("- {}", step));
                    }
                }
                parts.push(String::new());
            } else {
                parts.push(format!(
                    "Previous step ({}): completed successfully.\n",
                    prev.state
                ));
            }

            // Also show eval score if available
            if let Some(score) = prev.eval_score {
                parts.push(format!("Eval score: {:.0}%\n", score * 100.0));
            }
        }

        // ── 7. Guardrails (rendered explicitly) ────────────────────────────────
        if !context.guardrails.is_empty() {
            parts.push("## Guardrails\n".to_string());
            parts.push("Before advancing to the next state, these must pass:".to_string());
            for guard in &context.guardrails {
                parts.push(format!("- {} (must exit 0)", guard));
            }
            parts.push(String::new());
        }

        // ── 8. Available tools (with parameter schemas) ───────────────────
        if !context.tool_schemas.is_empty() {
            parts.push("## Available Tools\n".to_string());
            for tool in &context.tool_schemas {
                parts.push(format!("### {}\n{}", tool.name, tool.description));
                if let Some(params) = tool.parameters.as_object() {
                    if let Some(props) = params.get("properties").and_then(|p| p.as_object()) {
                        let required: Vec<&str> = params.get("required")
                            .and_then(|r| r.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                            .unwrap_or_default();
                        for (pname, pschema) in props {
                            let pname_str = pname.as_str();
                            let type_str = pschema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                            let desc = pschema.get("description").and_then(|d| d.as_str()).unwrap_or("");
                            let req_flag = if required.contains(&pname_str) { " (required)" } else { " (optional)" };
                            if desc.is_empty() {
                                parts.push(format!("- {}{}: {}", pname, req_flag, type_str));
                            } else {
                                parts.push(format!("- {}{}: {} — {}", pname, req_flag, type_str, desc));
                            }
                        }
                    }
                }
                parts.push(String::new());
            }
        } else if !context.available_tools.is_empty() {
            parts.push(format!(
                "Available tools: {}\n",
                context.available_tools.join(", ")
            ));
        }

        parts.join("\n")
    }
}

impl Default for PromptBuilderImpl {
    fn default() -> Self { Self::new() }
}

/// Substitute `{{variable}}` placeholders in text with values from a JSON object.
fn substitute_vars(text: &str, variables: &serde_json::Value) -> String {
    let mut result = text.to_string();
    if let Some(obj) = variables.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}

const DEFAULT_SYSTEM_PREFIX: &str = "\
## Rules
1. Do exactly what the \"Do\" section says
2. Verify the \"Given\" preconditions first
3. Achieve every item in \"Expected\"
4. Use the available tools to accomplish the task
5. If you cannot complete the task, call the failed tool with a reason
6. When finished, ALWAYS call the done tool with a brief summary — do NOT just stop generating text
7. Do NOT try to change workflow state — state transitions are handled automatically
8. Be concise — avoid re-reading files you've already seen. Summarize your findings and move on";

/// Control tools that are ALWAYS available to every scenario.
pub const CONTROL_TOOLS: &[&str] = &["done", "failed", "escalate", "retry"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorkflowParserImpl, StepOutcome};

    fn parse_workflow(content: &str) -> Workflow {
        WorkflowParserImpl::new().parse(content).unwrap()
    }

    // ─── Mock context provider for testing ────────────────────────────────────

    struct MockProvider {
        persona: Option<PersonaData>,
        contexts: Vec<ContextEntry>,
        reasoning: Option<ReasoningData>,
        knowledge: Vec<KnowledgeEntry>,
        adrs: Vec<AdrEntry>,
    }

    impl MockProvider {
        fn with_persona() -> Self {
            Self {
                persona: Some(PersonaData {
                    instructions: "You are an expert architect.".to_string(),
                    fap_table: vec![("WHO".into(), "Senior Architect".into()), ("WHAT".into(), "Design systems".into())],
                    ov_requirements: vec!["Always use trait-first design".to_string()],
                }),
                contexts: vec![ContextEntry {
                    title: "Current Architecture".into(),
                    content: "The system uses gix for git operations.".into(),
                    relevance: "Critical".into(),
                }],
                reasoning: Some(ReasoningData {
                    steps: vec!["Analyzed existing code".into(), "Identified coupling".into()],
                    conclusion: "Need to extract trait".into(),
                    confidence: 0.85,
                }),
                knowledge: vec![KnowledgeEntry {
                    title: "Trait-first pattern".into(),
                    content: "Every domain defines traits. Implementations are separate crates.".into(),
                    knowledge_type: "Concept".into(),
                    confidence: 0.95,
                }],
                adrs: vec![AdrEntry {
                    title: "ADR-012: Trait-first architecture".into(),
                    context: "Need swappable implementations.".into(),
                    decision: "Every domain is trait-based.".into(),
                    consequences: "More crates but cleaner boundaries.".into(),
                }],
            }
        }
    }

    impl EngramContextProvider for MockProvider {
        fn load_persona(&self, _assignee: &str) -> Option<PersonaData> { self.persona.clone() }
        fn load_task_context(&self, _task_id: &str, _tags: &[String]) -> Vec<ContextEntry> { self.contexts.clone() }
        fn load_reasoning(&self, _task_id: &str, _state: &str) -> Option<ReasoningData> { self.reasoning.clone() }
        fn load_knowledge(&self, _query: &str, _limit: usize) -> Vec<KnowledgeEntry> { self.knowledge.clone() }
        fn load_adrs(&self, _tags: &[String]) -> Vec<AdrEntry> { self.adrs.clone() }
    }

    // ─── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_system_prompt_with_persona() {
        let builder = PromptBuilderImpl::with_provider(Box::new(MockProvider::with_persona()));
        let workflow = parse_workflow(
            r#"@workflow
Feature: Dev Workflow
  Description: A development workflow

  @state.coding
  Scenario: Write code
    When: Write tests

  Assignee:
    coding: architect
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(&workflow, "coding", scenario, Some("architect"));

        // Should have persona instructions
        assert!(prompt.contains("expert architect"));
        // Should have fap_table
        assert!(prompt.contains("Senior Architect"));
        assert!(prompt.contains("Design systems"));
        // Should have ov_requirements
        assert!(prompt.contains("trait-first design"));
        // Should have ADRs
        assert!(prompt.contains("ADR-012"));
        // Should have knowledge
        assert!(prompt.contains("Trait-first pattern"));
        // Should have workflow description
        assert!(prompt.contains("development workflow"));
    }

    #[test]
    fn test_user_prompt_with_context_and_reasoning() {
        let builder = PromptBuilderImpl::with_provider(Box::new(MockProvider::with_persona()));
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.step2
  Scenario: Step 2
    Given: Previous step done
    When: Implement the solution
    Then: Tests pass
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Extract storage trait".to_string(),
            current_state: "step2".to_string(),
            workflow_name: "dev".to_string(),
            available_tools: vec!["file_edit".to_string(), "bash".to_string()],
            tool_schemas: vec![],
            guardrails: vec!["cargo test --workspace".to_string()],
            previous_outcome: Some(StepOutcome {
                state: "step1".to_string(),
                agent_session_id: "s1".to_string(),
                success: true,
                eval_score: Some(0.9),
                next_state: None,
                error: None,
                summary: None,
            }),
            variables: serde_json::json!({}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);

        // Should have context
        assert!(prompt.contains("Current Architecture"));
        assert!(prompt.contains("gix"));
        // Should have previous step context (reasoning loaded from provider)
        assert!(prompt.contains("Previous Step"));
        assert!(prompt.contains("extract trait"));
        // Should have guardrails
        assert!(prompt.contains("Guardrails"));
        assert!(prompt.contains("cargo test"));
    }

    #[test]
    fn test_user_prompt_failure_recovery() {
        let builder = PromptBuilderImpl::with_provider(Box::new(MockProvider::with_persona()));
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.retry
  Scenario: Retry
    When: Try again
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Fix build".to_string(),
            current_state: "retry".to_string(),
            workflow_name: "test".to_string(),
            available_tools: vec![],
            tool_schemas: vec![],
            guardrails: vec![],
            previous_outcome: Some(StepOutcome {
                state: "coding".to_string(),
                agent_session_id: "s1".to_string(),
                success: false,
                eval_score: None,
                next_state: None,
                error: Some("cargo check failed".to_string()),
                summary: None,
            }),
            variables: serde_json::json!({}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);
        assert!(prompt.contains("failed"));
        assert!(prompt.contains("different approach"));
    }

    #[test]
    fn test_system_prompt_noop_fallback() {
        let builder = PromptBuilderImpl::new(); // NoopContextProvider
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Do things

  Assignee:
    working: coder
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(&workflow, "working", scenario, Some("coder"));

        // Should have assignee name but no persona data
        assert!(prompt.contains("coder"));
        assert!(prompt.contains("Rules"));
    }

    #[test]
    fn test_variable_substitution() {
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Process {{task_id}}
"#,
        );
        let builder = PromptBuilderImpl::new();
        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Do it".to_string(),
            current_state: "working".to_string(),
            workflow_name: "test".to_string(),
            available_tools: vec![],
            tool_schemas: vec![],
            guardrails: vec![],
            previous_outcome: None,
            variables: serde_json::json!({"task_id": "abc-123"}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);
        assert!(prompt.contains("Process abc-123"));
    }

    #[test]
    fn test_minimal_prompt_no_state_leak() {
        let workflow = parse_workflow(
            r#"@workflow
Feature: Complex Pipeline
  Description: A big multi-step pipeline

  @state.one
  Scenario: Step one
    When: First thing
  @state.two
  Scenario: Step two
    When: Second thing
  @state.three
  Scenario: Step three
    When: Third thing

  Transitions:
    one -> two: auto
    two -> three: auto
"#,
        );
        let builder = PromptBuilderImpl::new();
        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(&workflow, "one", scenario, None);

        // Must NOT leak other steps or state machine
        assert!(!prompt.contains("Step two"));
        assert!(!prompt.contains("Step three"));
        assert!(!prompt.contains("Transitions"));
    }

    #[test]
    fn test_guardrails_rendered() {
        let builder = PromptBuilderImpl::new();
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Write code
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Build".to_string(),
            current_state: "working".to_string(),
            workflow_name: "test".to_string(),
            available_tools: vec![],
            tool_schemas: vec![],
            guardrails: vec!["cargo test --workspace".to_string()],
            previous_outcome: None,
            variables: serde_json::json!({}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);
        assert!(prompt.contains("Guardrails"));
        assert!(prompt.contains("cargo test --workspace"));
        assert!(prompt.contains("must exit 0"));
    }

    #[test]
    fn test_persona_fap_table_rendering() {
        let builder = PromptBuilderImpl::with_provider(Box::new(MockProvider::with_persona()));
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Work

  Assignee:
    working: architect
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(&workflow, "working", scenario, Some("architect"));
        assert!(prompt.contains("Behavioral Framework"));
        assert!(prompt.contains("WHO"));
        assert!(prompt.contains("WHAT"));
    }

    #[test]
    fn test_adr_rendering() {
        let builder = PromptBuilderImpl::with_provider(Box::new(MockProvider::with_persona()));
        let workflow = parse_workflow(
            r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Work
"#,
        );
        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(&workflow, "working", scenario, None);
        assert!(prompt.contains("Architecture Decisions"));
        assert!(prompt.contains("ADR-012"));
        assert!(prompt.contains("trait-based"));
    }

    // ─── Full entity-graph integration test (knowledge 4109364e) ────────────
    //
    // This test uses the REAL entity content from engram entities linked to
    // task 0faeecc0. It verifies the complete prompt generation flow described
    // in knowledge entity 4109364e "How to use this entity set for testing".
    //
    // Entity inventory used:
    //   Personas:  713d1fc9 (architect), 689f85ad (deconstructor),
    //              8683728d (researcher), 3901135c (implementer)
    //   ADR:       f5d4a8a9 (ADR-020)
    //   Knowledge: 97a39ba1, 966c1abf (prompt templates)
    //   Context:   a2ee4b9a (weaknesses), ff23330b (entity fields)
    //   Reasoning: 7d489b2f (pre-load vs tool-call strategy)

    /// Provider that returns the REAL engram entity content for task 0faeecc0.
    struct EntityGraphProvider {
        #[allow(dead_code)]
        /// Which persona to return (assignee slug without -prompt-gen suffix).
        persona_slug: String,
    }

    impl EntityGraphProvider {
        fn new(persona_slug: &str) -> Self {
            Self { persona_slug: persona_slug.to_string() }
        }
    }

    impl EngramContextProvider for EntityGraphProvider {
        fn load_persona(&self, assignee: &str) -> Option<PersonaData> {
            // Real persona instructions from engram entities
            match assignee {
                // ID: 713d1fc9 — architect-prompt-gen
                "architect" => Some(PersonaData {
                    instructions: "You are a software architect. Your role is to explore problem spaces openly, consider multiple approaches, identify risks and unknowns, and record all ideas with proper context. When brainstorming, do not constrain yourself prematurely. Consider: architectural patterns, dependency implications, trait design, WASM compatibility, and cross-crate cohesion. Record findings as Context entities tagged with the appropriate phase.".to_string(),
                    fap_table: vec![],
                    ov_requirements: vec![],
                }),
                // ID: 689f85ad — deconstructor-prompt-gen
                "deconstructor" => Some(PersonaData {
                    instructions: "You are a deconstructor. Your role is to take broad brainstorm ideas and narrow them into specific, actionable scope. Define clear boundaries (what is in/out), measurable acceptance criteria, identify affected crates, and assess risks. Output a Reasoning entity with structured steps and conclusion. Be precise and unforgiving of vague scope.".to_string(),
                    fap_table: vec![],
                    ov_requirements: vec![],
                }),
                // ID: 8683728d — researcher-prompt-gen
                "researcher" => Some(PersonaData {
                    instructions: "You are a researcher. Investigate existing code, dependencies, and patterns before proposing solutions. Use file_read, file_ls, and bash tools to explore the codebase. Look at similar implementations in engram, vincents-llm, and agentic-loop crates. Store findings as Knowledge entities with appropriate knowledge_type (fact, pattern, rule, procedure). Reference ADRs and existing Context entities.".to_string(),
                    fap_table: vec![],
                    ov_requirements: vec![],
                }),
                // ID: 3901135c — implementer-prompt-gen
                "implementer" => Some(PersonaData {
                    instructions: "You are an implementer. Your role is to write clean, trait-first Rust code following the plan from the research and refinement phases. Follow existing code conventions in the crate you are editing. Use the provided ADRs and Knowledge entities as constraints. Run engram task list to track progress. Write tests following the test-harness-review skill (10 categories). Commit with engram task UUID references.".to_string(),
                    fap_table: vec![],
                    ov_requirements: vec![],
                }),
                _ => None,
            }
        }

        fn load_task_context(&self, _task_id: &str, _tags: &[String]) -> Vec<ContextEntry> {
            // Real context entities linked to task 0faeecc0
            vec![
                // ID: a2ee4b9a — Current prompt builder weaknesses
                ContextEntry {
                    title: "Current prompt builder weaknesses".to_string(),
                    content: "build_system_prompt ignores: workflow description, state description, state config, guardrails, references. Assignee is just a string name - persona instructions from YAML are never loaded. The DEFAULT_SYSTEM_PREFIX is 6 generic rules with almost no signal.".to_string(),
                    relevance: "High".to_string(),
                },
                // ID: ff23330b — Engram entities available for prompt enrichment
                ContextEntry {
                    title: "Engram entities available for prompt enrichment".to_string(),
                    content: "Persona: instructions, cov_questions, fap_table, ov_requirements (173 YAML files embedded). Context: title, content, source, relevance, tags, related_entities. Reasoning: steps, conclusion, confidence, task_id. Knowledge: title, content, knowledge_type, confidence, tags. Task: description, status, priority, context_ids, knowledge. Relationship: source_id, target_id, relationship_type. ADR: title, context, decision, consequences.".to_string(),
                    relevance: "Critical".to_string(),
                },
            ]
        }

        fn load_reasoning(&self, _task_id: &str, _state: &str) -> Option<ReasoningData> {
            // Real reasoning entity: ID 7d489b2f
            Some(ReasoningData {
                steps: vec![
                    "Pre-loading entities into prompts eliminates round-trip tool calls.".to_string(),
                    "Token cost analysis: injecting 2K tokens of context upfront vs 4-6K tokens for tool-call discovery.".to_string(),
                    "Risk: stale context if entities change mid-workflow.".to_string(),
                ],
                conclusion: "Pre-load approach is superior for deterministic workflows where entity set is known at step start.".to_string(),
                confidence: 0.85,
            })
        }

        fn load_knowledge(&self, _query: &str, _limit: usize) -> Vec<KnowledgeEntry> {
            // Real knowledge entities linked to task 0faeecc0
            vec![
                // ID: 97a39ba1 — System prompt template with persona injection
                KnowledgeEntry {
                    title: "System prompt template with persona injection".to_string(),
                    content: "System prompt should be built as: (1) persona instructions from YAML, (2) fap_table as behavioral framing, (3) ov_requirements as operational constraints, (4) relevant ADRs, (5) project knowledge, (6) control + work tools.".to_string(),
                    knowledge_type: "Procedure".to_string(),
                    confidence: 0.90,
                },
                // ID: 966c1abf — User prompt template with pre-loaded context
                KnowledgeEntry {
                    title: "User prompt template with pre-loaded context".to_string(),
                    content: "User prompt should be built as: (1) Task with full entity fields, (2) Given with pre-loaded Context entities, (3) Do with When steps, (4) Expected with Then steps, (5) Previous Reasoning, (6) Guardrails, (7) Available Tools with schemas.".to_string(),
                    knowledge_type: "Procedure".to_string(),
                    confidence: 0.90,
                },
            ]
        }

        fn load_adrs(&self, _tags: &[String]) -> Vec<AdrEntry> {
            // Real ADR-020: ID f5d4a8a9
            vec![AdrEntry {
                title: "ADR-020: Pre-load engram entities into agent prompts".to_string(),
                context: "Prompt builder treats engram as a tool the LLM calls, not a context source. This wastes tokens and delays context discovery.".to_string(),
                decision: "Pre-load all relevant engram entities (Persona, Context, Reasoning, Knowledge, ADRs) into prompts at build time.".to_string(),
                consequences: "Higher upfront token cost but fewer total tokens used. Requires EngramContextProvider trait to abstract engram Storage.".to_string(),
            }]
        }
    }

    /// Test the complete prompt generation flow for the brainstorming state
    /// with the architect persona, exactly as described in knowledge 4109364e.
    ///
    /// Verifies:
    ///   System prompt: persona instructions, ADR-020, knowledge, tools
    ///   User prompt:   task, context entities, reasoning, guardrails, error recovery
    #[test]
    fn test_entity_graph_system_prompt_architect() {
        let builder = PromptBuilderImpl::with_provider(
            Box::new(EntityGraphProvider::new("architect"))
        );

        let workflow = parse_workflow(
            r#"@workflow
Feature: Prompt Generation
  Description: Improve prompt generation to use engram entities

  @state.brainstorming
  Scenario: Brainstorm approaches
    Given: Task 0faeecc0 is assigned
    When: Explore the prompt builder and engram entity graph
    Then: Record findings as Context entities
    Tools:
      file_read:
        Usage: file_read --path <path>
      engram_query:
        Usage: engram_query --type Context --tags brainstorm

  @state.refining
  Scenario: Refine scope
    When: Narrow brainstorm into actionable scope
  @state.researching
  Scenario: Research codebase
    When: Investigate existing implementations
  @state.implementing
  Scenario: Implement changes
    When: Write the improved prompt builder

  Transitions:
    brainstorming -> refining: auto
    refining -> researching: auto
    researching -> implementing: auto

  Assignee:
    brainstorming: architect
    refining: deconstructor
    researching: researcher
    implementing: implementer
"#,
        );

        let scenario = workflow.scenarios.first().unwrap();
        let prompt = builder.build_system_prompt(
            &workflow,
            "brainstorming",
            scenario,
            Some("architect"),
        );

        // ── 1. Persona instructions (from 713d1fc9 architect-prompt-gen) ─────
        assert!(prompt.contains("software architect"),
            "system prompt must contain architect persona instructions");
        assert!(prompt.contains("explore problem spaces"),
            "system prompt must contain persona instructions detail");
        assert!(prompt.contains("architectural patterns"),
            "system prompt must contain persona domain keywords");

        // ── 2. Workflow description ─────────────────────────────────────────
        assert!(prompt.contains("Improve prompt generation"),
            "system prompt must include workflow description");

        // ── 3. ADR-020 (from f5d4a8a9) ─────────────────────────────────────
        assert!(prompt.contains("Architecture Decisions"),
            "system prompt must have ADR section");
        assert!(prompt.contains("ADR-020"),
            "system prompt must contain ADR-020 title");
        assert!(prompt.contains("Pre-load all relevant engram entities"),
            "system prompt must contain ADR decision");
        assert!(prompt.contains("EngramContextProvider"),
            "system prompt must reference ADR consequences");

        // ── 4. Project Knowledge (97a39ba1, 966c1abf) ───────────────────────
        assert!(prompt.contains("Project Knowledge"),
            "system prompt must have knowledge section");
        assert!(prompt.contains("persona instructions from YAML"),
            "system prompt must contain system prompt template knowledge");
        assert!(prompt.contains("User prompt should be built as"),
            "system prompt must contain user prompt template knowledge");

        // ── 5. Rules (always present) ───────────────────────────────────────
        assert!(prompt.contains("Rules"),
            "system prompt must have rules section");
        assert!(prompt.contains("done tool"),
            "system prompt must reference done tool");

        // ── 6. Tools ───────────────────────────────────────────────────────
        assert!(prompt.contains("file_read"),
            "system prompt must list work tool file_read");
        assert!(prompt.contains("engram_query"),
            "system prompt must list work tool engram_query");
        assert!(prompt.contains("done"),
            "system prompt must list control tools");
        assert!(prompt.contains("failed"),
            "system prompt must list control tool failed");

        // ── 7. Must NOT leak state machine ──────────────────────────────────
        assert!(!prompt.contains("refining"),
            "system prompt must not leak other states");
        assert!(!prompt.contains("researching"),
            "system prompt must not leak other states");
        assert!(!prompt.contains("Transitions"),
            "system prompt must not leak transitions");
    }

    #[test]
    fn test_entity_graph_user_prompt_with_previous_reasoning() {
        let builder = PromptBuilderImpl::with_provider(
            Box::new(EntityGraphProvider::new("deconstructor"))
        );

        let workflow = parse_workflow(
            r#"@workflow
Feature: Prompt Generation
  Description: Improve prompt generation to use engram entities

  @state.refining
  Scenario: Refine scope
    Given: Brainstorm context entities exist
    When: Narrow brainstorm into actionable scope
    Then: Output Reasoning entity with structured steps and conclusion
"#,
        );

        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Improve prompt generation to use engram entities (task 0faeecc0)".to_string(),
            current_state: "refining".to_string(),
            workflow_name: "Prompt Generation".to_string(),
            available_tools: vec!["file_read".to_string(), "engram_query".to_string(), "done".to_string()],
            tool_schemas: vec![],
            guardrails: vec!["cargo test --workspace".to_string(), "agent-check".to_string()],
            previous_outcome: Some(StepOutcome {
                state: "brainstorming".to_string(),
                agent_session_id: "session-abc".to_string(),
                success: true,
                eval_score: Some(0.92),
                next_state: None,
                error: None,
                summary: None,
            }),
            variables: serde_json::json!({"task_id": "0faeecc0"}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);

        // ── 1. Task (full description, not just string) ─────────────────────
        assert!(prompt.contains("0faeecc0"),
            "user prompt must contain task ID");
        assert!(prompt.contains("Improve prompt generation"),
            "user prompt must contain full task description");

        // ── 2. Context entities (a2ee4b9a, ff23330b) ────────────────────────
        assert!(prompt.contains("## Context"),
            "user prompt must have Context section");
        assert!(prompt.contains("Current prompt builder weaknesses"),
            "user prompt must contain context entity a2ee4b9a");
        assert!(prompt.contains("persona instructions from YAML are never loaded"),
            "user prompt must contain real weakness content");
        assert!(prompt.contains("Engram entities available for prompt enrichment"),
            "user prompt must contain context entity ff23330b");
        assert!(prompt.contains("fap_table"),
            "user prompt must contain entity field details");

        // ── 3. Given ───────────────────────────────────────────────────────
        assert!(prompt.contains("Brainstorm context entities exist"),
            "user prompt must contain Given preconditions");

        // ── 4. Do ──────────────────────────────────────────────────────────
        assert!(prompt.contains("Narrow brainstorm"),
            "user prompt must contain Do actions");

        // ── 5. Expected ────────────────────────────────────────────────────
        assert!(prompt.contains("Reasoning entity"),
            "user prompt must contain Expected outcomes");

        // ── 6. Previous reasoning (from 7d489b2f) ───────────────────────────
        assert!(prompt.contains("Previous Step"),
            "user prompt must have Previous Step section");
        assert!(prompt.contains("Pre-loading entities into prompts"),
            "user prompt must contain real reasoning steps");
        assert!(prompt.contains("Pre-load approach is superior"),
            "user prompt must contain reasoning conclusion");
        assert!(prompt.contains("85%"),
            "user prompt must contain reasoning confidence");

        // ── 7. Guardrails ──────────────────────────────────────────────────
        assert!(prompt.contains("## Guardrails"),
            "user prompt must have Guardrails section");
        assert!(prompt.contains("cargo test --workspace"),
            "user prompt must contain first guardrail");
        assert!(prompt.contains("agent-check"),
            "user prompt must contain second guardrail");
        assert!(prompt.contains("must exit 0"),
            "guardrails must specify exit 0 requirement");

        // ── 8. Tools reference ─────────────────────────────────────────────
        assert!(prompt.contains("file_read"),
            "user prompt must list available tools");
        assert!(prompt.contains("engram_query"),
            "user prompt must list available tools");

        // ── 9. Previous step reasoning (loaded from provider) ────────────
        assert!(prompt.contains("brainstorming"),
            "user prompt must reference previous state");
        assert!(prompt.contains("Previous Step"),
            "user prompt must have Previous Step section");
        assert!(prompt.contains("Pre-load approach"),
            "user prompt must contain reasoning conclusion");
    }

    #[test]
    fn test_entity_graph_user_prompt_failure_recovery() {
        let builder = PromptBuilderImpl::with_provider(
            Box::new(EntityGraphProvider::new("implementer"))
        );

        let workflow = parse_workflow(
            r#"@workflow
Feature: Prompt Generation
  Description: Improve prompt generation to use engram entities

  @state.implementing
  Scenario: Implement changes
    When: Write the improved prompt builder
    Then: All tests pass
"#,
        );

        let scenario = workflow.scenarios.first().unwrap();
        let context = StepContext {
            task_description: "Rewrite prompt.rs with EngramContextProvider".to_string(),
            current_state: "implementing".to_string(),
            workflow_name: "Prompt Generation".to_string(),
            available_tools: vec!["file_edit".to_string(), "bash".to_string(), "done".to_string()],
            tool_schemas: vec![],
            guardrails: vec!["cargo test -p agentic-loop-workflow".to_string()],
            previous_outcome: Some(StepOutcome {
                state: "researching".to_string(),
                agent_session_id: "session-xyz".to_string(),
                success: false,
                eval_score: None,
                next_state: None,
                error: Some("git2 version conflict blocks engram import".to_string()),
                summary: None,
            }),
            variables: serde_json::json!({}),
                model_hint: None,
        };

        let prompt = builder.build_user_prompt(&context, scenario);

        // ── Failure-specific: error shown ──────────────────────────────────
        assert!(prompt.contains("failed"),
            "user prompt must indicate failure");
        assert!(prompt.contains("git2 version conflict"),
            "user prompt must contain error detail");

        // ── Error recovery guidance ────────────────────────────────────────
        assert!(prompt.contains("different approach"),
            "user prompt must suggest different approach on failure");

        // ── Previous step still loaded ────────────────────────────────
        assert!(prompt.contains("Previous step"),
            "user prompt must still show previous step on failure");

        // ── Guardrails still rendered ──────────────────────────────────────
        assert!(prompt.contains("Guardrails"),
            "user prompt must still render guardrails on failure");
        assert!(prompt.contains("cargo test -p agentic-loop-workflow"),
            "user prompt must contain guardrail command");
    }

    #[test]
    fn test_entity_graph_all_four_personas() {
        // Verify all 4 personas from knowledge 4109364e load correctly
        for (slug, expected_phrase) in [
            ("architect", "explore problem spaces"),
            ("deconstructor", "narrow them into specific"),
            ("researcher", "Investigate existing code"),
            ("implementer", "trait-first Rust code"),
        ] {
            let builder = PromptBuilderImpl::with_provider(
                Box::new(EntityGraphProvider::new(slug))
            );
            let workflow = parse_workflow(
                r#"@workflow
Feature: Test
  Description: test

  @state.working
  Scenario: Work
    When: Work
"#,
            );
            let scenario = workflow.scenarios.first().unwrap();
            let prompt = builder.build_system_prompt(
                &workflow, "working", scenario, Some(slug),
            );
            assert!(prompt.contains(expected_phrase),
                "persona '{}' must inject its instructions (expected: {})", slug, expected_phrase);
        }
    }
}
