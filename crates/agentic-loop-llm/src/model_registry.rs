//! Model registry — hybrid discovery from live providers + embedded defaults.
//!
//! At startup (or on-demand), calls `list_models()` on each configured provider,
//! merges with the embedded providers.json, and caches results per session.
//! Auto-detects free/preview models by pricing or ID suffix.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::selector::ModelProfile;
use crate::provider_registry::{load_embedded_registry, resolve_available_providers, ResolvedProvider, ApiType};

/// A cached model entry with source tracking.
#[derive(Debug, Clone)]
pub struct DiscoveredModel {
    pub profile: ModelProfile,
    /// Where this model was discovered from.
    pub source: ModelSource,
}

/// How a model was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    /// Discovered via live `list_models()` API call.
    LiveApi,
    /// From the embedded providers.json (recommended models).
    Embedded,
    /// From git-refs custom overrides.
    GitRefs,
}

/// The model registry — hybrid live + embedded discovery.
pub struct ModelRegistry {
    /// Inner state behind RwLock for async access.
    inner: RwLock<ModelRegistryInner>,
}

struct ModelRegistryInner {
    /// All discovered models, keyed by "{provider}/{model_id}".
    models: HashMap<String, DiscoveredModel>,
    /// Whether discovery has been run.
    discovered: bool,
}

impl ModelRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(ModelRegistryInner {
                models: HashMap::new(),
                discovered: false,
            }),
        }
    }

    /// Discover models from all configured providers.
    ///
    /// 1. Loads embedded providers.json recommended models
    /// 2. For each resolved provider, calls `list_models()` live
    /// 3. Merges results (live overrides embedded on conflict)
    /// 4. Auto-detects free/preview models
    pub async fn discover_all(&self) -> Result<DiscoveryStats> {
        let mut inner = self.inner.write().await;
        inner.models.clear();

        let mut stats = DiscoveryStats::default();

        // Phase 1: Load embedded recommended models
        let embedded = load_embedded_registry()?;
        for (provider_name, entry) in &embedded.providers {
            for model in &entry.recommended_models {
                let key = format!("{}/{}", provider_name, model.id);
                let profile = ModelProfile {
                    provider: provider_name.clone(),
                    model: model.id.clone(),
                    context_window: model.context_window,
                    max_output_tokens: model.max_tokens,
                    function_calling: true, // Assume recommended models support tools
                    vision: false,
                    input_modalities: vec!["text".to_string()],
                    output_modalities: vec!["text".to_string()],
                    strengths: derive_strengths(&model.id),
                    cost_per_1k_input: 0.0, // Unknown from embedded
                    cost_per_1k_output: 0.0,
                    is_free: false,
                };

                inner.models.insert(key, DiscoveredModel {
                    profile,
                    source: ModelSource::Embedded,
                });
                stats.embedded_count += 1;
            }
        }

        // Phase 2: Live discovery from resolved providers
        let resolved = resolve_available_providers(&HashMap::new())?;
        stats.provider_count = resolved.len();

        for provider in &resolved {
            match self.discover_from_provider_sync(&mut inner, provider).await {
                Ok(count) => stats.live_count += count,
                Err(e) => {
                    tracing::warn!(
                        "Failed to discover from {} ({}): {}",
                        provider.name,
                        provider.base_url,
                        e
                    );
                    stats.provider_errors += 1;
                }
            }
        }

        // Phase 3: Count free models
        stats.free_count = inner.models.values().filter(|m| m.profile.is_free).count();
        stats.total_count = inner.models.len();
        inner.discovered = true;

        tracing::info!(
            "Model discovery complete: {} total ({} embedded, {} live, {} free, {} providers, {} errors)",
            stats.total_count, stats.embedded_count, stats.live_count,
            stats.free_count, stats.provider_count, stats.provider_errors
        );

        Ok(stats)
    }

    /// Discover models from a single resolved provider.
    async fn discover_from_provider_sync(
        &self,
        inner: &mut ModelRegistryInner,
        provider: &ResolvedProvider,
    ) -> Result<usize> {
        // Create provider directly for discovery (not through wrapper)
        let provider_instance = create_provider_for_discovery(provider)?;
        let models = provider_instance.list_models().await?;
        let mut count = 0;

        for m in models {
            let is_free = m.pricing.as_ref().map(|p| p.is_free).unwrap_or(false)
                || m.id.ends_with(":free")
                || m.id.ends_with(":preview");

            let key = format!("{}/{}", m.provider, m.id);
            let profile = ModelProfile {
                provider: m.provider.clone(),
                model: m.id.clone(),
                context_window: m.context_window,
                max_output_tokens: m.max_output_tokens,
                function_calling: m.capabilities.function_calling,
                vision: m.capabilities.vision,
                input_modalities: m.capabilities.input_modalities,
                output_modalities: m.capabilities.output_modalities,
                strengths: if m.capabilities.strengths.is_empty() {
                    derive_strengths(&m.id)
                } else {
                    m.capabilities.strengths
                },
                cost_per_1k_input: m.pricing.as_ref().map(|p| p.prompt_tokens).unwrap_or(0.0),
                cost_per_1k_output: m.pricing.as_ref().map(|p| p.completion_tokens).unwrap_or(0.0),
                is_free,
            };

            inner.models.insert(key, DiscoveredModel {
                profile,
                source: ModelSource::LiveApi,
            });
            count += 1;
        }

        Ok(count)
    }

    /// Get all discovered profiles.
    pub async fn profiles(&self) -> Vec<ModelProfile> {
        let inner = self.inner.read().await;
        inner.models.values().map(|d| d.profile.clone()).collect()
    }

    /// Get profiles for a specific provider.
    pub async fn profiles_for_provider(&self, provider: &str) -> Vec<ModelProfile> {
        let inner = self.inner.read().await;
        inner.models.values()
            .filter(|m| m.profile.provider == provider)
            .map(|d| d.profile.clone())
            .collect()
    }

    /// Get free models only.
    pub async fn free_models(&self) -> Vec<ModelProfile> {
        let inner = self.inner.read().await;
        inner.models.values()
            .filter(|m| m.profile.is_free)
            .map(|d| d.profile.clone())
            .collect()
    }

    /// Find a specific model by provider and ID.
    pub async fn find(&self, provider: &str, model_id: &str) -> Option<ModelProfile> {
        let inner = self.inner.read().await;
        let key = format!("{}/{}", provider, model_id);
        inner.models.get(&key).map(|d| d.profile.clone())
    }

    /// Whether discovery has been run.
    pub async fn is_discovered(&self) -> bool {
        self.inner.read().await.discovered
    }

    /// Total model count.
    pub async fn count(&self) -> usize {
        self.inner.read().await.models.len()
    }

    /// Build a ModelSelectorImpl from discovered profiles.
    pub async fn build_selector(&self) -> crate::selector::ModelSelectorImpl {
        let profiles = self.profiles().await;
        crate::selector::ModelSelectorImpl::with_profiles(profiles)
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics from a discovery run.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
    pub total_count: usize,
    pub embedded_count: usize,
    pub live_count: usize,
    pub free_count: usize,
    pub provider_count: usize,
    pub provider_errors: usize,
}

