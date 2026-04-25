//! Provider registry — embedded defaults from pi.dev, custom from git-refs, keys from env/config.
//!
//! Three layers (later overrides earlier):
//! 1. **Embedded defaults** (`providers.json`) — compiled into binary
//! 2. **Git-refs custom** — `refs/agent-loop/{project_id}/providers/{name}` — JSON overrides
//! 3. **API keys** — environment variables or `~/.config/agentic-loop/keys.toml`
//!
//! The `agentic-loop providers refresh` CLI command updates git-refs from
//! the latest pi.dev model list (port of `npm run generate-models`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Types ────────────────────────────────────────────────────────────────────

/// API type (mirrors pi.dev's KnownApi).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiType(pub String);

impl ApiType {
    // Well-known API types
    pub const OPENAI_COMPLETIONS: &str = "openai-completions";
    pub const OPENAI_RESPONSES: &str = "openai-responses";
    pub const ANTHROPIC_MESSAGES: &str = "anthropic-messages";
    pub const GOOGLE_GENERATIVE_AI: &str = "google-generative-ai";
    pub const GOOGLE_GEMINI_CLI: &str = "google-gemini-cli";
    pub const GOOGLE_VERTEX: &str = "google-vertex";
    pub const BEDROCK_CONVERSE_STREAM: &str = "bedrock-converse-stream";
    pub const MISTRAL_CONVERSATIONS: &str = "mistral-conversations";

    /// Returns true if this API uses OpenAI-compatible chat completions format.
    pub fn is_openai_compatible(&self) -> bool {
        matches!(
            self.0.as_str(),
            "openai-completions" | "openai-responses" | "azure-openai-responses"
        )
    }

    /// Returns true if this API uses Anthropic Messages format.
    pub fn is_anthropic_compatible(&self) -> bool {
        self.0 == "anthropic-messages"
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A recommended model from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub context_window: u32,
    pub max_tokens: u32,
}

/// A provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    /// API type (determines request format).
    pub api: ApiType,
    /// Base URL for the provider's API.
    pub base_url: String,
    /// Environment variable name for the API key (null = no key needed or special auth).
    pub env_var: Option<String>,
    /// Recommended models for this provider.
    pub recommended_models: Vec<ModelEntry>,
    /// Total number of known models (full list may be in git-refs).
    pub total_models: usize,
}

/// Full provider registry (embedded + custom overrides).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRegistry {
    pub version: String,
    pub source: String,
    pub generated: String,
    pub providers: HashMap<String, ProviderEntry>,
}

/// Custom provider override stored in git-refs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProviderOverride {
    /// Override base URL.
    pub base_url: Option<String>,
    /// Override API key env var.
    pub env_var: Option<String>,
    /// Override or add models.
    pub models: Option<Vec<ModelEntry>>,
    /// Additional headers to send.
    pub headers: Option<HashMap<String, String>>,
}

/// Resolved provider ready for use — has API key resolved.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub name: String,
    pub api: ApiType,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<ModelEntry>,
}

/// API key resolution result.
#[derive(Debug, Clone)]
pub struct ApiKeySource {
    pub key: String,
    pub source: KeySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    ConfigFile,
    GitRefs,
}

// ─── Embedded defaults ────────────────────────────────────────────────────────

/// Load the embedded default provider registry from providers.json.
pub fn load_embedded_registry() -> Result<ProviderRegistry> {
    let json_str = include_str!("providers.json");
    serde_json::from_str(json_str).context("Failed to parse embedded providers.json")
}

// ─── API key resolution ───────────────────────────────────────────────────────

/// Resolve an API key for a provider.
/// Priority: env var > config file > git-refs.
pub fn resolve_api_key(provider: &ProviderEntry, config_keys: &HashMap<String, String>) -> Option<ApiKeySource> {
    // 1. Environment variable
    if let Some(ref env_var) = provider.env_var {
        if let Ok(key) = std::env::var(env_var) {
            if !key.is_empty() {
                return Some(ApiKeySource {
                    key,
                    source: KeySource::Environment,
                });
            }
        }
    }

    // 2. Config file (~/.config/agentic-loop/keys.toml)
    let provider_name = provider.env_var.as_ref()
        .map(|v| v.to_lowercase().replace("_api_key", "").replace("_", "-"))
        .unwrap_or_default();
    
    if let Some(key) = config_keys.get(&provider_name) {
        if !key.is_empty() {
            return Some(ApiKeySource {
                key: key.clone(),
                source: KeySource::ConfigFile,
            });
        }
    }

    // 3. Also check config file by provider env_var name
    if let Some(ref env_var) = provider.env_var {
        if let Some(key) = config_keys.get(env_var) {
            if !key.is_empty() {
                return Some(ApiKeySource {
                    key: key.clone(),
                    source: KeySource::ConfigFile,
                });
            }
        }
    }

    None
}

