//! MCP protocol types.

use serde::{Deserialize, Serialize};

/// MCP protocol version
pub const MCP_VERSION: &str = "2024-11-05";

/// MCP JSON-RPC request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl McpRequest {
    /// Create a new request
    pub fn new(id: u64, method: &str, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// MCP JSON-RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

/// MCP error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// MCP notification (no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: Option<String>,
    /// Input schema (JSON Schema)
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    /// Result content
    pub content: Vec<McpContent>,
    /// Whether the tool call errored
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

/// MCP content types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { uri: String, text: Option<String> },
}

/// MCP resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Resource name
    pub name: String,
    /// Resource URI
    pub uri: String,
    /// Resource description
    pub description: Option<String>,
    /// MIME type
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
}

/// MCP resource content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContent {
    /// Resource URI
    pub uri: String,
    /// Content text
    pub text: Option<String>,
    /// Content blob (base64)
    pub blob: Option<String>,
    /// MIME type
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
}

/// MCP prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    /// Prompt name
    pub name: String,
    /// Prompt description
    pub description: Option<String>,
    /// Arguments
    pub arguments: Option<Vec<McpPromptArgument>>,
}

/// MCP prompt argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    /// Argument name
    pub name: String,
    /// Argument description
    pub description: Option<String>,
    /// Whether required
    pub required: Option<bool>,
}

/// MCP server capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerCapabilities {
    /// Tools capability
    pub tools: Option<McpToolsCapability>,
    /// Resources capability
    pub resources: Option<McpResourcesCapability>,
    /// Prompts capability
    pub prompts: Option<McpPromptsCapability>,
    /// Logging capability
    pub logging: Option<serde_json::Value>,
}

/// MCP tools capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpToolsCapability {
    /// Whether tool list can change
    #[serde(rename = "listChanged")]
    pub list_changed: Option<bool>,
}

/// MCP resources capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpResourcesCapability {
    /// Whether resource list can change
    #[serde(rename = "listChanged")]
    pub list_changed: Option<bool>,
    /// Whether subscribing is supported
    pub subscribe: Option<bool>,
}

/// MCP prompts capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpPromptsCapability {
    /// Whether prompt list can change
    #[serde(rename = "listChanged")]
    pub list_changed: Option<bool>,
}

/// MCP client capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpClientCapabilities {
    /// Roots capability
    pub roots: Option<McpRootsCapability>,
    /// Sampling capability
    pub sampling: Option<serde_json::Value>,
}

/// MCP roots capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpRootsCapability {
    /// Whether root list can change
    #[serde(rename = "listChanged")]
    pub list_changed: Option<bool>,
}

/// MCP initialization params
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInitializeParams {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// Client capabilities
    pub capabilities: McpClientCapabilities,
    /// Client info
    #[serde(rename = "clientInfo")]
    pub client_info: McpClientInfo,
}

/// MCP client info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientInfo {
    /// Client name
    pub name: String,
    /// Client version
    pub version: String,
}

/// MCP initialization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInitializeResult {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// Server capabilities
    pub capabilities: McpServerCapabilities,
    /// Server info
    #[serde(rename = "serverInfo")]
    pub server_info: Option<McpServerInfo>,
}

/// MCP server info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    /// Server name
    pub name: String,
    /// Server version
    pub version: Option<String>,
}

/// MCP list tools result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpListToolsResult {
    pub tools: Vec<McpTool>,
}

/// MCP list resources result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpListResourcesResult {
    pub resources: Vec<McpResource>,
}

/// MCP list prompts result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpListPromptsResult {
    pub prompts: Vec<McpPrompt>,
}

/// MCP call tool params
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCallToolParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

/// MCP read resource params
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpReadResourceParams {
    pub uri: String,
}

/// MCP read resource result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpReadResourceResult {
    pub contents: Vec<McpResourceContent>,
}
