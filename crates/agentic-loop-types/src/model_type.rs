//! Model type taxonomy — classifies what kind of brain a task needs.
//!
//! Used by:
//! - Tasks (task.model_type → which models are candidates)
//! - AgentPersona (default_model_type → default brain for this agent)
//! - ModelPool (filter candidates by type)
//! - Workflow states (optional per-step model hint)

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// What kind of brain a task needs. Determines which models are candidates in the pool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelType {
    /// Best available model regardless of cost. For complex reasoning, architecture, decomposition.
    Smart,
    /// Code generation / editing specialized. For implementation tasks.
    Coding,
    /// Fast responses, cost-optimized. For research, exploration, many quick calls.
    Fast,
    /// Deep reasoning / chain-of-thought. For evaluation, review, critical decisions.
    Reasoning,
    /// Vision/multimodal input. For screenshot analysis, diagram reading.
    Vision,
    /// Free models only. For cost-sensitive bulk operations.
    Free,
    /// No preference, use whatever is healthiest in the pool.
    #[default]
    Auto,
}

impl ModelType {
    /// All variants for iteration.
    pub fn all() -> &'static [ModelType] {
        &[
            ModelType::Smart,
            ModelType::Coding,
            ModelType::Fast,
            ModelType::Reasoning,
            ModelType::Vision,
            ModelType::Free,
            ModelType::Auto,
        ]
    }

    /// Parse from a comma-separated string of tags.
    /// "fast,free" → vec![Fast, Free]
    /// "coding" → vec![Coding]
    /// Empty string → vec![Auto]
    pub fn parse_tags(input: &str) -> Vec<ModelType> {
        let tags: Vec<ModelType> = input
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if tags.is_empty() {
            vec![ModelType::Auto]
        } else {
            tags
        }
    }

    /// Check if a model name matches this type's heuristic.
    /// Used for auto-classification during model discovery.
    pub fn matches_model_name(&self, model_id: &str) -> bool {
        let lower = model_id.to_lowercase();
        match self {
            ModelType::Coding => {
                lower.contains("coder") || lower.contains("code") || lower.contains("devstral")
            }
            ModelType::Reasoning => {
                lower.contains("think") || lower.contains("reason")
                    || lower.contains("-r1") || lower.contains("o1-")
                    || lower.contains("o3-") || lower.contains("deepseek-r")
            }
            ModelType::Fast => {
                // Small/fast models — heuristic based on name patterns
                lower.contains("flash") || lower.contains("mini") || lower.contains("nano")
                    || lower.contains("tiny") || lower.contains("lite") || lower.contains("fast")
            }
            ModelType::Vision => {
                lower.contains("vision") || lower.contains("vl") || lower.contains("vlm")
                    || lower.contains("gemma-3-") || lower.contains("gemma-4-")
            }
            ModelType::Free => {
                lower.contains(":free")
            }
            _ => true, // Smart, Auto match everything
        }
    }
}

impl fmt::Display for ModelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelType::Smart => write!(f, "smart"),
            ModelType::Coding => write!(f, "coding"),
            ModelType::Fast => write!(f, "fast"),
            ModelType::Reasoning => write!(f, "reasoning"),
            ModelType::Vision => write!(f, "vision"),
            ModelType::Free => write!(f, "free"),
            ModelType::Auto => write!(f, "auto"),
        }
    }
}

impl FromStr for ModelType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "smart" => Ok(ModelType::Smart),
            "coding" | "code" => Ok(ModelType::Coding),
            "fast" | "quick" => Ok(ModelType::Fast),
            "reasoning" | "reason" | "think" => Ok(ModelType::Reasoning),
            "vision" | "visual" | "image" => Ok(ModelType::Vision),
            "free" => Ok(ModelType::Free),
            "auto" | "default" | "any" => Ok(ModelType::Auto),
            other => Err(format!("Unknown model type: '{}'. Valid: smart, coding, fast, reasoning, vision, free, auto", other)),
        }
    }
}

