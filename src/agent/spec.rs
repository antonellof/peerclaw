//! Agent specification parsed from TOML files.

use serde::{Deserialize, Serialize};

/// Parsed agent specification from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub agent: AgentMeta,
    #[serde(default)]
    pub model: ModelSpec,
    #[serde(default)]
    pub capabilities: CapabilitiesSpec,
    #[serde(default)]
    pub budget: BudgetSpec,
    #[serde(default)]
    pub tools: ToolsSpec,
    #[serde(default)]
    pub channels: ChannelsSpec,
    #[serde(default)]
    pub memory: Option<MemorySpec>,
    #[serde(default)]
    pub routines: Option<RoutinesSpec>,
    #[serde(default)]
    pub network_policy: Option<NetworkPolicySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    #[serde(default = "default_model")]
    pub name: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub system_prompt: String,
}

fn default_model() -> String {
    "llama-3.2-3b".to_string()
}
fn default_max_tokens() -> u32 {
    2048
}
fn default_temperature() -> f32 {
    0.7
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            name: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            system_prompt: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesSpec {
    #[serde(default)]
    pub web_access: bool,
    #[serde(default)]
    pub storage: bool,
    #[serde(default)]
    pub vector_memory: bool,
    #[serde(default)]
    pub tool_building: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSpec {
    #[serde(default = "default_per_request")]
    pub per_request: f64,
    #[serde(default = "default_per_hour")]
    pub per_hour: f64,
    #[serde(default = "default_per_day")]
    pub per_day: f64,
    #[serde(default = "default_total")]
    pub total: f64,
}

fn default_per_request() -> f64 {
    2.0
}
fn default_per_hour() -> f64 {
    20.0
}
fn default_per_day() -> f64 {
    100.0
}
fn default_total() -> f64 {
    1000.0
}

impl Default for BudgetSpec {
    fn default() -> Self {
        Self {
            per_request: default_per_request(),
            per_hour: default_per_hour(),
            per_day: default_per_day(),
            total: default_total(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsSpec {
    #[serde(default)]
    pub builtin: Vec<String>,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelsSpec {
    #[serde(default)]
    pub repl: bool,
    #[serde(default)]
    pub websocket: bool,
    #[serde(default)]
    pub webhook: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySpec {
    pub collection: String,
    #[serde(default = "default_embedding")]
    pub embedding_model: String,
}

fn default_embedding() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutinesSpec {
    #[serde(default)]
    pub cron: Vec<CronRoutine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRoutine {
    pub schedule: String,
    pub task: String,
}

/// Network egress policy: deny-by-default or allow-by-default with rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicySpec {
    /// Default action: "deny" (allowlist mode) or "allow" (blocklist mode).
    #[serde(default = "default_policy_default")]
    pub default: String,
    /// Ordered list of egress rules.
    #[serde(default)]
    pub rules: Vec<NetworkPolicyRule>,
}

fn default_policy_default() -> String {
    "deny".to_string()
}

/// A single egress rule within the network policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyRule {
    /// Host pattern, e.g. "api.github.com" or "*.openai.com".
    pub host: String,
    /// Optional port restriction (e.g. 443). If absent, any port is matched.
    pub port: Option<u16>,
    /// Optional HTTP method restriction (e.g. ["GET", "POST"]).
    /// If absent, all methods are allowed for this rule.
    #[serde(default)]
    pub methods: Vec<String>,
    /// Optional tool restriction: only these tools may use this rule.
    /// If absent, any tool may use it.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Action for this rule: "allow" or "deny". Default is "allow".
    #[serde(default = "default_rule_action")]
    pub action: String,
}

fn default_rule_action() -> String {
    "allow".to_string()
}

impl AgentSpec {
    /// Parse an agent spec from TOML content.
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Load an agent spec from a file.
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::from_toml(&content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_spec() {
        let toml = r#"
[agent]
name = "test-bot"
description = "A test agent"
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        assert_eq!(spec.agent.name, "test-bot");
        assert_eq!(spec.model.name, "llama-3.2-3b");
    }

    #[test]
    fn test_parse_full_spec() {
        let toml = r#"
[agent]
name = "peerclaw-assistant"
description = "General-purpose assistant"
version = "1.0.0"

[model]
name = "llama3.2:3b"
max_tokens = 4096
temperature = 0.5
system_prompt = "You are helpful."

[capabilities]
web_access = true
storage = true

[budget]
per_request = 5.0
total = 500.0

[tools]
builtin = ["web_search", "web_fetch", "read_file"]

[channels]
repl = true
websocket = true
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        assert_eq!(spec.agent.name, "peerclaw-assistant");
        assert_eq!(spec.model.max_tokens, 4096);
        assert!(spec.capabilities.web_access);
        assert_eq!(spec.tools.builtin.len(), 3);
    }

    #[test]
    fn test_parse_network_policy() {
        let toml = r#"
[agent]
name = "policy-bot"
description = "Bot with network policy"

[network_policy]
default = "deny"

[[network_policy.rules]]
host = "api.github.com"
port = 443
methods = ["GET", "POST"]
tools = ["web_fetch"]

[[network_policy.rules]]
host = "*.openai.com"
port = 443
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        let policy = spec.network_policy.unwrap();
        assert_eq!(policy.default, "deny");
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[0].host, "api.github.com");
        assert_eq!(policy.rules[0].port, Some(443));
        assert_eq!(policy.rules[0].methods, vec!["GET", "POST"]);
        assert_eq!(policy.rules[0].tools, vec!["web_fetch"]);
        assert_eq!(policy.rules[0].action, "allow");
        assert_eq!(policy.rules[1].host, "*.openai.com");
        assert!(policy.rules[1].methods.is_empty());
        assert!(policy.rules[1].tools.is_empty());
    }

    #[test]
    fn test_parse_no_network_policy() {
        let toml = r#"
[agent]
name = "simple-bot"
description = "No policy"
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        assert!(spec.network_policy.is_none());
    }
}
