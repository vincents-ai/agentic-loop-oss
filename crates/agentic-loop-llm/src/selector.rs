//! Model selection logic with live provider discovery.
//!
//! Discovers models from configured providers at runtime via `list_models()`,
//! scores them against task requirements, and selects the best match.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Selection of a model for a given task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
    pub estimated_cost: f64,
    pub supports_function_calling: bool,
    /// Context window in tokens.
    #[serde(default)]
    pub context_window: u32,
    /// Whether this model is free.
    #[serde(default)]
    pub is_free: bool,
    /// Model strengths (e.g. "coding", "reasoning").
    #[serde(default)]
    pub strengths: Vec<String>,
    /// Score that led to this selection (for debugging).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Requirements for model selection.
#[derive(Debug, Clone, Default)]
pub struct TaskRequirements {
    pub task_type: TaskType,
    pub max_cost: Option<f64>,
    pub needs_function_calling: bool,
    pub preferred_provider: Option<String>,
    pub preferred_model: Option<String>,
    /// Required strengths (e.g. "coding", "reasoning").
    pub strengths_needed: Vec<String>,
    /// Required input modalities (e.g. "text", "image").
    pub input_modalities_needed: Vec<String>,
    /// Minimum context window in tokens.
    pub min_context_window: Option<u32>,
}

/// Type of task being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskType {
    #[default]
    Simple,
    Thinking,
    Coding,
    ComplexReasoning,
}

/// Default strength mapping per task type.
impl TaskType {
    pub fn default_strengths(&self) -> Vec<String> {
        match self {
            TaskType::Coding => vec!["coding".to_string()],
            TaskType::ComplexReasoning => vec!["reasoning".to_string()],
            TaskType::Thinking => vec!["reasoning".to_string()],
            TaskType::Simple => vec![],
        }
    }
}

/// A discovered model profile from a provider.
#[derive(Debug, Clone)]
pub struct ModelProfile {
    pub provider: String,
    pub model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub function_calling: bool,
    pub vision: bool,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub strengths: Vec<String>,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
    pub is_free: bool,
}

/// Selects the best model for a given task using live discovery.
pub struct ModelSelectorImpl {
    profiles: Vec<ModelProfile>,
}

impl ModelSelectorImpl {
    /// Create a selector with empty profile cache.
    /// Call `discover()` to populate from providers.
    pub fn new() -> Self {
        Self { profiles: Vec::new() }
    }

    /// Create a selector with pre-discovered profiles.
    pub fn with_profiles(profiles: Vec<ModelProfile>) -> Self {
        Self { profiles }
    }

