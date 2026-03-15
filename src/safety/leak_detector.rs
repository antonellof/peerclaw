//! Credential leak detection using pattern matching.
//!
//! Detects 15+ types of secrets and credentials:
//! - API keys (AWS, OpenAI, Anthropic, etc.)
//! - Private keys (RSA, SSH, PGP)
//! - Tokens (JWT, GitHub, Slack)
//! - Passwords in URLs or configs

use regex::Regex;
use std::sync::LazyLock;

/// A detected secret match
#[derive(Debug, Clone)]
pub struct LeakMatch {
    /// Name of the pattern that matched
    pub pattern_name: String,
    /// Start position in the text
    pub start: usize,
    /// End position in the text
    pub end: usize,
    /// The matched text (first 10 chars + ...)
    pub preview: String,
}

/// Secret pattern definition
#[derive(Debug, Clone)]
pub struct SecretPattern {
    /// Pattern name
    pub name: &'static str,
    /// Regex pattern
    pub regex: &'static str,
    /// Description
    pub description: &'static str,
}

/// All secret patterns
pub static SECRET_PATTERNS: &[SecretPattern] = &[
    // AWS
    SecretPattern {
        name: "aws_access_key",
        regex: r"(?i)AKIA[0-9A-Z]{16}",
        description: "AWS Access Key ID",
    },
    SecretPattern {
        name: "aws_secret_key",
        regex: r#"(?i)(?:aws)?_?(?:secret)?_?(?:access)?_?key['"]?\s*[:=]\s*['"]?([A-Za-z0-9/+=]{40})"#,
        description: "AWS Secret Access Key",
    },
    // OpenAI
    SecretPattern {
        name: "openai_api_key",
        regex: r"sk-(?:proj-)?[A-Za-z0-9]{32,}",
        description: "OpenAI API Key",
    },
    // Anthropic
    SecretPattern {
        name: "anthropic_api_key",
        regex: r"sk-ant-[A-Za-z0-9\-_]{32,}",
        description: "Anthropic API Key",
    },
    // GitHub
    SecretPattern {
        name: "github_token",
        regex: r"gh[pousr]_[A-Za-z0-9]{36,}",
        description: "GitHub Token",
    },
    SecretPattern {
        name: "github_fine_grained",
        regex: r"github_pat_[A-Za-z0-9]{22}_[A-Za-z0-9]{59}",
        description: "GitHub Fine-Grained PAT",
    },
    // Slack
    SecretPattern {
        name: "slack_token",
        regex: r"xox[baprs]-[0-9]{10,}-[A-Za-z0-9]{10,}",
        description: "Slack Token",
    },
    SecretPattern {
        name: "slack_webhook",
        regex: r"https://hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[A-Za-z0-9]+",
        description: "Slack Webhook URL",
    },
    // Discord
    SecretPattern {
        name: "discord_token",
        regex: r"[MN][A-Za-z\d]{23,}\.[\w-]{6}\.[\w-]{27}",
        description: "Discord Bot Token",
    },
    // Private Keys
    SecretPattern {
        name: "private_key_rsa",
        regex: r"-----BEGIN (?:RSA )?PRIVATE KEY-----",
        description: "RSA Private Key",
    },
    SecretPattern {
        name: "private_key_ssh",
        regex: r"-----BEGIN OPENSSH PRIVATE KEY-----",
        description: "SSH Private Key",
    },
    SecretPattern {
        name: "private_key_pgp",
        regex: r"-----BEGIN PGP PRIVATE KEY BLOCK-----",
        description: "PGP Private Key",
    },
    // JWT
    SecretPattern {
        name: "jwt_token",
        regex: r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        description: "JWT Token",
    },
    // Generic API Keys
    SecretPattern {
        name: "generic_api_key",
        regex: r#"(?i)(?:api[_-]?key|apikey|api_secret|api_token)['"]?\s*[:=]\s*['"]?([A-Za-z0-9\-_]{20,})"#,
        description: "Generic API Key",
    },
    // Passwords in URLs
    SecretPattern {
        name: "password_in_url",
        regex: r"(?i)(?:https?://)[^:]+:([^@]{8,})@",
        description: "Password in URL",
    },
    // Bearer tokens
    SecretPattern {
        name: "bearer_token",
        regex: r"(?i)bearer\s+[A-Za-z0-9\-_]{20,}",
        description: "Bearer Token",
    },
    // Stripe
    SecretPattern {
        name: "stripe_api_key",
        regex: r"sk_(?:live|test)_[A-Za-z0-9]{24,}",
        description: "Stripe API Key",
    },
    // Twilio
    SecretPattern {
        name: "twilio_api_key",
        regex: r"SK[A-Za-z0-9]{32}",
        description: "Twilio API Key",
    },
    // SendGrid
    SecretPattern {
        name: "sendgrid_api_key",
        regex: r"SG\.[A-Za-z0-9\-_]{22}\.[A-Za-z0-9\-_]{43}",
        description: "SendGrid API Key",
    },
    // Google
    SecretPattern {
        name: "google_api_key",
        regex: r"AIza[A-Za-z0-9\-_]{35}",
        description: "Google API Key",
    },
    // Heroku
    SecretPattern {
        name: "heroku_api_key",
        regex: r#"(?i)heroku[_-]?api[_-]?key['"]?\s*[:=]\s*['"]?([A-Fa-f0-9]{8}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{12})"#,
        description: "Heroku API Key",
    },
];

/// Compiled patterns (lazy initialization)
static COMPILED_PATTERNS: LazyLock<Vec<(SecretPattern, Regex)>> = LazyLock::new(|| {
    SECRET_PATTERNS
        .iter()
        .filter_map(|p| {
            Regex::new(p.regex)
                .ok()
                .map(|r| (p.clone(), r))
        })
        .collect()
});

