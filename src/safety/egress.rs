//! Network egress policy enforcement.
//!
//! Provides deny-by-default (allowlist) or allow-by-default (blocklist) network
//! egress control for agent tool execution. Policies are declared in the agent
//! TOML spec under `[network_policy]`.

use std::fmt;

use url::Url;

use crate::agent::spec::{NetworkPolicyRule, NetworkPolicySpec};

/// The default action when no rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDefault {
    /// Only explicitly allowed hosts may be contacted (allowlist).
    Deny,
    /// All hosts may be contacted unless explicitly denied (blocklist).
    Allow,
}

/// Action to take for a matching rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Allow,
    Deny,
}

/// Compiled egress rule for fast matching.
#[derive(Debug, Clone)]
pub struct EgressRule {
    /// Host pattern (lowercase). A leading "*." means match any subdomain.
    pub host_pattern: String,
    /// Whether the host pattern is a wildcard (starts with "*.")
    pub is_wildcard: bool,
    /// Optional port restriction.
    pub port: Option<u16>,
    /// Allowed HTTP methods (uppercase). Empty means all methods.
    pub methods: Vec<String>,
    /// Tools that may use this rule. Empty means all tools.
    pub tools: Vec<String>,
    /// Action when this rule matches.
    pub action: RuleAction,
}

/// Compiled egress policy ready for enforcement.
#[derive(Debug, Clone)]
pub struct EgressPolicy {
    /// Default action when no rule matches.
    pub default: PolicyDefault,
    /// Ordered rules (first match wins).
    pub rules: Vec<EgressRule>,
}

/// Error returned when an egress check fails.
#[derive(Debug, Clone)]
pub struct EgressDenied {
    pub url: String,
    pub tool_name: String,
    pub reason: String,
}

impl fmt::Display for EgressDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "egress denied for tool '{}' to '{}': {}",
            self.tool_name, self.url, self.reason
        )
    }
}

impl std::error::Error for EgressDenied {}

impl EgressPolicy {
    /// Build an egress policy from the agent spec's network_policy section.
    /// Returns `None` if no network_policy is defined (no restrictions).
    pub fn from_spec(spec: &Option<NetworkPolicySpec>) -> Option<Self> {
        let spec = spec.as_ref()?;

        let default = match spec.default.to_lowercase().as_str() {
            "allow" => PolicyDefault::Allow,
            _ => PolicyDefault::Deny,
        };

        let rules = spec
            .rules
            .iter()
            .map(|r| EgressRule::from_spec_rule(r))
            .collect();

        Some(Self { default, rules })
    }

    /// Create a deny-all policy (no egress allowed).
    pub fn deny_all() -> Self {
        Self {
            default: PolicyDefault::Deny,
            rules: vec![],
        }
    }

    /// Create an allow-all policy (no restrictions).
    pub fn allow_all() -> Self {
        Self {
            default: PolicyDefault::Allow,
            rules: vec![],
        }
    }

    /// Check whether a URL may be accessed by the given tool.
    ///
    /// Returns `Ok(())` if allowed, `Err(EgressDenied)` if blocked.
    pub fn check_egress(&self, url: &str, tool_name: &str) -> Result<(), EgressDenied> {
        self.check_egress_with_method(url, tool_name, None)
    }

    /// Check with an explicit HTTP method.
    pub fn check_egress_with_method(
        &self,
        url: &str,
        tool_name: &str,
        method: Option<&str>,
    ) -> Result<(), EgressDenied> {
        let parsed = Url::parse(url).map_err(|e| EgressDenied {
            url: url.to_string(),
            tool_name: tool_name.to_string(),
            reason: format!("invalid URL: {}", e),
        })?;

        let host = parsed
            .host_str()
            .ok_or_else(|| EgressDenied {
                url: url.to_string(),
                tool_name: tool_name.to_string(),
                reason: "URL has no host".to_string(),
            })?
            .to_lowercase();

        let port = parsed.port_or_known_default();

        // Walk rules in order; first match wins.
        for rule in &self.rules {
            if !rule.matches_host(&host) {
                continue;
            }
            if let Some(rule_port) = rule.port {
                if let Some(url_port) = port {
                    if rule_port != url_port {
                        continue;
                    }
                }
            }
            if !rule.methods.is_empty() {
                if let Some(m) = method {
                    if !rule.methods.iter().any(|rm| rm.eq_ignore_ascii_case(m)) {
                        continue;
                    }
                }
            }
            if !rule.tools.is_empty() && !rule.tools.iter().any(|t| t == tool_name) {
                continue;
            }

            // Rule matched.
            return match rule.action {
                RuleAction::Allow => Ok(()),
                RuleAction::Deny => Err(EgressDenied {
                    url: url.to_string(),
                    tool_name: tool_name.to_string(),
                    reason: format!("denied by rule for host pattern '{}'", rule.host_pattern),
                }),
            };
        }

        // No rule matched; apply default.
        match self.default {
            PolicyDefault::Allow => Ok(()),
            PolicyDefault::Deny => Err(EgressDenied {
                url: url.to_string(),
                tool_name: tool_name.to_string(),
                reason: "no matching allow rule (default deny)".to_string(),
            }),
        }
    }
}

