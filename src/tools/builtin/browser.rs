//! Browser snapshot tool: take webpage snapshots using headless Chrome.
//!
//! Strategy:
//! 1. Find Chrome/Chromium on the system.
//! 2. Use `--headless --dump-dom` to get JS-rendered DOM.
//! 3. Parse the HTML output to extract text, same as `web_fetch`.
//! 4. If Chrome is not available, fall back to a reqwest-based fetch (no JS).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::process::Command;

use crate::safety::ssrf;
use crate::tools::tool::{
    optional_i64, optional_str, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain,
    ToolError, ToolOutput,
};

/// Maximum DOM output size (10 MB).
const MAX_DOM_SIZE: usize = 10 * 1024 * 1024;

/// Maximum extracted text length.
const MAX_TEXT_LENGTH: usize = 100_000;

/// Default wait time after navigation (ms).
const DEFAULT_WAIT_MS: i64 = 2000;

/// Browser snapshot tool.
pub struct BrowserTool;

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Take a snapshot of a web page using a headless browser. \
         Renders JavaScript before extracting text. \
         Falls back to plain HTTP fetch if Chrome is not available. \
         Actions: 'snapshot' (default) captures page text, \
         'screenshot' saves a PNG screenshot to disk."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to visit"
                },
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'snapshot' (default) or 'screenshot'",
                    "enum": ["snapshot", "screenshot"]
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector to extract specific content (snapshot only)"
                },
                "text": {
                    "type": "string",
                    "description": "Not used currently, reserved for future 'type' action"
                },
                "wait_ms": {
                    "type": "integer",
                    "description": "Wait time in ms after page load (default: 2000)"
                },
                "output_path": {
                    "type": "string",
                    "description": "File path for screenshot output (screenshot action only)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let url = require_str(&params, "url")?;
        let action = optional_str(&params, "action").unwrap_or("snapshot");
        let selector = optional_str(&params, "selector");
        let wait_ms = optional_i64(&params, "wait_ms", DEFAULT_WAIT_MS);

        // Validate URL
        let parsed_url = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid URL: {}", e)))?;

        // Only allow http/https
        match parsed_url.scheme() {
            "http" | "https" => {}
            other => {
                return Err(ToolError::InvalidParameters(format!(
                    "Unsupported scheme '{}'. Only http and https are allowed.",
                    other
                )));
            }
        }

        // SSRF protection
        ssrf::validate_url(url).map_err(|e| ToolError::NotAuthorized(e.to_string()))?;

        match action {
            "snapshot" => {
                execute_snapshot(url, selector, wait_ms, ctx, start).await
            }
            "screenshot" => {
                let output_path = optional_str(&params, "output_path");
                execute_screenshot(url, output_path, wait_ms, ctx, start).await
            }
            other => Err(ToolError::InvalidParameters(format!(
                "Unknown action '{}'. Use 'snapshot' or 'screenshot'.",
                other
            ))),
        }
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        true // External content
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(5) // 5 requests per minute
    }
}

/// Execute a snapshot: render the page and extract text content.
async fn execute_snapshot(
    url: &str,
    selector: Option<&str>,
    wait_ms: i64,
    _ctx: &ToolContext,
    start: Instant,
) -> Result<ToolOutput, ToolError> {
    let chrome_path = find_chrome();
    let (text, title, final_url, method) = if let Some(chrome) = chrome_path {
        let dom = run_chrome_dump_dom(&chrome, url, wait_ms).await?;
        let title = extract_title(&dom);
        let text = if let Some(sel) = selector {
            // For selector-based extraction, we include a note since --dump-dom
            // gives us the full DOM. Basic CSS selector support.
            let full_text = html_to_text(&dom, MAX_TEXT_LENGTH);
            format!(
                "[Note: CSS selector '{}' requested but full page text returned - \
                 Chrome --dump-dom does not support selectors natively]\n\n{}",
                sel, full_text
            )
        } else {
            html_to_text(&dom, MAX_TEXT_LENGTH)
        };
        (text, title, url.to_string(), "chrome")
    } else {
        // Fallback to reqwest
        let (html, final_url) = fetch_with_reqwest(url).await?;
        let title = extract_title(&html);
        let text = html_to_text(&html, MAX_TEXT_LENGTH);
        (
            text,
            title,
            final_url,
            "fallback_http",
        )
    };

    let truncated = text.len() >= MAX_TEXT_LENGTH;

    let result = serde_json::json!({
        "url": url,
        "final_url": final_url,
        "title": title,
        "text": text,
        "text_length": text.len(),
        "truncated": truncated,
        "method": method,
        "js_rendered": method == "chrome",
    });

    Ok(ToolOutput::success(result, start.elapsed()))
}