/// Leak detector for scanning content
pub struct LeakDetector {
    /// Additional custom patterns
    custom_patterns: Vec<(String, Regex)>,
    /// Whether to use strict mode (block vs warn)
    strict_mode: bool,
}

impl LeakDetector {
    /// Create a new leak detector
    pub fn new() -> Self {
        Self {
            custom_patterns: Vec::new(),
            strict_mode: true,
        }
    }

    /// Create with custom patterns
    pub fn with_custom_patterns(patterns: Vec<(String, String)>) -> Self {
        let custom = patterns
            .into_iter()
            .filter_map(|(name, pattern)| {
                Regex::new(&pattern).ok().map(|r| (name, r))
            })
            .collect();

        Self {
            custom_patterns: custom,
            strict_mode: true,
        }
    }

    /// Set strict mode (block on detection)
    pub fn set_strict_mode(&mut self, strict: bool) {
        self.strict_mode = strict;
    }

    /// Scan content for secrets
    pub fn scan(&self, content: &str) -> Vec<LeakMatch> {
        let mut matches = Vec::new();

        // Check built-in patterns
        for (pattern, regex) in COMPILED_PATTERNS.iter() {
            for m in regex.find_iter(content) {
                let preview = if m.as_str().len() > 10 {
                    format!("{}...", &m.as_str()[..10])
                } else {
                    m.as_str().to_string()
                };

                matches.push(LeakMatch {
                    pattern_name: pattern.name.to_string(),
                    start: m.start(),
                    end: m.end(),
                    preview,
                });
            }
        }

        // Check custom patterns
        for (name, regex) in &self.custom_patterns {
            for m in regex.find_iter(content) {
                let preview = if m.as_str().len() > 10 {
                    format!("{}...", &m.as_str()[..10])
                } else {
                    m.as_str().to_string()
                };

                matches.push(LeakMatch {
                    pattern_name: name.clone(),
                    start: m.start(),
                    end: m.end(),
                    preview,
                });
            }
        }

        matches
    }

    /// Scan and clean (redact) secrets from content
    pub fn scan_and_clean(&self, content: &str, redaction: &str) -> Result<(String, usize), String> {
        let matches = self.scan(content);

        if matches.is_empty() {
            return Ok((content.to_string(), 0));
        }

        if self.strict_mode && matches.len() > 5 {
            return Err(format!(
                "Too many secrets detected ({}), blocking entire content",
                matches.len()
            ));
        }

        // Sort matches by position (reverse order for safe replacement)
        let mut sorted_matches = matches.clone();
        sorted_matches.sort_by(|a, b| b.start.cmp(&a.start));

        let mut cleaned = content.to_string();
        let mut redacted_count = 0;

        for m in sorted_matches {
            cleaned.replace_range(m.start..m.end, redaction);
            redacted_count += 1;
        }

        Ok((cleaned, redacted_count))
    }

    /// Check if content contains any secrets (quick check)
    pub fn contains_secrets(&self, content: &str) -> bool {
        // Quick check with built-in patterns
        for (_, regex) in COMPILED_PATTERNS.iter() {
            if regex.is_match(content) {
                return true;
            }
        }

        // Check custom patterns
        for (_, regex) in &self.custom_patterns {
            if regex.is_match(content) {
                return true;
            }
        }

        false
    }

    /// Add a custom pattern
    pub fn add_pattern(&mut self, name: &str, pattern: &str) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        self.custom_patterns.push((name.to_string(), regex));
        Ok(())
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aws_key_detection() {
        let detector = LeakDetector::new();

        let content = "My AWS key is AKIAIOSFODNN7EXAMPLE";
        let matches = detector.scan(content);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern_name, "aws_access_key");
    }

    #[test]
    fn test_openai_key_detection() {
        let detector = LeakDetector::new();

        let content = "OpenAI key: sk-proj-abcdefghijklmnopqrstuvwxyz12345678";
        let matches = detector.scan(content);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern_name, "openai_api_key");
    }

    #[test]
    fn test_github_token_detection() {
        let detector = LeakDetector::new();

        let content = "Token: ghp_abcdefghijklmnopqrstuvwxyz1234567890";
        let matches = detector.scan(content);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern_name, "github_token");
    }

    #[test]
    fn test_jwt_detection() {
        let detector = LeakDetector::new();

        let content = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let matches = detector.scan(content);

        assert!(!matches.is_empty());
    }

    #[test]
    fn test_private_key_detection() {
        let detector = LeakDetector::new();

        let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...";
        let matches = detector.scan(content);

        assert!(!matches.is_empty());
        assert_eq!(matches[0].pattern_name, "private_key_rsa");
    }

    #[test]
    fn test_clean_secrets() {
        let detector = LeakDetector::new();

        let content = "My key is AKIAIOSFODNN7EXAMPLE, don't share it!";
        let (cleaned, count) = detector.scan_and_clean(content, "[REDACTED]").unwrap();

        assert_eq!(count, 1);
        assert!(cleaned.contains("[REDACTED]"));
        assert!(!cleaned.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn test_no_false_positives() {
        let detector = LeakDetector::new();

        // Normal text should not trigger
        let content = "Hello, my name is John and I like programming.";
        let matches = detector.scan(content);
        assert!(matches.is_empty());

        // Short strings should not trigger
        let content = "api_key = 'short'";
        let matches = detector.scan(content);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_custom_pattern() {
        let mut detector = LeakDetector::new();
        detector.add_pattern("custom_secret", r"MYSECRET_[A-Z0-9]{10}").unwrap();

        let content = "Secret: MYSECRET_ABCD123456";
        let matches = detector.scan(content);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pattern_name, "custom_secret");
    }
}
