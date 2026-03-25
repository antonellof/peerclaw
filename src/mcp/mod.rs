//! Model Context Protocol (MCP) client implementation.
//!
//! MCP enables connecting to external context servers that provide:
//! - Additional tools and capabilities
//! - External data sources
//! - Specialized services
//!
//! See: https://github.com/modelcontextprotocol/specification

pub mod client;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub use client::McpClient;
pub use types::*;

/// MCP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Enable MCP
    pub enabled: bool,
    /// List of MCP servers to connect to
    pub servers: Vec<McpServerConfig>,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Auto-reconnect on failure
    pub auto_reconnect: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: Vec::new(),
            timeout_secs: 30,
            auto_reconnect: true,
        }
    }
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name
    pub name: String,
    /// Server URL (stdio:// or http://)
    pub url: String,
    /// Environment variables for stdio servers
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Command and args for stdio servers
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// MCP manager for handling multiple servers
pub struct McpManager {
    config: McpConfig,
    clients: Arc<RwLock<HashMap<String, McpClient>>>,
    tools: Arc<RwLock<HashMap<String, McpTool>>>,
    resources: Arc<RwLock<HashMap<String, McpResource>>>,
}

impl McpManager {
    /// Create a new MCP manager
    pub fn new(config: McpConfig) -> Self {
        Self {
            config,
            clients: Arc::new(RwLock::new(HashMap::new())),
            tools: Arc::new(RwLock::new(HashMap::new())),
            resources: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connect to all configured servers
    pub async fn connect_all(&self) -> anyhow::Result<()> {
        if !self.config.enabled {
            tracing::info!("MCP disabled");
            return Ok(());
        }

        for server_config in &self.config.servers {
            match self.connect(&server_config.name, server_config).await {
                Ok(_) => {
                    tracing::info!(server = %server_config.name, "Connected to MCP server");
                }
                Err(e) => {
                    tracing::error!(
                        server = %server_config.name,
                        error = %e,
                        "Failed to connect to MCP server"
                    );
                }
            }
        }

        Ok(())
    }

    /// Connect to a specific server
    pub async fn connect(&self, name: &str, config: &McpServerConfig) -> anyhow::Result<()> {
        let client = McpClient::connect(config).await?;

        // List available tools
        let tools = client.list_tools().await?;
        {
            let mut tool_map = self.tools.write();
            for tool in tools {
                let full_name = format!("{}:{}", name, tool.name);
                tool_map.insert(full_name, tool);
            }
        }

        // List available resources
        let resources = client.list_resources().await?;
        {
            let mut resource_map = self.resources.write();
            for resource in resources {
                let full_name = format!("{}:{}", name, resource.name);
                resource_map.insert(full_name, resource);
            }
        }

        // Store client
        {
            let mut clients = self.clients.write();
            clients.insert(name.to_string(), client);
        }

        Ok(())
    }

    /// Disconnect from a server
    pub async fn disconnect(&self, name: &str) -> anyhow::Result<()> {
        let client = {
            let mut clients = self.clients.write();
            clients.remove(name)
        };

        if let Some(client) = client {
            client.disconnect().await?;

            // Remove tools from this server
            {
                let mut tools = self.tools.write();
                tools.retain(|k, _| !k.starts_with(&format!("{}:", name)));
            }

            // Remove resources from this server
            {
                let mut resources = self.resources.write();
                resources.retain(|k, _| !k.starts_with(&format!("{}:", name)));
            }

            tracing::info!(server = %name, "Disconnected from MCP server");
        }

        Ok(())
    }

    /// Disconnect all servers
    pub async fn disconnect_all(&self) -> anyhow::Result<()> {
        let names: Vec<_> = {
            let clients = self.clients.read();
            clients.keys().cloned().collect()
        };

        for name in names {
            let _ = self.disconnect(&name).await;
        }

        Ok(())
    }

    /// List all available tools
    pub fn list_tools(&self) -> Vec<McpTool> {
        let tools = self.tools.read();
        tools.values().cloned().collect()
    }

    /// Tools with fully-qualified ids (`server:tool_name`) for LLM prompts and UI.
    pub fn list_tools_with_ids(&self) -> Vec<(String, McpTool)> {
        let tools = self.tools.read();
        tools.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Get a tool by name
    pub fn get_tool(&self, name: &str) -> Option<McpTool> {
        let tools = self.tools.read();
        tools.get(name).cloned()
    }

    /// Call a tool
    pub async fn call_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<McpToolResult> {
        // Parse server:tool name
        let (server_name, tool_name) = name
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid tool name format: {}", name))?;

        let client = {
            let clients = self.clients.read();
            clients.get(server_name).cloned()
        };

        let client =
            client.ok_or_else(|| anyhow::anyhow!("Server not connected: {}", server_name))?;

        client.call_tool(tool_name, params).await
    }

    /// List all available resources
    pub fn list_resources(&self) -> Vec<McpResource> {
        let resources = self.resources.read();
        resources.values().cloned().collect()
    }

    /// Read a resource
    pub async fn read_resource(&self, name: &str) -> anyhow::Result<McpResourceContent> {
        // Parse server:resource name
        let (server_name, resource_name) = name
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid resource name format: {}", name))?;

        let client = {
            let clients = self.clients.read();
            clients.get(server_name).cloned()
        };

        let client =
            client.ok_or_else(|| anyhow::anyhow!("Server not connected: {}", server_name))?;

        client.read_resource(resource_name).await
    }

    /// Get connected server count
    pub fn server_count(&self) -> usize {
        self.clients.read().len()
    }

    /// Names of servers with an active MCP session.
    pub fn connected_server_names(&self) -> Vec<String> {
        self.clients.read().keys().cloned().collect()
    }

    /// Get tool count
    pub fn tool_count(&self) -> usize {
        self.tools.read().len()
    }

    /// Check if a server is connected
    pub fn is_connected(&self, name: &str) -> bool {
        self.clients.read().contains_key(name)
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new(McpConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_config_default() {
        let config = McpConfig::default();
        assert!(config.enabled);
        assert!(config.servers.is_empty());
    }

    #[test]
    fn test_mcp_manager_creation() {
        let manager = McpManager::default();
        assert_eq!(manager.server_count(), 0);
        assert_eq!(manager.tool_count(), 0);
    }
}
