//! Workflow include and delegate resolution.
//!
//! Handles:
//! - `Include:` directives — imports states/scenarios from other workflows
//! - `Delegate:` directives — delegates state execution to child workflows

use agentic_loop_types::workflow::{
    Delegate, DelegateCompletion, Scenario, State, Transition, Workflow,
};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;

/// Resolves workflow includes and delegates.
pub struct WorkflowResolver {
    workflows_path: PathBuf,
}

impl WorkflowResolver {
    pub fn new(workflows_path: PathBuf) -> Self {
        Self { workflows_path }
    }

    /// Resolve all includes in a workflow, merging states and scenarios.
    pub fn resolve_includes(&self, workflow: &mut Workflow) -> Result<Vec<String>> {
        let mut merged_states = Vec::new();
        let mut merged_scenarios = Vec::new();
        let mut merged_transitions = Vec::new();
        let mut warnings = Vec::new();

        // Keep existing states/scenarios
        merged_states.append(&mut workflow.states);
        merged_scenarios.append(&mut workflow.scenarios);
        merged_transitions.append(&mut workflow.transitions);

        let mut seen_workflows = HashSet::new();
        for include in &workflow.includes {
            if !seen_workflows.insert(include.workflow.clone()) {
                warnings.push(format!("Duplicate include: {}", include.workflow));
                continue;
            }

            match self.load_workflow(&include.workflow) {
                Ok(included) => {
                    let prefix = include.prefix.as_deref().unwrap_or("");

                    for state in &included.states {
                        let name = if prefix.is_empty() {
                            state.name.clone()
                        } else {
                            format!("{}_{}", prefix, state.name)
                        };

                        // Filter by requested states
                        if include.states.is_empty()
                            || include.states.contains(&state.name)
                        {
                            if !merged_states.iter().any(|s| s.name == name) {
                                merged_states.push(State {
                                    name,
                                    description: state.description.clone(),
                                    config: state.config.clone(),
                                });
                            }
                        }
                    }

                    for scenario in &included.scenarios {
                        let scenario_name = if prefix.is_empty() {
                            scenario.name.clone()
                        } else {
                            format!("{}_{}", prefix, scenario.name)
                        };

                        if include.states.is_empty()
                            || include.states.contains(&scenario.state)
                        {
                            if !merged_scenarios.iter().any(|s| s.name == scenario_name) {
                                merged_scenarios.push(Scenario {
                                    name: scenario_name,
                                    state: if prefix.is_empty() {
                                        scenario.state.clone()
                                    } else {
                                        format!("{}_{}", prefix, scenario.state)
                                    },
                                    given: scenario.given.clone(),
                                    when: scenario.when.clone(),
                                    then: scenario.then.clone(),
                                    tools: scenario.tools.clone(),
                                    model_hint: scenario.model_hint.clone(),
                                });
                            }
                        }
                    }

                    for transition in &included.transitions {
                        let from = if prefix.is_empty() {
                            transition.from.clone()
                        } else {
                            format!("{}_{}", prefix, transition.from)
                        };
                        let to = if prefix.is_empty() {
                            transition.to.clone()
                        } else {
                            format!("{}_{}", prefix, transition.to)
                        };

                        merged_transitions.push(Transition {
                            from,
                            to,
                            condition: transition.condition.clone(),
                        });
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "Failed to include '{}': {}",
                        include.workflow, e
                    ));
                }
            }
        }

        workflow.states = merged_states;
        workflow.scenarios = merged_scenarios;
        workflow.transitions = merged_transitions;

