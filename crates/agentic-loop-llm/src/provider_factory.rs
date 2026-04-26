//! Provider factory — maps ApiType + credentials to `Arc<dyn LLMProvider>`.
//!
//! This is the **only file** that knows about concrete provider types.
//! Everything else in the crate depends on the `LLMProvider` trait.

use anyhow::{Context, Result};
use std::sync::Arc;
use vincents_llm::{LLMProvider, OpenAIProvider, AnthropicProvider};
use tracing::instrument;

use crate::provider_registry::ResolvedProvider;

/// Create a fully configured provider from a resolved provider config.
///
/// Returns `Arc<dyn LLMProvider>`, ready for use with `LlmExecutor` or `ModelPool`.
pub async fn create_provider(resolved: &ResolvedProvider) -> Result<Arc<dyn LLMProvider>> {
    let api_key = resolved
        .api_key
        .as_ref()
        .context(format!("No API key for provider '{}'", resolved.name))?;

    let provider: Arc<dyn LLMProvider> = if resolved.api.is_openai_compatible() {
        let config = vincents_llm::openai::OpenAIConfig {
            api_key: api_key.clone(),
            base_url: Some(resolved.base_url.clone()),
            ..Default::default()
        };
        Arc::new(OpenAIProvider::with_config(config)?)
    } else if resolved.api.is_anthropic_compatible() {
        if resolved.base_url != "https://api.anthropic.com" {
            Arc::new(AnthropicProvider::with_endpoint(api_key.clone(), resolved.base_url.clone()).await?)
        } else {
            Arc::new(AnthropicProvider::new(api_key.clone()).await?)
        }
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
        Arc::new(OpenAIProvider::with_config(config)?)
    };

    Ok(provider)
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
    async fn test_create_provider_no_key() {
        let resolved = make_resolved("test", "openai-completions", "https://api.test.com/v1", None);
        let result = create_provider(&resolved).await;
        assert!(result.is_err(), "Expected error for missing API key");
    }

    #[tokio::test]
    async fn test_create_provider_openai_compat() {
        let resolved = make_resolved(
            "test-openai",
            "openai-completions",
            "https://api.test.com/v1",
            Some("test-key"),
        );
        let provider = create_provider(&resolved).await.unwrap();
        // Provider name is the canonical type name, not the resolved name
        assert_eq!(provider.name(), "openai");
    }

    #[tokio::test]
    async fn test_create_provider_custom_fallback() {
        let resolved = make_resolved(
            "custom",
            "custom-api",
            "https://custom.api.com/v1",
            Some("test-key"),
        );
        let provider = create_provider(&resolved).await.unwrap();
        // Falls back to OpenAI-compatible
        assert_eq!(provider.name(), "openai");
    }
}
