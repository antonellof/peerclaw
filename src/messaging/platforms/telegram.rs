//! Telegram Bot API channel via HTTP (no external crate needed).
//!
//! Uses long polling (`getUpdates`) to receive messages and the `sendMessage`
//! endpoint to respond. Only requires `reqwest` which is already a dependency.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::messaging::{
    Channel, ChannelConfig, ChannelError, ChannelId, ChannelMessage, ChannelUser,
    MessageDirection, MessageId, Platform, UserTrust,
};

use super::TelegramConfig;

/// Telegram Bot API base URL.
const TELEGRAM_API: &str = "https://api.telegram.org";

/// Telegram channel using the Bot HTTP API.
pub struct TelegramChannel {
    /// Channel ID.
    id: ChannelId,
    /// Configuration.
    config: ChannelConfig,
    /// Telegram-specific config.
    tg_config: TelegramConfig,
    /// Whether connected.
    connected: bool,
    /// HTTP client.
    client: Option<reqwest::Client>,
    /// Incoming message queue (filled by the polling task).
    incoming_rx: Arc<RwLock<mpsc::Receiver<ChannelMessage>>>,
    /// Sender half (used by the polling task).
    incoming_tx: Arc<mpsc::Sender<ChannelMessage>>,
    /// Shutdown signal for the polling task.
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Handle to the background polling task.
    poll_handle: Option<tokio::task::JoinHandle<()>>,
}

impl TelegramChannel {
    /// Create a new Telegram channel from a `ChannelConfig`.
    ///
    /// The `settings` field of the config is expected to deserialize into
    /// [`TelegramConfig`].
    pub fn new(config: ChannelConfig) -> Result<Self, ChannelError> {
        let tg_config: TelegramConfig = if config.settings.is_null() {
            return Err(ChannelError::PlatformError(
                "Telegram channel requires settings with bot_token".into(),
            ));
        } else {
            serde_json::from_value(config.settings.clone())
                .map_err(|e| ChannelError::PlatformError(format!("Invalid Telegram config: {e}")))?
        };

        if tg_config.bot_token.is_empty() {
            return Err(ChannelError::AuthenticationFailed(
                "bot_token is required".into(),
            ));
        }

        let id = ChannelId::from_parts("telegram", &config.name);
        let (tx, rx) = mpsc::channel(256);

        Ok(Self {
            id,
            config,
            tg_config,
            connected: false,
            client: None,
            incoming_rx: Arc::new(RwLock::new(rx)),
            incoming_tx: Arc::new(tx),
            shutdown_tx: None,
            poll_handle: None,
        })
    }

    /// Build the full API URL for a method.
    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", TELEGRAM_API, self.tg_config.bot_token, method)
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn connect(&mut self) -> Result<(), ChannelError> {
        if self.connected {
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120)) // long-poll timeout
            .build()
            .map_err(|e| ChannelError::ConnectionFailed(e.to_string()))?;

