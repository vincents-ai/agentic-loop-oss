//! Web fetch tool — HTTP GET with HTML-to-text conversion.
//!
//! Fetches URLs and returns cleaned text content. Uses reqwest for HTTP
//! and a simple HTML tag stripper (no heavy markdown conversion dependency).

use agentic_loop_tools::Tool;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Fetch a URL and return text content.
#[derive(Clone)]
pub struct WebFetchTool {
    info: ToolInfo,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct FetchArgs {
    /// URL to fetch.
    url: String,
    /// Maximum response size in bytes (default: 1MB).
    #[serde(default = "default_max_size")]
    max_size: usize,
    /// Request timeout in seconds (default: 30).
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    /// Whether to return raw HTML instead of text.
    #[serde(default)]
    raw: bool,
}

fn default_max_size() -> usize { 1_048_576 }
fn default_timeout() -> u64 { 30 }

#[derive(Serialize)]
struct FetchResult {
    url: String,
    status: u16,
    content_type: Option<String>,
    content: String,
    size_bytes: usize,
    truncated: bool,
}

impl WebFetchTool {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("agentic-loop/0.1 (web-fetch-tool)")
            .build()
            .unwrap_or_default();

        Self {
            info: ToolInfo {
                name: "web_fetch".into(),
                description: "Fetch a URL and return text content. Strips HTML tags automatically.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to fetch"},
                        "max_size": {"type": "integer", "description": "Max response size in bytes (default: 1MB)"},
                        "timeout_secs": {"type": "integer", "description": "Timeout in seconds (default: 30)"},
                        "raw": {"type": "boolean", "description": "Return raw HTML instead of text"}
                    },
                    "required": ["url"]
                }),
            },
            client,
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    agentic_loop_tools::impl_clone_box!(WebFetchTool);
    async fn execute(&self, args: &[u8]) -> anyhow::Result<Vec<u8>> {
        let args: FetchArgs = serde_json::from_slice(args)?;

        let response = self.client
            .get(&args.url)
            .timeout(std::time::Duration::from_secs(args.timeout_secs))
            .send()
            .await?;

        let status = response.status().as_u16();
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let mut body = response.text().await?;
        let original_size = body.len();

        let truncated = if body.len() > args.max_size {
            body.truncate(args.max_size);
            true
        } else {
            false
        };

        let is_html = content_type.as_deref().map(|ct| ct.contains("html")).unwrap_or(false)
            || body.trim_start().starts_with("<!DOCTYPE") || body.trim_start().starts_with("<html");

        let content = if args.raw {
            body
        } else if is_html {
            strip_html(&body)
        } else {
            body
        };

        Ok(serde_json::to_vec(&FetchResult {
            url: args.url,
            status,
            content_type,
            content,
            size_bytes: original_size,
            truncated,
        })?)
    }

    fn info(&self) -> ToolInfo { self.info.clone() }
}

/// Simple HTML tag stripper — removes tags and decodes common entities.
/// No dependency on html2md or scraper; good enough for agent consumption.
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '<' {
            // Check for script/style blocks
            let rest: String = chars[i..].iter().take(7).collect();
            if rest.starts_with("<script") {
                in_script = true;
            } else if rest.starts_with("<style") {
                in_style = true;
            }
            let rest_lower = rest.to_lowercase();
            if rest_lower.starts_with("<script") { in_script = true; }
            if rest_lower.starts_with("<style") { in_style = true; }

            // Check for closing script/style
            let rest8: String = chars[i..].iter().take(9).collect();
            if rest8.to_lowercase().starts_with("</script") { in_script = false; }
            if rest8.to_lowercase().starts_with("</style") { in_style = false; }

            in_tag = true;
            i += 1;
            continue;
        }

        if chars[i] == '>' {
            in_tag = false;
            i += 1;
            continue;
        }

        if in_tag || in_script || in_style {
            i += 1;
            continue;
        }

        result.push(chars[i]);
        i += 1;
    }

    // Decode common entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Collapse whitespace
    let mut cleaned = String::new();
    let mut last_was_space = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_was_space {
                cleaned.push('\n');
                last_was_space = true;
            }
        } else {
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            last_was_space = false;
        }
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><h1>Hello</h1><p>World <b>bold</b></p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(text.contains("bold"));
        assert!(!text.contains("<"));
    }

    #[test]
    fn test_strip_html_script() {
        let html = "<html><head><script>alert('xss')</script></head><body>Content</body></html>";
        let text = strip_html(html);
        assert!(text.contains("Content"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_strip_html_entities() {
        let html = "<p>A &amp; B &lt; C &gt; D</p>";
        let text = strip_html(html);
        assert_eq!(text, "A & B < C > D");
    }

    #[test]
    fn test_strip_html_whitespace() {
        let html = "<p>Line 1</p>\n\n\n<p>Line 2</p>";
        let text = strip_html(html);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
        assert!(!text.contains("\n\n\n"));
    }

    #[test]
    fn test_fetch_args_deserialize() {
        let args: FetchArgs = serde_json::from_str(r#"{"url": "https://example.com"}"#).unwrap();
        assert_eq!(args.url, "https://example.com");
        assert_eq!(args.max_size, 1_048_576);
        assert_eq!(args.timeout_secs, 30);
        assert!(!args.raw);
    }

    #[test]
    fn test_fetch_args_custom() {
        let args: FetchArgs = serde_json::from_str(
            r#"{"url": "https://example.com", "max_size": 500, "timeout_secs": 5, "raw": true}"#
        ).unwrap();
        assert_eq!(args.max_size, 500);
        assert_eq!(args.timeout_secs, 5);
        assert!(args.raw);
    }

    #[tokio::test]
    async fn test_web_fetch_tool_info() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.info().name, "web_fetch");
    }
}
