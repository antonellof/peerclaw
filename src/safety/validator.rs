//! Input validation for safe processing.
//!
//! Validates:
//! - Message length and encoding
//! - File paths (no traversal)
//! - URLs (allowlist checking)
//! - JSON payloads

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed
    pub valid: bool,
    /// Validation errors
    pub errors: Vec<ValidationError>,
    /// Warnings (non-fatal)
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// Create a passing result
    pub fn ok() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create a failing result
    pub fn error(error: ValidationError) -> Self {
        Self {
            valid: false,
            errors: vec![error],
            warnings: Vec::new(),
        }
    }

    /// Add an error
    pub fn add_error(&mut self, error: ValidationError) {
        self.valid = false;
        self.errors.push(error);
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }
}

/// Validation error types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationError {
    /// Content too long
    TooLong { max: usize, actual: usize },
    /// Content too short
    TooShort { min: usize, actual: usize },
    /// Invalid encoding
    InvalidEncoding { reason: String },
    /// Path traversal attempt
    PathTraversal { path: String },
    /// Disallowed URL
    DisallowedUrl { url: String, reason: String },
    /// Invalid JSON
    InvalidJson { reason: String },
    /// Required field missing
    MissingField { field: String },
    /// Invalid format
    InvalidFormat { field: String, expected: String },
    /// Null bytes in input
    NullBytes,
    /// Control characters
    ControlCharacters,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong { max, actual } => {
                write!(f, "Content too long: {} bytes (max {})", actual, max)
            }
            Self::TooShort { min, actual } => {
                write!(f, "Content too short: {} bytes (min {})", actual, min)
            }
            Self::InvalidEncoding { reason } => {
                write!(f, "Invalid encoding: {}", reason)
            }
            Self::PathTraversal { path } => {
                write!(f, "Path traversal attempt: {}", path)
            }
            Self::DisallowedUrl { url, reason } => {
                write!(f, "Disallowed URL {}: {}", url, reason)
            }
            Self::InvalidJson { reason } => {
                write!(f, "Invalid JSON: {}", reason)
            }
            Self::MissingField { field } => {
                write!(f, "Missing required field: {}", field)
            }
            Self::InvalidFormat { field, expected } => {
                write!(f, "Invalid format for {}: expected {}", field, expected)
            }
            Self::NullBytes => {
                write!(f, "Null bytes in input")
            }
            Self::ControlCharacters => {
                write!(f, "Control characters in input")
            }
        }
    }
}

/// Input validator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Maximum message length
    pub max_message_length: usize,
    /// Minimum message length
    pub min_message_length: usize,
    /// Allow null bytes
    pub allow_null_bytes: bool,
    /// Allow control characters
    pub allow_control_chars: bool,
    /// URL allowlist (empty = allow all)
    pub url_allowlist: HashSet<String>,
    /// URL blocklist
    pub url_blocklist: HashSet<String>,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            max_message_length: 100_000,
            min_message_length: 1,
            allow_null_bytes: false,
            allow_control_chars: false,
            url_allowlist: HashSet::new(),
            url_blocklist: HashSet::from([
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "0.0.0.0".to_string(),
                "169.254.169.254".to_string(), // AWS metadata
                "[::1]".to_string(),
            ]),
        }
    }
}

/// Input validator
pub struct InputValidator {
    config: ValidatorConfig,
}

impl InputValidator {
    /// Create a new validator with default config
    pub fn new() -> Self {
        Self {
            config: ValidatorConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: ValidatorConfig) -> Self {
        Self { config }
    }

    /// Validate a message
    pub fn validate_message(&self, message: &str) -> ValidationResult {
        let mut result = ValidationResult::ok();

        // Length checks
        if message.len() > self.config.max_message_length {
            result.add_error(ValidationError::TooLong {
                max: self.config.max_message_length,
                actual: message.len(),
            });
        }

        if message.len() < self.config.min_message_length {
            result.add_error(ValidationError::TooShort {
                min: self.config.min_message_length,
                actual: message.len(),
            });
        }

        // Null bytes
        if !self.config.allow_null_bytes && message.contains('\0') {
            result.add_error(ValidationError::NullBytes);
        }

        // Control characters (except common whitespace)
        if !self.config.allow_control_chars {
            for c in message.chars() {
                if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
                    result.add_error(ValidationError::ControlCharacters);
                    break;
                }
            }
        }

        result
    }

    /// Validate a file path (check for traversal attacks)
    pub fn validate_path(&self, path: &str) -> ValidationResult {
        let mut result = ValidationResult::ok();

        // Check for null bytes
        if path.contains('\0') {
            result.add_error(ValidationError::NullBytes);
            return result;
        }

        // Check for path traversal
        let path_obj = Path::new(path);

        // Check components for ..
        for component in path_obj.components() {
            if let std::path::Component::ParentDir = component {
                result.add_error(ValidationError::PathTraversal {
                    path: path.to_string(),
                });
                return result;
            }
        }

        // Check for absolute paths (if not allowed)
        if path_obj.is_absolute() {
            result.add_warning("Absolute path provided".to_string());
        }

        // Check for suspicious patterns
        if path.contains("..") {
            result.add_error(ValidationError::PathTraversal {
                path: path.to_string(),
            });
        }

        result
    }