/// Execute a screenshot: save a PNG of the rendered page.
async fn execute_screenshot(
    url: &str,
    output_path: Option<&str>,
    wait_ms: i64,
    ctx: &ToolContext,
    start: Instant,
) -> Result<ToolOutput, ToolError> {
    let chrome_path = find_chrome().ok_or_else(|| {
        ToolError::ExecutionFailed(
            "Chrome/Chromium not found. Screenshot requires a headless browser.".to_string(),
        )
    })?;

    let screenshot_path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            ctx.working_dir
                .join(format!("screenshot_{}.png", chrono::Utc::now().format("%Y%m%d_%H%M%S")))
        });

    let mut cmd = Command::new(&chrome_path);
    cmd.args([
        "--headless",
        "--disable-gpu",
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--disable-extensions",
        "--disable-background-networking",
    ]);
    cmd.arg(format!("--screenshot={}", screenshot_path.display()));
    cmd.arg(format!("--window-size=1280,900"));

    if wait_ms > 0 {
        cmd.arg(format!("--virtual-time-budget={}", wait_ms));
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| ToolError::Timeout(30))?
        .map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to run Chrome: {}", e))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::ExecutionFailed(format!(
            "Chrome screenshot failed: {}",
            stderr.trim()
        )));
    }

    // Verify file was created
    let file_size = tokio::fs::metadata(&screenshot_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    if file_size == 0 {
        return Err(ToolError::ExecutionFailed(
            "Screenshot file was not created or is empty".to_string(),
        ));
    }

    let result = serde_json::json!({
        "url": url,
        "screenshot_path": screenshot_path.display().to_string(),
        "file_size": file_size,
        "method": "chrome",
    });

    Ok(ToolOutput::success(result, start.elapsed()))
}

/// Find Chrome/Chromium binary on the system.
fn find_chrome() -> Option<PathBuf> {
    // Try common names
    let candidates = [
        "google-chrome-stable",
        "google-chrome",
        "chromium-browser",
        "chromium",
        "chrome",
    ];

    for name in &candidates {
        if let Ok(path) = which::which(name) {
            return Some(path);
        }
    }

    // macOS application paths
    if cfg!(target_os = "macos") {
        let mac_paths = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        for path in &mac_paths {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    // Windows paths
    if cfg!(target_os = "windows") {
        let win_paths = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ];
        for path in &win_paths {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// Run Chrome with --dump-dom to get JS-rendered HTML.
async fn run_chrome_dump_dom(
    chrome_path: &PathBuf,
    url: &str,
    wait_ms: i64,
) -> Result<String, ToolError> {
    let mut cmd = Command::new(chrome_path);
    cmd.args([
        "--headless",
        "--disable-gpu",
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--disable-extensions",
        "--disable-background-networking",
        "--dump-dom",
    ]);

    if wait_ms > 0 {
        cmd.arg(format!("--virtual-time-budget={}", wait_ms));
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| ToolError::Timeout(30))?
        .map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to run Chrome: {}", e))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Chrome sometimes writes warnings to stderr but still succeeds
        if output.stdout.is_empty() {
            return Err(ToolError::ExecutionFailed(format!(
                "Chrome dump-dom failed: {}",
                stderr.trim()
            )));
        }
    }

    let dom = String::from_utf8_lossy(&output.stdout);
    if dom.len() > MAX_DOM_SIZE {
        Ok(dom[..MAX_DOM_SIZE].to_string())
    } else {
        Ok(dom.to_string())
    }
}

/// Fallback: fetch HTML with reqwest (no JS rendering).
async fn fetch_with_reqwest(url: &str) -> Result<(String, String), ToolError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; PeerClaw/0.3)")
        .build()
        .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e)))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::ExternalService(format!("Fetch failed: {}", e)))?;

    let final_url = response.url().to_string();

    if !response.status().is_success() {
        return Err(ToolError::ExternalService(format!(
            "HTTP {}: {}",
            response.status().as_u16(),
            response
                .status()
                .canonical_reason()
                .unwrap_or("Error")
        )));
    }

    let html = response
        .text()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read response: {}", e)))?;

    Ok((html, final_url))
}

