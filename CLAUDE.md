# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PeerClaw is a fully decentralized peer-to-peer AI agent network written in Rust. It ships as a single static binary where autonomous AI agents collaborate, share resources, and transact using a native token economy.

**Current Status:** v0.2 - Production-ready with full P2P networking, local inference, vector memory, skills system, and multi-platform messaging.

## Build Commands

```bash
# Build for current platform
cargo build --release

# Build static Linux binary
cargo build --release --target x86_64-unknown-linux-musl

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt

# Run single test
cargo test test_name

# Run tests in specific module
cargo test module_name::

# Run the binary
./target/release/peerclaw --help
```

## Architecture

### Single Binary Design

One statically-linked binary operates in multiple modes based on flags/subcommands. Every peer runs the same binary - roles (resource provider, agent host, gateway) are determined at runtime.

**CLI Structure:**
- `peerclaw serve` - Start peer node (with `--gpu`, `--storage`, `--web` flags)
- `peerclaw run <model>` - Ollama-style interactive chat
- `peerclaw chat` - Full-featured chat with slash commands
- `peerclaw models list|download` - Model management
- `peerclaw agent run|list|logs|stop` - Agent management
- `peerclaw network status|peers|discover` - Network operations
- `peerclaw wallet create|balance|send|history` - Token wallet
- `peerclaw tool build|install|list` - WASM tool management
- `peerclaw skill list|install|search` - Skill management
- `peerclaw vector create|insert|search` - Vector database
- `peerclaw job submit|status|list` - Job marketplace

### Core Modules

| Module | Location | Purpose |
|--------|----------|---------|
| Node | `src/node.rs` | Orchestrates all subsystems |
| P2P Network | `src/p2p/` | libp2p networking (Kademlia, GossipSub, mDNS) |
| Inference | `src/inference/` | GGUF model loading, caching, batch processing |
| Vector Store | `src/vector/` | vectX semantic search (HNSW, BM25, hybrid) |
| Job Manager | `src/job/` | Request/bid/execute/settle workflow |
| Wallet | `src/wallet/` | PCLAW token accounting, escrow |
| Tools | `src/tools/` | Builtin tools, WASM sandbox |
| Skills | `src/skills/` | SKILL.md prompt extensions |
| Safety | `src/safety/` | Leak detection, injection defense |
| Messaging | `src/messaging/` | Multi-platform channels |
| MCP | `src/mcp/` | Model Context Protocol client |
| Executor | `src/executor/` | Local/remote task routing |
| Web | `src/web/` | Dashboard, OpenAI-compatible API |

### Key Dependencies

| Subsystem | Crate |
|-----------|-------|
| Async Runtime | `tokio` |
| P2P Networking | `libp2p` 0.54 |
| Vector Database | `vectx` |
| WASM Sandbox | `wasmtime` 28.x |
| HTTP/Web | `axum` 0.7 |
| Database | `redb` 2.x |
| Serialization | `serde` + `rmp-serde` (MessagePack) |
| Crypto | `ed25519-dalek` 2.x, `blake3` |
| AI Inference | `llama-cpp-2` 0.1 |
| CLI | `clap` 4.x |
| Logging | `tracing` |

### Security Model

- WASM sandbox for untrusted tools with explicit capability grants
- Safety layer: leak detection, prompt injection defense, content policy
- Secrets injected at host boundary, never exposed to agent code
- All P2P communication encrypted via Noise protocol
- Ed25519 signatures on all messages
- Skill trust levels: Local > Installed > Network

### Agent Specification

Agents are defined in TOML files (`agent.toml`) specifying:
- Identity and model configuration
- Budget limits (per-request, per-hour, per-day, total)
- Capabilities (web_access, storage, tool_building, vector_memory)
- Allowed hosts for web access
- Tools (builtin + WASM + MCP)
- Skills (local + installed + network)
- Channels (REPL, webhook, websocket, Telegram, Discord, Slack)
- Routines (cron schedules, heartbeats, startup tasks)
- Memory (vector collection, embedding model)

### IronClaw Integration

The `ironclaw/` directory contains additional tools and channel adapters:
- `tools-src/` - 15+ external tools (Google, GitHub, Telegram, etc.)
- `channels-src/` - Platform adapters (Discord, Telegram, Slack, WhatsApp)

## Development Status

### Implemented (v0.2)
- [x] P2P networking with libp2p
- [x] GGUF inference with GPU acceleration
- [x] Job marketplace protocol
- [x] Token wallet with escrow
- [x] OpenAI-compatible API
- [x] Claude-Code-style CLI
- [x] Web dashboard
- [x] Batch aggregation
- [x] Vector memory (vectX)
- [x] Skills system (SKILL.md)
- [x] Safety layer
- [x] MCP integration
- [x] Multi-platform messaging

### Planned (v0.3)
- [ ] Distributed inference (pipeline parallelism)
- [ ] Dynamic WASM tool building
- [ ] Multi-agent collaboration
- [ ] Reputation system

### Future (v1.0)
- [ ] On-chain settlement
- [ ] Public tool registry
- [ ] Governance
- [ ] Firecracker microVM isolation
