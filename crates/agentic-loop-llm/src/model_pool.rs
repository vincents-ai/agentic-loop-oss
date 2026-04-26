//! Model pool — cross-provider model selection with 3-level health tracking.
//!
//! Each entry holds its own `Arc<dyn LLMProvider>` — no wrapper type needed.
//! The pool rotates between entries based on health scores, provider cooldown,
//! and the configured rotation strategy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentic_loop_types::{ModelCapability, ModelCostTier, ModelType};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};
use vincents_llm::{
    ChatCompletionRequest, ChatMessage, FunctionDefinition, LLMError,
    LLMProvider,
};

use crate::executor::{
    LlmExecutorTrait, LlmStepResult, ToolDefinition, ToolResult,
};

/// Rotation strategy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    #[default]
    Healthiest,
    RoundRobin,
    LeastRecentlyUsed,
}

impl std::fmt::Display for RotationStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotationStrategy::Healthiest => write!(f, "healthiest"),
            RotationStrategy::RoundRobin => write!(f, "round_robin"),
            RotationStrategy::LeastRecentlyUsed => write!(f, "lru"),
        }
    }
}

impl std::str::FromStr for RotationStrategy {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().replace(['-', '_'], "").as_str() {
            "healthiest" => Ok(RotationStrategy::Healthiest),
            "roundrobin" => Ok(RotationStrategy::RoundRobin),
            "lru" | "leastrecentlyused" => Ok(RotationStrategy::LeastRecentlyUsed),
            _ => Err(anyhow::anyhow!(
                "Invalid pool strategy '{}'. Choose: healthiest, round_robin, lru", s
            )),
        }
    }
}

/// Pool model entry — holds a trait object, not a concrete wrapper.
pub struct PoolModelEntry {
    pub provider_name: String,
    pub model: String,
    pub provider: Arc<dyn LLMProvider>,
    pub context_length: u32,
    pub capabilities: Vec<ModelCapability>,
    pub cost_tier: ModelCostTier,
}

impl PoolModelEntry {
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider_name, self.model)
    }

    pub fn matches_type(&self, mt: &ModelType) -> bool {
        match mt {
            ModelType::Auto => true,
            ModelType::Free => self.cost_tier == ModelCostTier::Free,
            ModelType::Coding => self.capabilities.contains(&ModelCapability::Coding) || self.cost_tier == ModelCostTier::Free,
            ModelType::Fast => true,
            ModelType::Reasoning => self.capabilities.contains(&ModelCapability::Reasoning),
            ModelType::Vision => self.capabilities.contains(&ModelCapability::Vision),
            ModelType::Smart => true,
        }
    }
}

/// Per-model health.
#[derive(Debug, Clone)]
pub struct ModelHealth {
    pub success_count: u32,
    pub failure_count: u32,
    pub last_used: Instant,
    pub cooldown_until: Option<Instant>,
    pub consecutive_failures: u32,
    pub permanently_skipped: bool,
}

impl ModelHealth {
    fn new() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            last_used: Instant::now(),
            cooldown_until: None,
            consecutive_failures: 0,
            permanently_skipped: false,
        }
    }

    pub fn score(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 { 0.5 } else { self.success_count as f64 / total as f64 }
    }

    pub fn is_available(&self, now: Instant) -> bool {
        if self.permanently_skipped { return false; }
        match self.cooldown_until {
            Some(until) => now >= until,
            None => true,
        }
    }
}

/// Per-provider health.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub models_total: usize,
    pub cooldown_until: Option<Instant>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

impl ProviderHealth {
    fn new(models_total: usize) -> Self {
        Self { models_total, cooldown_until: None, last_error: None, consecutive_failures: 0 }
    }

    pub fn is_available(&self, now: Instant) -> bool {
        match self.cooldown_until {
            Some(until) => now >= until,
            None => true,
        }
    }
}

