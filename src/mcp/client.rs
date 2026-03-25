//! MCP client for connecting to MCP servers.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use super::{types::*, McpServerCapabilities, McpServerConfig};

/// MCP client for communicating with an MCP server
#[derive(Clone)]
pub struct McpClient {
    inner: Arc<McpClientInner>,
}

struct McpClientInner {
    /// Server name
    name: String,
    /// Server capabilities
    capabilities: RwLock<Option<McpServerCapabilities>>,
    /// Request ID counter
    request_id: AtomicU64,
    /// Pending requests
    pending: RwLock<HashMap<u64, oneshot::Sender<McpResponse>>>,
    /// Message sender
    tx: mpsc::Sender<String>,
    /// Running flag
    running: std::sync::atomic::AtomicBool,
}

impl McpClient {
    /// Connect to an MCP server
    pub async fn connect(config: &McpServerConfig) -> anyhow::Result<Self> {
        if config.url.starts_with("stdio://") || config.command.is_some() {
            Self::connect_stdio(config).await
        } else if config.url.starts_with("http://") || config.url.starts_with("https://") {
            Self::connect_http(config).await
        } else {
            anyhow::bail!("Unsupported MCP URL scheme: {}", config.url)
        }
    }

    /// Connect via stdio (spawn process)
    async fn connect_stdio(config: &McpServerConfig) -> anyhow::Result<Self> {
        let command = config
            .command
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No command specified for stdio MCP server"))?;

        let mut cmd = Command::new(command);
        cmd.args(&config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stdout"))?;

        let (tx, mut rx) = mpsc::channel::<String>(100);

        let inner = Arc::new(McpClientInner {
            name: config.name.clone(),
            capabilities: RwLock::new(None),
            request_id: AtomicU64::new(1),
            pending: RwLock::new(HashMap::new()),
            tx,
            running: std::sync::atomic::AtomicBool::new(true),
        });

        // Spawn writer task
        let inner_writer = inner.clone();
        let mut stdin = stdin;
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if stdin.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.write_all(b"\n").await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
            inner_writer.running.store(false, Ordering::SeqCst);
        });

        // Spawn reader task
        let inner_reader = inner.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();

            while inner_reader.running.load(Ordering::SeqCst) {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(response) = serde_json::from_str::<McpResponse>(&line) {
                            let mut pending = inner_reader.pending.write();
                            if let Some(sender) = pending.remove(&response.id) {
                                let _ = sender.send(response);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            inner_reader.running.store(false, Ordering::SeqCst);
        });

        let client = Self { inner };

        // Initialize
        client.initialize().await?;

        Ok(client)
    }

    /// Connect via HTTP (not implemented yet)
    async fn connect_http(_config: &McpServerConfig) -> anyhow::Result<Self> {
        anyhow::bail!("HTTP MCP connections not yet implemented")
    }

    /// Initialize the connection
    async fn initialize(&self) -> anyhow::Result<()> {
        let params = McpInitializeParams {
            protocol_version: MCP_VERSION.to_string(),
            capabilities: McpClientCapabilities::default(),
            client_info: McpClientInfo {
                name: "peerclaw".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let response = self
            .request("initialize", Some(serde_json::to_value(params)?))
            .await?;

        if let Some(result) = response.result {
            let init_result: McpInitializeResult = serde_json::from_value(result)?;
            let mut capabilities = self.inner.capabilities.write();
            *capabilities = Some(init_result.capabilities);
        }

        // Send initialized notification
        self.notify("notifications/initialized", None).await?;

        Ok(())
    }

    /// Send a request
    async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> anyhow::Result<McpResponse> {
        let id = self.inner.request_id.fetch_add(1, Ordering::SeqCst);
        let request = McpRequest::new(id, method, params);

        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.inner.pending.write();
            pending.insert(id, tx);
        }

        let msg = serde_json::to_string(&request)?;
        self.inner.tx.send(msg).await?;

        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow::anyhow!("Request timeout"))?
            .map_err(|_| anyhow::anyhow!("Response channel closed"))?;

        if let Some(error) = &response.error {
            anyhow::bail!("MCP error {}: {}", error.code, error.message);
        }

        Ok(response)
    }

    /// Send a notification (no response expected)
    async fn notify(&self, method: &str, params: Option<serde_json::Value>) -> anyhow::Result<()> {
        let notification = McpNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        };

        let msg = serde_json::to_string(&notification)?;
        self.inner.tx.send(msg).await?;

        Ok(())
    }

    /// List available tools
    pub async fn list_tools(&self) -> anyhow::Result<Vec<McpTool>> {
        let response = self.request("tools/list", None).await?;

        if let Some(result) = response.result {
            let list_result: McpListToolsResult = serde_json::from_value(result)?;
            Ok(list_result.tools)
        } else {
            Ok(Vec::new())
        }
    }

    /// Call a tool
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> anyhow::Result<McpToolResult> {
        let params = McpCallToolParams {
            name: name.to_string(),
            arguments: Some(arguments),
        };

        let response = self
            .request("tools/call", Some(serde_json::to_value(params)?))
            .await?;

        if let Some(result) = response.result {
            let tool_result: McpToolResult = serde_json::from_value(result)?;
            Ok(tool_result)
        } else {
            Ok(McpToolResult {
                content: Vec::new(),
                is_error: true,
            })
        }
    }

    /// List available resources
    pub async fn list_resources(&self) -> anyhow::Result<Vec<McpResource>> {
        let response = self.request("resources/list", None).await?;

        if let Some(result) = response.result {
            let list_result: McpListResourcesResult = serde_json::from_value(result)?;
            Ok(list_result.resources)
        } else {
            Ok(Vec::new())
        }
    }

    /// Read a resource
    pub async fn read_resource(&self, uri: &str) -> anyhow::Result<McpResourceContent> {
        let params = McpReadResourceParams {
            uri: uri.to_string(),
        };

        let response = self
            .request("resources/read", Some(serde_json::to_value(params)?))
            .await?;

        if let Some(result) = response.result {
            let read_result: McpReadResourceResult = serde_json::from_value(result)?;
            read_result
                .contents
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("No content returned"))
        } else {
            anyhow::bail!("No result from read_resource")
        }
    }

    /// List available prompts
    pub async fn list_prompts(&self) -> anyhow::Result<Vec<McpPrompt>> {
        let response = self.request("prompts/list", None).await?;

        if let Some(result) = response.result {
            let list_result: McpListPromptsResult = serde_json::from_value(result)?;
            Ok(list_result.prompts)
        } else {
            Ok(Vec::new())
        }
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        self.inner.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Get server capabilities
    pub fn capabilities(&self) -> Option<McpServerCapabilities> {
        self.inner.capabilities.read().clone()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.inner.running.load(Ordering::SeqCst)
    }

    /// Get server name
    pub fn name(&self) -> &str {
        &self.inner.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_request() {
        let request = McpRequest::new(1, "tools/list", None);
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.id, 1);
        assert_eq!(request.method, "tools/list");
    }
}