    /// Discover models from a resolved provider via `list_models()`.
    /// Appends to the existing profile cache.
    pub async fn discover_from_provider(
        &mut self,
        provider: &dyn vincents_llm::provider::LLMProvider,
    ) -> Result<()> {
        match provider.list_models().await {
            Ok(models) => {
                for m in models {
                    let profile = ModelProfile {
                        provider: m.provider.clone(),
                        model: m.id.clone(),
                        context_window: m.context_window,
                        max_output_tokens: m.max_output_tokens,
                        function_calling: m.capabilities.function_calling,
                        vision: m.capabilities.vision,
                        input_modalities: m.capabilities.input_modalities.clone(),
                        output_modalities: m.capabilities.output_modalities.clone(),
                        strengths: m.capabilities.strengths.clone(),
                        cost_per_1k_input: m.pricing.as_ref().map(|p| p.prompt_tokens).unwrap_or(0.0),
                        cost_per_1k_output: m.pricing.as_ref().map(|p| p.completion_tokens).unwrap_or(0.0),
                        is_free: m.pricing.as_ref().map(|p| p.is_free).unwrap_or(false),
                    };
                    self.profiles.push(profile);
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Failed to discover models from {}: {}", provider.name(), e);
                Ok(()) // Non-fatal — just skip this provider
            }
        }
    }

    /// Number of discovered profiles.
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Get all discovered profiles.
    pub fn profiles(&self) -> &[ModelProfile] {
        &self.profiles
    }

    /// Select the best model for the given requirements.
    pub fn select(&self, requirements: &TaskRequirements) -> Result<ModelSelection> {
        // 1. If preferred model specified, try exact match first
        if let Some(ref preferred) = requirements.preferred_model {
            if let Some(profile) = self.find_exact(preferred, requirements.preferred_provider.as_deref()) {
                return Ok(self.profile_to_selection(profile, None));
            }
        }

        // 2. Filter candidates
        let candidates: Vec<&ModelProfile> = self.profiles.iter().filter(|p| {
            if requirements.needs_function_calling && !p.function_calling { return false; }
            if let Some(ref provider) = requirements.preferred_provider {
                if p.provider != *provider { return false; }
            }
            if let Some(min_ctx) = requirements.min_context_window {
                if p.context_window < min_ctx { return false; }
            }
            true
        }).collect();

        if candidates.is_empty() {
            anyhow::bail!(
                "No model found matching requirements ({} profiles discovered)",
                self.profiles.len()
            );
        }

        // 3. Score and rank
        let mut scored: Vec<(f64, &ModelProfile)> = candidates
            .iter()
            .map(|p| (self.score_profile(p, requirements), *p))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // 4. Apply cost filter and pick best
        let est_input = 2000u64;
        let est_output = 1000u64;

        for (score, profile) in &scored {
            let cost = (profile.cost_per_1k_input * est_input as f64 / 1000.0)
                + (profile.cost_per_1k_output * est_output as f64 / 1000.0);
            if let Some(max_cost) = requirements.max_cost {
                if cost > max_cost { continue; }
            }
            return Ok(self.profile_to_selection(profile, Some(*score)));
        }

        // 5. Fallback: pick cheapest or first free model
        let cheapest = scored.iter().min_by(|a, b| {
            let ca = a.1.cost_per_1k_input + a.1.cost_per_1k_output;
            let cb = b.1.cost_per_1k_input + b.1.cost_per_1k_output;
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        });
        match cheapest {
            Some((score, profile)) => Ok(self.profile_to_selection(profile, Some(*score))),
            None => anyhow::bail!("No model available"),
        }
    }

    /// Find an exact model/provider match.
    fn find_exact(&self, model: &str, provider: Option<&str>) -> Option<&ModelProfile> {
        if let Some(provider) = provider {
            self.profiles.iter().find(|p| p.model == model && p.provider == provider)
        } else {
            self.profiles.iter().find(|p| p.model == model)
        }
    }

    /// Score a model profile against requirements.
    ///
    /// Scoring weights:
    /// - Strength match: +0.3 per matched strength
    /// - Modality match: +0.2 per matched input modality
    /// - Context window: +0.2 if >= min required
    /// - Function calling: +0.15 if needed and supported
    /// - Cost bonus: +0.2 if free, +0.1 if cheap
    fn score_profile(&self, profile: &ModelProfile, requirements: &TaskRequirements) -> f64 {
        let mut score = 0.0;

        // Strength matching
        let needed_strengths = if requirements.strengths_needed.is_empty() {
            requirements.task_type.default_strengths()
        } else {
            requirements.strengths_needed.clone()
        };

        for needed in &needed_strengths {
            if profile.strengths.iter().any(|s| s.eq_ignore_ascii_case(needed)) {
                score += 0.3;
            }
        }

        // Modality matching
        for needed in &requirements.input_modalities_needed {
            if profile.input_modalities.iter().any(|m| m.eq_ignore_ascii_case(needed)) {
                score += 0.2;
            }
        }

        // Context window
        if let Some(min_ctx) = requirements.min_context_window {
            if profile.context_window >= min_ctx {
                score += 0.2;
            }
        }

        // Function calling
        if requirements.needs_function_calling && profile.function_calling {
            score += 0.15;
        }

        // Cost bonus
        if profile.is_free {
            score += 0.2;
        } else {
            let total_cost = profile.cost_per_1k_input + profile.cost_per_1k_output;
            if total_cost < 0.005 {
                score += 0.1; // Cheap model bonus
            }
        }

        score
    }

    fn profile_to_selection(&self, profile: &ModelProfile, score: Option<f64>) -> ModelSelection {
        let estimated_cost = (profile.cost_per_1k_input * 2.0) + (profile.cost_per_1k_output * 1.0);
        ModelSelection {
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            estimated_cost,
            supports_function_calling: profile.function_calling,
            context_window: profile.context_window,
            is_free: profile.is_free,
            strengths: profile.strengths.clone(),
            score,
        }
    }
}

impl Default for ModelSelectorImpl {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile(
        provider: &str,
        model: &str,
        strengths: Vec<&str>,
        is_free: bool,
        function_calling: bool,
        context_window: u32,
    ) -> ModelProfile {
        ModelProfile {
            provider: provider.to_string(),
            model: model.to_string(),
            context_window,
            max_output_tokens: context_window,
            function_calling,
            vision: false,
            input_modalities: vec!["text".to_string()],
            output_modalities: vec!["text".to_string()],
            strengths: strengths.into_iter().map(|s| s.to_string()).collect(),
            cost_per_1k_input: if is_free { 0.0 } else { 0.005 },
            cost_per_1k_output: if is_free { 0.0 } else { 0.015 },
            is_free,
        }
    }

