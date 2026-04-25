//! LLM executor — drives agent steps via vincents-llm-wrapper.
//!
//! Each step:
//! 1. Build ChatCompletionRequest with system + user prompts + tool definitions
//! 2. Call LlmWrapper.chat_completion (with billing, quotas, session tracking)
//! 3. Parse response for tool calls
//! 4. Return structured result for the agent loop runner

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use vincents_llm::{
    ChatCompletionRequest, ChatMessage, FunctionDefinition,
};
use vincents_llm_wrapper::LlmWrapper;
use tracing::instrument;

// ─── LlmExecutorTrait ─────────────────────────────────────────────────────

/// Trait for LLM execution — implemented by both single LlmExecutor and ModelPool.
///
/// The runner holds `Option<Arc<dyn LlmExecutorTrait>>` so it doesn't care
/// whether it's talking to one model or a pool with fallback rotation.
#[async_trait::async_trait]
pub trait LlmExecutorTrait: Send + Sync {
    /// Execute a single agent step: send system + user prompts, get response.
    async fn execute_step(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult>;

    /// Execute a continuation with full conversation history (for tool dispatch loop).
    async fn execute_continuation(
        &self,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult>;

    /// Execute a follow-up step with tool results.
    async fn execute_with_tool_results(
        &self,
        system_prompt: &str,
        conversation_history: Vec<ChatMessage>,
        tool_results: Vec<ToolResult>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult>;

    /// Get the current model name.
    fn current_model(&self) -> &str;

    /// Get the current provider name.
    fn current_provider(&self) -> &str;

    /// Get the recommended model type for the next execution.
    /// This allows hierarchical agents to optimize cost by using
    /// cheaper models for simple tasks.
    fn recommended_model_type(&self) -> agentic_loop_types::ModelType;

    /// Set a model type hint for the next pick_model call.
    /// Pool implementations use this to filter candidates.
    /// Single-model executors ignore this.
    fn set_model_hint(&self, hint: Option<String>);
}

// ─── Data types ───────────────────────────────────────────────────────────

/// Result of a single LLM step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmStepResult {
    /// The assistant's text response (if any).
    pub text: Option<String>,
    /// Tool calls requested by the model.
    pub tool_calls: Vec<ToolCallRequest>,
    /// Token usage.
    pub tokens_used: u32,
    /// Estimated cost.
    pub estimated_cost: f64,
    /// Finish reason (e.g. "stop", "tool_calls").
    pub finish_reason: Option<String>,
    /// The session ID from the wrapper.
    pub session_id: String,
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Tool call ID (for matching responses).
    pub id: String,
    /// Name of the tool to call.
    pub name: String,
    /// JSON arguments for the tool.
    pub arguments: serde_json::Value,
}

/// Executes LLM steps via vincents-llm-wrapper.
pub struct LlmExecutor {
    wrapper: Arc<LlmWrapper>,
    provider_name: String,
    model: String,
    user_id: String,
}

impl LlmExecutor {
    /// Create a new executor.
    pub fn new(
        wrapper: Arc<LlmWrapper>,
        provider_name: String,
        model: String,
        user_id: String,
    ) -> Self {
        Self { wrapper, provider_name, model, user_id }
    }

    /// Execute a single agent step: send prompts, get response.
    #[instrument(skip(self, system_prompt, user_prompt, tools), fields(model = %self.model, tools = tools.len()))]
    pub async fn execute_step(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(max_tokens),
            ..Default::default()
        };

        // Add tool definitions as functions if provided
        if !tools.is_empty() {
            let functions: Vec<FunctionDefinition> = tools.iter().map(|t| FunctionDefinition {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                parameters: Some(t.parameters.clone()),
            }).collect();

            // Use the chat_completion_with_functions path via the provider directly
            // The wrapper doesn't expose function calling, so we go through the provider
            let provider = self.wrapper.get_provider(&self.provider_name)
                .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", self.provider_name))?;

            let response = provider.chat_completion_with_functions(request, functions).await?;

            return self.parse_response(response);
        }

        let context = vincents_llm_wrapper::UserContext {
            user_id: self.user_id.clone(),
            organization_id: None,
            billing_tier: vincents_llm_wrapper::BillingTier::Development,
            session_id: None,
        };

        let wrapped = self.wrapper.chat_completion(
            &self.provider_name,
            request,
            context,
        ).await?;

        self.parse_wrapped_response(wrapped)
    }

    /// Execute a continuation with full conversation history (for tool dispatch loop).
    #[instrument(skip(self, messages, tools), fields(model = %self.model, msg_count = messages.len()))]
    pub async fn execute_continuation(
        &self,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        let functions: Vec<FunctionDefinition> = tools.iter().map(|t| FunctionDefinition {
            name: t.name.clone(),
            description: Some(t.description.clone()),
            parameters: Some(t.parameters.clone()),
        }).collect();

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(max_tokens),
            ..Default::default()
        };

        let provider = self.wrapper.get_provider(&self.provider_name)
            .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", self.provider_name))?;

        let response = provider.chat_completion_with_functions(request, functions).await?;
        self.parse_response(response)
    }

