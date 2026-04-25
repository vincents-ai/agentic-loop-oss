//! File operation tools (read, write, edit, ls) using gix.
//!
//! These tools operate on files in a git repository using gix for
//! blob/tree operations. No subprocess calls.

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Read file contents.
#[derive(Clone)]
pub struct FileReadTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ReadResult {
    content: String,
    path: String,
    lines: usize,
}

impl FileReadTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_read".into(),
                description: "Read file contents. Supports offset/limit for large files.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to read"},
                        "offset": {"type": "integer", "description": "Line number to start from (1-indexed)"},
                        "limit": {"type": "integer", "description": "Maximum lines to read"}
                    },
                    "required": ["path"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    agentic_loop_tools::impl_clone_box!(FileReadTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: ReadArgs = serde_json::from_slice(args)?;

        let content = tokio::fs::read_to_string(&args.path).await?;
        let all_lines: Vec<&str> = content.lines().collect();
        let offset = args.offset.unwrap_or(1).saturating_sub(1);
        let limit = args.limit.unwrap_or(all_lines.len());

        let selected: Vec<&str> = all_lines
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        let result = ReadResult {
            content: selected.join("\n"),
            path: args.path,
            lines: selected.len(),
        };

        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// Write file contents.
#[derive(Clone)]
pub struct FileWriteTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct WriteResult {
    path: String,
    bytes_written: usize,
}

impl FileWriteTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_write".into(),
                description: "Write content to a file. Creates parent directories if needed.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to write"},
                        "content": {"type": "string", "description": "Content to write"}
                    },
                    "required": ["path", "content"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    agentic_loop_tools::impl_clone_box!(FileWriteTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: WriteArgs = serde_json::from_slice(args)?;

        // Create parent directories
        if let Some(parent) = Path::new(&args.path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let bytes = args.content.len();
        tokio::fs::write(&args.path, &args.content).await?;

        let result = WriteResult {
            path: args.path,
            bytes_written: bytes,
        };

        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// Edit file contents (find and replace).
#[derive(Clone)]
pub struct FileEditTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Serialize)]
struct EditResult {
    path: String,
    replacements: usize,
}

impl FileEditTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_edit".into(),
                description: "Edit a file by replacing exact text matches.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to edit"},
                        "old_text": {"type": "string", "description": "Exact text to find"},
                        "new_text": {"type": "string", "description": "Replacement text"}
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileEditTool {
    agentic_loop_tools::impl_clone_box!(FileEditTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: EditArgs = serde_json::from_slice(args)?;

        let content = tokio::fs::read_to_string(&args.path).await?;
        let count = content.matches(&args.old_text).count();

        if count == 0 {
            anyhow::bail!("Text not found in file: {}", args.path);
        }

        let new_content = content.replace(&args.old_text, &args.new_text);
        tokio::fs::write(&args.path, &new_content).await?;

        let result = EditResult {
            path: args.path,
            replacements: count,
        };

        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

/// List directory contents.
#[derive(Clone)]
pub struct FileLsTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct LsArgs {
    path: String,
}

#[derive(Serialize)]
struct LsResult {
    path: String,
    entries: Vec<LsEntry>,
}

#[derive(Serialize)]
struct LsEntry {
    name: String,
    is_dir: bool,
    size: Option<u64>,
}

impl FileLsTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_ls".into(),
                description: "List directory contents.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path to list"},
                        "recursive": {"type": "boolean", "description": "List recursively"}
                    },
                    "required": ["path"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileLsTool {
    agentic_loop_tools::impl_clone_box!(FileLsTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: LsArgs = serde_json::from_slice(args)?;
        let path = Path::new(&args.path);

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(path).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata().await?;
            entries.push(LsEntry {
                name,
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() { Some(metadata.len()) } else { None },
            });
        }

        entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
        });

        let result = LsResult {
            path: args.path,
            entries,
        };

        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_read_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");

        // Write
        let write_tool = FileWriteTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap(),
            "content": "Hello\nWorld\n"
        }))
        .unwrap();
        let result = write_tool.execute(&args).await.unwrap();
        let write_result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(write_result["bytes_written"], 12);

        // Read
        let read_tool = FileReadTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap()
        }))
        .unwrap();
        let result = read_tool.execute(&args).await.unwrap();
        let read_result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(read_result["content"], "Hello\nWorld");
        assert_eq!(read_result["lines"], 2);
    }

    #[tokio::test]
    async fn test_file_read_with_offset() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("lines.txt");
        tokio::fs::write(&path, "line1\nline2\nline3\nline4\nline5\n").await.unwrap();

        let read_tool = FileReadTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap(),
            "offset": 2,
            "limit": 2
        }))
        .unwrap();
        let result = read_tool.execute(&args).await.unwrap();
        let read_result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(read_result["content"], "line2\nline3");
        assert_eq!(read_result["lines"], 2);
    }

    #[tokio::test]
    async fn test_file_edit() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("edit.txt");
        tokio::fs::write(&path, "foo bar baz").await.unwrap();

        let edit_tool = FileEditTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap(),
            "old_text": "bar",
            "new_text": "REPLACED"
        }))
        .unwrap();
        let result = edit_tool.execute(&args).await.unwrap();
        let edit_result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(edit_result["replacements"], 1);

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "foo REPLACED baz");
    }

    #[tokio::test]
    async fn test_file_edit_not_found() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("edit.txt");
        tokio::fs::write(&path, "hello world").await.unwrap();

        let edit_tool = FileEditTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap(),
            "old_text": "nonexistent",
            "new_text": "x"
        }))
        .unwrap();
        assert!(edit_tool.execute(&args).await.is_err());
    }

    #[tokio::test]
    async fn test_file_ls() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "a").await.unwrap();
        tokio::fs::write(tmp.path().join("b.txt"), "bb").await.unwrap();
        tokio::fs::create_dir(tmp.path().join("subdir")).await.unwrap();

        let ls_tool = FileLsTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": tmp.path().to_str().unwrap()
        }))
        .unwrap();
        let result = ls_tool.execute(&args).await.unwrap();
        let ls_result: serde_json::Value = serde_json::from_slice(&result).unwrap();

        let entries = ls_result["entries"].as_array().unwrap();
        assert!(entries.len() >= 3);
        // Directories should be first
        assert_eq!(entries[0]["is_dir"], true);
        assert_eq!(entries[0]["name"], "subdir");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested/deep/file.txt");

        let write_tool = FileWriteTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "path": path.to_str().unwrap(),
            "content": "deep content"
        }))
        .unwrap();
        let result = write_tool.execute(&args).await.unwrap();
        let write_result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(write_result["bytes_written"], 12);

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "deep content");
    }
}