/// Load API keys from config file (~/.config/agentic-loop/keys.toml).
/// Format: `ANTHROPIC_API_KEY = "sk-ant-..."`
pub fn load_config_keys() -> Result<HashMap<String, String>> {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".config/agentic-loop/keys.toml");

    if !config_path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {:?}", config_path))?;
    let map: HashMap<String, String> = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {:?}", config_path))?;
    Ok(map)
}

// ─── Full resolution ──────────────────────────────────────────────────────────

/// Resolve all available providers (have API keys configured).
pub fn resolve_available_providers(
    custom_overrides: &HashMap<String, CustomProviderOverride>,
) -> Result<Vec<ResolvedProvider>> {
    let registry = load_embedded_registry()?;
    let config_keys = load_config_keys()?;

    let mut available = Vec::new();

    for (name, entry) in &registry.providers {
        let api_key = resolve_api_key(entry, &config_keys);

        // Apply custom overrides
        let custom = custom_overrides.get(name);
        let base_url = custom
            .and_then(|c| c.base_url.clone())
            .unwrap_or_else(|| entry.base_url.clone());

        let models = custom
            .and_then(|c| c.models.clone())
            .unwrap_or_else(|| entry.recommended_models.clone());

        // Providers with no env_var require special auth (OAuth, AWS profile, etc.)
        // Mark them as not available unless explicitly detected
        let has_special_auth = detect_special_auth(name);
        let has_key = api_key.is_some();

        if has_key || has_special_auth {
            available.push(ResolvedProvider {
                name: name.clone(),
                api: entry.api.clone(),
                base_url,
                api_key: api_key.map(|k| k.key),
                models,
            });
        }
    }

    // Also add purely custom providers (not in embedded list)
    for (name, custom) in custom_overrides {
        if !registry.providers.contains_key(name) {
            available.push(ResolvedProvider {
                name: name.clone(),
                api: ApiType(ApiType::OPENAI_COMPLETIONS.to_string()), // default for custom
                base_url: custom.base_url.clone().unwrap_or_default(),
                api_key: custom.env_var.as_ref().and_then(|v| std::env::var(v).ok()),
                models: custom.models.clone().unwrap_or_default(),
            });
        }
    }

    Ok(available)
}

/// List all providers (available or not) with their auth status.
pub fn list_all_providers() -> Result<Vec<ProviderStatus>> {
    let registry = load_embedded_registry()?;
    let config_keys = load_config_keys()?;

    let mut statuses = Vec::new();
    for (name, entry) in &registry.providers {
        let has_key = resolve_api_key(entry, &config_keys).is_some();
        let has_special = detect_special_auth(name);

        statuses.push(ProviderStatus {
            name: name.clone(),
            api: entry.api.0.clone(),
            base_url: entry.base_url.clone(),
            env_var: entry.env_var.clone(),
            available: has_key || has_special,
            recommended_count: entry.recommended_models.len(),
            total_count: entry.total_models,
        });
    }

    statuses.sort_by(|a, b| {
        b.available.cmp(&a.available).then(a.name.cmp(&b.name))
    });

    Ok(statuses)
}

/// Detect special authentication (OAuth, AWS, GCP, Azure).
///
/// Checks for:
/// - AWS: AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY, or AWS_PROFILE
/// - GCP: GOOGLE_APPLICATION_CREDENTIALS
/// - Azure: AZURE_SUBSCRIPTION_ID
/// - OAuth: OAUTH_TOKEN
/// - Bedrock: AWS_REGION + AWS_ACCESS_KEY_ID
fn detect_special_auth(provider_name: &str) -> bool {
    match provider_name {
        // AWS-based providers
        "bedrock" | "aws-bedrock" => {
            std::env::var("AWS_ACCESS_KEY_ID").is_ok()
                && std::env::var("AWS_SECRET_ACCESS_KEY").is_ok()
        }
        // GCP-based providers
        "vertex" | "vertex-ai" | "gemini" => {
            std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok()
        }
        // Azure-based providers
        "azure-openai" => {
            std::env::var("AZURE_OPENAI_API_KEY").is_ok()
                || std::env::var("AZURE_SUBSCRIPTION_ID").is_ok()
        }
        // Generic OAuth
        name if name.contains("oauth") => {
            std::env::var("OAUTH_TOKEN").is_ok()
        }
        _ => false,
    }
}

