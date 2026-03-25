//! Content policy enforcement.
//!
//! Rule-based content filtering for safety and compliance:
//! - Block dangerous content (malware, exploits)
//! - Warn about sensitive topics
//! - Enforce content guidelines

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Policy action to take
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    /// Allow the content
    Allow,
    /// Warn but allow
    Warn,
    /// Block the content
    Block,
}

/// A policy rule
#[derive(Debug)]
pub struct PolicyRule {
    /// Rule name
    pub name: String,
    /// Rule description
    pub description: String,
    /// Pattern to match
    pattern: Regex,
    /// Action to take
    pub action: PolicyAction,
    /// Category
    pub category: PolicyCategory,
}

/// Policy violation
#[derive(Debug, Clone)]
pub struct PolicyViolation {
    /// Rule that was violated
    pub rule: String,
    /// Description of the violation
    pub description: String,
    /// Action to take
    pub action: PolicyAction,
    /// Category
    pub category: PolicyCategory,
    /// Matched text preview
    pub preview: String,
}

/// Categories of policy rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyCategory {
    /// Security-related (malware, exploits)
    Security,
    /// Privacy-related (PII, tracking)
    Privacy,
    /// Legal/compliance (copyright, illegal content)
    Legal,
    /// Safety (harmful instructions)
    Safety,
    /// Content guidelines (spam, abuse)
    Content,
}

/// Default policy rules
static DEFAULT_RULES: LazyLock<Vec<PolicyRule>> = LazyLock::new(|| {
    vec![
        // Security rules
        PolicyRule {
            name: "malware_code".to_string(),
            description: "Potential malware or exploit code".to_string(),
            pattern: Regex::new(r"(?i)(?:rm\s+-rf\s+/|:\(\)\s*\{\s*:\|:&\s*\}\s*;:|fork\s*bomb|reverse\s*shell)").unwrap(),
            action: PolicyAction::Block,
            category: PolicyCategory::Security,
        },
        PolicyRule {
            name: "sql_injection".to_string(),
            description: "SQL injection pattern".to_string(),
            pattern: Regex::new(r"(?i)(?:'\s*(?:OR|AND)\s+'.{0,10}=|UNION\s+(?:ALL\s+)?SELECT|INSERT\s+INTO.*VALUES|DROP\s+TABLE|--\s*$)").unwrap(),
            action: PolicyAction::Warn,
            category: PolicyCategory::Security,
        },
        PolicyRule {
            name: "xss_payload".to_string(),
            description: "XSS attack pattern".to_string(),
            pattern: Regex::new(r"(?i)<script[^>]*>.*?</script>|javascript:\s*[^\s]+|on(?:error|load|click)\s*=").unwrap(),
            action: PolicyAction::Warn,
            category: PolicyCategory::Security,
        },
        // Privacy rules
        PolicyRule {
            name: "ssn_pattern".to_string(),
            description: "Social Security Number pattern".to_string(),
            pattern: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
            action: PolicyAction::Warn,
            category: PolicyCategory::Privacy,
        },
        PolicyRule {
            name: "credit_card".to_string(),
            description: "Credit card number pattern".to_string(),
            pattern: Regex::new(r"\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9][0-9])[0-9]{12})\b").unwrap(),
            action: PolicyAction::Warn,
            category: PolicyCategory::Privacy,
        },
        // Safety rules
        PolicyRule {
            name: "harmful_instructions".to_string(),
            description: "Potentially harmful instructions".to_string(),
            pattern: Regex::new(r"(?i)how\s+to\s+(?:make|build|create)\s+(?:a\s+)?(?:bomb|explosive|weapon|poison)").unwrap(),
            action: PolicyAction::Block,
            category: PolicyCategory::Safety,
        },
        // Content rules
        PolicyRule {
            name: "excessive_caps".to_string(),
            description: "Excessive capitalization (spam indicator)".to_string(),
            pattern: Regex::new(r"[A-Z\s]{50,}").unwrap(),
            action: PolicyAction::Warn,
            category: PolicyCategory::Content,
        },
    ]
});

/// Content policy
pub struct Policy {
    /// Active rules
    rules: Vec<PolicyRule>,
    /// Whether strict mode is enabled (block on any violation)
    strict_mode: bool,
}

