//! Git operation tools (status, diff, log, show) for the project repository.
//!
//! These tools operate on the project's git repo (not engram's internal storage).
//! Uses gix for repository reads where practical, git CLI for complex operations
//! like diff where gix's API is unwieldy.

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ─── git_status ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GitStatusTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GitStatusArgs {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct GitStatusResult {
    path: String,
    branch: Option<String>,
    staged: Vec<String>,
    unstaged: Vec<String>,
    untracked: Vec<String>,
}

impl GitStatusTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "git_status".into(),
                description: "Show git working tree status (branch, staged, unstaged, untracked files).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Repository path (default: current directory)"}
                    }
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for GitStatusTool {
    agentic_loop_tools::impl_clone_box!(GitStatusTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GitStatusArgs = serde_json::from_slice(args)?;
        let repo_path = args.path.as_deref().unwrap_or(".");

        let output = tokio::process::Command::new("git")
            .args(["-C", repo_path, "status", "--porcelain=v1", "--branch"])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut branch = None;
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();

        for line in stdout.lines() {
            if line.starts_with("## ") {
                branch = line[3..].split("...").next().map(|s| s.to_string());
                continue;
            }
            if line.len() < 4 { continue; }
            let status = &line[..2];
            let file = line[3..].to_string();
            match status {
                "A " | "M " | "D " | "R " | "C " => staged.push(file),
                "AM" | "MM" => { staged.push(file.clone()); unstaged.push(file); }
                " M" | " D" => unstaged.push(file),
                "??" => untracked.push(file),
                _ => {}
            }
        }

        Ok(serde_json::to_vec(&GitStatusResult {
            path: repo_path.to_string(),
            branch,
            staged,
            unstaged,
            untracked,
        })?)
    }

    fn info(&self) -> ToolInfo { self.info.clone() }
}

// ─── git_log ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GitLogTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GitLogArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_max")]
    max_count: usize,
}

fn default_max() -> usize { 20 }

#[derive(Serialize, Deserialize)]
struct GitLogResult {
    commits: Vec<CommitInfo>,
    total: usize,
}

#[derive(Serialize, Deserialize)]
struct CommitInfo {
    hash: String,
    author: String,
    date: String,
    message: String,
}

impl GitLogTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "git_log".into(),
                description: "Show recent git commit history.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Repository path"},
                        "max_count": {"type": "integer", "description": "Maximum commits (default: 20)"}
                    }
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for GitLogTool {
    agentic_loop_tools::impl_clone_box!(GitLogTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GitLogArgs = serde_json::from_slice(args)?;
        let repo_path = args.path.as_deref().unwrap_or(".");

        let output = tokio::process::Command::new("git")
            .args(["-C", repo_path, "log", &format!("-{}", args.max_count),
                   "--pretty=format:%h|%an|%ai|%s"])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<CommitInfo> = stdout.lines().filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() == 4 {
                Some(CommitInfo {
                    hash: parts[0].to_string(),
                    author: parts[1].to_string(),
                    date: parts[2][..16].to_string(), // truncate to "2024-01-01 12:00"
                    message: parts[3].to_string(),
                })
            } else { None }
        }).collect();

        let total = commits.len();
        Ok(serde_json::to_vec(&GitLogResult { commits, total })?)
    }

    fn info(&self) -> ToolInfo { self.info.clone() }
}

// ─── git_diff ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GitDiffTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GitDiffArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[derive(Serialize, Deserialize)]
struct GitDiffResult {
    path: String,
    diff: String,
    files_changed: usize,
}

impl GitDiffTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "git_diff".into(),
                description: "Show git diff of working tree or staged changes.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Repository path"},
                        "file": {"type": "string", "description": "Specific file to diff"},
                        "staged": {"type": "boolean", "description": "Show staged changes"}
                    }
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    agentic_loop_tools::impl_clone_box!(GitDiffTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GitDiffArgs = serde_json::from_slice(args)?;
        let repo_path = args.path.as_deref().unwrap_or(".");

        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["-C", repo_path, "diff"]);
        if args.staged { cmd.arg("--staged"); }
        if let Some(ref f) = args.file { cmd.arg("--").arg(f); }

        let output = cmd.output().await?;
        let mut diff = String::from_utf8_lossy(&output.stdout).to_string();

        if diff.len() > 10000 {
            diff = format!("{}...\n[truncated, {} total bytes]", &diff[..10000], diff.len());
        }

        let files_changed = diff.lines()
            .filter(|l| l.starts_with("diff --git"))
            .count();

        Ok(serde_json::to_vec(&GitDiffResult {
            path: repo_path.to_string(),
            diff,
            files_changed,
        })?)
    }

    fn info(&self) -> ToolInfo { self.info.clone() }
}

