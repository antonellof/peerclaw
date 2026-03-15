# PeerClaw Quickstart Guide

Get up and running with PeerClaw in minutes.

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourorg/peerclaw.git
cd peerclaw

# Build release binary
cargo build --release

# Binary is at ./target/release/peerclaw
```

### Verify Installation

```bash
./target/release/peerclaw version
# peerclaw 0.1.0
```

## Quick Start

### 1. Create a Wallet

Every peer needs an identity (Ed25519 keypair). Create one:

```bash
peerclaw wallet create
```

Output:
```
✓ Wallet created successfully!
  Address: 12D3KooWQL62BcJz9zqRNRnDkKfYiHSdSUG5n7LZ4xRZBPPDT9at
  Keyfile: ~/.peerclaw/identity.key
  Balance: 0.000000 PCLAW
```

### 2. Start a Node

Start your peer node to join the network:

```bash
peerclaw serve
```

Output:
```
INFO  Starting PeerClaw node...
INFO  Peer ID: 12D3KooWQL62BcJz9zqRNRnDkKfYiHSdSUG5n7LZ4xRZBPPDT9at
INFO  Listening on /ip4/0.0.0.0/tcp/0
INFO  Node running. Press Ctrl+C to stop.
```

### 3. Check Wallet Balance

```bash
peerclaw wallet balance
```

Output:
```
Wallet Balance
--------------
  Available:      0.000000 PCLAW
  In escrow:      0.000000 PCLAW
  Staked:         0.000000 PCLAW
  ─────────────────────────
  Total:          0.000000 PCLAW
```

## Running Multiple Nodes (P2P Testing)

To test P2P features locally, run multiple nodes with separate data directories:

### Terminal 1 - Node A

```bash
# Create directory and wallet for Node A
mkdir -p /tmp/peerclaw-node-a
PEERCLAWD_HOME=/tmp/peerclaw-node-a peerclaw wallet create

# Start Node A
PEERCLAWD_HOME=/tmp/peerclaw-node-a peerclaw serve
```

### Terminal 2 - Node B

```bash
# Create directory and wallet for Node B
mkdir -p /tmp/peerclaw-node-b
PEERCLAWD_HOME=/tmp/peerclaw-node-b peerclaw wallet create

# Start Node B
PEERCLAWD_HOME=/tmp/peerclaw-node-b peerclaw serve
```

The nodes will automatically discover each other via mDNS on the local network.

## CLI Commands Reference

### Wallet Commands

```bash
# Create new wallet
peerclaw wallet create

# Show wallet info
peerclaw wallet info

# Check balance
peerclaw wallet balance

# Send tokens
peerclaw wallet send <RECIPIENT_ADDRESS> <AMOUNT>

# View transaction history
peerclaw wallet history

# Stake tokens as resource provider
peerclaw wallet stake <AMOUNT>

# Unstake tokens
peerclaw wallet unstake <AMOUNT>

# Show active escrows
peerclaw wallet escrows
```

### Network Commands

```bash
# Show network status
peerclaw network status

# List connected peers
peerclaw network peers

# Force peer discovery
peerclaw network discover
```

### Node Commands

```bash
# Start node with default settings
peerclaw serve

# Start with web UI enabled
peerclaw serve --web

# Start with GPU resources advertised
peerclaw serve --gpu

# Start with storage contribution
peerclaw serve --storage 50GB
```

### Agent Commands

```bash
# Run an agent from spec file
peerclaw agent run agent.toml

# List running agents
peerclaw agent list

# View agent logs
peerclaw agent logs <AGENT_ID>

# Stop an agent
peerclaw agent stop <AGENT_ID>
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PEERCLAWD_HOME` | Base directory for data | `~/.peerclaw` |
| `PEERCLAWD_WEB_ENABLED` | Enable web dashboard | `false` |
| `PEERCLAWD_WEB_ADDR` | Web server address | `127.0.0.1:8080` |
| `PEERCLAWD_BOOTSTRAP` | Bootstrap peer addresses | (empty) |

### Config File

Create `~/.peerclaw/config.toml`:

```toml
[p2p]
# Listen addresses for P2P connections
listen_addresses = ["/ip4/0.0.0.0/tcp/0"]
# Bootstrap peers to connect to
bootstrap_peers = []
# Enable local network discovery
mdns_enabled = true

[web]
# Enable embedded web dashboard
enabled = false
# Dashboard listen address
listen_addr = "127.0.0.1:8080"

[resources]
# Advertise GPU resources
advertise_gpu = false

[database]
# Database file path
path = "~/.peerclaw/data/peerclaw.redb"
```

## Example: Agent Configuration

Create an agent specification in `agent.toml`:

```toml
[agent]
name = "my-assistant"
version = "0.1.0"
description = "A helpful AI assistant"

[model]
provider = "network"
model = "llama-3.2-8b"
max_tokens_per_request = 2048

[budget]
max_spend_per_hour = 100
max_spend_total = 1000

[capabilities]
web_access = true
storage = true

[web_access]
allowed_hosts = ["*.wikipedia.org", "arxiv.org"]
max_requests_per_minute = 10

[tools]
builtin = ["web_fetch", "web_search"]

[channels]
repl = true
```

Run the agent:

```bash
peerclaw agent run agent.toml
```

## Troubleshooting

### Node won't start

Check if another instance is running:
```bash
ps aux | grep peerclaw
```

### Peers not discovering each other

1. Ensure both nodes are on the same network
2. Check firewall settings - mDNS uses UDP port 5353
3. Verify mDNS is enabled in config

### Database errors

Reset the database:
```bash
rm -rf ~/.peerclaw/data/peerclaw.redb
```

## Next Steps

- Read the [Token Economy Spec](../PEERCLAWD-TOKEN-ECONOMY.md) to understand PCLAW tokens
- Explore the [README](../README.md) for architecture details
- Join the network and start contributing resources!

---

*PeerClaw v0.1 - Quickstart Guide*