        // Verify the token by calling getMe
        let resp = client
            .get(&self.api_url("getMe"))
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionFailed(format!("getMe failed: {e}")))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::AuthenticationFailed(format!(
                "Telegram getMe returned error: {text}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ChannelError::ConnectionFailed(e.to_string()))?;

        let bot_name = body
            .pointer("/result/username")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        tracing::info!(
            channel_id = %self.id,
            bot = %bot_name,
            "Telegram channel connected"
        );

        self.client = Some(client.clone());

        // Start the long-polling background task
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        let tx = self.incoming_tx.clone();
        let channel_id = self.id.clone();
        let api_url = self.api_url("getUpdates");
        let allowed_chats = self.tg_config.allowed_chats.clone();

        let handle = tokio::spawn(async move {
            let mut offset: i64 = 0;

            loop {
                // Check for shutdown
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                let params = serde_json::json!({
                    "offset": offset,
                    "timeout": 30,
                    "allowed_updates": ["message"]
                });

                let resp = match client.post(&api_url).json(&params).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("Telegram poll error: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let body: serde_json::Value = match resp.json().await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("Telegram parse error: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let updates = match body.get("result").and_then(|r| r.as_array()) {
                    Some(arr) => arr.clone(),
                    None => continue,
                };

                for update in updates {
                    // Advance offset past this update
                    if let Some(uid) = update.get("update_id").and_then(|v| v.as_i64()) {
                        offset = uid + 1;
                    }

                    // Extract the message object
                    let msg = match update.get("message") {
                        Some(m) => m,
                        None => continue,
                    };

                    let chat_id = msg
                        .pointer("/chat/id")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    // Filter by allowed chats if configured
                    if !allowed_chats.is_empty() && !allowed_chats.contains(&chat_id) {
                        continue;
                    }

                    let text = msg
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if text.is_empty() {
                        continue; // skip non-text messages for now
                    }

                    let from = msg.get("from");
                    let user_id = from
                        .and_then(|f| f.get("id"))
                        .and_then(|v| v.as_i64())
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let first_name = from
                        .and_then(|f| f.get("first_name"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let username = from
                        .and_then(|f| f.get("username"))
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    let sender = ChannelUser {
                        id: user_id,
                        name: first_name,
                        username,
                        is_self: false,
                        trust_level: UserTrust::Verified,
                        peer_id: None,
                    };

                    let message_id = msg
                        .get("message_id")
                        .and_then(|v| v.as_i64())
                        .map(|id| MessageId::from_external("telegram", &id.to_string()))
                        .unwrap_or_else(MessageId::new);

                    let channel_msg = ChannelMessage {
                        id: message_id,
                        channel_id: channel_id.clone(),
                        conversation_id: Some(chat_id.to_string()),
                        direction: MessageDirection::Incoming,
                        message_type: crate::messaging::MessageType::Text,
                        sender,
                        content: text,
                        attachments: Vec::new(),
                        reply_to: None,
                        timestamp: chrono::Utc::now(),
                        metadata: serde_json::json!({ "chat_id": chat_id }),
                        routing: None,
                    };

                    if tx.send(channel_msg).await.is_err() {
                        // Channel closed, exit polling
                        return;
                    }
                }
            }
        });

        self.poll_handle = Some(handle);
        self.connected = true;
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), ChannelError> {
        // Signal the polling task to stop
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
        self.client = None;
        self.connected = false;
        tracing::info!(channel_id = %self.id, "Telegram channel disconnected");
        Ok(())
    }

    async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let client = self.client.as_ref().ok_or(ChannelError::NotConnected)?;

        // Determine the chat_id from the message metadata or conversation_id
        let chat_id = message
            .metadata
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                message
                    .conversation_id
                    .as_ref()
                    .and_then(|c| c.parse::<i64>().ok())
            })
            .ok_or_else(|| {
                ChannelError::InvalidFormat("Missing chat_id in message metadata".into())
            })?;

        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": message.content,
        });

        // Set parse_mode if configured
        if !self.tg_config.parse_mode.is_empty() {
            payload["parse_mode"] = serde_json::Value::String(self.tg_config.parse_mode.clone());
        }

        // Support reply_to
        if let Some(ref reply_to) = message.reply_to {
            // Extract numeric message ID from "telegram:12345" format
            let parts: Vec<&str> = reply_to.0.splitn(2, ':').collect();
            if parts.len() == 2 {
                if let Ok(mid) = parts[1].parse::<i64>() {
                    payload["reply_to_message_id"] = serde_json::Value::Number(mid.into());
                }
            }
        }

        let resp = client
            .post(&self.api_url("sendMessage"))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ChannelError::PlatformError(format!("sendMessage failed: {e}")))?;

        if resp.status() == 429 {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let retry = body
                .pointer("/parameters/retry_after")
                .and_then(|v| v.as_u64())
                .unwrap_or(5);
            return Err(ChannelError::RateLimited {
                retry_after_secs: retry,
            });
        }

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::PlatformError(format!(
                "Telegram sendMessage error: {text}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ChannelError::PlatformError(e.to_string()))?;

        let sent_id = body
            .pointer("/result/message_id")
            .and_then(|v| v.as_i64())
            .map(|id| MessageId::from_external("telegram", &id.to_string()))
            .unwrap_or_else(MessageId::new);

        Ok(sent_id)
    }

    async fn receive(&mut self) -> Result<ChannelMessage, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming_rx.write().await;
        rx.recv().await.ok_or(ChannelError::Closed)
    }

    async fn try_receive(&mut self) -> Result<Option<ChannelMessage>, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming_rx.write().await;
        match rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(ChannelError::Closed),
        }
    }

    async fn edit(&self, message_id: &MessageId, new_content: String) -> Result<(), ChannelError> {
        let client = self.client.as_ref().ok_or(ChannelError::NotConnected)?;

        // Parse "telegram:<msg_id>" format
        let parts: Vec<&str> = message_id.0.splitn(2, ':').collect();
        if parts.len() != 2 || parts[0] != "telegram" {
            return Err(ChannelError::InvalidFormat(
                "Expected message ID in telegram:<id> format".into(),
            ));
        }
        let tg_msg_id: i64 = parts[1]
            .parse()
            .map_err(|_| ChannelError::InvalidFormat("Invalid Telegram message ID".into()))?;

        // We need a chat_id but don't have it here; this is a limitation.
        // Callers should include chat_id in the message metadata.
        // For now, return an error explaining the limitation.
        let _ = (client, tg_msg_id, new_content);
        Err(ChannelError::PlatformError(
            "editMessageText requires chat_id; use send() with reply_to instead".into(),
        ))
    }

    async fn start_typing(&self, conversation_id: &str) -> Result<(), ChannelError> {
        let client = self.client.as_ref().ok_or(ChannelError::NotConnected)?;
        let chat_id: i64 = conversation_id
            .parse()
            .map_err(|_| ChannelError::InvalidFormat("conversation_id must be a chat ID".into()))?;

        let payload = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing"
        });

        let _ = client
            .post(&self.api_url("sendChatAction"))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ChannelError::PlatformError(format!("sendChatAction failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_channel_requires_token() {
        let config = ChannelConfig {
            platform: Platform::Telegram,
            name: "test".to_string(),
            settings: serde_json::json!({}),
            ..Default::default()
        };

        // Empty bot_token should fail
        let result = TelegramChannel::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_telegram_channel_creation() {
        let config = ChannelConfig {
            platform: Platform::Telegram,
            name: "test".to_string(),
            settings: serde_json::json!({
                "bot_token": "123456:ABC-DEF",
                "allowed_chats": [],
                "parse_mode": "Markdown"
            }),
            ..Default::default()
        };

        let channel = TelegramChannel::new(config).unwrap();
        assert_eq!(channel.platform(), Platform::Telegram);
        assert!(!channel.is_connected());
    }
}
