//! Engram entity tools — store, query, search, and manage relationships.
//!
//! `engram_store`: Create or update an entity in engram.
//! `engram_query`: Query entities with filters.
//! `engram_search`: Full-text search across entities.
//! `engram_get`: Retrieve a single entity by ID and type.
//! `engram_relationship`: Query relationships between entities.
//!
//! All tools delegate to engram's `Storage` trait via `spawn_blocking`
//! to avoid blocking the async runtime.

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use engram::storage::{GitRefsStorage, QueryFilter, SortOrder, Storage};
use engram::storage::relationship_storage::RelationshipStorage;
use engram::entities::GenericEntity;

// ─── Shared storage handle ────────────────────────────────────────────────────

/// Thread-safe handle to engram's Storage.
#[derive(Clone)]
pub struct EngramStorage {
    inner: Arc<Mutex<GitRefsStorage>>,
}

impl EngramStorage {
    /// Create a new storage handle pointing at the given repo path.
    pub fn new(repo_path: &str, agent: &str) -> Result<Self, anyhow::Error> {
        let storage = GitRefsStorage::new(repo_path, agent)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(storage)),
        })
    }

    /// Wrap an existing storage instance.
    pub fn from_storage(storage: GitRefsStorage) -> Self {
        Self {
            inner: Arc::new(Mutex::new(storage)),
        }
    }
}

// ─── engram_store ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngramStoreTool {
    info: ToolInfo,
    storage: EngramStorage,
}

#[derive(Deserialize)]
struct StoreArgs {
    entity_type: String,
    data: serde_json::Value,
    #[serde(default = "default_agent")]
    agent: String,
}

fn default_agent() -> String { "agentic-loop".to_string() }

#[derive(Serialize)]
struct StoreResult {
    id: String,
    entity_type: String,
    stored: bool,
}

impl EngramStoreTool {
    pub fn new(storage: EngramStorage) -> Self {
        Self {
            info: ToolInfo {
                name: "engram_store".to_string(),
                description: "Store an entity in engram.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "entity_type": { "type": "string" },
                        "data": { "type": "object" },
                        "agent": { "type": "string" }
                    },
                    "required": ["entity_type", "data"]
                }),
            },
            storage,
        }
    }
}

#[async_trait]
impl Tool for EngramStoreTool {
    agentic_loop_tools::impl_clone_box!(EngramStoreTool);
    fn info(&self) -> ToolInfo { self.info.clone() }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let store_args: StoreArgs = serde_json::from_slice(args)?;
        let storage = self.storage.clone();
        let entity_type = store_args.entity_type.clone();
        let id = uuid::Uuid::new_v4().to_string();

        let entity = GenericEntity {
            id: id.clone(),
            entity_type: store_args.entity_type,
            agent: store_args.agent,
            timestamp: chrono::Utc::now(),
            data: store_args.data,
        };

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut storage = storage.inner.lock()
                .map_err(|e| anyhow::anyhow!("lock: {}", e))?;
            storage.store(&entity)?;
            Ok(())
        }).await??;

        Ok(serde_json::to_vec(&StoreResult { id, entity_type, stored: true })?)
    }
}

// ─── engram_query ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngramQueryTool {
    info: ToolInfo,
    storage: EngramStorage,
}

#[derive(Deserialize)]
struct QueryArgs {
    entity_type: String,
    #[serde(default)]
    filters: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    text_search: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_limit() -> usize { 20 }

#[derive(Serialize)]
struct QueryResultOut {
    entities: Vec<serde_json::Value>,
    total: usize,
}

impl EngramQueryTool {
    pub fn new(storage: EngramStorage) -> Self {
        Self {
            info: ToolInfo {
                name: "engram_query".to_string(),
                description: "Query engram entities with filters and text search.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "entity_type": { "type": "string" },
                        "filters": { "type": "object" },
                        "text_search": { "type": "string" },
                        "limit": { "type": "integer" },
                        "offset": { "type": "integer" }
                    },
                    "required": ["entity_type"]
                }),
            },
            storage,
        }
    }
}

#[async_trait]
impl Tool for EngramQueryTool {
    agentic_loop_tools::impl_clone_box!(EngramQueryTool);
    fn info(&self) -> ToolInfo { self.info.clone() }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let query_args: QueryArgs = serde_json::from_slice(args)?;
        let storage = self.storage.clone();

        let entities = tokio::task::spawn_blocking(move || {
            let storage = storage.inner.lock()
                .map_err(|e| anyhow::anyhow!("lock: {}", e))?;

            if let Some(ref text) = query_args.text_search {
                let types = vec![query_args.entity_type.clone()];
                let results = storage.text_search(text, Some(&types), Some(query_args.limit))?;
                Ok::<_, anyhow::Error>(results)
            } else {
                let filter = QueryFilter {
                    entity_type: Some(query_args.entity_type),
                    agent: None,
                    text_search: None,
                    field_filters: query_args.filters.unwrap_or_default(),
                    time_range: None,
                    sort_by: None,
                    sort_order: SortOrder::Desc,
                    limit: Some(query_args.limit),
                    offset: Some(query_args.offset),
                };
                let result = storage.query(&filter)?;
                Ok(result.entities)
            }
        }).await??;

        let total = entities.len();
        let values: Vec<serde_json::Value> = entities.iter().map(|e| e.data.clone()).collect();
        Ok(serde_json::to_vec(&QueryResultOut { entities: values, total })?)
    }
}