/// Convert HTML to plain text (shared logic with web_fetch, simplified version).
fn html_to_text(html: &str, max_length: usize) -> String {
    let mut text = html.to_string();

    // Remove script and style content
    let script_re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    text = script_re.replace_all(&text, "").to_string();

    let style_re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    text = style_re.replace_all(&text, "").to_string();

    // Remove HTML comments
    let comment_re = regex::Regex::new(r"(?s)<!--.*?-->").unwrap();
    text = comment_re.replace_all(&text, "").to_string();

    // Remove head section
    let head_re = regex::Regex::new(r"(?is)<head[^>]*>.*?</head>").unwrap();
    text = head_re.replace_all(&text, "").to_string();

    // Convert common tags to text
    text = text.replace("<br>", "\n");
    text = text.replace("<br/>", "\n");
    text = text.replace("<br />", "\n");
    text = text.replace("</p>", "\n\n");
    text = text.replace("</div>", "\n");
    text = text.replace("</li>", "\n");
    text = text.replace("</h1>", "\n\n");
    text = text.replace("</h2>", "\n\n");
    text = text.replace("</h3>", "\n\n");
    text = text.replace("</h4>", "\n\n");
    text = text.replace("</h5>", "\n");
    text = text.replace("</h6>", "\n");
    text = text.replace("</tr>", "\n");
    text = text.replace("</td>", "\t");
    text = text.replace("</th>", "\t");

    // Remove all remaining HTML tags
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
    text = tag_re.replace_all(&text, "").to_string();

    // Decode HTML entities
    text = text.replace("&nbsp;", " ");
    text = text.replace("&amp;", "&");
    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&quot;", "\"");
    text = text.replace("&#39;", "'");
    text = text.replace("&apos;", "'");

    // Clean up whitespace
    let ws_re = regex::Regex::new(r"[ \t]+").unwrap();
    text = ws_re.replace_all(&text, " ").to_string();

    let nl_re = regex::Regex::new(r"\n{3,}").unwrap();
    text = nl_re.replace_all(&text, "\n\n").to_string();

    text = text.trim().to_string();

    // Truncate if needed
    if text.len() > max_length {
        let mut end = max_length;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        if let Some(pos) = text.rfind(' ') {
            text.truncate(pos);
        }
        text.push_str("...");
    }

    text
}

/// Extract title from HTML.
fn extract_title(html: &str) -> Option<String> {
    let title_re = regex::Regex::new(r"(?i)<title[^>]*>([^<]+)</title>").ok()?;
    title_re
        .captures(html)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_chrome_returns_option() {
        // Just verify it doesn't panic; may or may not find Chrome
        let _ = find_chrome();
    }

    #[test]
    fn test_html_to_text_basic() {
        let html = "<html><body><p>Hello <b>World</b>!</p></body></html>";
        let text = html_to_text(html, 1000);
        assert!(text.contains("Hello"), "text: {}", text);
        assert!(text.contains("World"), "text: {}", text);
    }

    #[test]
    fn test_html_to_text_strips_scripts() {
        let html =
            "<html><body><script>alert('xss')</script><p>Safe text</p></body></html>";
        let text = html_to_text(html, 1000);
        assert!(!text.contains("alert"));
        assert!(text.contains("Safe text"));
    }

    #[test]
    fn test_html_to_text_strips_styles() {
        let html = "<html><body><style>body{color:red}</style><p>Visible</p></body></html>";
        let text = html_to_text(html, 1000);
        assert!(!text.contains("color:red"));
        assert!(text.contains("Visible"));
    }

    #[test]
    fn test_html_to_text_truncation() {
        let html = "<p>A long paragraph with some text content.</p>";
        let text = html_to_text(html, 20);
        assert!(text.len() <= 25); // Allow for "..." suffix
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(
            extract_title("<html><head><title>My Page</title></head></html>"),
            Some("My Page".to_string())
        );
        assert_eq!(extract_title("<html><body>No title</body></html>"), None);
    }

    #[test]
    fn test_url_validation_blocks_ssrf() {
        // Verify SSRF validation would catch internal URLs
        assert!(ssrf::validate_url("http://127.0.0.1:8080").is_err());
        assert!(ssrf::validate_url("http://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn test_url_validation_allows_public() {
        assert!(ssrf::validate_url("https://8.8.8.8").is_ok());
    }
}
