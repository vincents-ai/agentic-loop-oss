//! Workflow data types.

use serde::{Deserialize, Serialize};

/// A parsed .workflow file (Gherkin-like template).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub states: Vec<State>,
    pub initial_state: String,
    pub scenarios: Vec<Scenario>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub guardrails: Vec<String>,
    #[serde(default)]
    pub assignee: Option<Vec<String>>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub config: serde_json::Value,
    /// Included workflows — reusable state/scenario snippets.
    #[serde(default)]
    pub includes: Vec<IncludeWorkflow>,
    /// State-to-workflow delegations.
    #[serde(default)]
    pub delegates: Vec<Delegate>,
}

/// A named state within a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// A scenario (step definition) within a workflow state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub state: String,
    pub name: String,
    pub given: Vec<String>,
    pub when: Vec<String>,
    pub then: Vec<String>,
    /// Tool names available for this scenario.
    /// Empty = all tools available.
    pub tools: Vec<String>,
    /// Model type hint for this scenario (e.g., "fast,free", "coding", "smart").
    /// Empty = use default model selection.
    #[serde(default)]
    pub model_hint: Option<String>,
}

/// A transition between workflow states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
}

/// Data overlay for a .workflow template (project-specific).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDataOverlay {
    pub workflow_name: String,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub variables: serde_json::Value,
    #[serde(default)]
    pub state_overrides: serde_json::Value,
}

/// Resolved workflow ready for execution (template + overlay merged).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedWorkflow {
    pub workflow: Workflow,
    pub overlay: WorkflowDataOverlay,
}

/// Include directive — imports reusable states/scenarios from another workflow.
///
/// In the DSL:
/// ```gherkin
/// Include:
///   - workflow: shared/review_steps
///     states: [review, approve, reject]
///   - workflow: shared/testing
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludeWorkflow {
    /// Name/path of the workflow to include.
    pub workflow: String,
    /// Specific states to include (empty = all).
    #[serde(default)]
    pub states: Vec<String>,
    /// Optional prefix for included state names (to avoid collisions).
    #[serde(default)]
    pub prefix: Option<String>,
    /// Variables to pass to the included workflow.
    #[serde(default)]
    pub variables: serde_json::Value,
}

/// Delegate directive — delegates a state's execution to another workflow.
///
/// In the DSL:
/// ```gherkin
/// Delegate:
///   implement -> implementation_workflow:
///     pass_variables: true
///     on_complete: return
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegate {
    /// Source state name that triggers delegation.
    pub from_state: String,
    /// Target workflow to delegate to.
    pub workflow: String,
    /// Whether to pass parent variables to the child workflow.
    #[serde(default = "default_true_val")]
    pub pass_variables: bool,
    /// What to do when the delegated workflow completes.
    #[serde(default)]
    pub on_complete: DelegateCompletion,
    /// Variables to inject into the child workflow.
    #[serde(default)]
    pub inject: serde_json::Value,
}

/// What happens when a delegated workflow completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DelegateCompletion {
    /// Return to the parent workflow and continue.
    #[serde(rename = "return")]
    Return,
    /// Move to a specific state in the parent workflow.
    #[serde(rename = "goto")]
    Goto(String),
    /// The delegation result becomes the parent's outcome.
    #[serde(rename = "merge")]
    Merge,
}

impl Default for DelegateCompletion {
    fn default() -> Self { Self::Return }
}

fn default_true_val() -> bool { true }