impl Policy {
    /// Create an empty policy
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            strict_mode: false,
        }
    }

    /// Create a policy with default rules
    pub fn with_defaults() -> Self {
        Self {
            rules: DEFAULT_RULES.clone(),
            strict_mode: false,
        }
    }

    /// Enable strict mode
    pub fn set_strict_mode(&mut self, strict: bool) {
        self.strict_mode = strict;
    }

    /// Add a custom rule
    pub fn add_rule(
        &mut self,
        name: &str,
        description: &str,
        pattern: &str,
        action: PolicyAction,
        category: PolicyCategory,
    ) -> Result<(), regex::Error> {
        let regex = Regex::new(pattern)?;
        self.rules.push(PolicyRule {
            name: name.to_string(),
            description: description.to_string(),
            pattern: regex,
            action,
            category,
        });
        Ok(())
    }

    /// Check content against all rules
    pub fn check(&self, content: &str) -> Vec<PolicyViolation> {
        let mut violations = Vec::new();

        for rule in &self.rules {
            if let Some(m) = rule.pattern.find(content) {
                let preview = if m.as_str().len() > 30 {
                    format!("{}...", &m.as_str()[..30])
                } else {
                    m.as_str().to_string()
                };

                violations.push(PolicyViolation {
                    rule: rule.name.clone(),
                    description: rule.description.clone(),
                    action: if self.strict_mode {
                        PolicyAction::Block
                    } else {
                        rule.action
                    },
                    category: rule.category,
                    preview,
                });
            }
        }

        violations
    }

    /// Check if content passes all policies
    pub fn is_allowed(&self, content: &str) -> bool {
        let violations = self.check(content);
        !violations.iter().any(|v| v.action == PolicyAction::Block)
    }

    /// Get all rules
    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    /// Remove a rule by name
    pub fn remove_rule(&mut self, name: &str) {
        self.rules.retain(|r| r.name != name);
    }

    /// Clear all rules
    pub fn clear(&mut self) {
        self.rules.clear();
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl Clone for PolicyRule {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            pattern: Regex::new(self.pattern.as_str()).unwrap(),
            action: self.action,
            category: self.category,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = Policy::default();
        assert!(!policy.rules().is_empty());
    }

    #[test]
    fn test_malware_detection() {
        let policy = Policy::default();

        let content = "Run this command: rm -rf /";
        let violations = policy.check(content);

        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.rule == "malware_code"));
        assert!(violations.iter().any(|v| v.action == PolicyAction::Block));
    }

    #[test]
    fn test_sql_injection_warning() {
        let policy = Policy::default();

        let content = "SELECT * FROM users WHERE id = '' OR '1'='1'";
        let violations = policy.check(content);

        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.rule == "sql_injection"));
    }

    #[test]
    fn test_privacy_detection() {
        let policy = Policy::default();

        let content = "My SSN is 123-45-6789";
        let violations = policy.check(content);

        assert!(!violations.is_empty());
        assert!(violations
            .iter()
            .any(|v| v.category == PolicyCategory::Privacy));
    }

    #[test]
    fn test_credit_card_detection() {
        let policy = Policy::default();

        let content = "Card number: 4111111111111111";
        let violations = policy.check(content);

        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.rule == "credit_card"));
    }

    #[test]
    fn test_custom_rule() {
        let mut policy = Policy::new();
        policy
            .add_rule(
                "custom_block",
                "Block custom pattern",
                r"BLOCK_THIS",
                PolicyAction::Block,
                PolicyCategory::Content,
            )
            .unwrap();

        let content = "Please BLOCK_THIS content";
        assert!(!policy.is_allowed(content));
    }

    #[test]
    fn test_strict_mode() {
        let mut policy = Policy::default();
        policy.set_strict_mode(true);

        // Even warnings become blocks in strict mode
        let content = "Some suspicious caps: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let violations = policy.check(content);

        assert!(violations.iter().all(|v| v.action == PolicyAction::Block));
    }

    #[test]
    fn test_normal_content_passes() {
        let policy = Policy::default();

        let content = "Hello, I'm learning to program in Rust. Can you help me?";
        let violations = policy.check(content);

        assert!(violations.is_empty());
        assert!(policy.is_allowed(content));
    }
}
