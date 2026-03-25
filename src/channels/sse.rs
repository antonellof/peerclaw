//! Server-Sent Events (SSE) manager for real-time streaming.
//!
//! Provides:
//! - Connection management with limits
//! - Event broadcasting to all connected clients
//! - Keepalive heartbeats
//! - Event filtering by type

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// SSE event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SseEvent {
    /// Text response
    Response { content: String },
    /// Streaming chunk
    StreamChunk { content: String, done: bool },
    /// Thinking indicator
    Thinking { message: String },
    /// Tool execution started
    ToolStarted {
        tool: String,
        params: serde_json::Value,
    },
    /// Tool execution completed
    ToolCompleted {
        tool: String,
        success: bool,
        duration_ms: u64,
    },
    /// Tool result
    ToolResult {
        tool: String,
        result: serde_json::Value,
    },
    /// Job started
    JobStarted { job_id: String, title: String },
    /// Job message
    JobMessage { job_id: String, message: String },
    /// Job status update
    JobStatus {
        job_id: String,
        status: String,
        progress: Option<f32>,
    },
    /// Job completed
    JobResult {
        job_id: String,
        success: bool,
        result: Option<String>,
    },
    /// Approval needed
    ApprovalNeeded {
        action: String,
        description: String,
        timeout_secs: u32,
    },
    /// Error occurred
    Error {
        message: String,
        code: Option<String>,
    },
    /// Heartbeat (keepalive)
    Heartbeat { timestamp: u64 },
    /// Custom event
    Custom {
        name: String,
        data: serde_json::Value,
    },
}

impl SseEvent {
    /// Format as SSE string
    pub fn to_sse_string(&self) -> String {
        let event_type = match self {
            SseEvent::Response { .. } => "response",
            SseEvent::StreamChunk { .. } => "stream_chunk",
            SseEvent::Thinking { .. } => "thinking",
            SseEvent::ToolStarted { .. } => "tool_started",
            SseEvent::ToolCompleted { .. } => "tool_completed",
            SseEvent::ToolResult { .. } => "tool_result",
            SseEvent::JobStarted { .. } => "job_started",
            SseEvent::JobMessage { .. } => "job_message",
            SseEvent::JobStatus { .. } => "job_status",
            SseEvent::JobResult { .. } => "job_result",
            SseEvent::ApprovalNeeded { .. } => "approval_needed",
            SseEvent::Error { .. } => "error",
            SseEvent::Heartbeat { .. } => "heartbeat",
            SseEvent::Custom { name, .. } => name,
        };

        let data = serde_json::to_string(self).unwrap_or_default();

        format!("event: {}\ndata: {}\n\n", event_type, data)
    }
}

/// SSE manager for handling connections and broadcasting
pub struct SseManager {
    /// Broadcast sender
    tx: broadcast::Sender<SseEvent>,
    /// Connection count
    connection_count: Arc<AtomicU64>,
    /// Maximum connections
    max_connections: u64,
    /// Keepalive interval
    keepalive_interval: Duration,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl SseManager {
    /// Create a new SSE manager
    pub fn new(max_connections: u64) -> Self {
        let (tx, _) = broadcast::channel(256);

        Self {
            tx,
            connection_count: Arc::new(AtomicU64::new(0)),
            max_connections,
            keepalive_interval: Duration::from_secs(30),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the keepalive task
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);

        let tx = self.tx.clone();
        let running = self.running.clone();
        let interval = self.keepalive_interval;

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                tokio::time::sleep(interval).await;

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let _ = tx.send(SseEvent::Heartbeat { timestamp });
            }
        });
    }

    /// Stop the keepalive task
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> Option<SseSubscription> {
        // Check connection limit
        let count = self.connection_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.max_connections {
            self.connection_count.fetch_sub(1, Ordering::SeqCst);
            return None;
        }

        Some(SseSubscription {
            rx: self.tx.subscribe(),
            connection_count: self.connection_count.clone(),
        })
    }

    /// Broadcast an event to all subscribers
    pub fn broadcast(&self, event: SseEvent) {
        let _ = self.tx.send(event);
    }

    /// Send a text response
    pub fn send_response(&self, content: &str) {
        self.broadcast(SseEvent::Response {
            content: content.to_string(),
        });
    }

    /// Send a stream chunk
    pub fn send_chunk(&self, content: &str, done: bool) {
        self.broadcast(SseEvent::StreamChunk {
            content: content.to_string(),
            done,
        });
    }

    /// Send thinking indicator
    pub fn send_thinking(&self, message: &str) {
        self.broadcast(SseEvent::Thinking {
            message: message.to_string(),
        });
    }

    /// Send tool started
    pub fn send_tool_started(&self, tool: &str, params: serde_json::Value) {
        self.broadcast(SseEvent::ToolStarted {
            tool: tool.to_string(),
            params,
        });
    }

    /// Send tool completed
    pub fn send_tool_completed(&self, tool: &str, success: bool, duration_ms: u64) {
        self.broadcast(SseEvent::ToolCompleted {
            tool: tool.to_string(),
            success,
            duration_ms,
        });
    }

    /// Send error
    pub fn send_error(&self, message: &str, code: Option<&str>) {
        self.broadcast(SseEvent::Error {
            message: message.to_string(),
            code: code.map(|s| s.to_string()),
        });
    }

    /// Get current connection count
    pub fn connection_count(&self) -> u64 {
        self.connection_count.load(Ordering::SeqCst)
    }

    /// Check if at capacity
    pub fn is_at_capacity(&self) -> bool {
        self.connection_count() >= self.max_connections
    }
}

impl Default for SseManager {
    fn default() -> Self {
        Self::new(100)
    }
}

/// SSE subscription handle
pub struct SseSubscription {
    rx: broadcast::Receiver<SseEvent>,
    connection_count: Arc<AtomicU64>,
}

impl SseSubscription {
    /// Receive the next event
    pub async fn recv(&mut self) -> Option<SseEvent> {
        loop {
            match self.rx.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Skip lagged events
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    }
}

impl Drop for SseSubscription {
    fn drop(&mut self) {
        self.connection_count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// SSE response builder for axum
pub struct SseResponse {
    events: Vec<SseEvent>,
}

impl SseResponse {
    /// Create a new SSE response
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Add an event
    pub fn event(mut self, event: SseEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Build the response body
    pub fn build(self) -> String {
        self.events
            .iter()
            .map(|e| e.to_sse_string())
            .collect::<Vec<_>>()
            .join("")
    }
}

impl Default for SseResponse {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_event_format() {
        let event = SseEvent::Response {
            content: "Hello!".to_string(),
        };

        let sse = event.to_sse_string();
        assert!(sse.starts_with("event: response\n"));
        assert!(sse.contains("\"content\":\"Hello!\""));
    }

    #[tokio::test]
    async fn test_sse_manager() {
        let manager = SseManager::new(10);

        // Subscribe
        let mut sub = manager.subscribe().unwrap();
        assert_eq!(manager.connection_count(), 1);

        // Broadcast
        manager.send_response("Test message");

        // Receive
        let event = sub.recv().await.unwrap();
        match event {
            SseEvent::Response { content } => {
                assert_eq!(content, "Test message");
            }
            _ => panic!("Wrong event type"),
        }

        // Drop subscription
        drop(sub);
        assert_eq!(manager.connection_count(), 0);
    }

    #[test]
    fn test_connection_limit() {
        let manager = SseManager::new(2);

        let _sub1 = manager.subscribe().unwrap();
        let _sub2 = manager.subscribe().unwrap();

        // Third should fail
        assert!(manager.subscribe().is_none());
        assert!(manager.is_at_capacity());
    }
}