/// Derive model strengths from ID heuristics.
fn derive_strengths(model_id: &str) -> Vec<String> {
    let id = model_id.to_lowercase();
    let mut strengths = Vec::new();

    if id.contains("coder") || id.contains("code") || id.contains("deepseek-coder") || id.contains("qwen2.5-coder") {
        strengths.push("coding".to_string());
    }
    if id.contains("reason") || id.contains("o1") || id.contains("o3") || id.contains("think") || id.contains("deepseek-r") {
        strengths.push("reasoning".to_string());
    }
    if id.contains("math") || id.contains("mathstral") {
        strengths.push("math".to_string());
    }
    if id.contains("vision") || id.contains("gpt-4o") || id.contains("claude-3") || id.contains("gemini") {
        strengths.push("vision".to_string());
    }
    if id.contains("creative") || id.contains("story") {
        strengths.push("creative".to_string());
    }

    strengths
}

/// Create a boxed LLMProvider for discovery (list_models) based on API type.
fn create_provider_for_discovery(
    resolved: &ResolvedProvider,
) -> Result<Arc<dyn vincents_llm::provider::LLMProvider>> {
    let api_key = resolved.api_key.clone().unwrap_or_default();
    let base_url = resolved.base_url.clone();

    match resolved.api.as_str() {
        ApiType::OPENAI_COMPLETIONS | ApiType::OPENAI_RESPONSES => {
            let config = vincents_llm::openai::OpenAIConfig {
                api_key: api_key.clone(),
                organization_id: None,
                base_url: Some(base_url.clone()),
                default_model: None,
            };
            let provider = vincents_llm::openai::OpenAIProvider::with_config(config)?;
            Ok(Arc::new(provider))
        }
        ApiType::ANTHROPIC_MESSAGES => {
            // Anthropic::with_endpoint is async, so we can't call it here directly.
            // Use with_config instead.
            let config = vincents_llm::anthropic::AnthropicConfig {
                api_key: api_key.clone(),
                base_url: Some(base_url),
                default_model: None,
            };
            let provider = vincents_llm::anthropic::AnthropicProvider::with_config(config)?;
            Ok(Arc::new(provider))
        }
        ApiType::GOOGLE_GENERATIVE_AI | ApiType::GOOGLE_GEMINI_CLI => {
            let config = vincents_llm::gemini::GeminiConfig {
                api_key: api_key.clone(),
                default_model: None,
                base_url: Some(base_url),
            };
            let provider = vincents_llm::gemini::GeminiProvider::with_config(config)?;
            Ok(Arc::new(provider))
        }
        // OpenRouter and other OpenAI-compatible providers
        _ => {
            let config = vincents_llm::openai::OpenAIConfig {
                api_key: api_key.clone(),
                organization_id: None,
                base_url: Some(base_url),
                default_model: None,
            };
            let provider = vincents_llm::openai::OpenAIProvider::with_config(config)?;
            Ok(Arc::new(provider))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_strengths() {
        assert!(derive_strengths("deepseek-coder-v2").contains(&"coding".to_string()));
        assert!(derive_strengths("o1-preview").contains(&"reasoning".to_string()));
        assert!(derive_strengths("gpt-4o").contains(&"vision".to_string()));
        assert!(derive_strengths("mathstral-7b").contains(&"math".to_string()));
        assert!(derive_strengths("llama-3-8b").is_empty());
    }

    #[test]
    fn test_registry_new() {
        let registry = ModelRegistry::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(!registry.is_discovered().await);
            assert_eq!(registry.count().await, 0);
        });
    }

    #[test]
    fn test_find_empty() {
        let registry = ModelRegistry::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(registry.find("openai", "gpt-4o").await.is_none());
        });
    }

    #[test]
    fn test_profiles_empty() {
        let registry = ModelRegistry::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(registry.profiles().await.is_empty());
            assert!(registry.free_models().await.is_empty());
        });
    }
}