/// Cooldown constants.
pub struct CooldownDefaults;
impl CooldownDefaults {
    pub const NETWORK: Duration = Duration::from_secs(120);
    pub const QUOTA: Duration = Duration::from_secs(7200);
    pub const RATE_LIMIT: Duration = Duration::from_secs(60);
    pub const SERVER: Duration = Duration::from_secs(30);
    pub const PROVIDER_ESCALATION_THRESHOLD: u32 = 3;
}

/// Error scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorScope {
    Model { duration: Duration },
    Provider { duration: Duration },
    ContextExceeded,
    PermanentSkip,
    NoCooldown,
}

pub fn classify_error(error: &LLMError) -> ErrorScope {
    match error {
        LLMError::RateLimitError { retry_after, .. } => {
            let duration = retry_after.map(|s| Duration::from_secs(s)).unwrap_or(CooldownDefaults::RATE_LIMIT);
            ErrorScope::Model { duration }
        }
        LLMError::QuotaExceeded { .. } => ErrorScope::Provider { duration: CooldownDefaults::QUOTA },
        LLMError::HttpError { status_code, message, .. } => {
            match status_code {
                Some(429) => ErrorScope::Model { duration: CooldownDefaults::RATE_LIMIT },
                Some(500..=599) => ErrorScope::Model { duration: CooldownDefaults::SERVER },
                None => {
                    let msg = message.to_lowercase();
                    let is_net = msg.contains("dns") || msg.contains("connection refused") || msg.contains("tls");
                    if is_net { ErrorScope::Provider { duration: CooldownDefaults::NETWORK } }
                    else { ErrorScope::Model { duration: Duration::from_secs(60) } }
                }
                _ => ErrorScope::Model { duration: Duration::from_secs(60) },
            }
        }
        LLMError::TimeoutError { .. } => ErrorScope::Model { duration: CooldownDefaults::SERVER },
        LLMError::ModelNotAvailable { .. } => ErrorScope::PermanentSkip,
        LLMError::ContextLengthExceeded { .. } => ErrorScope::ContextExceeded,
        LLMError::AuthenticationError { .. } => ErrorScope::PermanentSkip,
        LLMError::ProviderError { .. } => ErrorScope::Model { duration: CooldownDefaults::RATE_LIMIT },
        LLMError::StreamingError { .. } => ErrorScope::Model { duration: Duration::from_secs(15) },
        LLMError::ContentFilterError { .. } => ErrorScope::NoCooldown,
        _ => ErrorScope::Model { duration: Duration::from_secs(60) },
    }
}

/// Health report types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHealthReport {
    pub provider: String,
    pub model: String,
    pub success_count: u32,
    pub failure_count: u32,
    pub score: f64,
    pub available: bool,
    pub cooldown_remaining_secs: Option<u64>,
    pub permanently_skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealthReport {
    pub provider: String,
    pub models_total: usize,
    pub available: bool,
    pub cooldown_remaining_secs: Option<u64>,
    pub last_error: Option<String>,
}

/// Model pool.
pub struct ModelPool {
    entries: Vec<PoolModelEntry>,
    model_health: RwLock<HashMap<String, ModelHealth>>,
    provider_health: RwLock<HashMap<String, ProviderHealth>>,
    current_index: RwLock<usize>,
    strategy: RotationStrategy,
    last_provider: RwLock<Option<String>>,
    /// Model type hint set before each pick_model call.
    model_hint: std::sync::RwLock<Option<String>>,
}

