//! LLM trait definitions for swappable execution.
//!
//! These traits abstract the LLM layer so the runner can work with
//! any provider implementation:
//! - `AgentRunner` — execute agent steps (send prompts, get tool calls)
//! - `CostEstimator` — estimate cost before execution
//! - `ModelInfoProvider` — query model capabilities
//!
//! `LlmExecutor` implements all three traits.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Result of a single agent step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStepResult {
    /// The assistant's text response (if any).
    pub text: Option<String>,
    /// Tool calls requested by the model.
    pub tool_calls: Vec<ToolCallDef>,
    /// Token usage.
    pub tokens_used: u32,
    /// Estimated cost in USD.
    pub estimated_cost: f64,
    /// Finish reason (e.g. "stop", "tool_calls").
    pub finish_reason: Option<String>,
}

/// A tool call definition from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDef {
    /// Tool call ID.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// JSON arguments.
    pub arguments: serde_json::Value,
}

/// Cost estimate before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated input tokens.
    pub input_tokens: u32,
    /// Estimated output tokens.
    pub output_tokens: u32,
    /// Estimated cost in USD.
    pub cost_usd: f64,
    /// Confidence level (0.0–1.0).
    pub confidence: f64,
}

/// Model capability info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Model identifier (e.g. "gpt-4o").
    pub model_id: String,
    /// Provider name (e.g. "openai").
    pub provider: String,
    /// Maximum context window in tokens.
    pub context_window: u32,
    /// Whether the model supports function/tool calling.
    pub supports_tools: bool,
    /// Whether the model supports streaming.
    pub supports_streaming: bool,
    /// Cost per 1K input tokens in USD.
    pub cost_per_1k_input: f64,
    /// Cost per 1K output tokens in USD.
    pub cost_per_1k_output: f64,
}

impl fmt::Display for ModelCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} (ctx: {}K, tools: {})",
            self.provider, self.model_id,
            self.context_window / 1000,
            self.supports_tools
        )
    }
}

/// Tool definition for LLM function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON schema for parameters.
    pub parameters: serde_json::Value,
}

/// Trait for executing agent steps via an LLM.
#[async_trait]
pub trait AgentRunner: Send + Sync {
    /// Execute a single agent step.
    async fn execute_step(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[ToolDef],
        max_tokens: u32,
    ) -> Result<AgentStepResult>;

    /// Continue a multi-turn conversation with tool results.
    async fn continue_step(
        &self,
        conversation_history: &[ConversationMessage],
        tool_results: &[ToolResultEntry],
        tools: &[ToolDef],
        max_tokens: u32,
    ) -> Result<AgentStepResult>;

    /// Get the model identifier.
    fn model_id(&self) -> &str;

    /// Get the provider name.
    fn provider_name(&self) -> &str;
}

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCallDef>>,
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool result to feed back to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEntry {
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}

/// Trait for estimating costs before execution.
pub trait CostEstimator: Send + Sync {
    /// Estimate cost for a single step.
    fn estimate_step_cost(
        &self,
        system_prompt_tokens: u32,
        user_prompt_tokens: u32,
        expected_output_tokens: u32,
        tool_count: usize,
    ) -> CostEstimate;

    /// Get cost per 1K input tokens.
    fn cost_per_1k_input(&self) -> f64;

    /// Get cost per 1K output tokens.
    fn cost_per_1k_output(&self) -> f64;
}

/// Trait for querying model capabilities.
pub trait ModelInfoProvider: Send + Sync {
    /// Get model capabilities.
    fn capabilities(&self) -> ModelCapabilities;

    /// Get maximum context window.
    fn context_window(&self) -> u32 {
        self.capabilities().context_window
    }