/// Model capabilities for filtering pool candidates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    /// Supports function/tool calling.
    ToolCalling,
    /// Accepts image inputs.
    Vision,
    /// Specialized for code generation.
    Coding,
    /// Supports chain-of-thought / reasoning.
    Reasoning,
    /// Supports streaming responses.
    Streaming,
}

/// Cost tier for model classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelCostTier {
    /// Free tier (pricing.prompt == "0").
    #[default]
    Free,
    /// Preview / beta models (limited availability).
    Preview,
    /// Paid models (requires billing).
    Paid,
}

impl fmt::Display for ModelCostTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelCostTier::Free => write!(f, "free"),
            ModelCostTier::Preview => write!(f, "preview"),
            ModelCostTier::Paid => write!(f, "paid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_type_roundtrip() {
        for mt in ModelType::all() {
            let s = mt.to_string();
            let parsed: ModelType = s.parse().unwrap();
            assert_eq!(*mt, parsed);
        }
    }

    #[test]
    fn test_model_type_from_str_aliases() {
        assert_eq!("code".parse(), Ok(ModelType::Coding));
        assert_eq!("quick".parse(), Ok(ModelType::Fast));
        assert_eq!("reason".parse(), Ok(ModelType::Reasoning));
        assert_eq!("visual".parse(), Ok(ModelType::Vision));
        assert_eq!("any".parse(), Ok(ModelType::Auto));
    }

    #[test]
    fn test_model_type_invalid() {
        assert!("unknown".parse::<ModelType>().is_err());
    }

    #[test]
    fn test_model_type_serde() {
        let mt = ModelType::Coding;
        let json = serde_json::to_string(&mt).unwrap();
        assert_eq!(json, "\"coding\"");
        let parsed: ModelType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ModelType::Coding);
    }

    #[test]
    fn test_model_type_default() {
        assert_eq!(ModelType::default(), ModelType::Auto);
    }

    #[test]
    fn test_parse_tags_single() {
        let tags = ModelType::parse_tags("coding");
        assert_eq!(tags, vec![ModelType::Coding]);
    }

    #[test]
    fn test_parse_tags_multiple() {
        let tags = ModelType::parse_tags("fast, free");
        assert_eq!(tags, vec![ModelType::Fast, ModelType::Free]);
    }

    #[test]
    fn test_parse_tags_empty() {
        let tags = ModelType::parse_tags("");
        assert_eq!(tags, vec![ModelType::Auto]);
    }

    #[test]
    fn test_matches_model_name_coding() {
        assert!(ModelType::Coding.matches_model_name("qwen/qwen3-coder:free"));
        assert!(ModelType::Coding.matches_model_name("devstral-small"));
        assert!(!ModelType::Coding.matches_model_name("tencent/hy3-preview:free"));
    }

    #[test]
    fn test_matches_model_name_reasoning() {
        assert!(ModelType::Reasoning.matches_model_name("deepseek-r1:free"));
        assert!(ModelType::Reasoning.matches_model_name("openai/o1-preview"));
        assert!(!ModelType::Reasoning.matches_model_name("qwen/qwen3-coder:free"));
    }

    #[test]
    fn test_matches_model_name_fast() {
        assert!(ModelType::Fast.matches_model_name("google/gemma-2-flash:free"));
        assert!(ModelType::Fast.matches_model_name("nvidia/nemotron-nano-9b-v2:free"));
        assert!(ModelType::Fast.matches_model_name("meta-llama/llama-3.3-mini:free"));
        assert!(!ModelType::Fast.matches_model_name("tencent/hy3-preview:free"));
    }

    #[test]
    fn test_matches_model_name_free() {
        assert!(ModelType::Free.matches_model_name("tencent/hy3-preview:free"));
        assert!(!ModelType::Free.matches_model_name("openai/gpt-4o"));
    }

    #[test]
    fn test_cost_tier_display() {
        assert_eq!(ModelCostTier::Free.to_string(), "free");
        assert_eq!(ModelCostTier::Preview.to_string(), "preview");
        assert_eq!(ModelCostTier::Paid.to_string(), "paid");
    }
}