impl ModelPool {
    pub fn new(entries: Vec<PoolModelEntry>, strategy: RotationStrategy) -> Self {
        let now = Instant::now();
        let mut model_health = HashMap::new();
        let mut provider_counts: HashMap<String, usize> = HashMap::new();
        for entry in &entries {
            model_health.insert(entry.key(), ModelHealth::new());
            *provider_counts.entry(entry.provider_name.clone()).or_insert(0) += 1;
        }
        let provider_health: HashMap<String, ProviderHealth> = provider_counts
            .into_iter().map(|(p, c)| (p, ProviderHealth::new(c))).collect();
        for h in model_health.values_mut() { h.last_used = now; }
        Self {
            entries,
            model_health: RwLock::new(model_health),
            provider_health: RwLock::new(provider_health),
            current_index: RwLock::new(0),
            strategy,
            last_provider: RwLock::new(None),
            model_hint: std::sync::RwLock::new(None),
        }
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn entries(&self) -> &[PoolModelEntry] { &self.entries }
    pub fn strategy(&self) -> RotationStrategy { self.strategy }

    pub async fn pick_model(&self, model_type: Option<&ModelType>) -> Result<usize> {
        let now = Instant::now();
        let model_health = self.model_health.read().await;
        let provider_health = self.provider_health.read().await;
        let last_provider = self.last_provider.read().await.clone();
        let current = *self.current_index.read().await;

        let provider_ok = |p: &str| provider_health.get(p).map(|h| h.is_available(now)).unwrap_or(true);
        let model_ok = |k: &str| model_health.get(k).map(|h| h.is_available(now)).unwrap_or(true);
        let type_ok = |e: &PoolModelEntry| match model_type { Some(mt) => e.matches_type(mt), None => true };

        let mut candidates = Vec::new();
        for (i, entry) in self.entries.iter().enumerate() {
            if !provider_ok(&entry.provider_name) { continue; }
            if !model_ok(&entry.key()) { continue; }
            if !type_ok(entry) { continue; }
            let h = model_health.get(&entry.key()).unwrap();
            candidates.push((i, h.score(), entry.provider_name.clone(), h.last_used));
        }

        if candidates.is_empty() {
            return Err(anyhow::anyhow!("All models in cooldown"));
        }

        let picked = match self.strategy {
            RotationStrategy::Healthiest => {
                candidates.sort_by(|a, b| {
                    let sc = b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal);
                    if sc != std::cmp::Ordering::Equal { return sc; }
                    let ad = last_provider.as_ref().map(|lp| a.2 != *lp).unwrap_or(true);
                    let bd = last_provider.as_ref().map(|lp| b.2 != *lp).unwrap_or(true);
                    let dc = bd.cmp(&ad);
                    if dc != std::cmp::Ordering::Equal { return dc; }
                    a.3.cmp(&b.3)
                });
                candidates[0].0
            }
            RotationStrategy::RoundRobin => {
                let start = current + 1;
                for offset in 0..self.entries.len() {
                    let idx = (start + offset) % self.entries.len();
                    if candidates.iter().any(|(i, ..)| *i == idx) { return Ok(idx); }
                }
                candidates[0].0
            }
            RotationStrategy::LeastRecentlyUsed => {
                candidates.sort_by_key(|c| c.3);
                candidates[0].0
            }
        };
        Ok(picked)
    }

    pub async fn mark_success(&self, index: usize) {
        let entry = &self.entries[index];
        let key = entry.key();
        let provider = entry.provider_name.clone();
        let mut model_health = self.model_health.write().await;
        if let Some(h) = model_health.get_mut(&key) {
            h.success_count += 1;
            h.consecutive_failures = 0;
            h.cooldown_until = None;
            h.last_used = Instant::now();
        }
        let mut provider_health = self.provider_health.write().await;
        if let Some(ph) = provider_health.get_mut(&provider) { ph.consecutive_failures = 0; }
        *self.current_index.write().await = index;
        *self.last_provider.write().await = Some(provider);
    }

