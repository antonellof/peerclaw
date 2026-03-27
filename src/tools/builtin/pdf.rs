//! PDF reading tool: extract text from PDF files.
//!
//! Strategy:
//! 1. If `pdftotext` (poppler-utils) is available on the system, use it for
//!    high-fidelity extraction.
//! 2. Otherwise, fall back to a built-in parser that decodes FlateDecode
//!    streams and extracts text operators (Tj, TJ, ') from content streams.

use std::io::Read as _;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use async_trait::async_trait;
use tokio::process::Command;

use crate::tools::tool::{
    optional_str, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError,
    ToolOutput,
};

/// Maximum PDF file size (50 MB).
const MAX_PDF_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum output text length.
const MAX_TEXT_LENGTH: usize = 200_000;

/// PDF read tool.
pub struct PdfReadTool;

#[async_trait]
impl Tool for PdfReadTool {
    fn name(&self) -> &str {
        "pdf_read"
    }

    fn description(&self) -> &str {
        "Extract text content from a PDF file. Uses pdftotext if available, \
         otherwise falls back to built-in extraction. Returns page-separated text."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the PDF file"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range to extract, e.g. '1-5' or '3' (default: all pages)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let path_str = require_str(&params, "path")?;
        let pages = optional_str(&params, "pages");

        // Resolve path relative to working directory
        let path = if Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            ctx.working_dir.join(path_str)
        };

        // Validate file exists and size
        let metadata = tokio::fs::metadata(&path).await.map_err(|e| {
            ToolError::InvalidParameters(format!("Cannot access file '{}': {}", path.display(), e))
        })?;

        if metadata.len() > MAX_PDF_SIZE {
            return Err(ToolError::InvalidParameters(format!(
                "PDF file too large: {} bytes (max {} bytes)",
                metadata.len(),
                MAX_PDF_SIZE
            )));
        }

        if !path
            .extension()
            .map_or(false, |ext| ext.eq_ignore_ascii_case("pdf"))
        {
            return Err(ToolError::InvalidParameters(
                "File does not have a .pdf extension".to_string(),
            ));
        }

        // Parse page range
        let (first_page, last_page) = parse_page_range(pages)?;

        // Try pdftotext first, fall back to built-in extraction
        let text = if which::which("pdftotext").is_ok() {
            extract_with_pdftotext(&path, first_page, last_page).await?
        } else {
            let bytes = tokio::fs::read(&path).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to read PDF file: {}", e))
            })?;
            extract_builtin(&bytes, first_page, last_page)?
        };

        // Truncate if too long
        let truncated = text.len() > MAX_TEXT_LENGTH;
        let text = if truncated {
            let mut t = text;
            t.truncate(MAX_TEXT_LENGTH);
            // Find last complete line
            if let Some(pos) = t.rfind('\n') {
                t.truncate(pos);
            }
            t.push_str("\n...[truncated]");
            t
        } else {
            text
        };

        let page_count = text.matches("[Page ").count();

        let result = serde_json::json!({
            "path": path.display().to_string(),
            "text": text,
            "text_length": text.len(),
            "truncated": truncated,
            "pages_extracted": page_count,
            "method": if which::which("pdftotext").is_ok() { "pdftotext" } else { "builtin" },
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false // Local file content
    }

    fn rate_limit(&self) -> Option<u32> {
        None
    }
}

/// Parse a page range string like "1-5" or "3" into (first, last) 1-based page numbers.
fn parse_page_range(pages: Option<&str>) -> Result<(Option<u32>, Option<u32>), ToolError> {
    let Some(pages) = pages else {
        return Ok((None, None));
    };

    let pages = pages.trim();
    if pages.is_empty() {
        return Ok((None, None));
    }

    if let Some((start, end)) = pages.split_once('-') {
        let first: u32 = start.trim().parse().map_err(|_| {
            ToolError::InvalidParameters(format!("Invalid page range start: '{}'", start.trim()))
        })?;
        let last: u32 = end.trim().parse().map_err(|_| {
            ToolError::InvalidParameters(format!("Invalid page range end: '{}'", end.trim()))
        })?;
        if first == 0 || last == 0 {
            return Err(ToolError::InvalidParameters(
                "Page numbers are 1-based".to_string(),
            ));
        }
        if first > last {
            return Err(ToolError::InvalidParameters(format!(
                "Invalid page range: {} > {}",
                first, last
            )));
        }
        Ok((Some(first), Some(last)))
    } else {
        let page: u32 = pages.parse().map_err(|_| {
            ToolError::InvalidParameters(format!("Invalid page number: '{}'", pages))
        })?;
        if page == 0 {
            return Err(ToolError::InvalidParameters(
                "Page numbers are 1-based".to_string(),
            ));
        }
        Ok((Some(page), Some(page)))
    }
}

