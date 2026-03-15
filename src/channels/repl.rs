//! REPL channel for command-line interaction.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::{Channel, ChannelCapabilities, IncomingMessage, OutgoingResponse};

/// REPL channel for interactive command-line use
pub struct ReplChannel {
    name: String,
    running: Arc<std::sync::atomic::AtomicBool>,
    prompt: String,
    response_buffer: Arc<RwLock<Vec<String>>>,
}

impl ReplChannel {
    /// Create a new REPL channel
    pub fn new() -> Self {
        Self {
            name: "repl".to_string(),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            prompt: "> ".to_string(),
            response_buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set custom prompt
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = prompt.to_string();
        self
    }
}

impl Default for ReplChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for ReplChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Interactive command-line interface"
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        let running = self.running.clone();
        let prompt = self.prompt.clone();
        let response_buffer = self.response_buffer.clone();

        // Spawn blocking read loop
        tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let handle = stdin.lock();

            for line in handle.lines() {
                if !running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                match line {
                    Ok(input) => {
                        let input = input.trim();
                        if input.is_empty() {
                            continue;
                        }

                        // Handle exit commands
                        if matches!(input.to_lowercase().as_str(), "exit" | "quit" | "/quit" | "/exit") {
                            running.store(false, std::sync::atomic::Ordering::SeqCst);
                            break;
                        }

                        // Create message
                        let msg = IncomingMessage::new("repl", "local", input);

                        // Send to agent
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }

                        // Wait for and print responses
                        std::thread::sleep(std::time::Duration::from_millis(100));

                        // Print buffered responses
                        {
                            let mut buffer = response_buffer.write();
                            for response in buffer.drain(..) {
                                println!("{}", response);
                            }
                        }

                        // Print prompt
                        print!("{}", prompt);
                        let _ = io::stdout().flush();
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to read input");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    async fn send(&self, response: OutgoingResponse) -> anyhow::Result<()> {
        let mut buffer = self.response_buffer.write();
        buffer.push(response.content);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            can_send_text: true,
            can_send_attachments: false,
            can_receive_attachments: false,
            supports_threading: false,
            supports_streaming: true,
            supports_rich_format: true,
            max_message_length: 1_000_000,
            rate_limit: None,
        }
    }
}
