//! # agentic-loop-storage-gitrefs
//!
//! Git refs storage backend for agentic-loop entities.
//!
//! Entities are stored as Git blobs with refs in the format:
//! `refs/agent-loop/{project_id}/{entity_type}/{entity_id}`
//!
//! All entity data is JSON. Config values stored under:
//! `refs/agent-loop/{project_id}/config/{key}`

use agentic_loop_storage::{ConfigStore, EntityStore};
use agentic_loop_types::storage::{BackendCapability, QueryFilter, QueryResult};
use anyhow::{Context, Result};
use async_trait::async_trait;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};
use gix::refs::FullName;
use gix::refs::Target;
use std::path::Path;
use tokio::task::spawn_blocking;
use tracing::{debug, instrument};

/// Capabilities provided by this backend.
const CAPABILITIES: &[BackendCapability] = &[
    BackendCapability::EntityStore,
    BackendCapability::ConfigStore,
];

/// Git refs storage backend.
pub struct GitRefsBackend {
    repo_path: std::path::PathBuf,
    name: String,
}

impl GitRefsBackend {
    /// Create a new git refs backend for a repo at the given path.
    pub fn new(repo_path: &Path) -> Result<Self> {
        let _ = gix::discover(repo_path)
            .with_context(|| format!("Not a git repository: {}", repo_path.display()))?;
        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            name: format!("gitrefs:{}", repo_path.display()),
        })
    }

    /// Build the ref name for an entity.
    fn entity_ref(project_id: &str, entity_type: &str, entity_id: &str) -> String {
        format!(
            "refs/agent-loop/{}/{}/{}",
            project_id, entity_type, entity_id
        )
    }

    /// Build the ref prefix for listing entities of a type.
    fn entity_ref_prefix(project_id: &str, entity_type: &str) -> String {
        format!("refs/agent-loop/{}/{}/", project_id, entity_type)
    }

    /// Build the ref name for a config key.
    fn config_ref(project_id: &str, key: &str) -> String {
        format!("refs/agent-loop/{}/config/{}", project_id, key)
    }
}

/// Write a blob and point a ref at it.
fn write_blob_ref(repo: &gix::Repository, ref_name: &str, data: &[u8]) -> Result<()> {
    let blob_id = repo
        .write_object(&gix::objs::Blob {
            data: data.to_vec(),
        })
        .with_context(|| "Failed to write blob")?;

    let full_name =
        FullName::try_from(ref_name).with_context(|| format!("Invalid ref name: {}", ref_name))?;

    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange::default(),
            expected: PreviousValue::Any,
            new: Target::Object(blob_id.detach()),
        },
        name: full_name,
        deref: false,
    })
    .with_context(|| format!("Failed to update ref: {}", ref_name))?;

    Ok(())
}

/// Read a blob from a ref.
fn read_blob_ref(repo: &gix::Repository, ref_name: &str) -> Result<Option<Vec<u8>>> {
    let reference = match repo
        .try_find_reference(ref_name)
        .with_context(|| format!("Failed to find ref: {}", ref_name))?
    {
        Some(r) => r,
        None => return Ok(None),
    };

    let target_id = reference.try_id().with_context(|| {
        format!(
            "Expected object ref, got symbolic: {}",
            ref_name
        )
    })?;

    let obj = repo
        .find_object(target_id)
        .with_context(|| format!("Failed to find object for ref: {}", ref_name))?;

    Ok(Some(obj.data.to_vec()))
}

/// Delete a ref.
fn delete_ref(repo: &gix::Repository, ref_name: &str) -> Result<()> {
    let full_name =
        FullName::try_from(ref_name).with_context(|| format!("Invalid ref name: {}", ref_name))?;

    repo.edit_reference(RefEdit {
        change: Change::Delete {
            expected: PreviousValue::Any,
            log: gix::refs::transaction::RefLog::AndReference,
        },
        name: full_name,
        deref: false,
    })
    .with_context(|| format!("Failed to delete ref: {}", ref_name))?;

    Ok(())
}

/// List all refs matching a prefix, returning the suffix after the prefix.
fn list_refs_with_prefix(repo: &gix::Repository, prefix: &str) -> Result<Vec<String>> {
    let platform = repo
        .references()
        .with_context(|| "Failed to get references platform")?;
    let all_refs = platform
        .all()
        .with_context(|| "Failed to iterate references")?;

    let mut ids = Vec::new();
    for r_result in all_refs {
        let r = r_result.map_err(|e| anyhow::anyhow!("Failed to read reference: {}", e))?;
        let name = String::from_utf8_lossy(r.name().as_bstr()).to_string();
        if let Some(suffix) = name.strip_prefix(prefix) {
            ids.push(suffix.to_string());
        }
    }

    Ok(ids)
}

