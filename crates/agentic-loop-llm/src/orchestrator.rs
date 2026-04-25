//! TaskOrchestrator — hierarchical task decomposition and delegation.
//!
//! Coordinates multi-level agent execution where tasks can be decomposed
//! recursively with appropriate model types per level.

use std::collections::HashMap;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

use agentic_loop_types::session::TaskResult;
use agentic_loop_types::ModelType;

/// A hierarchical task with decomposition context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchicalTask {
    /// Unique ID.
    pub id: String,
    /// Description.
    pub description: String,
    /// Model type for execution.
    pub model_type: ModelType,
    /// Parent task ID (if child).
    pub parent_id: Option<String>,
    /// Child tasks (if decomposed).
    pub children: Vec<String>,
    /// Status.
    pub status: HierarchicalTaskStatus,
    /// Execution depth (0 = root).
    pub depth: u32,
    /// Max depth allowed.
    pub max_depth: u32,
    /// Attempts.
    pub attempts: u32,
    /// Max attempts before escalation.
    pub max_attempts: u32,
    /// Escalation message (if escalated).
    pub escalation_message: Option<String>,
}

impl HierarchicalTask {
    pub fn new(id: String, description: String, model_type: ModelType) -> Self {
        Self {
            id,
            description,
            model_type,
            parent_id: None,
            children: vec![],
            status: HierarchicalTaskStatus::Pending,
            depth: 0,
            max_depth: 3,
            attempts: 0,
            max_attempts: 3,
            escalation_message: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.children.is_empty()
    }

    pub fn can_continue(&self) -> bool {
        self.attempts < self.max_attempts && !self.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HierarchicalTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Escalated,
    Blocked,
}

/// Strategy for decomposing a task.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DecompositionStrategy {
    /// Sequential: children one at a time (preserves context).
    #[default]
    Sequential,
    /// Parallel: children concurrently (faster).
    Parallel,
    /// Auto: decide based on task complexity.
    Auto,
}

/// Orchestrator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Max depth for decomposition.
    pub max_depth: u32,
    /// Max attempts before escalation.
    pub max_attempts: u32,
    /// Strategy.
    pub strategy: DecompositionStrategy,
    /// Enable escalation to human.
    pub escalation_enabled: bool,
    /// Model type for planning.
    pub planning_model: ModelType,
    /// Model type for implementation.
    pub implementation_model: ModelType,
    /// Model type for review.
    pub review_model: ModelType,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_attempts: 3,
            strategy: DecompositionStrategy::Auto,
            escalation_enabled: true,
            planning_model: ModelType::Reasoning,
            implementation_model: ModelType::Coding,
            review_model: ModelType::Smart,
        }
    }
}

/// Result of decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecompositionResult {
    pub tasks: Vec<HierarchicalTask>,
    pub strategy: DecompositionStrategy,
}

/// TaskOrchestratorTrait — coordinate hierarchical execution.
pub trait TaskOrchestratorTrait: Send + Sync {
    /// Decompose a task into subtasks.
    fn decompose(&self, task: &HierarchicalTask) -> Result<DecompositionResult>;

    /// Dispatch a task for execution.
    fn dispatch(&self, task: &HierarchicalTask) -> Result<TaskResult>;

    /// Escalate to human.
    fn escalate(&self, task: &HierarchicalTask, reason: &str) -> Result<Option<String>>;

    /// Check if task should escalate.
    fn should_escalate(&self, task: &HierarchicalTask) -> bool;
}

/// Simple in-memory orchestrator.
pub struct TaskOrchestrator {
    config: OrchestratorConfig,
    tasks: RwLock<HashMap<String, HierarchicalTask>>,
    results: RwLock<HashMap<String, TaskResult>>,
}