    /// Whether the model supports tool calling.
    fn supports_tools(&self) -> bool {
        self.capabilities().supports_tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_capabilities_display() {
        let caps = ModelCapabilities {
            model_id: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            context_window: 128000,
            supports_tools: true,
            supports_streaming: true,
            cost_per_1k_input: 0.0025,
            cost_per_1k_output: 0.01,
        };
        assert_eq!(caps.to_string(), "openai/gpt-4o (ctx: 128K, tools: true)");
    }

    #[test]
    fn test_cost_estimate_defaults() {
        let est = CostEstimate {
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: 0.005,
            confidence: 0.8,
        };
        assert_eq!(est.input_tokens, 1000);
        assert!(est.confidence > 0.0 && est.confidence <= 1.0);
    }

    #[test]
    fn test_tool_def_serialization() {
        let tool = ToolDef {
            name: "file_read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: ToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "file_read");
    }

    #[test]
    fn test_conversation_message_serialization() {
        let msg = ConversationMessage {
            role: MessageRole::Assistant,
            content: "Hello".to_string(),
            tool_calls: Some(vec![ToolCallDef {
                id: "tc-1".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"command": "ls"}),
            }]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ConversationMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, MessageRole::Assistant);
        assert!(parsed.tool_calls.is_some());
    }

    struct MockRunner;

    #[async_trait]
    impl AgentRunner for MockRunner {
        async fn execute_step(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
            _tools: &[ToolDef],
            _max_tokens: u32,
        ) -> Result<AgentStepResult> {
            Ok(AgentStepResult {
                text: Some("done".to_string()),
                tool_calls: vec![],
                tokens_used: 100,
                estimated_cost: 0.001,
                finish_reason: Some("stop".to_string()),
            })
        }
        async fn continue_step(
            &self,
            _history: &[ConversationMessage],
            _results: &[ToolResultEntry],
            _tools: &[ToolDef],
            _max_tokens: u32,
        ) -> Result<AgentStepResult> {
            Ok(AgentStepResult {
                text: None,
                tool_calls: vec![],
                tokens_used: 50,
                estimated_cost: 0.0005,
                finish_reason: Some("stop".to_string()),
            })
        }
        fn model_id(&self) -> &str { "mock-model" }
        fn provider_name(&self) -> &str { "mock" }
    }

    struct MockCostEstimator;

    impl CostEstimator for MockCostEstimator {
        fn estimate_step_cost(&self, input: u32, user: u32, output: u32, _tools: usize) -> CostEstimate {
            CostEstimate {
                input_tokens: input + user,
                output_tokens: output,
                cost_usd: (input + user) as f64 * 0.001 + output as f64 * 0.002,
                confidence: 0.9,
            }
        }
        fn cost_per_1k_input(&self) -> f64 { 0.001 }
        fn cost_per_1k_output(&self) -> f64 { 0.002 }
    }

    struct MockModelInfo;

    impl ModelInfoProvider for MockModelInfo {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                model_id: "mock".to_string(),
                provider: "test".to_string(),
                context_window: 8000,
                supports_tools: true,
                supports_streaming: false,
                cost_per_1k_input: 0.001,
                cost_per_1k_output: 0.002,
            }
        }
    }

    #[tokio::test]
    async fn test_mock_agent_runner() {
        let runner = MockRunner;
        let result = runner.execute_step("sys", "user", &[], 1000).await.unwrap();
        assert_eq!(result.text.as_deref(), Some("done"));
        assert_eq!(runner.model_id(), "mock-model");
        assert_eq!(runner.provider_name(), "mock");
    }

    #[test]
    fn test_mock_cost_estimator() {
        let estimator = MockCostEstimator;
        let estimate = estimator.estimate_step_cost(500, 200, 300, 2);
        assert_eq!(estimate.input_tokens, 700);
        assert_eq!(estimate.output_tokens, 300);
        assert!(estimate.cost_usd > 0.0);
    }

    #[test]
    fn test_mock_model_info() {
        let info = MockModelInfo;
        assert_eq!(info.context_window(), 8000);
        assert!(info.supports_tools());
    }
}