// ─── engram_search ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngramSearchTool {
    info: ToolInfo,
    storage: EngramStorage,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    #[serde(default)]
    entity_types: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Serialize)]
struct SearchResultOut {
    results: Vec<SearchHit>,
    total: usize,
}

#[derive(Serialize)]
struct SearchHit {
    id: String,
    entity_type: String,
    snippet: String,
}

impl EngramSearchTool {
    pub fn new(storage: EngramStorage) -> Self {
        Self {
            info: ToolInfo {
                name: "engram_search".to_string(),
                description: "Full-text search across engram entities.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "entity_types": { "type": "array", "items": { "type": "string" } },
                        "limit": { "type": "integer" }
                    },
                    "required": ["query"]
                }),
            },
            storage,
        }
    }
}

#[async_trait]
impl Tool for EngramSearchTool {
    agentic_loop_tools::impl_clone_box!(EngramSearchTool);
    fn info(&self) -> ToolInfo { self.info.clone() }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let search_args: SearchArgs = serde_json::from_slice(args)?;
        let storage = self.storage.clone();

        let entities = tokio::task::spawn_blocking(move || {
            let storage = storage.inner.lock()
                .map_err(|e| anyhow::anyhow!("lock: {}", e))?;

            match search_args.entity_types {
                Some(ref types) if !types.is_empty() => {
                    storage.text_search(&search_args.query, Some(types.as_slice()), Some(search_args.limit))
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }
                _ => {
                    storage.text_search(&search_args.query, None, Some(search_args.limit))
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }
            }
        }).await??;

        let hits: Vec<SearchHit> = entities.iter().map(|e| {
            let content_str = serde_json::to_string(&e.data).unwrap_or_default();
            let snippet = if content_str.len() > 200 { &content_str[..200] } else { &content_str }.to_string();
            SearchHit {
                id: e.id.clone(),
                entity_type: e.entity_type.clone(),
                snippet,
            }
        }).collect();

        let total = hits.len();
        Ok(serde_json::to_vec(&SearchResultOut { results: hits, total })?)
    }
}

// ─── engram_get ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngramGetTool {
    info: ToolInfo,
    storage: EngramStorage,
}

#[derive(Deserialize)]
struct GetArgs {
    id: String,
    entity_type: String,
}

impl EngramGetTool {
    pub fn new(storage: EngramStorage) -> Self {
        Self {
            info: ToolInfo {
                name: "engram_get".to_string(),
                description: "Retrieve a single engram entity by ID and type.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "entity_type": { "type": "string" }
                    },
                    "required": ["id", "entity_type"]
                }),
            },
            storage,
        }
    }
}

#[async_trait]
impl Tool for EngramGetTool {
    agentic_loop_tools::impl_clone_box!(EngramGetTool);
    fn info(&self) -> ToolInfo { self.info.clone() }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let get_args: GetArgs = serde_json::from_slice(args)?;
        let storage = self.storage.clone();
        let id = get_args.id.clone();

        let result = tokio::task::spawn_blocking(move || {
            let storage = storage.inner.lock()
                .map_err(|e| anyhow::anyhow!("lock: {}", e))?;
            storage.get(&get_args.id, &get_args.entity_type)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await??;

        match result {
            Some(entity) => Ok(serde_json::to_vec(&entity)?),
            None => Ok(serde_json::to_vec(&serde_json::json!({"error": "not found", "id": id}))?),
        }
    }
}

// ─── engram_relationship ──────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EngramRelationshipTool {
    info: ToolInfo,
    storage: EngramStorage,
}

#[derive(Deserialize)]
struct RelationshipArgs {
    entity_id: String,
}

#[derive(Serialize)]
struct RelationshipResultOut {
    relationships: Vec<RelationshipEntry>,
    total: usize,
}

#[derive(Serialize)]
struct RelationshipEntry {
    id: String,
    source_id: String,
    target_id: String,
    relationship_type: String,
}

impl EngramRelationshipTool {
    pub fn new(storage: EngramStorage) -> Self {
        Self {
            info: ToolInfo {
                name: "engram_relationship".to_string(),
                description: "Query relationships for an engram entity.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "entity_id": { "type": "string" }
                    },
                    "required": ["entity_id"]
                }),
            },
            storage,
        }
    }
}

#[async_trait]
impl Tool for EngramRelationshipTool {
    agentic_loop_tools::impl_clone_box!(EngramRelationshipTool);
    fn info(&self) -> ToolInfo { self.info.clone() }

    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let rel_args: RelationshipArgs = serde_json::from_slice(args)?;
        let storage = self.storage.clone();

        let relationships = tokio::task::spawn_blocking(move || {
            let storage = storage.inner.lock()
                .map_err(|e| anyhow::anyhow!("lock: {}", e))?;
            storage.get_entity_relationships(&rel_args.entity_id)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await??;

        let entries: Vec<RelationshipEntry> = relationships.iter().map(|r| {
            RelationshipEntry {
                id: r.id.clone(),
                source_id: r.source_id.clone(),
                target_id: r.target_id.clone(),
                relationship_type: r.relationship_type.to_string(),
            }
        }).collect();

        let total = entries.len();
        Ok(serde_json::to_vec(&RelationshipResultOut { relationships: entries, total })?)
    }
}
