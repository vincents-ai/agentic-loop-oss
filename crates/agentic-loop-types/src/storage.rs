//! Storage data types.

use serde::{Deserialize, Serialize};

/// Capabilities a storage backend can provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendCapability {
    EntityStore,
    KnowledgeSearch,
    DocumentFetch,
    StructuredQuery,
    ConfigStore,
}

/// Filter for querying entities.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryFilter {
    pub entity_type: Option<String>,
    pub tags: Vec<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Result of a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub entities: Vec<serde_json::Value>,
    pub total: usize,
    pub has_more: bool,
}

/// Search result from knowledge search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entity_id: String,
    pub score: f64,
    pub snippet: String,
}

/// A document from a document fetch backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub content: String,
    pub source: String,
    pub metadata: serde_json::Value,
}