impl TaskOrchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            tasks: RwLock::new(HashMap::new()),
            results: RwLock::new(HashMap::new()),
        }
    }

    pub async fn submit(&self, task: HierarchicalTask) -> String {
        let id = task.id.clone();
        self.tasks.write().await.insert(id.clone(), task);
        id
    }

    pub async fn get(&self, id: &str) -> Option<HierarchicalTask> {
        self.tasks.read().await.get(id).cloned()
    }

    pub async fn complete(&self, id: &str, result: TaskResult) {
        let success = result.success;
        self.results.write().await.insert(id.to_string(), result);
        if let Some(t) = self.tasks.write().await.get_mut(id) {
            t.status = if success {
                HierarchicalTaskStatus::Completed
            } else {
                HierarchicalTaskStatus::Failed
            };
        }
    }

    pub async fn get_result(&self, id: &str) -> Option<TaskResult> {
        self.results.read().await.get(id).cloned()
    }

    /// Execute a hierarchical task.
    pub async fn execute(&self, task_id: &str) -> Result<TaskResult> {
        let task = self.get(task_id).await.ok_or_else(|| anyhow::anyhow!("task not found"))?;
        
        match task.status {
            HierarchicalTaskStatus::Completed => {
                return Ok(self.get_result(task_id).await.unwrap_or_else(|| TaskResult {
                    success: true,
                    summary: "completed".to_string(),
                    steps: vec![],
                    tool_calls: vec![],
                    total_tokens: 0,
                    cost_usd: 0.0,
                    duration_ms: 0,
                }));
            }
            HierarchicalTaskStatus::Failed | HierarchicalTaskStatus::Escalated => {
                return Ok(TaskResult {
                    success: false,
                    summary: "task failed or escalated".to_string(),
                    steps: vec![],
                    tool_calls: vec![],
                    total_tokens: 0,
                    cost_usd: 0.0,
                    duration_ms: 0,
                });
            }
            _ => {}
        }

        info!("Executing hierarchical task: {}", task.description);

        // Mark in progress.
        if let Some(t) = self.tasks.write().await.get_mut(task_id) {
            t.status = HierarchicalTaskStatus::InProgress;
        }

        // Try execution.
        let result = self.dispatch(&task)?;
        let is_success = result.success;
        
        // Mark complete.
        self.complete(task_id, result).await;

        // Check if needs decomposition/second try.
        if !is_success && self.should_escalate(&task) {
            warn!("Task {} needs escalation", task_id);
            self.escalate(&task, "max attempts reached")?;
        }

        Ok(TaskResult {
            success: is_success,
            summary: if is_success { "executed successfully" } else { "failed" }.to_string(),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 0,
            cost_usd: 0.0,
            duration_ms: 0,
        })
    }
}

impl TaskOrchestratorTrait for TaskOrchestrator {
    fn decompose(&self, task: &HierarchicalTask) -> Result<DecompositionResult> {
        // Simple heuristic decomposition.
        // In a full implementation, this would use LLM to decompose.
        
        // If depth < max_depth and task is complex, decompose.
        if task.depth >= task.max_depth || task.description.len() < 100 {
            return Ok(DecompositionResult {
                tasks: vec![],
                strategy: DecompositionStrategy::Sequential,
            });
        }

        // Split by newlines or bullets for simple decomposition.
        let parts: Vec<&str> = task.description
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();

        if parts.len() <= 1 {
            return Ok(DecompositionResult {
                tasks: vec![],
                strategy: DecompositionStrategy::Sequential,
            });
        }

        let model = match task.depth {
            0 => self.config.planning_model,
            1 => self.config.implementation_model,
            _ => self.config.review_model,
        };

        let mut children = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let child = HierarchicalTask {
                id: format!("{}-sub{}", task.id, i),
                description: part.to_string(),
                model_type: model,
                parent_id: Some(task.id.clone()),
                children: vec![],
                status: HierarchicalTaskStatus::Pending,
                depth: task.depth + 1,
                max_depth: task.max_depth,
                attempts: 0,
                max_attempts: self.config.max_attempts,
                escalation_message: None,
            };
            children.push(child);
        }

        let strategy = if children.len() > 3 {
            DecompositionStrategy::Parallel
        } else {
            DecompositionStrategy::Sequential
        };