        Ok(warnings)
    }

    /// Find a delegate for a given state.
    pub fn find_delegate<'a>(
        workflow: &'a Workflow,
        state: &str,
    ) -> Option<&'a Delegate> {
        workflow.delegates.iter().find(|d| d.from_state == state)
    }

    /// Resolve the completion action for a delegated workflow.
    pub fn resolve_completion(
        delegate: &Delegate,
        _child_final_state: &str,
    ) -> DelegateCompletion {
        match &delegate.on_complete {
            DelegateCompletion::Return => DelegateCompletion::Return,
            DelegateCompletion::Goto(target) => {
                DelegateCompletion::Goto(target.clone())
            }
            DelegateCompletion::Merge => DelegateCompletion::Merge,
        }
    }

    /// Load a workflow by name from the workflows path.
    fn load_workflow(&self, name: &str) -> Result<Workflow> {
        // Try with and without .workflow extension
        let candidates = [
            format!("{}.workflow", name),
            format!("{}.workflow", name.replace('.', "/")),
            name.to_string(),
        ];

        for candidate in &candidates {
            let path = self.workflows_path.join(candidate);
            if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("Reading {}", path.display()))?;
                let parser = crate::parser::WorkflowParserImpl::new();
                let workflow = parser.parse(&content)?;
                return Ok(workflow);
            }
        }

        anyhow::bail!(
            "Workflow '{}' not found in {}",
            name,
            self.workflows_path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentic_loop_types::workflow::IncludeWorkflow;

    #[test]
    fn test_find_delegate() {
        let workflow = Workflow {
            name: "parent".to_string(),
            description: String::new(),
            states: vec![],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![],
            delegates: vec![Delegate {
                from_state: "implement".to_string(),
                workflow: "implementation_workflow".to_string(),
                pass_variables: true,
                on_complete: DelegateCompletion::Return,
                inject: serde_json::Value::Null,
            }],
        };

        assert!(WorkflowResolver::find_delegate(&workflow, "implement").is_some());
        assert!(WorkflowResolver::find_delegate(&workflow, "plan").is_none());
    }

    #[test]
    fn test_resolve_completion() {
        let delegate = Delegate {
            from_state: "implement".to_string(),
            workflow: "impl".to_string(),
            pass_variables: true,
            on_complete: DelegateCompletion::Goto("review".to_string()),
            inject: serde_json::Value::Null,
        };
        let completion = WorkflowResolver::resolve_completion(&delegate, "done");
        assert_eq!(completion, DelegateCompletion::Goto("review".to_string()));
    }

    #[test]
    fn test_resolve_completion_return() {
        let delegate = Delegate {
            from_state: "test".to_string(),
            workflow: "test_wf".to_string(),
            pass_variables: true,
            on_complete: DelegateCompletion::Return,
            inject: serde_json::Value::Null,
        };
        let completion = WorkflowResolver::resolve_completion(&delegate, "done");
        assert_eq!(completion, DelegateCompletion::Return);
    }

    #[test]
    fn test_resolve_includes_empty() {
        let resolver = WorkflowResolver::new(PathBuf::from("/tmp"));
        let mut workflow = Workflow {
            name: "test".to_string(),
            description: String::new(),
            states: vec![State { name: "start".to_string(), description: "Start".to_string(), config: serde_json::Value::Null }],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![],
            delegates: vec![],
        };
        let warnings = resolver.resolve_includes(&mut workflow).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(workflow.states.len(), 1);
    }

    #[test]
    fn test_resolve_includes_missing_workflow() {
        let resolver = WorkflowResolver::new(PathBuf::from("/nonexistent"));
        let mut workflow = Workflow {
            name: "test".to_string(),
            description: String::new(),
            states: vec![],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![IncludeWorkflow {
                workflow: "missing".to_string(),
                states: vec![],
                prefix: None,
                variables: serde_json::Value::Null,
            }],
            delegates: vec![],
        };
        let warnings = resolver.resolve_includes(&mut workflow).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Failed to include"));
    }

    #[test]
    fn test_include_workflow_serialization() {
        let inc = IncludeWorkflow {
            workflow: "shared/review".to_string(),
            states: vec!["review".to_string(), "approve".to_string()],
            prefix: Some("shared_".to_string()),
            variables: serde_json::json!({"reviewer": "team-lead"}),
        };
        let json = serde_json::to_string(&inc).unwrap();
        let parsed: IncludeWorkflow = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.workflow, "shared/review");
        assert_eq!(parsed.states.len(), 2);
        assert_eq!(parsed.prefix.as_deref(), Some("shared_"));
    }

    #[test]
    fn test_delegate_serialization() {
        let del = Delegate {
            from_state: "implement".to_string(),
            workflow: "impl_wf".to_string(),
            pass_variables: true,
            on_complete: DelegateCompletion::Goto("review".to_string()),
            inject: serde_json::json!({"strict": true}),
        };
        let json = serde_json::to_string(&del).unwrap();
        let parsed: Delegate = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from_state, "implement");
        assert_eq!(parsed.on_complete, DelegateCompletion::Goto("review".to_string()));
    }

    #[test]
    fn test_delegate_completion_default() {
        assert_eq!(DelegateCompletion::default(), DelegateCompletion::Return);
    }

    #[test]
    fn test_duplicate_include_warning() {
        let resolver = WorkflowResolver::new(PathBuf::from("/nonexistent"));
        let mut workflow = Workflow {
            name: "test".to_string(),
            description: String::new(),
            states: vec![],
            initial_state: "start".to_string(),
            scenarios: vec![],
            transitions: vec![],
            guardrails: vec![],
            assignee: None,
            references: vec![],
            config: serde_json::Value::Null,
            includes: vec![
                IncludeWorkflow {
                    workflow: "shared".to_string(),
                    states: vec![],
                    prefix: None,
                    variables: serde_json::Value::Null,
                },
                IncludeWorkflow {
                    workflow: "shared".to_string(),
                    states: vec![],
                    prefix: None,
                    variables: serde_json::Value::Null,
                },
            ],
            delegates: vec![],
        };
        let warnings = resolver.resolve_includes(&mut workflow).unwrap();
        assert!(warnings.iter().any(|w| w.contains("Duplicate")));
    }
}