// ─── git_show ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GitShowTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GitShowArgs {
    revision: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    file: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct GitShowResult {
    revision: String,
    content: String,
}

impl GitShowTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "git_show".into(),
                description: "Show commit details or a file at a specific revision.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "revision": {"type": "string", "description": "Commit hash, branch, or tag"},
                        "path": {"type": "string", "description": "Repository path"},
                        "file": {"type": "string", "description": "File path to show at revision"}
                    },
                    "required": ["revision"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for GitShowTool {
    agentic_loop_tools::impl_clone_box!(GitShowTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GitShowArgs = serde_json::from_slice(args)?;
        let repo_path = args.path.as_deref().unwrap_or(".");

        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["-C", repo_path]);

        if let Some(ref file) = args.file {
            cmd.args(["show", &format!("{}:{}", args.revision, file)]);
        } else {
            cmd.args(["show", "--stat", "--patch", &args.revision]);
        }

        let output = cmd.output().await?;
        let mut content = String::from_utf8_lossy(&output.stdout).to_string();

        if content.len() > 50000 {
            content = format!("{}...\n[truncated, {} total bytes]", &content[..50000], content.len());
        }

        Ok(serde_json::to_vec(&GitShowResult {
            revision: args.revision,
            content,
        })?)
    }

    fn info(&self) -> ToolInfo { self.info.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo(tmp: &TempDir) {
        let p = tmp.path().to_string_lossy().to_string();
        std::process::Command::new("git").args(["init", &p]).output().unwrap();
        std::process::Command::new("git").args(["-C", &p, "config", "user.email", "t@t.com"]).output().unwrap();
        std::process::Command::new("git").args(["-C", &p, "config", "user.name", "T"]).output().unwrap();
    }

    fn commit(tmp: &TempDir, file: &str, content: &str, msg: &str) {
        let p = tmp.path().to_string_lossy().to_string();
        std::fs::write(tmp.path().join(file), content).unwrap();
        std::process::Command::new("git").args(["-C", &p, "add", file]).output().unwrap();
        std::process::Command::new("git").args(["-C", &p, "commit", "-m", msg]).output().unwrap();
    }

    #[tokio::test]
    async fn test_git_status() {
        let tmp = TempDir::new().unwrap();
        init_repo(&tmp);
        commit(&tmp, "a.txt", "hi", "init");
        std::fs::write(tmp.path().join("b.txt"), "new").unwrap();

        let tool = GitStatusTool::new();
        let args = serde_json::to_vec(&serde_json::json!({"path": tmp.path().to_str().unwrap()})).unwrap();
        let r: GitStatusResult = serde_json::from_slice(&tool.execute(&args).await.unwrap()).unwrap();
        assert!(r.branch.is_some());
        assert!(r.untracked.contains(&"b.txt".to_string()));
    }

    #[tokio::test]
    async fn test_git_log() {
        let tmp = TempDir::new().unwrap();
        init_repo(&tmp);
        commit(&tmp, "a.txt", "v1", "first");
        commit(&tmp, "a.txt", "v2", "second");

        let tool = GitLogTool::new();
        let args = serde_json::to_vec(&serde_json::json!({"path": tmp.path().to_str().unwrap()})).unwrap();
        let r: GitLogResult = serde_json::from_slice(&tool.execute(&args).await.unwrap()).unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.commits[0].message, "second");
    }

    #[tokio::test]
    async fn test_git_diff() {
        let tmp = TempDir::new().unwrap();
        init_repo(&tmp);
        commit(&tmp, "a.txt", "hello", "init");
        std::fs::write(tmp.path().join("a.txt"), "hello world").unwrap();

        let tool = GitDiffTool::new();
        let args = serde_json::to_vec(&serde_json::json!({"path": tmp.path().to_str().unwrap()})).unwrap();
        let r: GitDiffResult = serde_json::from_slice(&tool.execute(&args).await.unwrap()).unwrap();
        assert!(r.diff.contains("hello"));
    }

    #[tokio::test]
    async fn test_git_show() {
        let tmp = TempDir::new().unwrap();
        init_repo(&tmp);
        commit(&tmp, "a.txt", "content here", "init");

        let tool = GitShowTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "revision": "HEAD", "path": tmp.path().to_str().unwrap(), "file": "a.txt"
        })).unwrap();
        let r: GitShowResult = serde_json::from_slice(&tool.execute(&args).await.unwrap()).unwrap();
        assert_eq!(r.content, "content here");
    }
}