    /// Execute a follow-up step with tool results.
    #[instrument(skip(self, conversation_history, tool_results, tools), fields(model = %self.model))]
    pub async fn execute_with_tool_results(
        &self,
        system_prompt: &str,
        conversation_history: Vec<ChatMessage>,
        tool_results: Vec<ToolResult>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(conversation_history);

        // Add tool results as messages
        for result in tool_results {
            messages.push(ChatMessage::Tool {
                tool_call_id: result.tool_call_id,
                content: result.content,
            });
        }

        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(max_tokens),
            ..Default::default()
        };

        if !tools.is_empty() {
            let functions: Vec<FunctionDefinition> = tools.iter().map(|t| FunctionDefinition {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                parameters: Some(t.parameters.clone()),
            }).collect();

            let provider = self.wrapper.get_provider(&self.provider_name)
                .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", self.provider_name))?;

            let response = provider.chat_completion_with_functions(request, functions).await?;
            return self.parse_response(response);
        }

        let context = vincents_llm_wrapper::UserContext {
            user_id: self.user_id.clone(),
            organization_id: None,
            billing_tier: vincents_llm_wrapper::BillingTier::Development,
            session_id: None,
        };

        let wrapped = self.wrapper.chat_completion(
            &self.provider_name,
            request,
            context,
        ).await?;

        self.parse_wrapped_response(wrapped)
    }

    fn parse_response(
        &self,
        response: vincents_llm::ChatCompletionResponse,
    ) -> Result<LlmStepResult> {
        let tokens_used = response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);

        let choice = response.choices.into_iter().next();
        let (text, finish_reason, tool_calls) = match choice {
            Some(c) => {
                let text = match &c.message {
                    ChatMessage::Assistant { content, .. } => content.clone(),
                    _ => None,
                };
                let finish = c.finish_reason.clone();

                // Extract function calls from assistant message
                let calls = self.extract_tool_calls(&c.message);
                (text, finish, calls)
            }
            None => (None, None, Vec::new()),
        };

        Ok(LlmStepResult {
            text,
            tool_calls,
            tokens_used,
            estimated_cost: 0.0, // filled from wrapped response
            finish_reason,
            session_id: String::new(),
        })
    }

    fn parse_wrapped_response(
        &self,
        wrapped: vincents_llm_wrapper::WrappedResponse,
    ) -> Result<LlmStepResult> {
        let mut result = self.parse_response(wrapped.response)?;
        result.estimated_cost = wrapped.cost_estimate.total_cost;
        result.session_id = wrapped.session_id;
        Ok(result)
    }

    fn extract_tool_calls(&self, message: &ChatMessage) -> Vec<ToolCallRequest> {
        match message {
            ChatMessage::Assistant { tool_calls, .. } => {
                match tool_calls {
                    Some(calls) => calls.iter().filter_map(|tc| {
                        match tc {
                            vincents_llm::ToolCall::Function(func) => match func {
                                vincents_llm::FunctionCall::Custom(custom) => Some(ToolCallRequest {
                                    id: custom.id.clone().unwrap_or_default(),
                                    name: custom.name.clone(),
                                    arguments: serde_json::from_str(&custom.arguments)
                                        .unwrap_or(serde_json::Value::Null),
                                }),
                                _ => None,
                            },
                        }
                    }).collect(),
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// Get the model name.
    pub fn model(&self) -> &str { &self.model }

    /// Get the provider name.
    pub fn provider(&self) -> &str { &self.provider_name }
}

/// Implement LlmExecutorTrait for LlmExecutor — delegates to inherent methods.
#[async_trait::async_trait]
impl LlmExecutorTrait for LlmExecutor {
    async fn execute_step(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        self.execute_step(system_prompt, user_prompt, tools, max_tokens).await
    }

    async fn execute_continuation(
        &self,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        self.execute_continuation(messages, tools, max_tokens).await
    }

    async fn execute_with_tool_results(
        &self,
        system_prompt: &str,
        conversation_history: Vec<ChatMessage>,
        tool_results: Vec<ToolResult>,
        tools: &[ToolDefinition],
        max_tokens: u32,
    ) -> Result<LlmStepResult> {
        self.execute_with_tool_results(
            system_prompt, conversation_history, tool_results, tools, max_tokens,
        ).await
    }

    fn current_model(&self) -> &str {
        self.model()
    }

    fn current_provider(&self) -> &str {
        self.provider()
    }

    fn recommended_model_type(&self) -> agentic_loop_types::ModelType {
        // Default to Smart for single-model executor
        agentic_loop_types::ModelType::Smart
    }

    fn set_model_hint(&self, _hint: Option<String>) {
        // Single-model executor ignores model hints
    }
}

/// Implement AgentRunner trait for swappable execution.
#[async_trait::async_trait]
impl crate::traits::AgentRunner for LlmExecutor {
    async fn execute_step(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &[crate::traits::ToolDef],
        max_tokens: u32,
    ) -> Result<crate::traits::AgentStepResult> {
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| ToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        }).collect();
        let result = self.execute_step(system_prompt, user_prompt, &tool_defs, max_tokens).await?;
        Ok(crate::traits::AgentStepResult {
            text: result.text,
            tool_calls: result.tool_calls.into_iter().map(|tc| crate::traits::ToolCallDef {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            }).collect(),
            tokens_used: result.tokens_used,
            estimated_cost: result.estimated_cost,
            finish_reason: result.finish_reason,
        })
    }

    async fn continue_step(
        &self,
        conversation_history: &[crate::traits::ConversationMessage],
        tool_results: &[crate::traits::ToolResultEntry],
        tools: &[crate::traits::ToolDef],
        max_tokens: u32,
    ) -> Result<crate::traits::AgentStepResult> {
        let mut messages: Vec<ChatMessage> = conversation_history.iter().map(|m| {
            match m.role {
                crate::traits::MessageRole::System => ChatMessage::system(&m.content),
                crate::traits::MessageRole::User => ChatMessage::user(&m.content),
                crate::traits::MessageRole::Assistant => {
                    let calls = m.tool_calls.as_ref().map(|calls| {
                        calls.iter().map(|c| vincents_llm::ToolCall::Function(
                            vincents_llm::FunctionCall::Custom(vincents_llm::CustomFunctionCall {
                                id: Some(c.id.clone()),
                                name: c.name.clone(),
                                arguments: c.arguments.to_string(),
                            })
                        )).collect::<Vec<_>>()
                    });
                    ChatMessage::Assistant { content: Some(m.content.clone()), tool_calls: calls, name: None }
                }
                crate::traits::MessageRole::Tool => ChatMessage::Tool {
                    tool_call_id: String::new(),
                    content: m.content.clone(),
                },
            }
        }).collect();

        for result in tool_results {
            messages.push(ChatMessage::Tool {
                tool_call_id: result.tool_call_id.clone(),
                content: result.result.clone(),
            });
        }

        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| ToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        }).collect();

        let result = self.execute_continuation(messages, &tool_defs, max_tokens).await?;
        Ok(crate::traits::AgentStepResult {
            text: result.text,
            tool_calls: result.tool_calls.into_iter().map(|tc| crate::traits::ToolCallDef {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            }).collect(),
            tokens_used: result.tokens_used,
            estimated_cost: result.estimated_cost,
            finish_reason: result.finish_reason,
        })
    }

    fn model_id(&self) -> &str { &self.model }
    fn provider_name(&self) -> &str { &self.provider_name }
}

