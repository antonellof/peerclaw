//! Channel manager for coordinating multiple input channels.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::{Channel, IncomingMessage, OutgoingResponse};

/// Channel manager
pub struct ChannelManager {
    /// Registered channels
    channels: Arc<RwLock<HashMap<String, Arc<dyn Channel>>>>,
    /// Message sender for incoming messages
    message_tx: mpsc::Sender<IncomingMessage>,
    /// Running state
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new(message_tx: mpsc::Sender<IncomingMessage>) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Register a channel
    pub fn register(&self, channel: Arc<dyn Channel>) {
        let name = channel.name().to_string();
        let mut channels = self.channels.write();
        channels.insert(name.clone(), channel);
        tracing::info!(channel = %name, "Registered channel");
    }

    /// Unregister a channel
    pub fn unregister(&self, name: &str) -> Option<Arc<dyn Channel>> {
        let mut channels = self.channels.write();
        let channel = channels.remove(name);
        if channel.is_some() {
            tracing::info!(channel = %name, "Unregistered channel");
        }
        channel
    }

    /// Get a channel by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Channel>> {
        let channels = self.channels.read();
        channels.get(name).cloned()
    }

    /// List all channel names
    pub fn list(&self) -> Vec<String> {
        let channels = self.channels.read();
        channels.keys().cloned().collect()
    }

    /// Start all channels
    pub async fn start_all(&self) -> anyhow::Result<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let channels: Vec<_> = {
            let channels = self.channels.read();
            channels.values().cloned().collect()
        };

        for channel in channels {
            let name = channel.name().to_string();
            let tx = self.message_tx.clone();
            let name_for_spawn = name.clone();

            tokio::spawn(async move {
                if let Err(e) = channel.start(tx).await {
                    tracing::error!(channel = %name_for_spawn, error = %e, "Channel start failed");
                }
            });

            tracing::info!(channel = %name, "Started channel");
        }

        Ok(())
    }

    /// Stop all channels
    pub async fn stop_all(&self) -> anyhow::Result<()> {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        let channels: Vec<_> = {
            let channels = self.channels.read();
            channels.values().cloned().collect()
        };

        for channel in channels {
            let name = channel.name().to_string();
            if let Err(e) = channel.stop().await {
                tracing::error!(channel = %name, error = %e, "Channel stop failed");
            } else {
                tracing::info!(channel = %name, "Stopped channel");
            }
        }

        Ok(())
    }

    /// Send a response to the appropriate channel
    pub async fn send(&self, response: OutgoingResponse) -> anyhow::Result<()> {
        let channel = self
            .get(&response.channel)
            .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", response.channel))?;

        channel.send(response).await
    }

    /// Health check all channels
    pub async fn health_check(&self) -> HashMap<String, bool> {
        let channels: Vec<_> = {
            let channels = self.channels.read();
            channels
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        let mut results = HashMap::new();
        for (name, channel) in channels {
            let healthy = channel.health_check().await.is_ok();
            results.insert(name, healthy);
        }

        results
    }

    /// Check if manager is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get channel count
    pub fn channel_count(&self) -> usize {
        self.channels.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockChannel {
        name: String,
    }

    #[async_trait::async_trait]
    impl Channel for MockChannel {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Mock channel for testing"
        }

        async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send(&self, _response: OutgoingResponse) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_channel_manager() {
        let (tx, _rx) = mpsc::channel(100);
        let manager = ChannelManager::new(tx);

        let channel = Arc::new(MockChannel {
            name: "test".to_string(),
        });

        manager.register(channel);
        assert_eq!(manager.channel_count(), 1);
        assert!(manager.get("test").is_some());

        manager.unregister("test");
        assert_eq!(manager.channel_count(), 0);
    }
}