    pub async fn mark_failure(&self, index: usize, error: &LLMError) -> bool {
        let entry = &self.entries[index];
        let key = entry.key();
        let provider = entry.provider_name.clone();
        let scope = classify_error(error);
        let now = Instant::now();
        let mut escalated = false;

        let mut model_health = self.model_health.write().await;
        match &scope {
            ErrorScope::Model { duration } => {
                if let Some(h) = model_health.get_mut(&key) {
                    h.failure_count += 1;
                    h.consecutive_failures += 1;
                    h.cooldown_until = Some(now + *duration);
                    h.last_used = now;
                }
            }
            ErrorScope::Provider { duration } => {
                drop(model_health);
                let mut provider_health = self.provider_health.write().await;
                if let Some(ph) = provider_health.get_mut(&provider) {
                    ph.cooldown_until = Some(now + *duration);
                    ph.last_error = Some(error.to_string());
                    ph.consecutive_failures += 1;
                }
                escalated = true;
                let mut model_health = self.model_health.write().await;
                if let Some(h) = model_health.get_mut(&key) {
                    h.failure_count += 1;
                    h.last_used = now;
                }
            }
            ErrorScope::ContextExceeded | ErrorScope::NoCooldown => {
                if let Some(h) = model_health.get_mut(&key) { h.last_used = now; }
            }
            ErrorScope::PermanentSkip => {
                if let Some(h) = model_health.get_mut(&key) { h.permanently_skipped = true; }
            }
        }

        if !escalated {
            let model_health = self.model_health.read().await;
            let pf: u32 = self.entries.iter()
                .filter(|e| e.provider_name == provider)
                .filter_map(|e| model_health.get(&e.key()).map(|h| h.consecutive_failures)).sum();
            if pf >= CooldownDefaults::PROVIDER_ESCALATION_THRESHOLD {
                drop(model_health);
                let mut provider_health = self.provider_health.write().await;
                if let Some(ph) = provider_health.get_mut(&provider) {
                    ph.cooldown_until = Some(now + CooldownDefaults::QUOTA);
                    ph.consecutive_failures += 1;
                }
                escalated = true;
            }
        }

        *self.current_index.write().await = index;
        escalated
    }

    pub async fn model_report(&self) -> Vec<ModelHealthReport> {
        let now = Instant::now();
        let model_health = self.model_health.read().await;
        self.entries.iter().map(|e| {
            let h = model_health.get(&e.key()).unwrap();
            ModelHealthReport {
                provider: e.provider_name.clone(),
                model: e.model.clone(),
                success_count: h.success_count,
                failure_count: h.failure_count,
                score: h.score(),
                available: h.is_available(now),
                cooldown_remaining_secs: h.cooldown_until.map(|u| u.duration_since(now).as_secs()),
                permanently_skipped: h.permanently_skipped,
            }
        }).collect()
    }

    pub async fn provider_report(&self) -> Vec<ProviderHealthReport> {
        let now = Instant::now();
        let provider_health = self.provider_health.read().await;
        provider_health.iter().map(|(n, h)| ProviderHealthReport {
            provider: n.clone(),
            models_total: h.models_total,
            available: h.is_available(now),
            cooldown_remaining_secs: h.cooldown_until.map(|u| u.duration_since(now).as_secs()),
            last_error: h.last_error.clone(),
        }).collect()
    }
}

/// Execute a request against a pool entry's provider, with timeout.
async fn execute_with_entry(
    entry: &PoolModelEntry,
    request: ChatCompletionRequest,
    tools: &[ToolDefinition],
    timeout: Duration,
) -> Result<vincents_llm::ChatCompletionResponse, LLMError> {
    if !tools.is_empty() {
        let functions: Vec<FunctionDefinition> = tools.iter().map(|t| FunctionDefinition {
            name: t.name.clone(),
            description: Some(t.description.clone()),
            parameters: Some(t.parameters.clone()),
        }).collect();
        tokio::time::timeout(timeout, entry.provider.chat_completion_with_functions(request, functions))
            .await
            .map_err(|_| LLMError::TimeoutError {
                timeout_secs: timeout.as_secs(),
                message: Some(format!("Model {} timed out", entry.model)),
            })?
    } else {
        tokio::time::timeout(timeout, entry.provider.chat_completion(request))
            .await
            .map_err(|_| LLMError::TimeoutError {
                timeout_secs: timeout.as_secs(),
                message: Some(format!("Model {} timed out", entry.model)),
            })?
    }
}