impl EgressRule {
    fn from_spec_rule(r: &NetworkPolicyRule) -> Self {
        let host_lower = r.host.to_lowercase();
        let is_wildcard = host_lower.starts_with("*.");
        let action = match r.action.to_lowercase().as_str() {
            "deny" => RuleAction::Deny,
            _ => RuleAction::Allow,
        };
        Self {
            host_pattern: host_lower,
            is_wildcard,
            port: r.port,
            methods: r.methods.iter().map(|m| m.to_uppercase()).collect(),
            tools: r.tools.clone(),
            action,
        }
    }

    /// Check whether this rule's host pattern matches the given hostname.
    fn matches_host(&self, host: &str) -> bool {
        if self.is_wildcard {
            // Pattern "*.example.com" matches "sub.example.com" and "example.com"
            let suffix = &self.host_pattern[1..]; // ".example.com"
            host.ends_with(suffix) || host == &suffix[1..] // exact base domain
        } else {
            host == self.host_pattern
        }
    }
}

/// Convenience: set of tool names that make network requests (builtin).
/// Used to decide whether an egress check is needed.
pub const NETWORK_TOOLS: &[&str] = &["http", "web_fetch", "web_search", "browser"];

/// Returns true if the given tool name is known to make network requests.
pub fn is_network_tool(name: &str) -> bool {
    NETWORK_TOOLS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::spec::{NetworkPolicyRule, NetworkPolicySpec};

    fn make_deny_policy(rules: Vec<NetworkPolicyRule>) -> EgressPolicy {
        EgressPolicy::from_spec(&Some(NetworkPolicySpec {
            default: "deny".to_string(),
            rules,
        }))
        .unwrap()
    }

    fn make_allow_policy(rules: Vec<NetworkPolicyRule>) -> EgressPolicy {
        EgressPolicy::from_spec(&Some(NetworkPolicySpec {
            default: "allow".to_string(),
            rules,
        }))
        .unwrap()
    }

    fn allow_rule(host: &str) -> NetworkPolicyRule {
        NetworkPolicyRule {
            host: host.to_string(),
            port: None,
            methods: vec![],
            tools: vec![],
            action: "allow".to_string(),
        }
    }

    fn deny_rule(host: &str) -> NetworkPolicyRule {
        NetworkPolicyRule {
            host: host.to_string(),
            port: None,
            methods: vec![],
            tools: vec![],
            action: "deny".to_string(),
        }
    }

    // -- Basic deny-by-default tests --

    #[test]
    fn test_deny_default_blocks_unlisted() {
        let policy = make_deny_policy(vec![allow_rule("api.github.com")]);
        assert!(policy
            .check_egress("https://evil.com/steal", "web_fetch")
            .is_err());
    }

    #[test]
    fn test_deny_default_allows_listed() {
        let policy = make_deny_policy(vec![allow_rule("api.github.com")]);
        assert!(policy
            .check_egress("https://api.github.com/repos", "web_fetch")
            .is_ok());
    }

    // -- Basic allow-by-default tests --

    #[test]
    fn test_allow_default_passes_unlisted() {
        let policy = make_allow_policy(vec![deny_rule("evil.com")]);
        assert!(policy
            .check_egress("https://api.github.com/repos", "web_fetch")
            .is_ok());
    }

    #[test]
    fn test_allow_default_blocks_denied() {
        let policy = make_allow_policy(vec![deny_rule("evil.com")]);
        assert!(policy
            .check_egress("https://evil.com/steal", "web_fetch")
            .is_err());
    }

    // -- Wildcard host matching --

    #[test]
    fn test_wildcard_matches_subdomain() {
        let policy = make_deny_policy(vec![allow_rule("*.openai.com")]);
        assert!(policy
            .check_egress("https://api.openai.com/v1/chat", "http")
            .is_ok());
        assert!(policy
            .check_egress("https://cdn.openai.com/file", "http")
            .is_ok());
    }

    #[test]
    fn test_wildcard_matches_base_domain() {
        let policy = make_deny_policy(vec![allow_rule("*.openai.com")]);
        assert!(policy
            .check_egress("https://openai.com/", "http")
            .is_ok());
    }

    #[test]
    fn test_wildcard_does_not_match_unrelated() {
        let policy = make_deny_policy(vec![allow_rule("*.openai.com")]);
        assert!(policy
            .check_egress("https://notopenai.com/", "http")
            .is_err());
    }

    // -- Port matching --

    #[test]
    fn test_port_restriction() {
        let mut rule = allow_rule("api.github.com");
        rule.port = Some(443);
        let policy = make_deny_policy(vec![rule]);

        // HTTPS default port 443 => allowed
        assert!(policy
            .check_egress("https://api.github.com/repos", "web_fetch")
            .is_ok());
        // Explicit port 8080 => no match => default deny
        assert!(policy
            .check_egress("https://api.github.com:8080/repos", "web_fetch")
            .is_err());
    }

    // -- Method restriction --

    #[test]
    fn test_method_restriction() {
        let rule = NetworkPolicyRule {
            host: "api.github.com".to_string(),
            port: None,
            methods: vec!["GET".to_string()],
            tools: vec![],
            action: "allow".to_string(),
        };
        let policy = make_deny_policy(vec![rule]);

        assert!(policy
            .check_egress_with_method("https://api.github.com/repos", "web_fetch", Some("GET"))
            .is_ok());
        assert!(policy
            .check_egress_with_method("https://api.github.com/repos", "web_fetch", Some("DELETE"))
            .is_err());
    }

    // -- Tool restriction --

    #[test]
    fn test_tool_restriction() {
        let rule = NetworkPolicyRule {
            host: "api.github.com".to_string(),
            port: None,
            methods: vec![],
            tools: vec!["web_fetch".to_string()],
            action: "allow".to_string(),
        };
        let policy = make_deny_policy(vec![rule]);

        assert!(policy
            .check_egress("https://api.github.com/repos", "web_fetch")
            .is_ok());
        // http tool is not listed => rule does not match => default deny
        assert!(policy
            .check_egress("https://api.github.com/repos", "http")
            .is_err());
    }

    // -- Edge cases --

    #[test]
    fn test_no_policy_returns_none() {
        assert!(EgressPolicy::from_spec(&None).is_none());
    }

    #[test]
    fn test_deny_all() {
        let policy = EgressPolicy::deny_all();
        assert!(policy
            .check_egress("https://anything.com/", "web_fetch")
            .is_err());
    }

    #[test]
    fn test_allow_all() {
        let policy = EgressPolicy::allow_all();
        assert!(policy
            .check_egress("https://anything.com/", "web_fetch")
            .is_ok());
    }

    #[test]
    fn test_invalid_url() {
        let policy = EgressPolicy::allow_all();
        assert!(policy.check_egress("not a url", "web_fetch").is_err());
    }

    #[test]
    fn test_first_match_wins() {
        // First rule denies, second would allow.
        let rules = vec![deny_rule("api.github.com"), allow_rule("*.github.com")];
        let policy = make_deny_policy(rules);
        assert!(policy
            .check_egress("https://api.github.com/repos", "web_fetch")
            .is_err());
        // A different subdomain should still be allowed by the second rule.
        assert!(policy
            .check_egress("https://raw.github.com/file", "web_fetch")
            .is_ok());
    }

    #[test]
    fn test_is_network_tool() {
        assert!(is_network_tool("http"));
        assert!(is_network_tool("web_fetch"));
        assert!(!is_network_tool("echo"));
        assert!(!is_network_tool("file_read"));
    }
}
