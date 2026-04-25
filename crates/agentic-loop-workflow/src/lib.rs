//! # agentic-loop-workflow
//!
//! Workflow engine traits, custom .workflow parser, and execution engine.

pub mod parser;
pub mod engine;
pub mod prompt;
pub mod adaptation;
pub mod builtin_steps;
pub mod resolver;
pub mod compiler;
pub mod visualization;

pub use parser::WorkflowParserImpl;
pub use engine::{WorkflowEngineImpl, WorkflowSession};
pub use prompt::CONTROL_TOOLS;
pub use prompt::PromptBuilderImpl;
pub use prompt::{
    EngramContextProvider, NoopContextProvider,
    PersonaData, ContextEntry, ReasoningData, KnowledgeEntry, AdrEntry,
};

/// Outcome of executing a single workflow step.
#[derive(Debug, Clone)]
pub struct StepOutcome {
    /// State that was executed.
    pub state: String,
    /// Agent session ID (for pi-compatible JSONL).
    pub agent_session_id: String,
    /// Whether the step succeeded.
    pub success: bool,
    /// Evaluation score (0.0 - 1.0), if eval ran.
    pub eval_score: Option<f64>,
    /// Next state to transition to, if determined.
    pub next_state: Option<String>,
    /// Error message if step failed.
    pub error: Option<String>,
    /// Brief summary of what was accomplished (passed to next step as context).
    pub summary: Option<String>,
}

/// Context for building a step prompt.
#[derive(Debug, Clone)]
pub struct StepContext {
    pub task_description: String,
    pub current_state: String,
    pub workflow_name: String,
    pub available_tools: Vec<String>,
    /// Full tool info (name, description, parameter schema) for prompt rendering.
    pub tool_schemas: Vec<agentic_loop_types::tool::ToolInfo>,
    pub guardrails: Vec<String>,
    pub previous_outcome: Option<StepOutcome>,
    pub variables: serde_json::Value,
    /// Model type hint from workflow state (e.g., "fast,free", "coding").
    pub model_hint: Option<String>,
}

/// Status of a workflow session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Aborted,
}

/// Registry of custom step handlers.
///
/// Allows registering named step handlers that can be referenced
/// in .workflow files (e.g. `Given I use step "review_code"`).
pub trait StepRegistry: Send + Sync {
    /// Register a step handler by name.
    fn register(&mut self, name: String, handler: Box<dyn StepHandler>);

    /// Get a registered step handler.
    fn get(&self, name: &str) -> Option<&dyn StepHandler>;

    /// List all registered step names.
    fn list(&self) -> Vec<String>;
}

/// A custom step handler that can execute workflow steps.
#[async_trait::async_trait]
pub trait StepHandler: Send + Sync {
    /// Name of this step handler.
    fn name(&self) -> &str;

    /// Description of what this step does.
    fn description(&self) -> &str;

    /// Execute the step with the given context.
    async fn execute(&self, context: &StepContext) -> StepOutcome;
}

/// Human checkpoint — pauses workflow until a human approves.
///
/// Defined in .workflow files as:
/// ```gherkin
/// Then I wait for human approval on "description"
/// ```
#[derive(Debug, Clone)]
pub struct HumanCheckpoint {
    /// Description of what needs human review.
    pub description: String,
    /// Who needs to review (None = anyone).
    pub required_reviewer: Option<String>,
    /// Timeout in seconds (None = no timeout).
    pub timeout_secs: Option<u64>,
    /// Whether the checkpoint was approved.
    pub approved: Option<bool>,
    /// Optional reason for approval/rejection.
    pub reason: Option<String>,
}

impl HumanCheckpoint {
    /// Create a new checkpoint requiring human approval.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            required_reviewer: None,
            timeout_secs: None,
            approved: None,
            reason: None,
        }
    }

    /// Set required reviewer.
    pub fn with_reviewer(mut self, reviewer: impl Into<String>) -> Self {
        self.required_reviewer = Some(reviewer.into());
        self
    }

    /// Set timeout.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Approve the checkpoint.
    pub fn approve(mut self, reason: Option<String>) -> Self {
        self.approved = Some(true);
        self.reason = reason;
        self
    }

    /// Reject the checkpoint.
    pub fn reject(mut self, reason: Option<String>) -> Self {
        self.approved = Some(false);
        self.reason = reason;
        self
    }

    /// Whether the checkpoint has been resolved.
    pub fn is_resolved(&self) -> bool {
        self.approved.is_some()
    }

    /// Whether the checkpoint was approved.
    pub fn is_approved(&self) -> bool {
        self.approved == Some(true)
    }
}

