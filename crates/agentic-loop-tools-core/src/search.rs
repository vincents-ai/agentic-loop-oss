//! Search tools (grep, glob) for finding files and content.
//!
//! `file_grep`: Searches file contents using regex. Uses `grep` crate for
//! pure-Rust regex matching (no subprocess).
//!
//! `file_glob`: Finds files matching a glob pattern using the `glob` crate.

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ─── file_grep ────────────────────────────────────────────────────────────────

/// Search file contents with regex pattern matching.
#[derive(Clone)]
pub struct FileGrepTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GrepArgs {
    /// Regex pattern to search for.
    pattern: String,
    /// Directory or file to search in.
    #[serde(default)]
    path: Option<String>,
    /// File extensions to include (e.g. ["rs", "toml"]).
    #[serde(default)]
    extensions: Option<Vec<String>>,
    /// Maximum number of matches to return.
    #[serde(default = "default_max_matches")]
    max_matches: usize,
    /// Whether to include line numbers.
    #[serde(default = "default_true")]
    line_numbers: bool,
    /// Whether search is case-insensitive.
    #[serde(default)]
    case_insensitive: bool,
}

fn default_max_matches() -> usize { 50 }
fn default_true() -> bool { true }

#[derive(Serialize, Deserialize)]
struct GrepResult {
    pattern: String,
    matches: Vec<GrepMatch>,
    total_files_searched: usize,
    total_matches: usize,
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct GrepMatch {
    file: String,
    line: Option<usize>,
    content: String,
}

impl FileGrepTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_grep".into(),
                description: "Search file contents with regex pattern. Returns matching lines with file paths and line numbers.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Regex pattern to search for"},
                        "path": {"type": "string", "description": "Directory or file to search in (default: current directory)"},
                        "extensions": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "File extensions to include (e.g. [\"rs\", \"toml\"])"
                        },
                        "max_matches": {"type": "integer", "description": "Maximum matches to return (default: 50)"},
                        "line_numbers": {"type": "boolean", "description": "Include line numbers (default: true)"},
                        "case_insensitive": {"type": "boolean", "description": "Case-insensitive search (default: false)"}
                    },
                    "required": ["pattern"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileGrepTool {
    agentic_loop_tools::impl_clone_box!(FileGrepTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GrepArgs = serde_json::from_slice(args)?;
        let search_path = args.path.as_deref().unwrap_or(".");
        let pattern = regex::RegexBuilder::new(&args.pattern)
            .case_insensitive(args.case_insensitive)
            .build()?;

        let mut matches = Vec::new();
        let mut files_searched = 0;
        let mut truncated = false;

        search_files(
            Path::new(search_path),
            &pattern,
            &args.extensions,
            args.max_matches,
            args.line_numbers,
            &mut matches,
            &mut files_searched,
            &mut truncated,
        )?;

        let total_matches = matches.len();
        let result = GrepResult {
            pattern: args.pattern,
            matches,
            total_files_searched: files_searched,
            total_matches,
            truncated,
        };

        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }
}

fn search_files(
    path: &Path,
    pattern: &regex::Regex,
    extensions: &Option<Vec<String>>,
    max_matches: usize,
    line_numbers: bool,
    matches: &mut Vec<GrepMatch>,
    files_searched: &mut usize,
    truncated: &mut bool,
) -> anyhow::Result<()> {
    if path.is_file() {
        search_file(path, pattern, max_matches, line_numbers, matches, truncated)?;
        *files_searched += 1;
        return Ok(());
    }

    for entry in walkdir(path)? {
        if *truncated { break; }
        if !entry.is_file() { continue; }

        // Extension filter
        if let Some(ref exts) = extensions {
            let ext = entry.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !exts.iter().any(|e| e == ext) {
                continue;
            }
        }

        // Skip hidden files and common non-source dirs
        let name = entry.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') { continue; }

        search_file(&entry, pattern, max_matches, line_numbers, matches, truncated)?;
        *files_searched += 1;
    }

    Ok(())
}

fn search_file(
    path: &Path,
    pattern: &regex::Regex,
    max_matches: usize,
    line_numbers: bool,
    matches: &mut Vec<GrepMatch>,
    truncated: &mut bool,
) -> anyhow::Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // Skip binary/unreadable files
    };

    let path_str = path.to_string_lossy().to_string();

    for (i, line) in content.lines().enumerate() {
        if matches.len() >= max_matches {
            *truncated = true;
            break;
        }

        if pattern.is_match(line) {
            let content = if line.len() > 500 {
                format!("{}...", &line[..500])
            } else {
                line.to_string()
            };
            matches.push(GrepMatch {
                file: path_str.clone(),
                line: if line_numbers { Some(i + 1) } else { None },
                content,
            });
        }
    }

    Ok(())
}

/// Simple directory walk (no external deps).
fn walkdir(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut stack = vec![path.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                // Skip hidden dirs and common build/target dirs
                if name.starts_with('.') || name == "target" || name == "node_modules"
                    || name == "__pycache__" || name == ".git"
                {
                    continue;
                }
                stack.push(path);
            } else {
                result.push(path);
            }
        }
    }

    Ok(result)
}

// ─── file_glob ────────────────────────────────────────────────────────────────

/// Find files matching a glob pattern.
#[derive(Clone)]
pub struct FileGlobTool {
    info: ToolInfo,
}