/// Parse an LLM response into a step result.
fn parse_pool_response(response: vincents_llm::ChatCompletionResponse) -> LlmStepResult {
    let tokens_used = response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0);
    let choice = response.choices.into_iter().next();
    let (text, finish, tool_calls) = match choice {
        Some(c) => {
            let txt = match &c.message {
                ChatMessage::Assistant { content, .. } => content.clone(),
                _ => None,
            };
            let calls = crate::executor::LlmExecutor::extract_tool_calls(&c.message);
            (txt, c.finish_reason.clone(), calls)
        }
        None => (None, None, vec![]),
    };
    LlmStepResult {
        text,
        tool_calls,
        tokens_used,
        estimated_cost: 0.0,
        finish_reason: finish,
    }
}

#[async_trait::async_trait]
impl LlmExecutorTrait for ModelPool {
    async fn execute_step(&self, system_prompt: &str, user_prompt: &str, tools: &[ToolDefinition], max_tokens: u32) -> Result<LlmStepResult> {
        let max_attempts = self.entries.len().max(1);
        let per_model_timeout = Duration::from_secs(120);

        for attempt in 1..=max_attempts {
            let hint_str = self.model_hint.read().ok().and_then(|g| g.clone());
            let model_type_filter = hint_str.as_ref().and_then(|h| {
                h.split(',')
                    .filter_map(|s| s.trim().parse::<agentic_loop_types::ModelType>().ok())
                    .find(|_| true)
            });
            let idx = self.pick_model(model_type_filter.as_ref()).await?;
            let entry = &self.entries[idx];
            info!("ModelPool executing on {}/{}", entry.provider_name, entry.model);

            let messages = vec![ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];
            let request = ChatCompletionRequest {
                model: entry.model.clone(),
                messages,
                max_tokens: Some(max_tokens),
                ..Default::default()
            };

            match execute_with_entry(entry, request, tools, per_model_timeout).await {
                Ok(response) => {
                    self.mark_success(idx).await;
                    return Ok(parse_pool_response(response));
                }
                Err(llm_err) => {
                    warn!("ModelPool {}/{} failed: {}", entry.provider_name, entry.model, llm_err);
                    let escalated = self.mark_failure(idx, &llm_err).await;
                    if escalated { info!("Provider {} escalated", entry.provider_name); }
                    if !llm_err.is_retryable() { return Err(anyhow::anyhow!("{}", llm_err)); }
                    let _ = attempt; // suppress unused warning
                }
            }
        }

        Err(anyhow::anyhow!("ModelPool exhausted"))
    }

    async fn execute_continuation(&self, messages: Vec<ChatMessage>, tools: &[ToolDefinition], max_tokens: u32) -> Result<LlmStepResult> {
        let idx = *self.current_index.read().await;
        let entry = &self.entries[idx];
        let request = ChatCompletionRequest {
            model: entry.model.clone(),
            messages,
            max_tokens: Some(max_tokens),
            ..Default::default()
        };

        match execute_with_entry(entry, request, tools, Duration::from_secs(120)).await {
            Ok(response) => {
                self.mark_success(idx).await;
                Ok(parse_pool_response(response))
            }
            Err(llm_err) => {
                self.mark_failure(idx, &llm_err).await;
                Err(anyhow::anyhow!("{}", llm_err))
            }
        }
    }

    async fn execute_with_tool_results(&self, system_prompt: &str, conversation_history: Vec<ChatMessage>, tool_results: Vec<ToolResult>, tools: &[ToolDefinition], max_tokens: u32) -> Result<LlmStepResult> {
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(conversation_history);
        for r in tool_results { messages.push(ChatMessage::Tool { tool_call_id: r.tool_call_id, content: r.content }); }
        self.execute_continuation(messages, tools, max_tokens).await
    }

    fn current_model(&self) -> &str { &self.entries[*self.current_index.blocking_read()].model }
    fn current_provider(&self) -> &str { &self.entries[*self.current_index.blocking_read()].provider_name }
    fn recommended_model_type(&self) -> agentic_loop_types::ModelType {
        let idx = *self.current_index.blocking_read();
        let entry = &self.entries[idx];
        match entry.cost_tier {
            agentic_loop_types::ModelCostTier::Free => agentic_loop_types::ModelType::Free,
            _ => agentic_loop_types::ModelType::Smart,
        }
    }

