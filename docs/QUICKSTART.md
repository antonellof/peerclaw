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
# peerclaw 0.2.0
```

## Quick Start

### 1. Download a Model

```bash
mkdir -p ~/.peerclaw/models

# Llama 3.2 1B (~770MB) - fast, good for testing
curl -L -o ~/.peerclaw/models/llama-3.2-1b-instruct-q4_k_m.gguf \
  "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf"

# Llama 3.2 3B (~2GB) - better quality
curl -L -o ~/.peerclaw/models/llama-3.2-3b-instruct-q4_k_m.gguf \
  "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf"
```

### 2. Run Interactive Chat

```bash
# Ollama-style quick chat
peerclaw run llama-3.2-1b

# Full-featured chat with slash commands
peerclaw chat
```

### 3. Create a Wallet

Every peer needs an identity (Ed25519 keypair):

```bash
peerclaw wallet create
```

Output:
```
Wallet created successfully!
  Address: 12D3KooWQL62BcJz9zqRNRnDkKfYiHSdSUG5n7LZ4xRZBPPDT9at
  Keyfile: ~/.peerclaw/identity.key
  Balance: 0.000000 PCLAW
```

### 4. Start a Node

Start your peer node to join the network:

```bash
# Basic node
peerclaw serve

# With web dashboard
peerclaw serve --web 127.0.0.1:8080

# As a resource provider (accept jobs)
peerclaw serve --provider
```

Output:
```
INFO  Starting PeerClaw node...
INFO  Peer ID: 12D3KooWQL62BcJz9zqRNRnDkKfYiHSdSUG5n7LZ4xRZBPPDT9at
INFO  Listening on /ip4/0.0.0.0/tcp/0
INFO  Web dashboard at http://127.0.0.1:8080
INFO  Node running. Press Ctrl+C to stop.
```

## Chat Mode

The full-featured chat mode supports slash commands:

```bash
peerclaw chat
```

### Slash Commands

| Command | Description |
|---------|-------------|
| `/help` | Show all commands |
| `/model <name>` | Switch model |
| `/temperature <n>` | Set temperature (0.0-2.0) |
| `/max_tokens <n>` | Set max output tokens |
| `/settings` | Open settings menu |
| `/status` | Show runtime status |
| `/peers` | List connected peers |
| `/balance` | Show token balance |
| `/tools` | List available tools |
| `/tool_exec <name> <args>` | Execute a tool |
| `/distributed <on\|off>` | Toggle network inference |
| `/stream <on\|off>` | Toggle streaming output |
| `/clear` | Clear conversation history |
| `/history` | Show conversation history |
| `/export <file>` | Export conversation |

## Vector Memory

Store and search information semantically:

```bash
# Create a collection
peerclaw vector create notes

# Insert data
peerclaw vector insert notes "PeerClaw uses libp2p for networking"
peerclaw vector insert notes "WASM sandbox provides tool isolation"
peerclaw vector insert notes "Vector search uses HNSW indexing"

# Semantic search
peerclaw vector search notes "how does networking work" -k 3
```

## Skills

Install and manage prompt extensions:

```bash
# List installed skills
peerclaw skill list

# Install a skill
peerclaw skill install ./skills/code-review.md

