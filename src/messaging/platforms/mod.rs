//! Platform-specific channel implementations.
//!
//! Each platform module provides a Channel implementation for a specific
//! messaging platform (Telegram, Discord, Slack, etc.).

mod p2p;
mod repl;
mod telegram;
mod webhook;

// Re-export platform implementations
pub use p2p::P2pChannel;
pub use repl::ReplChannel;
pub use telegram::TelegramChannel;
pub use webhook::WebhookChannel;

// Platform channel stubs (would be WASM modules in production)
// These are placeholders showing the expected interface

use serde::{Deserialize, Serialize};

use super::{Channel, ChannelConfig, ChannelError, Platform};

/// Telegram bot channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot token from @BotFather.
    pub bot_token: String,
    /// Allowed chat IDs (empty = all).
    pub allowed_chats: Vec<i64>,
    /// Webhook URL (if using webhook mode).
    pub webhook_url: Option<String>,
    /// Parse mode (Markdown, HTML, MarkdownV2).
    pub parse_mode: String,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            bot_token: String::new(),
            allowed_chats: Vec::new(),
            webhook_url: None,
            parse_mode: "Markdown".to_string(),
        }
    }
}

/// Discord bot channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    /// Bot token.
    pub bot_token: String,
    /// Guild ID (server).
    pub guild_id: Option<u64>,
    /// Channel IDs to listen to.
    pub channel_ids: Vec<u64>,
    /// Command prefix.
    pub prefix: String,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            bot_token: String::new(),
            guild_id: None,
            channel_ids: Vec::new(),
            prefix: "!".to_string(),
        }
    }
}

/// Slack app channel configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlackConfig {
    /// Bot OAuth token.
    pub bot_token: String,
    /// App-level token (for Socket Mode).
    pub app_token: Option<String>,
    /// Signing secret.
    pub signing_secret: String,
    /// Channels to join.
    pub channels: Vec<String>,
}

/// Matrix client channel configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatrixConfig {
    /// Homeserver URL.
    pub homeserver_url: String,
    /// User ID (@user:homeserver).
    pub user_id: String,
    /// Access token.
    pub access_token: String,
    /// Rooms to join.
    pub rooms: Vec<String>,
}

/// Create a channel from configuration.
///
/// This factory function creates the appropriate channel implementation
/// based on the platform type.
pub fn create_channel(config: ChannelConfig) -> Result<Box<dyn Channel>, ChannelError> {
    match config.platform {
        Platform::Repl => Ok(Box::new(ReplChannel::new(config))),
        Platform::Webhook => Ok(Box::new(WebhookChannel::new(config))),
        Platform::P2p => Ok(Box::new(P2pChannel::new(config))),
        Platform::Telegram => {
            TelegramChannel::new(config).map(|ch| Box::new(ch) as Box<dyn Channel>)
        }
        Platform::Discord => Err(ChannelError::PlatformError(
            "Discord channel requires the ironclaw adapter (ironclaw/channels-src/discord). \
             Install it with `peerclaw skill install ironclaw-discord` or use a webhook channel instead."
                .into(),
        )),
        Platform::Slack => Err(ChannelError::PlatformError(
            "Slack channel requires the ironclaw adapter (ironclaw/channels-src/slack). \
             Install it with `peerclaw skill install ironclaw-slack` or use a webhook channel instead."
                .into(),
        )),
        Platform::Matrix => Err(ChannelError::PlatformError(
            "Matrix channel requires the ironclaw adapter (ironclaw/channels-src/matrix). \
             Install it with `peerclaw skill install ironclaw-matrix` or use a webhook channel instead."
                .into(),
        )),
        Platform::WebSocket => {
            // WebSocket is handled by the web module
            Err(ChannelError::PlatformError(
                "WebSocket channels are created through the web server".into(),
            ))
        }
        Platform::Wasm => Err(ChannelError::PlatformError(
            "WASM channels require a module path in settings".into(),
        )),
    }
}

/// WASM channel loader (placeholder).
///
/// In production, this would load a WASM component that implements
/// the Channel trait through the component model.
pub struct WasmChannelLoader {
    /// Path to the WASM module.
    pub module_path: std::path::PathBuf,
}

impl WasmChannelLoader {
    /// Load a WASM channel module.
    pub fn load(&self, config: ChannelConfig) -> Result<Box<dyn Channel>, ChannelError> {
        let _ = config;
        // This would use wasmtime to load and instantiate the component
        Err(ChannelError::PlatformError(
            "WASM channel loading not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_repl_channel() {
        let config = ChannelConfig {
            platform: Platform::Repl,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = create_channel(config);
        assert!(channel.is_ok());
    }

    #[test]
    fn test_create_telegram_channel_requires_settings() {
        // Without settings, creation should fail with a clear error
        let config = ChannelConfig {
            platform: Platform::Telegram,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = create_channel(config);
        assert!(channel.is_err());
    }

    #[test]
    fn test_create_telegram_channel_with_token() {
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

        let channel = create_channel(config);
        assert!(channel.is_ok());
    }

    #[test]
    fn test_discord_returns_ironclaw_message() {
        let config = ChannelConfig {
            platform: Platform::Discord,
            name: "test".to_string(),
            ..Default::default()
        };

        match create_channel(config) {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("ironclaw"),
                    "Error should mention ironclaw adapter: {msg}"
                );
            }
            Ok(_) => panic!("Discord channel should not be created without ironclaw"),
        }
    }
}