#[derive(Deserialize)]
struct GlobArgs {
    /// Glob pattern (e.g. "**/*.rs", "src/**/*.toml").
    pattern: String,
    /// Base directory for the search.
    #[serde(default)]
    path: Option<String>,
    /// Maximum number of results.
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize { 100 }

#[derive(Serialize, Deserialize)]
struct GlobResult {
    pattern: String,
    files: Vec<String>,
    total: usize,
    truncated: bool,
}

impl FileGlobTool {
    pub fn new() -> Self {
        Self {
            info: ToolInfo {
                name: "file_glob".into(),
                description: "Find files matching a glob pattern (e.g. \"**/*.rs\", \"src/**/*.toml\").".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Glob pattern (e.g. \"**/*.rs\")"},
                        "path": {"type": "string", "description": "Base directory (default: current directory)"},
                        "max_results": {"type": "integer", "description": "Maximum results (default: 100)"}
                    },
                    "required": ["pattern"]
                }),
            },
        }
    }
}

#[async_trait]
impl Tool for FileGlobTool {
    agentic_loop_tools::impl_clone_box!(FileGlobTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: GlobArgs = serde_json::from_slice(args)?;
        let base = args.path.as_deref().unwrap_or(".");
        let full_pattern = if args.pattern.starts_with('/') {
            args.pattern.clone()
        } else {
            format!("{}/{}", base, args.pattern)
        };

        let mut files = Vec::new();
        let mut truncated = false;

        for entry in glob::glob(&full_pattern)?.flatten() {
            if files.len() >= args.max_results {
                truncated = true;
                break;
            }
            if entry.is_file() {
                files.push(entry.to_string_lossy().to_string());
            }
        }

        let total = files.len();
        let result = GlobResult {
            pattern: args.pattern,
            files,
            total,
            truncated,
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
    async fn test_grep_basic() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "hello world\nfoo bar\nhello again").await.unwrap();
        tokio::fs::write(tmp.path().join("b.txt"), "no match here").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "hello",
            "path": tmp.path().to_str().unwrap()
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.total_matches, 2);
        assert_eq!(grep_result.total_files_searched, 2);
        assert!(!grep_result.truncated);
    }

    #[tokio::test]
    async fn test_grep_with_extension_filter() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.rs"), "fn main() {}").await.unwrap();
        tokio::fs::write(tmp.path().join("b.txt"), "fn main() in text").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "fn main",
            "path": tmp.path().to_str().unwrap(),
            "extensions": ["rs"]
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.total_matches, 1);
        assert_eq!(grep_result.total_files_searched, 1);
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "Hello World").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "hello",
            "path": tmp.path().to_str().unwrap(),
            "case_insensitive": true
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.total_matches, 1);
    }

    #[tokio::test]
    async fn test_grep_regex_pattern() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.rs"), "fn foo() {}\nfn bar() {}\nstruct Baz;").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "fn \\w+",
            "path": tmp.path().to_str().unwrap()
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.total_matches, 2);
    }

    #[tokio::test]
    async fn test_grep_max_matches() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "match\nmatch\nmatch\nmatch\nmatch").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "match",
            "path": tmp.path().to_str().unwrap(),
            "max_matches": 2
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.total_matches, 2);
        assert!(grep_result.truncated);
    }

    #[tokio::test]
    async fn test_grep_line_numbers() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "line1\nmatch here\nline3").await.unwrap();

        let tool = FileGrepTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "match",
            "path": tmp.path().to_str().unwrap(),
            "line_numbers": true
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let grep_result: GrepResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(grep_result.matches[0].line, Some(2));
    }

    #[tokio::test]
    async fn test_glob_basic() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.rs"), "").await.unwrap();
        tokio::fs::write(tmp.path().join("b.rs"), "").await.unwrap();
        tokio::fs::write(tmp.path().join("c.txt"), "").await.unwrap();

        let tool = FileGlobTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "**/*.rs",
            "path": tmp.path().to_str().unwrap()
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let glob_result: GlobResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(glob_result.total, 2);
        assert!(!glob_result.truncated);
    }

    #[tokio::test]
    async fn test_glob_nested() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("src/lib");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        tokio::fs::write(nested.join("mod.rs"), "").await.unwrap();
        tokio::fs::write(tmp.path().join("main.rs"), "").await.unwrap();

        let tool = FileGlobTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "**/*.rs",
            "path": tmp.path().to_str().unwrap()
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let glob_result: GlobResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(glob_result.total, 2);
    }

    #[tokio::test]
    async fn test_glob_no_match() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "").await.unwrap();

        let tool = FileGlobTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "**/*.py",
            "path": tmp.path().to_str().unwrap()
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let glob_result: GlobResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(glob_result.total, 0);
    }

    #[tokio::test]
    async fn test_glob_max_results() {
        let tmp = TempDir::new().unwrap();
        for i in 0..10 {
            tokio::fs::write(tmp.path().join(format!("f{}.rs", i)), "").await.unwrap();
        }

        let tool = FileGlobTool::new();
        let args = serde_json::to_vec(&serde_json::json!({
            "pattern": "**/*.rs",
            "path": tmp.path().to_str().unwrap(),
            "max_results": 3
        })).unwrap();
        let result = tool.execute(&args).await.unwrap();
        let glob_result: GlobResult = serde_json::from_slice(&result).unwrap();
        assert_eq!(glob_result.total, 3);
        assert!(glob_result.truncated);
    }
}