    #[test]
    fn test_select_for_coding() {
        let selector = ModelSelectorImpl::with_profiles(vec![
            make_profile("openrouter", "deepseek/deepseek-chat", vec!["coding"], false, true, 128000),
            make_profile("openrouter", "google/gemma-3", vec![], true, true, 128000),
        ]);
        let selection = selector.select(&TaskRequirements {
            task_type: TaskType::Coding,
            max_cost: Some(0.1),
            needs_function_calling: true,
            ..Default::default()
        }).unwrap();
        assert!(selection.supports_function_calling);
        assert!(selection.strengths.contains(&"coding".to_string()));
    }

    #[test]
    fn test_select_preferred_model() {
        let selector = ModelSelectorImpl::with_profiles(vec![
            make_profile("openai", "gpt-4o-mini", vec![], false, true, 128000),
            make_profile("openrouter", "deepseek/deepseek-chat", vec!["coding"], false, true, 128000),
        ]);
        let selection = selector.select(&TaskRequirements {
            task_type: TaskType::Simple,
            preferred_provider: Some("openai".into()),
            preferred_model: Some("gpt-4o-mini".into()),
            ..Default::default()
        }).unwrap();
        assert_eq!(selection.model, "gpt-4o-mini");
        assert_eq!(selection.provider, "openai");
    }

    #[test]
    fn test_select_free_model() {
        let selector = ModelSelectorImpl::with_profiles(vec![
            make_profile("openrouter", "paid-model", vec!["coding"], false, true, 128000),
            make_profile("openrouter", "free-model", vec!["coding"], true, true, 128000),
        ]);
        let selection = selector.select(&TaskRequirements {
            task_type: TaskType::Coding,
            needs_function_calling: true,
            ..Default::default()
        }).unwrap();
        // Free model should win due to cost bonus
        assert!(selection.is_free);
    }

    #[test]
    fn test_select_with_strength_requirement() {
        let selector = ModelSelectorImpl::with_profiles(vec![
            make_profile("openrouter", "model-a", vec!["coding"], false, true, 128000),
            make_profile("openrouter", "model-b", vec!["reasoning", "coding"], false, true, 128000),
        ]);
        let selection = selector.select(&TaskRequirements {
            task_type: TaskType::Simple,
            strengths_needed: vec!["reasoning".to_string()],
            needs_function_calling: true,
            ..Default::default()
        }).unwrap();
        assert!(selection.strengths.contains(&"reasoning".to_string()));
    }

    #[test]
    fn test_select_with_context_window() {
        let selector = ModelSelectorImpl::with_profiles(vec![
            make_profile("openrouter", "small-ctx", vec![], false, true, 4096),
            make_profile("openrouter", "big-ctx", vec![], false, true, 200000),
        ]);
        let selection = selector.select(&TaskRequirements {
            task_type: TaskType::Simple,
            min_context_window: Some(100000),
            needs_function_calling: false,
            ..Default::default()
        }).unwrap();
        assert_eq!(selection.model, "big-ctx");
    }

    #[test]
    fn test_no_profiles_discovered() {
        let selector = ModelSelectorImpl::new();
        let result = selector.select(&TaskRequirements {
            task_type: TaskType::Coding,
            needs_function_calling: true,
            ..Default::default()
        });
        assert!(result.is_err());
    }
}