/// Extract text using the `pdftotext` command (from poppler-utils).
async fn extract_with_pdftotext(
    path: &Path,
    first_page: Option<u32>,
    last_page: Option<u32>,
) -> Result<String, ToolError> {
    let mut cmd = Command::new("pdftotext");

    if let Some(first) = first_page {
        cmd.arg("-f").arg(first.to_string());
    }
    if let Some(last) = last_page {
        cmd.arg("-l").arg(last.to_string());
    }

    // -layout preserves the original physical layout
    cmd.arg("-layout");
    // Output to stdout
    cmd.arg(path.as_os_str()).arg("-");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output())
        .await
        .map_err(|_| ToolError::Timeout(60))?
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to run pdftotext: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::ExecutionFailed(format!(
            "pdftotext failed: {}",
            stderr.trim()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();

    // Add page markers by splitting on form-feed characters (pdftotext uses \f)
    let pages: Vec<&str> = text.split('\x0C').collect();
    let mut result = String::new();
    let start_page = first_page.unwrap_or(1) as usize;
    for (i, page) in pages.iter().enumerate() {
        let page_text = page.trim();
        if page_text.is_empty() && i == pages.len() - 1 {
            // Skip trailing empty page (common with pdftotext)
            continue;
        }
        result.push_str(&format!("[Page {}]\n", start_page + i));
        result.push_str(page_text);
        result.push_str("\n\n");
    }

    Ok(result)
}

/// Built-in PDF text extraction (no external dependencies).
///
/// Walks through the raw PDF bytes looking for content streams, decompresses
/// FlateDecode streams, and extracts text from PDF text operators.
fn extract_builtin(
    data: &[u8],
    first_page: Option<u32>,
    last_page: Option<u32>,
) -> Result<String, ToolError> {
    // Verify PDF magic
    if data.len() < 5 || &data[..5] != b"%PDF-" {
        return Err(ToolError::InvalidParameters(
            "Not a valid PDF file (missing %PDF- header)".to_string(),
        ));
    }

    let mut all_text = Vec::new();

    // Find all stream/endstream pairs and try to extract text
    let mut pos = 0;
    while pos < data.len() {
        if let Some(stream_start) = find_bytes(data, b"stream", pos) {
            // Check for the dictionary before the stream to see if it's FlateDecode
            let dict_region_start = if stream_start > 512 {
                stream_start - 512
            } else {
                0
            };
            let dict_region = &data[dict_region_start..stream_start];
            let is_flate = find_bytes(dict_region, b"FlateDecode", 0).is_some();

            // Find the actual stream data start (after "stream\r\n" or "stream\n")
            let mut data_start = stream_start + 6; // skip "stream"
            if data_start < data.len() && data[data_start] == b'\r' {
                data_start += 1;
            }
            if data_start < data.len() && data[data_start] == b'\n' {
                data_start += 1;
            }

            // Find endstream
            if let Some(data_end) = find_bytes(data, b"endstream", data_start) {
                let stream_data = &data[data_start..data_end];

                // Try to decode and extract text
                let decoded = if is_flate {
                    decode_flate(stream_data)
                } else {
                    Some(stream_data.to_vec())
                };

                if let Some(decoded) = decoded {
                    let text = extract_text_operators(&decoded);
                    if !text.is_empty() {
                        all_text.push(text);
                    }
                }

                pos = data_end + 9; // skip "endstream"
            } else {
                pos = stream_start + 6;
            }
        } else {
            break;
        }
    }

    // Apply page filtering
    let first = first_page.unwrap_or(1) as usize;
    let last = last_page.map(|l| l as usize).unwrap_or(all_text.len());

    let mut result = String::new();
    for (i, text) in all_text.iter().enumerate() {
        let page_num = i + 1;
        if page_num < first || page_num > last {
            continue;
        }
        result.push_str(&format!("[Page {}]\n", page_num));
        result.push_str(text);
        result.push_str("\n\n");
    }

    if result.is_empty() && !all_text.is_empty() {
        // If page filtering removed everything, return a message
        return Err(ToolError::InvalidParameters(format!(
            "No text found in pages {}-{}. PDF has {} content streams.",
            first,
            last,
            all_text.len()
        )));
    }

    if result.is_empty() {
        // Try extracting any readable ASCII as last resort
        let ascii_text = extract_ascii_text(data);
        if !ascii_text.is_empty() {
            result.push_str("[Page 1]\n");
            result.push_str(&ascii_text);
            result.push('\n');
        } else {
            result.push_str("[No extractable text found - PDF may contain only images or use unsupported encoding]");
        }
    }

    Ok(result)
}

/// Find a byte pattern in data starting from offset.
fn find_bytes(data: &[u8], pattern: &[u8], start: usize) -> Option<usize> {
    if pattern.is_empty() || start + pattern.len() > data.len() {
        return None;
    }
    data[start..]
        .windows(pattern.len())
        .position(|w| w == pattern)
        .map(|p| p + start)
}

/// Decompress a FlateDecode (zlib/deflate) stream.
fn decode_flate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    let mut decoder = ZlibDecoder::new(data);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Extract text from PDF content stream operators.
///
/// Handles the following PDF text operators:
/// - `(text) Tj`  - show text string
/// - `[(text) ...] TJ` - show text with positioning
/// - `(text) '`   - move to next line and show text
fn extract_text_operators(content: &[u8]) -> String {
    let mut result = String::new();
    let text = String::from_utf8_lossy(content);

    // Extract text from Tj operators: (text) Tj
    // and TJ operators: [(text) num (text) ...] TJ
    // and ' operator: (text) '
    let mut chars = text.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch == '(' {
            // Start of a PDF string literal
            let string_content = extract_pdf_string(&text[i..]);
            if !string_content.is_empty() {
                result.push_str(&string_content);
            }
        } else if ch == 'B' && text[i..].starts_with("BT") {
            // Begin Text block - add a space
            if !result.is_empty() && !result.ends_with('\n') && !result.ends_with(' ') {
                result.push(' ');
            }
        } else if ch == 'E' && text[i..].starts_with("ET") {
            // End Text block - add a newline
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
        } else if ch == 'T' {
            // Check for Td, TD (text positioning that implies line break)
            if i + 1 < text.len() {
                let next = text.as_bytes().get(i + 1).copied().unwrap_or(0);
                if next == b'd' || next == b'D' || next == b'*' {
                    if !result.is_empty() && !result.ends_with('\n') && !result.ends_with(' ') {
                        result.push(' ');
                    }
                }
            }
        }
    }

    // Clean up the result
    clean_extracted_text(&result)
}

