//! Web search tool: search the web via DuckDuckGo HTML.

use std::time::Instant;

use async_trait::async_trait;
use reqwest::Client;

use crate::safety::ssrf;
use crate::tools::tool::{
    optional_i64, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError,
    ToolOutput,
};

/// Default timeout for search requests.
const SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// DuckDuckGo HTML search endpoint.
const DUCKDUCKGO_HTML_URL: &str = "https://html.duckduckgo.com/html/";

/// Web search tool using DuckDuckGo HTML search (no API key needed).
pub struct WebSearchTool {
    client: Client,
}

impl WebSearchTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(SEARCH_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; PeerClaw/0.2)")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using DuckDuckGo. Returns titles, URLs, and snippets \
         for matching results. No API key required."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 5, max: 20)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let query = require_str(&params, "query")?;
        let max_results = optional_i64(&params, "max_results", 5).clamp(1, 20) as usize;

        // SSRF protection on the search URL
        ssrf::validate_url(DUCKDUCKGO_HTML_URL)
            .map_err(|e| ToolError::NotAuthorized(e.to_string()))?;

        // Build the search URL with properly encoded query
        let search_url = url::Url::parse_with_params(DUCKDUCKGO_HTML_URL, &[("q", query)])
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid query: {}", e)))?
            .to_string();

        let response = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| ToolError::ExternalService(format!("Search request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExternalService(format!(
                "DuckDuckGo returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read response: {}", e)))?;

        let results = parse_duckduckgo_results(&html, max_results);

        let result = serde_json::json!({
            "query": query,
            "result_count": results.len(),
            "results": results,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        true // External content
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(10) // 10 searches per minute
    }
}

/// Parse DuckDuckGo HTML search results into structured data.
///
/// DuckDuckGo HTML results are structured as:
/// ```html
/// <div class="result results_links results_links_deep web-result">
///   <a class="result__a" href="...">Title</a>
///   <a class="result__snippet">Snippet text...</a>
/// </div>
/// ```
pub fn parse_duckduckgo_results(html: &str, max_results: usize) -> Vec<serde_json::Value> {
    let mut results = Vec::new();

    // Split on result divs — each result block contains class="result"
    // We look for <a class="result__a" ...> for title/URL
    // and <a class="result__snippet"> for snippet text
    let result_title_re =
        regex::Regex::new(r#"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)
            .unwrap();
    let snippet_re =
        regex::Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#).unwrap();

    // Find all result blocks by splitting on result__a anchors
    let title_matches: Vec<_> = result_title_re.captures_iter(html).collect();
    let snippet_matches: Vec<_> = snippet_re.captures_iter(html).collect();

    for (i, title_cap) in title_matches.iter().enumerate() {
        if results.len() >= max_results {
            break;
        }

        let raw_url = &title_cap[1];
        let raw_title = &title_cap[2];

        // Extract the actual URL from DuckDuckGo's redirect URL
        let url = extract_ddg_url(raw_url);

        // Strip HTML tags from title
        let title = strip_html_tags(raw_title);

        // Get corresponding snippet if available
        let snippet = snippet_matches
            .get(i)
            .map(|s| strip_html_tags(&s[1]))
            .unwrap_or_default();

        // Skip ad results (they typically have uddg= but no real domain)
        if title.is_empty() || url.is_empty() {
            continue;
        }

        results.push(serde_json::json!({
            "title": title,
            "url": url,
            "snippet": snippet,
        }));
    }

    results
}

/// Extract the actual destination URL from DuckDuckGo's redirect URL.
///
/// DuckDuckGo wraps URLs like:
/// `//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=...`
fn extract_ddg_url(raw: &str) -> String {
    // DuckDuckGo wraps result URLs as redirect links with a `uddg` query parameter.
    // Parse the wrapper URL to extract the real destination.
    if raw.contains("uddg=") {
        // Build a full URL so the url crate can parse query params
        let full = if raw.starts_with("//") {
            format!("https:{}", raw)
        } else {
            raw.to_string()
        };
        if let Ok(parsed) = url::Url::parse(&full) {
            for (key, value) in parsed.query_pairs() {
                if key == "uddg" {
                    return value.into_owned();
                }
            }
        }
        raw.to_string()
    } else if raw.starts_with("http") {
        raw.to_string()
    } else {
        raw.to_string()
    }
}

/// Strip HTML tags from a string and decode common entities.
fn strip_html_tags(s: &str) -> String {
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = tag_re.replace_all(s, "").to_string();
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample DuckDuckGo HTML response for testing the parser.
    const SAMPLE_DDG_HTML: &str = r#"
<html>
<body>
<div class="results">
  <div class="result results_links results_links_deep web-result">
    <h2 class="result__title">
      <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Frust-lang.org%2F&amp;rut=abc123">Rust Programming Language</a>
    </h2>
    <a class="result__snippet">A language empowering everyone to build reliable and efficient software.</a>
  </div>
  <div class="result results_links results_links_deep web-result">
    <h2 class="result__title">
      <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fbook%2F&amp;rut=def456">The Rust Programming Language - Book</a>
    </h2>
    <a class="result__snippet">An introductory book about Rust, commonly known as &quot;The Book&quot;.</a>
  </div>
  <div class="result results_links results_links_deep web-result">
    <h2 class="result__title">
      <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Frust-lang%2Frust&amp;rut=ghi789">rust-lang/rust: The Rust compiler</a>
    </h2>
    <a class="result__snippet">Empowering everyone to build <b>reliable</b> and efficient software. GitHub repo.</a>
  </div>
</div>
</body>
</html>
"#;

    #[test]
    fn test_parse_duckduckgo_results() {
        let results = parse_duckduckgo_results(SAMPLE_DDG_HTML, 5);
        assert_eq!(results.len(), 3);

        assert_eq!(results[0]["title"], "Rust Programming Language");
        assert_eq!(results[0]["url"], "https://rust-lang.org/");
        assert!(results[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("reliable and efficient"));

        assert_eq!(results[1]["title"], "The Rust Programming Language - Book");
        assert_eq!(results[1]["url"], "https://doc.rust-lang.org/book/");
        // HTML entity should be decoded
        assert!(results[1]["snippet"]
            .as_str()
            .unwrap()
            .contains("\"The Book\""));

        assert_eq!(results[2]["title"], "rust-lang/rust: The Rust compiler");
        assert_eq!(results[2]["url"], "https://github.com/rust-lang/rust");
        // Bold tags should be stripped
        assert!(!results[2]["snippet"].as_str().unwrap().contains("<b>"));
    }

    #[test]
    fn test_parse_respects_max_results() {
        let results = parse_duckduckgo_results(SAMPLE_DDG_HTML, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_parse_empty_html() {
        let results = parse_duckduckgo_results("<html><body></body></html>", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_ddg_url() {
        assert_eq!(
            extract_ddg_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc"),
            "https://example.com"
        );
        assert_eq!(
            extract_ddg_url("https://example.com/direct"),
            "https://example.com/direct"
        );
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(
            strip_html_tags("Hello <b>World</b> &amp; friends"),
            "Hello World & friends"
        );
    }
}
