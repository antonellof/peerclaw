//! Channel trait and configuration.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::{IncomingMessage, OutgoingResponse};

/// Channel trait for input sources
#[async_trait]
pub trait Channel: Send + Sync {
    /// Channel name
    fn name(&self) -> &str;

    /// Channel description
    fn description(&self) -> &str;

    /// Start the channel, sending messages to the provided sender
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()>;

    /// Send a response back through this channel
    async fn send(&self, response: OutgoingResponse) -> anyhow::Result<()>;

    /// Stop the channel
    async fn stop(&self) -> anyhow::Result<()>;

    /// Health check
    async fn health_check(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Get channel capabilities
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::default()
    }
}

/// Channel capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    /// Can send text responses
    pub can_send_text: bool,
    /// Can send attachments
    pub can_send_attachments: bool,
    /// Can receive attachments
    pub can_receive_attachments: bool,
    /// Supports threading
    pub supports_threading: bool,
    /// Supports streaming responses
    pub supports_streaming: bool,
    /// Supports rich formatting (markdown, etc.)
    pub supports_rich_format: bool,
    /// Maximum message length
    pub max_message_length: usize,
    /// Rate limit (messages per minute)
    pub rate_limit: Option<u32>,
}

impl Default for ChannelCapabilities {
    fn default() -> Self {
        Self {
            can_send_text: true,
            can_send_attachments: false,
            can_receive_attachments: false,
            supports_threading: false,
            supports_streaming: false,
            supports_rich_format: true,
            max_message_length: 4096,
            rate_limit: None,
        }
    }
}

/// Channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Channel name
    pub name: String,
    /// Channel type
    pub channel_type: ChannelType,
    /// Whether channel is enabled
    pub enabled: bool,
    /// Channel-specific settings
    pub settings: serde_json::Value,
    /// Capabilities override
    pub capabilities: Option<ChannelCapabilities>,
    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,
    /// Timeout for operations
    pub timeout: Duration,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            channel_type: ChannelType::Repl,
            enabled: true,
            settings: serde_json::Value::Null,
            capabilities: None,
            rate_limit: RateLimitConfig::default(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Channel types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    /// REPL (command line)
    Repl,
    /// HTTP webhook
    Webhook,
    /// WebSocket
    WebSocket,
    /// Server-Sent Events
    Sse,
    /// WASM-based channel
    Wasm,
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Messages per minute
    pub messages_per_minute: u32,
    /// Messages per hour
    pub messages_per_hour: u32,
    /// Burst allowance
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            messages_per_minute: 60,
            messages_per_hour: 1000,
            burst: 10,
        }
    }
}

/// Rate limiter
pub struct RateLimiter {
    config: RateLimitConfig,
    minute_count: std::sync::atomic::AtomicU32,
    hour_count: std::sync::atomic::AtomicU32,
    last_minute_reset: std::sync::RwLock<std::time::Instant>,
    last_hour_reset: std::sync::RwLock<std::time::Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            minute_count: std::sync::atomic::AtomicU32::new(0),
            hour_count: std::sync::atomic::AtomicU32::new(0),
            last_minute_reset: std::sync::RwLock::new(std::time::Instant::now()),
            last_hour_reset: std::sync::RwLock::new(std::time::Instant::now()),
        }
    }

    /// Check if request is allowed and increment counters
    pub fn check_and_increment(&self) -> bool {
        let now = std::time::Instant::now();

        // Reset minute counter if needed
        {
            let mut last_reset = self.last_minute_reset.write().unwrap();
            if now.duration_since(*last_reset) >= Duration::from_secs(60) {
                self.minute_count
                    .store(0, std::sync::atomic::Ordering::SeqCst);
                *last_reset = now;
            }
        }

        // Reset hour counter if needed
        {
            let mut last_reset = self.last_hour_reset.write().unwrap();
            if now.duration_since(*last_reset) >= Duration::from_secs(3600) {
                self.hour_count
                    .store(0, std::sync::atomic::Ordering::SeqCst);
                *last_reset = now;
            }
        }

        // Check limits
        let minute = self.minute_count.load(std::sync::atomic::Ordering::SeqCst);
        let hour = self.hour_count.load(std::sync::atomic::Ordering::SeqCst);

        if minute >= self.config.messages_per_minute || hour >= self.config.messages_per_hour {
            return false;
        }

        // Increment
        self.minute_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.hour_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        true
    }

    /// Get remaining quota
    pub fn remaining(&self) -> (u32, u32) {
        let minute = self.minute_count.load(std::sync::atomic::Ordering::SeqCst);
        let hour = self.hour_count.load(std::sync::atomic::Ordering::SeqCst);

        (
            self.config.messages_per_minute.saturating_sub(minute),
            self.config.messages_per_hour.saturating_sub(hour),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(RateLimitConfig {
            messages_per_minute: 5,
            messages_per_hour: 100,
            burst: 2,
        });

        // Should allow first 5 requests
        for _ in 0..5 {
            assert!(limiter.check_and_increment());
        }

        // Should deny 6th request
        assert!(!limiter.check_and_increment());
    }

    #[test]
    fn test_capabilities_default() {
        let caps = ChannelCapabilities::default();
        assert!(caps.can_send_text);
        assert!(!caps.can_send_attachments);
    }
}