    fn set_model_hint(&self, hint: Option<String>) {
        if let Ok(mut h) = self.model_hint.write() {
            *h = hint;
        }
    }
}

// ─── Live discovery ─────────────────────────────────────────────────────────

/// Discovered model info from OpenRouter /models endpoint.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DiscoveredModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub context_length: Option<u64>,
    pub pricing: Option<DiscoveredPricing>,
    pub supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DiscoveredPricing {
    pub prompt: Option<String>,
    pub completion: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ModelsResponse {
    data: Vec<DiscoveredModelInfo>,
}

/// Discover free tool-calling models from OpenRouter.
///
/// Takes a provider (already configured with OpenRouter credentials) and
/// queries the public /models endpoint. Returns entries ready for pool creation.
pub async fn discover_openrouter_models(
    provider: Arc<dyn LLMProvider>,
    min_context: u32,
) -> Result<Vec<PoolModelEntry>> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("OpenRouter /models returned {}", resp.status()));
    }

    let models: ModelsResponse = resp.json().await?;

    let mut entries: Vec<PoolModelEntry> = models.data
        .into_iter()
        .filter(|m| {
            let is_free = m.pricing.as_ref()
                .and_then(|p| p.prompt.as_ref())
                .map(|p| p == "0")
                .unwrap_or(false);
            if !is_free { return false; }

            let has_tools = m.supported_parameters.as_ref()
                .map(|params| params.iter().any(|p| p.to_lowercase() == "tools"))
                .unwrap_or(false);
            if !has_tools { return false; }

            let ctx = m.context_length.unwrap_or(0) as u32;
            if ctx < min_context { return false; }

            true
        })
        .map(|m| {
            let ctx = m.context_length.unwrap_or(0) as u32;
            let mut capabilities = vec![ModelCapability::ToolCalling];
            let model_id = &m.id;
            if ModelType::Coding.matches_model_name(model_id) {
                capabilities.push(ModelCapability::Coding);
            }
            if ModelType::Reasoning.matches_model_name(model_id) {
                capabilities.push(ModelCapability::Reasoning);
            }
            if ModelType::Vision.matches_model_name(model_id) {
                capabilities.push(ModelCapability::Vision);
            }

            PoolModelEntry {
                provider_name: "openrouter".to_string(),
                model: m.id.clone(),
                provider: provider.clone(),
                context_length: ctx,
                capabilities,
                cost_tier: ModelCostTier::Free,
            }
        })
        .collect();

    entries.sort_by(|a, b| b.context_length.cmp(&a.context_length));
    Ok(entries)
}

/// Create a ModelPool by discovering free models from OpenRouter.
pub async fn create_free_pool(
    provider: Arc<dyn LLMProvider>,
    strategy: RotationStrategy,
    min_context: u32,
) -> Result<ModelPool> {
    let entries = discover_openrouter_models(provider, min_context).await?;
    if entries.is_empty() {
        return Err(anyhow::anyhow!("No free tool-calling models found on OpenRouter"));
    }
    Ok(ModelPool::new(entries, strategy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation_strategy_default() {
        assert_eq!(RotationStrategy::default(), RotationStrategy::Healthiest);
    }

    #[test]
    fn test_error_scope_equality() {
        let s1 = ErrorScope::Model { duration: Duration::from_secs(60) };
        let s2 = ErrorScope::Model { duration: Duration::from_secs(60) };
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_cooldown_defaults() {
        assert_eq!(CooldownDefaults::NETWORK, Duration::from_secs(120));
        assert_eq!(CooldownDefaults::QUOTA, Duration::from_secs(7200));
        assert_eq!(CooldownDefaults::RATE_LIMIT, Duration::from_secs(60));
        assert_eq!(CooldownDefaults::PROVIDER_ESCALATION_THRESHOLD, 3);
    }
}
