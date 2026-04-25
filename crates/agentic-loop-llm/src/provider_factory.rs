//! Provider factory — maps ApiType + credentials to registered LlmWrapper.
//!
//! Given a resolved provider (name, api type, base_url, api_key), creates
//! the appropriate provider via vincents-llm and registers it in a LlmWrapper.

use anyhow::{Context, Result};
use std::sync::Arc;
use vincents_llm::{OpenAIProvider, AnthropicProvider};
use vincents_llm_wrapper::LlmWrapper;
use tracing::instrument;

use crate::provider_registry::ResolvedProvider;

/// Create a fully configured LlmWrapper with the given provider registered.
///
/// Returns the wrapper, ready for use with LlmExecutor.
pub async fn create_wrapper(resolved: &ResolvedProvider) -> Result<Arc<LlmWrapper>> {
    let api_key = resolved
        .api_key
        .as_ref()
        .context(format!("No API key for provider '{}'", resolved.name))?;

    let wrapper = LlmWrapper::new(None).await;

    if resolved.api.is_openai_compatible() {
        // OpenAI-compatible (includes ZAI, xai, groq, cerebras, openrouter, etc.)
        let config = vincents_llm::openai::OpenAIConfig {
            api_key: api_key.clone(),
            base_url: Some(resolved.base_url.clone()),
            ..Default::default()
        };
        let provider = OpenAIProvider::with_config(config)?;
        wrapper.register_provider(resolved.name.clone(), provider).await;
    } else if resolved.api.is_anthropic_compatible() {
        if resolved.base_url != "https://api.anthropic.com" {
            let provider = AnthropicProvider::with_endpoint(api_key.clone(), resolved.base_url.clone()).await?;
            wrapper.register_provider(resolved.name.clone(), provider).await;
        } else {
            wrapper.register_anthropic(resolved.name.clone(), api_key).await?;
        }
    } else if resolved.name == "openrouter" {
        wrapper.register_openrouter(resolved.name.clone(), api_key).await?;
    } else if resolved.name == "google" {
        wrapper.register_gemini(resolved.name.clone(), api_key).await?;
    } else {
        // Default: try OpenAI-compatible (most APIs speak this format)
        tracing::warn!(
            "Unknown API type '{}' for provider '{}', trying OpenAI-compatible",
            resolved.api.0,
            resolved.name
        );
        let config = vincents_llm::openai::OpenAIConfig {
            api_key: api_key.clone(),
            base_url: Some(resolved.base_url.clone()),
            ..Default::default()
        };
        let provider = OpenAIProvider::with_config(config)?;
        wrapper.register_provider(resolved.name.clone(), provider).await;
    }

    Ok(Arc::new(wrapper))
}

/// Resolve a provider by name from the registry.
/// Returns the first matching available provider.
    #[instrument(fields(provider = %provider_name))]
pub fn resolve_provider(provider_name: &str) -> Result<ResolvedProvider> {
    use crate::provider_registry::{resolve_available_providers, load_embedded_registry};

    let available = resolve_available_providers(&std::collections::HashMap::new())?;

    // Exact match first
    if let Some(p) = available.iter().find(|p| p.name == provider_name) {
        return Ok(p.clone());
    }

    // Prefix match
    if let Some(p) = available.iter().find(|p| p.name.starts_with(provider_name)) {
        return Ok(p.clone());
    }

    // Check if provider exists but isn't configured
    let registry = load_embedded_registry()?;
    if registry.providers.contains_key(provider_name) {
        let entry = &registry.providers[provider_name];
        let env_info = entry.env_var.as_ref()
            .map(|v| format!("Set {} environment variable", v))
            .unwrap_or_else(|| "Requires OAuth/special auth".to_string());
        anyhow::bail!("Provider '{}' exists but has no API key configured. {}", provider_name, env_info);
    }

    anyhow::bail!("Unknown provider '{}'. Use 'agentic-loop providers' to list available providers.", provider_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::{ApiType, ModelEntry};

    fn make_resolved(name: &str, api: &str, base_url: &str, api_key: Option<&str>) -> ResolvedProvider {
        ResolvedProvider {
            name: name.to_string(),
            api: ApiType(api.to_string()),
            base_url: base_url.to_string(),
            api_key: api_key.map(|s| s.to_string()),
            models: vec![ModelEntry {
                id: "test-model".into(),
                name: "Test".into(),
                reasoning: false,
                context_window: 128000,
                max_tokens: 4096,
            }],
        }
    }

    #[tokio::test]
    async fn test_create_wrapper_no_key() {
        let resolved = make_resolved("test", "openai-completions", "https://api.test.com/v1", None);
        let result = create_wrapper(&resolved).await;
        assert!(result.is_err(), "Expected error for missing API key");
    }

    #[tokio::test]
    async fn test_create_wrapper_openai_compat() {
        let resolved = make_resolved(
            "test-openai",
            "openai-completions",
            "https://api.test.com/v1",
            Some("test-key"),
        );
        let wrapper = create_wrapper(&resolved).await.unwrap();
        assert!(wrapper.provider_names().contains(&"test-openai".to_string()));
    }

    #[tokio::test]
    async fn test_create_wrapper_zai() {
        let resolved = make_resolved(
            "zai",
            "openai-completions",
            "https://api.z.ai/api/coding/paas/v4",
            Some("zai-test-key"),
        );
        let wrapper = create_wrapper(&resolved).await.unwrap();
        assert!(wrapper.provider_names().contains(&"zai".to_string()));
    }

    #[tokio::test]
    async fn test_create_wrapper_anthropic() {
        let resolved = make_resolved(
            "anthropic",
            "anthropic-messages",
            "https://api.anthropic.com",
            Some("test-key"),
        );
        let wrapper = create_wrapper(&resolved).await.unwrap();
        assert!(wrapper.provider_names().contains(&"anthropic".to_string()));
    }

    #[tokio::test]
    async fn test_create_wrapper_fallback() {
        let resolved = make_resolved(
            "custom",
            "custom-api",
            "https://custom.api.com/v1",
            Some("test-key"),
        );
        let wrapper = create_wrapper(&resolved).await.unwrap();
        assert!(wrapper.provider_names().contains(&"custom".to_string()));
    }
}