    /// Validate a URL
    pub fn validate_url(&self, url: &str) -> ValidationResult {
        let mut result = ValidationResult::ok();

        // Parse URL
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                result.add_error(ValidationError::InvalidFormat {
                    field: "url".to_string(),
                    expected: format!("valid URL ({})", e),
                });
                return result;
            }
        };

        // Check scheme
        if !matches!(parsed.scheme(), "http" | "https") {
            result.add_error(ValidationError::DisallowedUrl {
                url: url.to_string(),
                reason: "Only http/https allowed".to_string(),
            });
            return result;
        }

        // Get host
        let host = match parsed.host_str() {
            Some(h) => h,
            None => {
                result.add_error(ValidationError::InvalidFormat {
                    field: "url".to_string(),
                    expected: "URL with host".to_string(),
                });
                return result;
            }
        };

        // Check blocklist
        for blocked in &self.config.url_blocklist {
            if host == blocked || host.ends_with(&format!(".{}", blocked)) {
                result.add_error(ValidationError::DisallowedUrl {
                    url: url.to_string(),
                    reason: format!("Host {} is blocked", host),
                });
                return result;
            }
        }

        // Check allowlist (if not empty)
        if !self.config.url_allowlist.is_empty() {
            let allowed = self.config.url_allowlist.iter().any(|pattern| {
                if pattern.starts_with("*.") {
                    let suffix = &pattern[1..];
                    host.ends_with(suffix)
                } else {
                    host == pattern
                }
            });

            if !allowed {
                result.add_error(ValidationError::DisallowedUrl {
                    url: url.to_string(),
                    reason: "Host not in allowlist".to_string(),
                });
            }
        }

        result
    }

    /// Validate JSON structure
    pub fn validate_json(
        &self,
        json: &serde_json::Value,
        required_fields: &[&str],
    ) -> ValidationResult {
        let mut result = ValidationResult::ok();

        let obj = match json.as_object() {
            Some(o) => o,
            None => {
                result.add_error(ValidationError::InvalidJson {
                    reason: "Expected object".to_string(),
                });
                return result;
            }
        };

        for field in required_fields {
            if !obj.contains_key(*field) {
                result.add_error(ValidationError::MissingField {
                    field: field.to_string(),
                });
            }
        }

        result
    }

    /// Add URL to allowlist
    pub fn allow_url(&mut self, pattern: &str) {
        self.config.url_allowlist.insert(pattern.to_string());
    }

    /// Add URL to blocklist
    pub fn block_url(&mut self, pattern: &str) {
        self.config.url_blocklist.insert(pattern.to_string());
    }
}

impl Default for InputValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_validation() {
        let validator = InputValidator::new();

        // Valid message
        let result = validator.validate_message("Hello, world!");
        assert!(result.valid);

        // Empty message
        let result = validator.validate_message("");
        assert!(!result.valid);

        // Message with null byte
        let result = validator.validate_message("Hello\0world");
        assert!(!result.valid);
    }

    #[test]
    fn test_path_validation() {
        let validator = InputValidator::new();

        // Valid path
        let result = validator.validate_path("docs/readme.md");
        assert!(result.valid);

        // Path traversal
        let result = validator.validate_path("../../../etc/passwd");
        assert!(!result.valid);

        // Null byte
        let result = validator.validate_path("file\0.txt");
        assert!(!result.valid);
    }

    #[test]
    fn test_url_validation() {
        let validator = InputValidator::new();

        // Valid URL
        let result = validator.validate_url("https://api.example.com/data");
        assert!(result.valid);

        // Localhost blocked
        let result = validator.validate_url("http://localhost:8080/admin");
        assert!(!result.valid);

        // AWS metadata blocked
        let result = validator.validate_url("http://169.254.169.254/latest/meta-data/");
        assert!(!result.valid);

        // Invalid scheme
        let result = validator.validate_url("file:///etc/passwd");
        assert!(!result.valid);
    }

    #[test]
    fn test_url_allowlist() {
        let mut validator = InputValidator::new();
        validator.allow_url("*.github.com");
        validator.allow_url("api.openai.com");

        // Allowed by wildcard
        let result = validator.validate_url("https://api.github.com/repos");
        assert!(result.valid);

        // Allowed exact
        let result = validator.validate_url("https://api.openai.com/v1/chat");
        assert!(result.valid);

        // Not in allowlist
        let result = validator.validate_url("https://example.com/data");
        assert!(!result.valid);
    }

    #[test]
    fn test_json_validation() {
        let validator = InputValidator::new();

        let json = serde_json::json!({
            "name": "test",
            "value": 42
        });

        // Valid
        let result = validator.validate_json(&json, &["name"]);
        assert!(result.valid);

        // Missing field
        let result = validator.validate_json(&json, &["name", "missing"]);
        assert!(!result.valid);
    }
}
