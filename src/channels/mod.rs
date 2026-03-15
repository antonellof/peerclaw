//! Multi-channel input system.
//!
//! Provides a unified interface for receiving messages from multiple sources:
//! - REPL (command line)
//! - HTTP webhooks
//! - WebSocket connections
//! - WASM-based channels (Telegram, Slack, Discord)
//!
//! All channels send messages through a common interface, allowing the agent
//! to handle them uniformly regardless of source.

pub mod channel;
pub mod manager;
pub mod repl;
pub mod webhook;
pub mod sse;

use serde::{Deserialize, Serialize};

pub use channel::{Channel, ChannelCapabilities, ChannelConfig};
pub use manager::ChannelManager;
pub use repl::ReplChannel;
pub use webhook::WebhookChannel;
pub use sse::{SseManager, SseEvent};

/// Incoming message from any channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// Unique message ID
    pub id: String,
    /// User identifier
    pub user_id: String,
    /// Optional thread/conversation ID
    pub thread_id: Option<String>,
    /// Channel name
    pub channel: String,
    /// Message content
    pub content: String,
    /// Attachments
    pub attachments: Vec<Attachment>,
    /// Additional metadata
    pub metadata: MessageMetadata,
}

/// Outgoing response to a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingResponse {
    /// Response ID (usually matches incoming message ID)
    pub id: String,
    /// User to respond to
    pub user_id: String,
    /// Thread/conversation ID
    pub thread_id: Option<String>,
    /// Channel name
    pub channel: String,
    /// Response content
    pub content: String,
    /// Attachments to send
    pub attachments: Vec<Attachment>,
    /// Response type
    pub response_type: ResponseType,
}

/// Response type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseType {
    /// Normal text response
    Text,
    /// Streaming response (partial)
    Stream,
    /// Final response in a stream
    StreamEnd,
    /// Error response
    Error,
    /// Thinking/processing indicator
    Thinking,
    /// Tool execution update
    ToolUpdate,
}

/// Message attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Attachment name/filename
    pub name: String,
    /// MIME type
    pub mime_type: String,
    /// Content (base64 encoded for binary)
    pub content: AttachmentContent,
    /// Size in bytes
    pub size: usize,
}

/// Attachment content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentContent {
    /// Text content
    Text(String),
    /// Binary content (base64)
    Binary(String),
    /// URL reference
    Url(String),
}

/// Message metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Original platform-specific data
    pub platform_data: Option<serde_json::Value>,
    /// Timestamp
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether this is a command (starts with /)
    pub is_command: bool,
    /// Command name if applicable
    pub command: Option<String>,
    /// Priority level
    pub priority: MessagePriority,
}

/// Message priority
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessagePriority {
    /// Low priority (batch processing OK)
    Low,
    /// Normal priority
    #[default]
    Normal,
    /// High priority (process immediately)
    High,
    /// Urgent (interrupt current work)
    Urgent,
}

impl IncomingMessage {
    /// Create a new incoming message
    pub fn new(channel: &str, user_id: &str, content: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            thread_id: None,
            channel: channel.to_string(),
            content: content.to_string(),
            attachments: Vec::new(),
            metadata: MessageMetadata::default(),
        }
    }

    /// Set thread ID
    pub fn with_thread(mut self, thread_id: &str) -> Self {
        self.thread_id = Some(thread_id.to_string());
        self
    }

    /// Check if this is a command
    pub fn is_command(&self) -> bool {
        self.content.starts_with('/')
    }

    /// Get command name if this is a command
    pub fn command_name(&self) -> Option<&str> {
        if self.is_command() {
            self.content[1..].split_whitespace().next()
        } else {
            None
        }
    }

    /// Get command arguments if this is a command
    pub fn command_args(&self) -> Option<&str> {
        if self.is_command() {
            let content = &self.content[1..];
            content.find(' ').map(|pos| content[pos + 1..].trim())
        } else {
            None
        }
    }
}

impl OutgoingResponse {
    /// Create a text response
    pub fn text(id: &str, channel: &str, user_id: &str, content: &str) -> Self {
        Self {
            id: id.to_string(),
            user_id: user_id.to_string(),
            thread_id: None,
            channel: channel.to_string(),
            content: content.to_string(),
            attachments: Vec::new(),
            response_type: ResponseType::Text,
        }
    }

    /// Create a streaming response
    pub fn stream(id: &str, channel: &str, user_id: &str, chunk: &str) -> Self {
        Self {
            id: id.to_string(),
            user_id: user_id.to_string(),
            thread_id: None,
            channel: channel.to_string(),
            content: chunk.to_string(),
            attachments: Vec::new(),
            response_type: ResponseType::Stream,
        }
    }

    /// Create an error response
    pub fn error(id: &str, channel: &str, user_id: &str, error: &str) -> Self {
        Self {
            id: id.to_string(),
            user_id: user_id.to_string(),
            thread_id: None,
            channel: channel.to_string(),
            content: error.to_string(),
            attachments: Vec::new(),
            response_type: ResponseType::Error,
        }
    }

    /// Set thread ID
    pub fn with_thread(mut self, thread_id: &str) -> Self {
        self.thread_id = Some(thread_id.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incoming_message() {
        let msg = IncomingMessage::new("test", "user123", "Hello world");
        assert_eq!(msg.channel, "test");
        assert_eq!(msg.user_id, "user123");
        assert!(!msg.is_command());
    }

    #[test]
    fn test_command_parsing() {
        let msg = IncomingMessage::new("test", "user", "/help me");
        assert!(msg.is_command());
        assert_eq!(msg.command_name(), Some("help"));
        assert_eq!(msg.command_args(), Some("me"));
    }

    #[test]
    fn test_outgoing_response() {
        let resp = OutgoingResponse::text("123", "test", "user", "Hello!");
        assert_eq!(resp.response_type, ResponseType::Text);
    }
}
