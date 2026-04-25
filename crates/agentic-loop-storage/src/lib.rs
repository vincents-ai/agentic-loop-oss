//! # agentic-loop-storage
//!
//! Storage trait definitions for agentic-loop.
//! All traits are object-safe and WASM-compatible (serialized args/results).

use agentic_loop_types::storage::{BackendCapability, Document, QueryFilter, QueryResult, SearchResult};
use anyhow::Result;
use async_trait::async_trait;

/// Core entity storage trait. All backends implement this.
#[async_trait]
pub trait EntityStore: Send + Sync {
    /// Store an entity (JSON). Overwrites if exists.
    async fn store(&self, project_id: &str, entity_type: &str, entity_id: &str, data: &[u8]) -> Result<()>;

    /// Get an entity by ID.
    async fn get(&self, project_id: &str, entity_type: &str, entity_id: &str) -> Result<Option<Vec<u8>>>;

    /// List entity IDs of a given type.
    async fn list_ids(&self, project_id: &str, entity_type: &str) -> Result<Vec<String>>;

    /// Delete an entity.
    async fn delete(&self, project_id: &str, entity_type: &str, entity_id: &str) -> Result<()>;

    /// Query entities with a filter.
    async fn query(&self, project_id: &str, filter: &QueryFilter) -> Result<QueryResult>;

    /// Check if the backend is healthy.
    async fn health_check(&self) -> Result<bool>;

    /// Get the name of this backend.
    fn name(&self) -> &str;

    /// Get capabilities of this backend.
    fn capabilities(&self) -> &[BackendCapability];
}

/// Semantic search over entities.
#[async_trait]
pub trait KnowledgeSearch: Send + Sync {
    /// Search entities by semantic query.
    async fn search(&self, project_id: &str, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
}

/// Fetch documents from external sources (GitHub, Confluence, etc.).
#[async_trait]
pub trait DocumentFetch: Send + Sync {
    /// Fetch a document by URL or ID.
    async fn fetch(&self, source: &str, id: &str) -> Result<Document>;

    /// List available documents from a source.
    async fn list_sources(&self) -> Result<Vec<String>>;
}

/// Configuration storage.
#[async_trait]
pub trait ConfigStore: Send + Sync {
    /// Get a config value.
    async fn get_config(&self, project_id: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a config value.
    async fn set_config(&self, project_id: &str, key: &str, value: &[u8]) -> Result<()>;
}

/// Registry of storage backends per project.
#[async_trait]
pub trait BackendRegistry: Send + Sync {
    /// Get the primary entity store for a project.
    async fn primary_store(&self, project_id: &str) -> Result<Box<dyn EntityStore>>;

    /// Get a backend by name and capability.
    async fn get_backend(&self, project_id: &str, capability: BackendCapability) -> Option<Box<dyn EntityStore>>;

    /// Register a backend for a project.
    async fn register_backend(&self, project_id: &str, backend: Box<dyn EntityStore>) -> Result<()>;

    /// List all registered project IDs.
    async fn list_projects(&self) -> Result<Vec<String>>;
}

/// Storage error types.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Entity not found: {entity_type}/{entity_id}")]
    NotFound { entity_type: String, entity_id: String },

    #[error("Entity already exists: {entity_type}/{entity_id}")]
    AlreadyExists { entity_type: String, entity_id: String },

    #[error("Invalid data for entity {entity_type}/{entity_id}: {reason}")]
    InvalidData { entity_type: String, entity_id: String, reason: String },

    #[error("Backend unavailable: {backend}")]
    BackendUnavailable { backend: String },

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Storage operation failed: {0}")]
    Other(String),
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_storage_error_display() {
        let e = StorageError::NotFound { entity_type: "task".to_string(), entity_id: "abc".to_string() };
        assert!(e.to_string().contains("task/abc"));
    }

    #[test]
    fn test_storage_error_invalid_data() {
        let e = StorageError::InvalidData {
            entity_type: "session".to_string(),
            entity_id: "s1".to_string(),
            reason: "missing field".to_string(),
        };
        assert!(e.to_string().contains("missing field"));
    }

    #[test]
    fn test_storage_error_from_io() {
        let e = StorageError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(matches!(e, StorageError::Io(_)));
    }

    #[test]
    fn test_storage_error_project_not_found() {
        let e = StorageError::ProjectNotFound("missing".to_string());
        assert_eq!(e.to_string(), "Project not found: missing");
    }
}
