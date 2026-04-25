//! # agentic-loop-llm
//!
//! LLM integration for model selection and agent execution.
//! Wraps vincents-llm provider trait with model selection logic and
//! tool-calling execution.
//!
//! Provider registry: embedded defaults from pi.dev, custom from git-refs,
//! API keys from env vars or config file.

mod selector;
mod executor;
pub mod model_pool;
pub mod orchestrator;
pub mod provider_registry;
pub mod provider_factory;
pub mod model_registry;
pub mod traits;
pub mod retry;

pub use retry::{RetryPolicy, with_retry};
pub use traits::{
    AgentRunner, CostEstimator, ModelInfoProvider,
    AgentStepResult, ToolCallDef, ToolDef, CostEstimate, ModelCapabilities,
    ConversationMessage, MessageRole, ToolResultEntry,
};

pub use selector::{ModelSelectorImpl, ModelSelection, ModelProfile, TaskRequirements, TaskType};
pub use executor::{LlmExecutor, LlmExecutorTrait, LlmStepResult, ToolCallRequest, ToolDefinition, ToolResult};
pub use model_pool::{
    ModelPool, PoolModelEntry, ModelHealth, ProviderHealth,
    RotationStrategy, ErrorScope, CooldownDefaults,
    ModelHealthReport, ProviderHealthReport,
    classify_error, discover_openrouter_models, create_free_pool,
};
pub use orchestrator::{
    HierarchicalTask, HierarchicalTaskStatus, DecompositionStrategy,
    DecompositionResult, OrchestratorConfig,
    TaskOrchestrator, TaskOrchestratorTrait,
    execute_hierarchical,
};
pub use provider_factory::{create_wrapper, resolve_provider};
pub use model_registry::{ModelRegistry, DiscoveredModel, ModelSource, DiscoveryStats};
pub use provider_registry::{
    load_embedded_registry, resolve_available_providers, list_all_providers,
    get_provider_detail,
    ProviderRegistry, ProviderEntry, ModelEntry, ApiType, ResolvedProvider,
    ProviderStatus, ProviderDetail, CustomProviderOverride,
};