#[async_trait]
impl EntityStore for GitRefsBackend {
    #[instrument(
        skip(self, data),
        fields(project_id = %project_id, entity_type = %entity_type, entity_id = %entity_id)
    )]
    async fn store(
        &self,
        project_id: &str,
        entity_type: &str,
        entity_id: &str,
        data: &[u8],
    ) -> Result<()> {
        let ref_name = Self::entity_ref(project_id, entity_type, entity_id);
        let data = data.to_vec();
        let repo_path = self.repo_path.clone();

        debug!("Storing entity via ref: {}", ref_name);

        spawn_blocking(move || -> Result<()> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            write_blob_ref(&repo, &ref_name, &data)
        })
        .await
        .with_context(|| "spawn_blocking panicked")??;

        Ok(())
    }

    #[instrument(
        skip(self),
        fields(project_id = %project_id, entity_type = %entity_type, entity_id = %entity_id)
    )]
    async fn get(
        &self,
        project_id: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        let ref_name = Self::entity_ref(project_id, entity_type, entity_id);
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<Option<Vec<u8>>> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            read_blob_ref(&repo, &ref_name)
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    #[instrument(skip(self), fields(project_id = %project_id, entity_type = %entity_type))]
    async fn list_ids(&self, project_id: &str, entity_type: &str) -> Result<Vec<String>> {
        let prefix = Self::entity_ref_prefix(project_id, entity_type);
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<Vec<String>> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            list_refs_with_prefix(&repo, &prefix)
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    #[instrument(
        skip(self),
        fields(project_id = %project_id, entity_type = %entity_type, entity_id = %entity_id)
    )]
    async fn delete(
        &self,
        project_id: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        let ref_name = Self::entity_ref(project_id, entity_type, entity_id);
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<()> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            delete_ref(&repo, &ref_name)
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    async fn query(&self, project_id: &str, filter: &QueryFilter) -> Result<QueryResult> {
        let entity_type = filter.entity_type.clone().unwrap_or_default();
        let prefix = Self::entity_ref_prefix(project_id, &entity_type);
        let limit = filter.limit.unwrap_or(100);
        let offset = filter.offset.unwrap_or(0);
        let tags = filter.tags.clone();
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<QueryResult> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            let ids = list_refs_with_prefix(&repo, &prefix)?;

            let total = ids.len();
            let paged: Vec<String> = ids.into_iter().skip(offset).take(limit).collect();

            let mut entities = Vec::new();
            for id in &paged {
                let ref_name = format!("{}{}", prefix, id);
                if let Some(data) = read_blob_ref(&repo, &ref_name)? {
                    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&data) {
                        // Tag filtering
                        if !tags.is_empty() {
                            if let Some(entity_tags) = val.get("tags").and_then(|t| t.as_array())
                            {
                                let entity_tag_strs: Vec<&str> =
                                    entity_tags.iter().filter_map(|t| t.as_str()).collect();
                                if !tags
                                    .iter()
                                    .all(|t| entity_tag_strs.contains(&t.as_str()))
                                {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        entities.push(val);
                    }
                }
            }

            Ok(QueryResult {
                entities,
                total,
                has_more: offset + limit < total,
            })
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    async fn health_check(&self) -> Result<bool> {
        let repo_path = self.repo_path.clone();
        spawn_blocking(move || -> Result<bool> {
            let _ = gix::open(&repo_path)?;
            Ok(true)
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> &[BackendCapability] {
        CAPABILITIES
    }
}

#[async_trait]
impl ConfigStore for GitRefsBackend {
    async fn get_config(&self, project_id: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let ref_name = Self::config_ref(project_id, key);
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<Option<Vec<u8>>> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            read_blob_ref(&repo, &ref_name)
        })
        .await
        .with_context(|| "spawn_blocking panicked")?
    }

    async fn set_config(&self, project_id: &str, key: &str, value: &[u8]) -> Result<()> {
        let ref_name = Self::config_ref(project_id, key);
        let value = value.to_vec();
        let repo_path = self.repo_path.clone();

        spawn_blocking(move || -> Result<()> {
            let repo = gix::open(&repo_path)
                .with_context(|| format!("Failed to open repo: {}", repo_path.display()))?;
            write_blob_ref(&repo, &ref_name, &value)
        })
        .await
        .with_context(|| "spawn_blocking panicked")??;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_test_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path();

        // Initialize a git repo
        gix::init(repo_path).unwrap();

        // Create initial commit so HEAD exists
        let repo = gix::open(repo_path).unwrap();
        let empty_tree = gix::objs::Tree { entries: vec![] };
        let tree_id = repo.write_object(&empty_tree).unwrap();

        let sig = gix::actor::Signature {
            name: "test".into(),
            email: "test@localhost".into(),
            time: gix::date::Time::now_local_or_utc(),
        };
        let commit = gix::objs::Commit {
            tree: tree_id.detach(),
            parents: Default::default(),
            author: sig.clone(),
            committer: sig,
            message: "initial\n".into(),
            encoding: None,
            extra_headers: Default::default(),
        };
        let commit_id = repo.write_object(&commit).unwrap();

        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange::default(),
                expected: PreviousValue::MustNotExist,
                new: Target::Object(commit_id.detach()),
            },
            name: FullName::try_from("refs/heads/main").unwrap(),
            deref: false,
        })
        .unwrap();

        // Set HEAD to main
        repo.edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange::default(),
                expected: PreviousValue::Any,
                new: Target::Symbolic(
                    gix::refs::FullName::try_from("refs/heads/main").unwrap(),
                ),
            },
            name: FullName::try_from("HEAD").unwrap(),
            deref: false,
        })
        .unwrap();

        tmp
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let entity = serde_json::json!({
            "title": "Test Task",
            "status": "todo",
            "tags": ["test"]
        });
        let data = serde_json::to_vec(&entity).unwrap();

        backend
            .store("project1", "task", "abc-123", &data)
            .await
            .unwrap();

        let result = backend.get("project1", "task", "abc-123").await.unwrap();
        assert!(result.is_some());

        let retrieved: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(retrieved["title"], "Test Task");
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let result = backend
            .get("project1", "task", "nonexistent")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_ids() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let entity = serde_json::json!({"title": "Test"});
        let data = serde_json::to_vec(&entity).unwrap();

        backend
            .store("project1", "task", "id-1", &data)
            .await
            .unwrap();
        backend
            .store("project1", "task", "id-2", &data)
            .await
            .unwrap();
        backend
            .store("project1", "task", "id-3", &data)
            .await
            .unwrap();

        let ids = backend.list_ids("project1", "task").await.unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"id-1".to_string()));
        assert!(ids.contains(&"id-2".to_string()));
        assert!(ids.contains(&"id-3".to_string()));
    }

    #[tokio::test]
    async fn test_delete() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let entity = serde_json::json!({"title": "Delete Me"});
        let data = serde_json::to_vec(&entity).unwrap();

        backend
            .store("project1", "task", "to-delete", &data)
            .await
            .unwrap();
        let result = backend
            .get("project1", "task", "to-delete")
            .await
            .unwrap();
        assert!(result.is_some());

        backend
            .delete("project1", "task", "to-delete")
            .await
            .unwrap();
        let result = backend
            .get("project1", "task", "to-delete")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_project_isolation() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let entity = serde_json::json!({"title": "Project A Task"});
        let data = serde_json::to_vec(&entity).unwrap();

        backend
            .store("project-a", "task", "shared-id", &data)
            .await
            .unwrap();

        // Different project should not see it
        let ids = backend.list_ids("project-b", "task").await.unwrap();
        assert!(ids.is_empty());

        // Original project should see it
        let ids = backend.list_ids("project-a", "task").await.unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[tokio::test]
    async fn test_config_store() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let config = b"debug: true";
        backend
            .set_config("project1", "settings", config)
            .await
            .unwrap();

        let result = backend
            .get_config("project1", "settings")
            .await
            .unwrap();
        assert_eq!(result, Some(b"debug: true".to_vec()));
    }

    #[tokio::test]
    async fn test_query_with_tag_filter() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let entity_with_tags = serde_json::json!({
            "title": "Tagged",
            "tags": ["rust", "important"]
        });
        let entity_without_tags = serde_json::json!({
            "title": "Untagged"
        });

        backend
            .store(
                "p1",
                "task",
                "tagged",
                &serde_json::to_vec(&entity_with_tags).unwrap(),
            )
            .await
            .unwrap();
        backend
            .store(
                "p1",
                "task",
                "untagged",
                &serde_json::to_vec(&entity_without_tags).unwrap(),
            )
            .await
            .unwrap();

        let filter = QueryFilter {
            entity_type: Some("task".to_string()),
            tags: vec!["rust".to_string()],
            limit: Some(100),
            offset: None,
        };
        let result = backend.query("p1", &filter).await.unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0]["title"], "Tagged");
        assert_eq!(result.total, 2);
    }

    #[tokio::test]
    async fn test_health_check() {
        let tmp = init_test_repo();
        let backend = GitRefsBackend::new(tmp.path()).unwrap();

        let healthy = backend.health_check().await.unwrap();
        assert!(healthy);
    }
}