# Search network for skills
peerclaw skill search "data analysis"
```

## CLI Commands Reference

### Model Commands

```bash
peerclaw models list              # List downloaded models
peerclaw models download <model>  # Download from HuggingFace
peerclaw pull <model>             # Alias for download
```

### Wallet Commands

```bash
peerclaw wallet create            # Create new wallet
peerclaw wallet info              # Show wallet info
peerclaw wallet balance           # Check balance
peerclaw wallet send <addr> <amt> # Send tokens
peerclaw wallet history           # Transaction history
peerclaw wallet escrows           # Show active escrows
```

### Network Commands

```bash
peerclaw network status           # Show network status
peerclaw peers list               # List connected peers
peerclaw network discover         # Force peer discovery
```

### Job Commands

```bash
peerclaw job submit <spec>        # Submit job to network
peerclaw job status <id>          # Check job status
peerclaw job list                 # List active jobs
```

### Agent Commands

```bash
peerclaw agent run agent.toml     # Run an agent
peerclaw agent list               # List running agents
peerclaw agent logs <id>          # View agent logs
peerclaw agent stop <id>          # Stop an agent
peerclaw agent attach <id>        # Attach to agent REPL
```

### Tool Commands

```bash
peerclaw tool list                # List available tools
peerclaw tool info <name>         # Show tool details
peerclaw tool build <path>        # Build WASM tool
peerclaw tool install <path>      # Install WASM tool
```

## Running Multiple Nodes (P2P Testing)

To test P2P features locally, run multiple nodes with separate data directories:

### Terminal 1 - Node A

```bash
mkdir -p /tmp/peerclaw-node-a
PEERCLAW_HOME=/tmp/peerclaw-node-a peerclaw wallet create
PEERCLAW_HOME=/tmp/peerclaw-node-a peerclaw serve --web 127.0.0.1:8080
```

### Terminal 2 - Node B

```bash
mkdir -p /tmp/peerclaw-node-b
PEERCLAW_HOME=/tmp/peerclaw-node-b peerclaw wallet create
PEERCLAW_HOME=/tmp/peerclaw-node-b peerclaw serve --web 127.0.0.1:8081
```

Nodes automatically discover each other via mDNS on the local network.

### Test Cluster (Automated)

```bash
peerclaw test cluster --nodes 3
```

This spawns 3 nodes with separate data directories for testing.

## OpenAI-Compatible API

Start a node with the web server enabled:

```bash
peerclaw serve --web 127.0.0.1:8080
```

Use any OpenAI SDK:

```python
from openai import OpenAI

client = OpenAI(base_url="http://localhost:8080/v1", api_key="unused")
response = client.chat.completions.create(
    model="llama-3.2-3b",
    messages=[{"role": "user", "content": "Hello!"}],
    stream=True
)
for chunk in response:
    print(chunk.choices[0].delta.content, end="")
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PEERCLAW_HOME` | Base directory for data | `~/.peerclaw` |
| `PEERCLAW_LOG` | Log level | `info` |

### Config File

Create `~/.peerclaw/config.toml`:

```toml
[p2p]
listen_addresses = ["/ip4/0.0.0.0/tcp/0"]
bootstrap_peers = []
mdns_enabled = true

[web]
enabled = false
listen_addr = "127.0.0.1:8080"

[inference]
models_path = "~/.peerclaw/models"
default_model = "llama-3.2-3b"

[vector]
embedding_dim = 384
persistence_path = "~/.peerclaw/vector"

[safety]
leak_detection = true
injection_defense = true
```

## Example: Agent Configuration

Create an agent specification in `agent.toml`:

```toml
[agent]
name = "my-assistant"
version = "0.1.0"
description = "A helpful AI assistant"

[model]
provider = "local"
model = "llama-3.2-3b"
max_tokens_per_request = 2048

[budget]
max_spend_per_hour = 100
max_spend_total = 1000

[capabilities]
web_access = true
vector_memory = true

[web_access]
allowed_hosts = ["*.wikipedia.org", "arxiv.org"]
max_requests_per_minute = 10

[tools]
builtin = ["web_fetch", "memory_search", "memory_write"]

[channels]
repl = true

[memory]
collection = "assistant-memory"
auto_persist = true
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

### Model loading fails

1. Check model file exists in `~/.peerclaw/models/`
2. Ensure enough RAM for model size
3. Check file isn't corrupted (re-download)

### Database errors

Reset the database:
```bash
rm -rf ~/.peerclaw/data/peerclaw.redb
```

### Out of memory

Reduce model size or use quantized versions:
- Q4_K_M: Good balance of quality and memory
- Q4_0: Smaller, slightly lower quality
- Q8_0: Higher quality, more memory

## Next Steps

- Read the [Architecture](ARCHITECTURE.md) for system design
- Explore [Token Economy](TOKENS.md) to understand PCLAW tokens
- Check [Security](SECURITY.md) for safety features
- Learn about [Agents](AGENTS.md) for autonomous AI deployment

---

*PeerClaw v0.2 — Quickstart Guide*
