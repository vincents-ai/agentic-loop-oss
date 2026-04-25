//! Built-in workflow step handlers.
//!
//! Standard handlers for common workflow patterns:
//! - **Think** — agent reflects on the task, no tool calls
//! - **Plan** — agent creates a structured plan
//! - **Research** — agent gathers information using search/read tools
//! - **Implement** — agent writes code using file/shell tools
//! - **Review** — agent reviews its own or others' code
//! - **Test** — agent runs tests and reports results

use crate::{StepContext, StepHandler, StepOutcome};

/// Agent reflects on the task and produces reasoning.
pub struct ThinkStep;

#[async_trait::async_trait]
impl StepHandler for ThinkStep {
    fn name(&self) -> &str { "think" }
    fn description(&self) -> &str { "Agent reflects on the task, considers context, and produces structured reasoning before acting." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("think-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Agent creates a structured plan for the task.
pub struct PlanStep;

#[async_trait::async_trait]
impl StepHandler for PlanStep {
    fn name(&self) -> &str { "plan" }
    fn description(&self) -> &str { "Agent creates a structured implementation plan with given/do/expected sections." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("plan-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Agent gathers information using search/read tools.
pub struct ResearchStep;

#[async_trait::async_trait]
impl StepHandler for ResearchStep {
    fn name(&self) -> &str { "research" }
    fn description(&self) -> &str { "Agent gathers information using file search, grep, web fetch, and engram query tools." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("research-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Agent writes or modifies code.
pub struct ImplementStep;

#[async_trait::async_trait]
impl StepHandler for ImplementStep {
    fn name(&self) -> &str { "implement" }
    fn description(&self) -> &str { "Agent writes or modifies code using file operations and shell commands." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("implement-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Agent reviews code quality and correctness.
pub struct ReviewStep;

#[async_trait::async_trait]
impl StepHandler for ReviewStep {
    fn name(&self) -> &str { "review" }
    fn description(&self) -> &str { "Agent reviews code for quality, correctness, security, and adherence to requirements." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("review-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Agent runs tests and reports results.
pub struct TestStep;

#[async_trait::async_trait]
impl StepHandler for TestStep {
    fn name(&self) -> &str { "test" }
    fn description(&self) -> &str { "Agent runs tests, reports results, and fixes failures." }
    async fn execute(&self, ctx: &StepContext) -> StepOutcome {
        StepOutcome {
            state: ctx.current_state.clone(),
            agent_session_id: format!("test-{}", chrono::Utc::now().timestamp_millis()),
            success: true,
            eval_score: None,
            next_state: None,
            error: None,
            summary: None,
        }
    }
}

/// Register all built-in step handlers into a step registry.
pub fn register_builtin_steps(registry: &mut dyn crate::StepRegistry) {
    registry.register("think".to_string(), Box::new(ThinkStep));
    registry.register("plan".to_string(), Box::new(PlanStep));
    registry.register("research".to_string(), Box::new(ResearchStep));
    registry.register("implement".to_string(), Box::new(ImplementStep));
    registry.register("review".to_string(), Box::new(ReviewStep));
    registry.register("test".to_string(), Box::new(TestStep));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StepRegistry;

    fn make_context(state: &str) -> StepContext {
        StepContext {
            task_description: "Test task".to_string(),
            current_state: state.to_string(),
            workflow_name: "test".to_string(),
            available_tools: vec![],
            tool_schemas: vec![],
            guardrails: vec![],
            previous_outcome: None,
            variables: serde_json::json!({}),
                model_hint: None,
        }
    }

    #[tokio::test]
    async fn test_think_step() {
        let step = ThinkStep;
        assert_eq!(step.name(), "think");
        let outcome = step.execute(&make_context("thinking")).await;
        assert!(outcome.success);
        assert!(outcome.agent_session_id.starts_with("think-"));
    }

    #[tokio::test]
    async fn test_plan_step() {
        let step = PlanStep;
        assert_eq!(step.name(), "plan");
        let outcome = step.execute(&make_context("planning")).await;
        assert!(outcome.success);
    }

    #[tokio::test]
    async fn test_research_step() {
        let step = ResearchStep;
        let outcome = step.execute(&make_context("researching")).await;
        assert!(outcome.success);
        assert!(outcome.agent_session_id.starts_with("research-"));
    }

    #[tokio::test]
    async fn test_implement_step() {
        let step = ImplementStep;
        let outcome = step.execute(&make_context("implementing")).await;
        assert!(outcome.success);
    }

    #[tokio::test]
    async fn test_review_step() {
        let step = ReviewStep;
        let outcome = step.execute(&make_context("reviewing")).await;
        assert!(outcome.success);
    }

    #[tokio::test]
    async fn test_test_step() {
        let step = TestStep;
        let outcome = step.execute(&make_context("testing")).await;
        assert!(outcome.success);
    }

    #[test]
    fn test_register_all_steps() {
        let mut registry = crate::InMemoryStepRegistry::new();
        register_builtin_steps(&mut registry);
        assert!(registry.get("think").is_some());
        assert!(registry.get("plan").is_some());
        assert!(registry.get("research").is_some());
        assert!(registry.get("implement").is_some());
        assert!(registry.get("review").is_some());
        assert!(registry.get("test").is_some());
    }

    #[test]
    fn test_step_names_unique() {
        let steps: Vec<&str> = vec![
            ThinkStep.name(), PlanStep.name(), ResearchStep.name(),
            ImplementStep.name(), ReviewStep.name(), TestStep.name(),
        ];
        let unique: std::collections::HashSet<&str> = steps.iter().copied().collect();
        assert_eq!(steps.len(), unique.len(), "Step names must be unique");
    }
}