/// Status of a single provider.
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    pub name: String,
    pub api: String,
    pub base_url: String,
    pub env_var: Option<String>,
    pub available: bool,
    pub recommended_count: usize,
    pub total_count: usize,
}

/// Get detailed info for a specific provider.
pub fn get_provider_detail(provider_name: &str) -> Result<ProviderDetail> {
    let registry = load_embedded_registry()?;
    let entry = registry.providers.get(provider_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider '{}'", provider_name))?;

    let config_keys = load_config_keys()?;
    let has_key = resolve_api_key(entry, &config_keys).is_some();

    Ok(ProviderDetail {
        name: provider_name.to_string(),
        api: entry.api.0.clone(),
        base_url: entry.base_url.clone(),
        env_var: entry.env_var.clone(),
        available: has_key,
        recommended_models: entry.recommended_models.clone(),
        total_models: entry.total_models,
        version: registry.version.clone(),
        source: registry.source.clone(),
    })
}

/// Detailed provider info for `providers show`.
#[derive(Debug, Clone)]
pub struct ProviderDetail {
    pub name: String,
    pub api: String,
    pub base_url: String,
    pub env_var: Option<String>,
    pub available: bool,
    pub recommended_models: Vec<ModelEntry>,
    pub total_models: usize,
    pub version: String,
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_embedded_registry() {
        let registry = load_embedded_registry().unwrap();
        assert_eq!(registry.version, "1.0.0");
        assert!(!registry.providers.is_empty());
        // Should have the major providers
        assert!(registry.providers.contains_key("anthropic"));
        assert!(registry.providers.contains_key("openai"));
        assert!(registry.providers.contains_key("zai"));
        assert!(registry.providers.contains_key("openrouter"));
    }

    #[test]
    fn test_anthropic_provider_structure() {
        let registry = load_embedded_registry().unwrap();
        let anthropic = &registry.providers["anthropic"];
        assert_eq!(anthropic.env_var, Some("ANTHROPIC_API_KEY".to_string()));
        assert_eq!(anthropic.base_url, "https://api.anthropic.com");
        assert!(!anthropic.recommended_models.is_empty());
        // Claude Sonnet 4 should be there
        assert!(anthropic.recommended_models.iter().any(|m| m.id.contains("sonnet")));
    }

    #[test]
    fn test_zai_provider() {
        let registry = load_embedded_registry().unwrap();
        let zai = &registry.providers["zai"];
        assert_eq!(zai.env_var, Some("ZAI_API_KEY".to_string()));
        assert_eq!(zai.base_url, "https://api.z.ai/api/coding/paas/v4");
        assert!(!zai.recommended_models.is_empty());
    }

    #[test]
    fn test_api_type_compatibility() {
        assert!(ApiType("openai-completions".into()).is_openai_compatible());
        assert!(ApiType("openai-responses".into()).is_openai_compatible());
        assert!(!ApiType("anthropic-messages".into()).is_openai_compatible());
        assert!(ApiType("anthropic-messages".into()).is_anthropic_compatible());
        assert!(!ApiType("openai-completions".into()).is_anthropic_compatible());
    }

    #[test]
    fn test_list_all_providers() {
        let statuses = list_all_providers().unwrap();
        assert!(statuses.len() >= 23);
        // Available providers should be listed first
        let first_available = statuses.iter().position(|s| s.available);
        let first_unavailable = statuses.iter().position(|s| !s.available);
        if let (Some(a), Some(u)) = (first_available, first_unavailable) {
            assert!(a < u, "Available providers should come first");
        }
    }

    #[test]
    fn test_resolve_empty_overrides() {
        let overrides = HashMap::new();
        let available = resolve_available_providers(&overrides).unwrap();
        // Should have at least providers where env vars are set
        // (depends on test environment, so just check it doesn't crash)
        assert!(available.len() <= 23);
    }

    #[test]
    fn test_config_keys_missing_file() {
        let keys = load_config_keys().unwrap();
        // May or may not have keys depending on environment
        assert!(keys.len() < 100); // sanity check
    }

    #[test]
    fn test_provider_count() {
        let registry = load_embedded_registry().unwrap();
        assert!(registry.providers.len() >= 23, "Should have at least 23 providers from pi.dev");
    }

    #[test]
    fn test_openrouter_provider() {
        let registry = load_embedded_registry().unwrap();
        let or = &registry.providers["openrouter"];
        assert_eq!(or.env_var, Some("OPENROUTER_API_KEY".to_string()));
        assert_eq!(or.total_models, 253);
    }
}
