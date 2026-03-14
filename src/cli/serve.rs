//! `peerclawd serve` command - Start a peer node.

use clap::Args;
use std::net::SocketAddr;

use crate::config::Config;
use crate::node::Node;

#[derive(Args)]
pub struct ServeArgs {
    /// Advertise GPU resources
    #[arg(long)]
    pub gpu: bool,

    /// Limit CPU contribution (number of cores)
    #[arg(long)]
    pub cpu: Option<u16>,

    /// Allocate distributed storage (e.g., "50GB")
    #[arg(long)]
    pub storage: Option<String>,

    /// Enable embedded web UI on this address
    #[arg(long, value_name = "ADDR")]
    pub web: Option<SocketAddr>,

    /// Join existing network via known peer
    #[arg(long, value_name = "MULTIADDR")]
    pub bootstrap: Option<String>,

    /// Path to wallet keyfile
    #[arg(long, value_name = "PATH")]
    pub wallet: Option<std::path::PathBuf>,

    /// Listen address for P2P (default: /ip4/0.0.0.0/tcp/0)
    #[arg(long, value_name = "MULTIADDR")]
    pub listen: Option<String>,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    tracing::info!("Starting PeerClaw'd node...");

    // Load configuration
    let mut config = Config::load()?;

    // Apply CLI overrides
    if let Some(web_addr) = args.web {
        config.web.enabled = true;
        config.web.listen_addr = web_addr;
    }

    if let Some(bootstrap) = args.bootstrap {
        config.p2p.bootstrap_peers.push(bootstrap);
    }

    if let Some(listen) = args.listen {
        config.p2p.listen_addresses.push(listen);
    }

    if args.gpu {
        config.resources.advertise_gpu = true;
    }

    if let Some(cpu) = args.cpu {
        config.resources.cpu_cores = Some(cpu);
    }

    // Create and run the node
    let mut node = Node::new(config).await?;
    node.run().await?;

    Ok(())
}
