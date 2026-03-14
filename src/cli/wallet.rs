//! `peerclawd wallet` commands - Wallet operations.

use clap::Subcommand;
use std::path::PathBuf;

use crate::bootstrap;
use crate::identity::NodeIdentity;

#[derive(Subcommand)]
pub enum WalletCommand {
    /// Generate new keypair and wallet
    Create {
        /// Output path for the wallet file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Check token balance
    Balance,

    /// Transfer tokens
    Send {
        /// Recipient peer ID or address
        #[arg(value_name = "TO")]
        to: String,

        /// Amount to send
        #[arg(value_name = "AMOUNT")]
        amount: u64,
    },

    /// Transaction history
    History,

    /// Show wallet info (peer ID, public key)
    Info,
}

pub async fn run(cmd: WalletCommand) -> anyhow::Result<()> {
    match cmd {
        WalletCommand::Create { output } => {
            let path = output.unwrap_or_else(|| bootstrap::base_dir().join("identity.key"));

            if path.exists() {
                anyhow::bail!("Wallet already exists at {:?}. Use --output to specify a different path.", path);
            }

            // Generate new identity
            let identity = NodeIdentity::generate();

            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Save to file
            identity.save(&path)?;

            println!("Wallet created successfully!");
            println!("  Peer ID: {}", identity.peer_id());
            println!("  Saved to: {:?}", path);
        }
        WalletCommand::Balance => {
            // TODO: Query balance from network
            println!("Balance: 0 tokens");
            println!("(Token economy not yet implemented)");
        }
        WalletCommand::Send { to, amount } => {
            // TODO: Send tokens
            println!("Sending {} tokens to {}...", amount, to);
            println!("(Token transfers not yet implemented)");
        }
        WalletCommand::History => {
            // TODO: Show transaction history
            println!("Transaction History");
            println!("-------------------");
            println!("No transactions yet");
        }
        WalletCommand::Info => {
            let identity_path = bootstrap::base_dir().join("identity.key");

            if !identity_path.exists() {
                println!("No wallet found. Run 'peerclawd wallet create' to create one.");
                return Ok(());
            }

            let identity = NodeIdentity::load(&identity_path)?;
            println!("Wallet Info");
            println!("-----------");
            println!("Peer ID: {}", identity.peer_id());
            println!("Public Key: {}", hex::encode(identity.public_key_bytes()));
        }
    }

    Ok(())
}