/// Implement CostEstimator for pre-execution cost estimates.
impl crate::traits::CostEstimator for LlmExecutor {
    fn estimate_step_cost(
        &self,
        system_prompt_tokens: u32,
        user_prompt_tokens: u32,
        expected_output_tokens: u32,
        _tool_count: usize,
    ) -> crate::traits::CostEstimate {
        let input = (system_prompt_tokens + user_prompt_tokens) as f64;
        let output = expected_output_tokens as f64;
        // Rough estimate — real pricing comes from provider registry
        let cost = input * 0.00001 + output * 0.00003;
        crate::traits::CostEstimate {
            input_tokens: system_prompt_tokens + user_prompt_tokens,
            output_tokens: expected_output_tokens,
            cost_usd: cost,
            confidence: 0.5,
        }
    }
    fn cost_per_1k_input(&self) -> f64 { 0.01 }
    fn cost_per_1k_output(&self) -> f64 { 0.03 }
}

/// Implement ModelInfoProvider for capability queries.
impl crate::traits::ModelInfoProvider for LlmExecutor {
    fn capabilities(&self) -> crate::traits::ModelCapabilities {
        crate::traits::ModelCapabilities {
            model_id: self.model.clone(),
            provider: self.provider_name.clone(),
            context_window: 128_000,
            supports_tools: true,
            supports_streaming: true,
            cost_per_1k_input: 0.01,
            cost_per_1k_output: 0.03,
        }
    }
}

/// A tool definition for function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A tool result to feed back to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_serialization() {
        let def = ToolDefinition {
            name: "file_read".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "file_read");
    }

    #[test]
    fn test_tool_call_request_parsing() {
        let tcr = ToolCallRequest {
            id: "call-123".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "ls"}),
        };
        assert_eq!(tcr.name, "bash");
        assert_eq!(tcr.arguments["command"], "ls");
    }

    #[test]
    fn test_tool_result() {
        let tr = ToolResult {
            tool_call_id: "call-123".into(),
            content: "file1.txt\nfile2.txt".into(),
        };
        assert_eq!(tr.tool_call_id, "call-123");
    }

    #[test]
    fn test_llm_step_result_serialization() {
        let result = LlmStepResult {
            text: Some("Done".into()),
            tool_calls: vec![],
            tokens_used: 150,
            estimated_cost: 0.002,
            finish_reason: Some("stop".into()),
            session_id: "sess-1".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: LlmStepResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tokens_used, 150);
        assert_eq!(parsed.text, Some("Done".into()));
    }
}