/// Extract a PDF string literal starting at '(' accounting for nested parens and escapes.
fn extract_pdf_string(s: &str) -> String {
    let mut result = String::new();
    let mut depth = 0;
    let mut escaped = false;
    let mut started = false;

    for ch in s.chars() {
        if !started {
            if ch == '(' {
                started = true;
                depth = 1;
            }
            continue;
        }

        if escaped {
            match ch {
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                '(' => result.push('('),
                ')' => result.push(')'),
                '\\' => result.push('\\'),
                _ => {} // Ignore other escape sequences
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '(' => {
                depth += 1;
                result.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }

    result
}

/// Clean up extracted text: normalize whitespace, remove control characters.
fn clean_extracted_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = false;

    for ch in text.chars() {
        if ch == '\n' {
            if !last_was_space || !result.ends_with('\n') {
                result.push('\n');
            }
            last_was_space = true;
        } else if ch.is_whitespace() || ch.is_control() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else if ch.is_ascii_graphic() || ch.is_alphanumeric() || ch == ' ' {
            result.push(ch);
            last_was_space = false;
        }
    }

    result.trim().to_string()
}

/// Last-resort extraction: pull readable ASCII text from the raw PDF bytes.
fn extract_ascii_text(data: &[u8]) -> String {
    let mut result = String::new();
    let mut current_word = String::new();

    for &byte in data {
        if byte.is_ascii_graphic() || byte == b' ' {
            current_word.push(byte as char);
        } else {
            if current_word.len() > 3 {
                // Filter out PDF syntax keywords
                let lower = current_word.to_lowercase();
                if !lower.contains("obj")
                    && !lower.contains("endobj")
                    && !lower.contains("stream")
                    && !lower.contains("xref")
                    && !lower.contains("/type")
                    && !lower.contains("/font")
                    && !lower.contains("/page")
                    && !lower.contains("trailer")
                    && !lower.starts_with("<<")
                    && !lower.starts_with(">>")
                {
                    if !result.is_empty() {
                        result.push(' ');
                    }
                    result.push_str(current_word.trim());
                }
            }
            current_word.clear();
        }
    }

    // Truncate to reasonable size
    if result.len() > 50000 {
        result.truncate(50000);
        if let Some(pos) = result.rfind(' ') {
            result.truncate(pos);
        }
        result.push_str("...");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_page_range_none() {
        let (first, last) = parse_page_range(None).unwrap();
        assert!(first.is_none());
        assert!(last.is_none());
    }

    #[test]
    fn test_parse_page_range_single() {
        let (first, last) = parse_page_range(Some("3")).unwrap();
        assert_eq!(first, Some(3));
        assert_eq!(last, Some(3));
    }

    #[test]
    fn test_parse_page_range_range() {
        let (first, last) = parse_page_range(Some("1-5")).unwrap();
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(5));
    }

    #[test]
    fn test_parse_page_range_invalid() {
        assert!(parse_page_range(Some("0")).is_err());
        assert!(parse_page_range(Some("5-3")).is_err());
        assert!(parse_page_range(Some("abc")).is_err());
    }

    #[test]
    fn test_extract_pdf_string() {
        assert_eq!(extract_pdf_string("(Hello World)"), "Hello World");
        assert_eq!(extract_pdf_string("(nested (parens))"), "nested (parens)");
        assert_eq!(
            extract_pdf_string("(escaped \\n newline)"),
            "escaped \n newline"
        );
        assert_eq!(extract_pdf_string("(escaped \\( paren)"), "escaped ( paren");
    }

    #[test]
    fn test_find_bytes() {
        let data = b"hello world stream data endstream";
        assert_eq!(find_bytes(data, b"stream", 0), Some(12));
        assert_eq!(find_bytes(data, b"endstream", 0), Some(24));
        assert_eq!(find_bytes(data, b"missing", 0), None);
    }

    #[test]
    fn test_extract_builtin_invalid_pdf() {
        let data = b"not a pdf file";
        assert!(extract_builtin(data, None, None).is_err());
    }

    #[test]
    fn test_extract_builtin_minimal_pdf() {
        // A minimal valid PDF with text content
        let pdf = b"%PDF-1.0\n\
            1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
            2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
            3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n\
            4 0 obj\n<< /Length 44 >>\nstream\nBT /F1 12 Tf 100 700 Td (Hello PDF) Tj ET\nendstream\nendobj\n\
            xref\n0 5\ntrailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n0\n%%EOF";

        let result = extract_builtin(pdf, None, None).unwrap();
        assert!(
            result.contains("Hello PDF"),
            "Expected 'Hello PDF' in result: {}",
            result
        );
    }

    #[test]
    fn test_clean_extracted_text() {
        assert_eq!(clean_extracted_text("  hello   world  "), "hello world");
        assert_eq!(clean_extracted_text("line1\n\nline2"), "line1\nline2");
    }
}