        Ok(DecompositionResult { tasks: children, strategy })
    }

    fn dispatch(&self, task: &HierarchicalTask) -> Result<TaskResult> {
        // Placeholder: in full impl, would execute via LLM with model_type.
        // For now, return mock result.
        
        info!("Dispatching task {} with model {:?}", task.id, task.model_type);
        
        Ok(TaskResult {
            success: true,
            summary: format!("executed via {:?}", task.model_type),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 100,
            cost_usd: 0.001,
            duration_ms: 100,
        })
    }

    fn escalate(&self, task: &HierarchicalTask, reason: &str) -> Result<Option<String>> {
        if !self.config.escalation_enabled {
            return Ok(None);
        }

        let message = format!(
            "Task '{}' (depth={}) failed after {} attempts: {}\n\
             Requires human intervention to proceed.",
            task.description,
            task.depth,
            task.attempts,
            reason
        );

        Ok(Some(message))
    }

    fn should_escalate(&self, task: &HierarchicalTask) -> bool {
        task.attempts >= task.max_attempts && self.config.escalation_enabled
    }
}

/// Execute with hierarchical decomposition.
pub async fn execute_hierarchical(
    orchestrator: &TaskOrchestrator,
    task: HierarchicalTask,
) -> Result<TaskResult> {
    let task_id = orchestrator.submit(task).await;
    orchestrator.execute(&task_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.planning_model, ModelType::Reasoning);
        assert_eq!(config.implementation_model, ModelType::Coding);
    }

    #[test]
    fn test_hierarchical_task_new() {
        let task = HierarchicalTask::new(
            "test-1".to_string(),
            "Fix the bug in authentication".to_string(),
            ModelType::Coding,
        );
        assert_eq!(task.depth, 0);
        assert_eq!(task.status, HierarchicalTaskStatus::Pending);
        // New task = leaf to execute (has no children to decompose)
        assert!(task.is_terminal());
    }

    #[tokio::test]
    async fn test_orchestrator_submit() {
        let config = OrchestratorConfig::default();
        let orchestrator = TaskOrchestrator::new(config);
        
        let task = HierarchicalTask::new(
            "test-1".to_string(),
            "Do something".to_string(),
            ModelType::Smart,
        );
        
        let id = orchestrator.submit(task).await;
        assert_eq!(id, "test-1");
        
        let retrieved = orchestrator.get(&id).await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_orchestrator_execute() {
        let config = OrchestratorConfig::default();
        let orchestrator = TaskOrchestrator::new(config);
        
        let task = HierarchicalTask::new(
            "test-1".to_string(),
            "Simple task".to_string(),
            ModelType::Smart,
        );
        
        let result = execute_hierarchical(&orchestrator, task).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_task_is_terminal() {
        let task = HierarchicalTask::new("test".to_string(), "desc".to_string(), ModelType::Auto);
        // No children = terminal (leaf task)
        assert!(task.is_terminal());
        
        // With children = not terminal (has subtasks)
        let mut task_with_children = HierarchicalTask::new("test".to_string(), "desc".to_string(), ModelType::Auto);
        task_with_children.children = vec!["child-1".to_string()];
        assert!(!task_with_children.is_terminal());
    }

    #[tokio::test]
    async fn test_task_complete() {
        let config = OrchestratorConfig::default();
        let orchestrator = TaskOrchestrator::new(config);
        
        let task = HierarchicalTask::new(
            "test-1".to_string(),
            "Do something".to_string(),
            ModelType::Smart,
        );
        
        let id = orchestrator.submit(task).await;
        let result = TaskResult {
            success: true,
            summary: "done".to_string(),
            steps: vec![],
            tool_calls: vec![],
            total_tokens: 50,
            cost_usd: 0.001,
            duration_ms: 50,
        };
        orchestrator.complete(&id, result).await;
        
        let stored = orchestrator.get_result(&id).await;
        assert!(stored.is_some());
        assert!(stored.unwrap().success);
    }

    #[test]
    fn test_decomposition_strategy_default() {
        assert_eq!(DecompositionStrategy::default(), DecompositionStrategy::Sequential);
    }
}