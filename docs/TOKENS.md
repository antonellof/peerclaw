# Token Economy

The **PCLAW** token is the internal accounting unit for P2P job execution. Agents spend tokens to run tasks on remote nodes; providers earn tokens by serving them.

> **Current scope:** local accounting only. Tokens track resource usage across the mesh but do not settle on-chain. The focus is on making P2P agent job execution work end-to-end.

## Overview

| Property | Value |
|----------|-------|
| **Symbol** | PCLAW |
| **Precision** | 6 decimals (1 PCLAW = 1,000,000 μPCLAW) |
| **Initial balance** | 1,000 PCLAW per new wallet |

## How Tokens Flow

```
Agent submits job → Escrow locks tokens → Provider executes →
  Success → Provider credited, escrow released
  Failure → Agent refunded
  Timeout → Auto-refunded (swept every 60s)
```

Each node has a single wallet (Ed25519 keypair). Balance is persisted to redb across restarts.

## Pricing

Base rates (per 1K tokens for inference, flat for tools):

| Service | Cost |
|---------|------|
| Inference (small model) | 0.5 PCLAW |
| Inference (medium 30B+) | 2.0 PCLAW |
| Inference (large 70B+) | 5.0 PCLAW |
| Web fetch | 0.1 PCLAW |
| WASM tool call | 0.02 PCLAW |
| Vector search | 0.05 PCLAW |

Providers can set a `price_multiplier` to adjust rates.

## Agent Budgets

Agents enforce four spending limits (in PCLAW):

```toml
[budget]
per_request = 10.0   # ~20K tokens at small model rate
per_hour = 100.0
per_day = 500.0
total = 5000.0
```

The agent ReAct loop checks budget before each LLM call.

## CLI

```bash
peerclaw wallet balance            # Show available / in-escrow / total
peerclaw wallet history            # Recent transactions
peerclaw serve --share-inference   # Earn tokens by serving LLM requests
```

## What's Deferred

These are planned but not yet implemented:

- Cross-node settlement (provider on remote peer credited via signed GossipSub message)
- Reputation system (bid scoring uses reputation weight, but no tracking yet)
- Staking / slashing
- On-chain settlement
- Payment channels

---

*Simplified March 2026 — focus is P2P agent job execution, not financial infrastructure.*