/// Default in-memory StepRegistry implementation.
pub struct InMemoryStepRegistry {
    handlers: std::collections::HashMap<String, Box<dyn StepHandler>>,
}

impl InMemoryStepRegistry {
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }
}

impl Default for InMemoryStepRegistry {
    fn default() -> Self { Self::new() }
}

impl StepRegistry for InMemoryStepRegistry {
    fn register(&mut self, name: String, handler: Box<dyn StepHandler>) {
        self.handlers.insert(name, handler);
    }

    fn get(&self, name: &str) -> Option<&dyn StepHandler> {
        self.handlers.get(name).map(|h| h.as_ref())
    }

    fn list(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoStepHandler;

    #[async_trait::async_trait]
    impl StepHandler for EchoStepHandler {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echo step" }
        async fn execute(&self, ctx: &StepContext) -> StepOutcome {
            StepOutcome {
                state: ctx.current_state.clone(),
                agent_session_id: "echo".to_string(),
                success: true,
                eval_score: Some(1.0),
                next_state: None,
                error: None,
                summary: None,
            }
        }
    }

    #[test]
    fn test_step_registry_register_get() {
        let mut reg = InMemoryStepRegistry::new();
        reg.register("echo".to_string(), Box::new(EchoStepHandler));
        assert!(reg.get("echo").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn test_step_registry_list() {
        let mut reg = InMemoryStepRegistry::new();
        reg.register("echo".to_string(), Box::new(EchoStepHandler));
        let names = reg.list();
        assert_eq!(names, vec!["echo"]);
    }

    #[test]
    fn test_human_checkpoint_new() {
        let cp = HumanCheckpoint::new("Review this code");
        assert!(!cp.is_resolved());
        assert!(!cp.is_approved());
        assert_eq!(cp.description, "Review this code");
    }

    #[test]
    fn test_human_checkpoint_approve() {
        let cp = HumanCheckpoint::new("Review").approve(Some("LGTM".to_string()));
        assert!(cp.is_resolved());
        assert!(cp.is_approved());
        assert_eq!(cp.reason, Some("LGTM".to_string()));
    }

    #[test]
    fn test_human_checkpoint_reject() {
        let cp = HumanCheckpoint::new("Review").reject(Some("Needs fixes".to_string()));
        assert!(cp.is_resolved());
        assert!(!cp.is_approved());
    }

    #[test]
    fn test_human_checkpoint_with_reviewer() {
        let cp = HumanCheckpoint::new("Review")
            .with_reviewer("alice")
            .with_timeout(300);
        assert_eq!(cp.required_reviewer, Some("alice".to_string()));
        assert_eq!(cp.timeout_secs, Some(300));
    }

    #[tokio::test]
    async fn test_step_handler_execute() {
        let handler = EchoStepHandler;
        let ctx = StepContext {
            task_description: "test".to_string(),
            current_state: "coding".to_string(),
            workflow_name: "dev".to_string(),
            available_tools: vec![],
            tool_schemas: vec![],
            guardrails: vec![],
            previous_outcome: None,
            variables: serde_json::json!({}),
                model_hint: None,
        };
        let outcome = handler.execute(&ctx).await;
        assert!(outcome.success);
        assert_eq!(outcome.state, "coding");
    }
}

/// Workflow error types.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("Workflow not found: {0}")]
    NotFound(String),

    #[error("Parse error in workflow '{name}': {reason}")]
    ParseError { name: String, reason: String },

    #[error("Invalid state transition: from '{from}' with outcome '{outcome}'")]
    InvalidTransition { from: String, outcome: String },

    #[error("No scenario for state: {0}")]
    NoScenario(String),

    #[error("Workflow already completed: {0}")]
    AlreadyCompleted(String),

    #[error("Workflow session not found: {0}")]
    SessionNotFound(String),

    #[error("Step limit exceeded: {max} steps")]
    StepLimitExceeded { max: usize },

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Workflow error: {0}")]
    Other(String),
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_workflow_error_not_found() {
        let e = WorkflowError::NotFound("dev".to_string());
        assert!(e.to_string().contains("dev"));
    }

    #[test]
    fn test_workflow_error_invalid_transition() {
        let e = WorkflowError::InvalidTransition {
            from: "working".to_string(),
            outcome: "skipped".to_string(),
        };
        assert!(e.to_string().contains("working"));
        assert!(e.to_string().contains("skipped"));
    }

    #[test]
    fn test_workflow_error_step_limit() {
        let e = WorkflowError::StepLimitExceeded { max: 50 };
        assert!(e.to_string().contains("50"));
    }
}
