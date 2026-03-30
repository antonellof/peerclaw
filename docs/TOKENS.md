# Token Economy

The PeerClaw token (**PCLAW**) is the native utility token that powers every transaction in the network. Agents spend PCLAW to consume resources, peers earn PCLAW by providing them.

## Token Overview

| Property | Value |
|----------|-------|
| **Name** | PeerClaw Token |
| **Symbol** | PCLAW |
| **Decimals** | 6 (smallest unit: 1 μPCLAW) |
| **Max Supply** | 1,000,000,000 |

## Token Utility

| Use Case | Description |
|----------|-------------|
| **Inference** | Pay peers for LLM inference |
| **Storage** | Rent distributed storage |
| **Web Access** | Token-gated web scraping |
| **Tool Execution** | Pay for WASM tool runs |
| **Vector Search** | Pay for semantic search queries |
| **Staking** | Stake to become verified provider |

## Wallet Architecture

Every entity in the network has a wallet (Ed25519 keypair):

```bash
$ peerclaw wallet create
  Wallet created
  Address:  12D3KooW...
  Keyfile:  ~/.peerclaw/identity.key
  Balance:  0.000000 PCLAW

$ peerclaw wallet balance
  Available:   1,250.00 PCLAW
  In escrow:     180.00 PCLAW  (3 active jobs)
  Staked:      5,000.00 PCLAW
  Total:       6,430.00 PCLAW
```

### Wallet Types

| Type | Owner | Purpose |
|------|-------|---------|
| **Peer Wallet** | Human operator | Receives rewards, pays for services, holds stake |
| **Agent Wallet** | Autonomous AI | Spends on inference, storage, tools |
| **Operator Wallet** | Human deployer | Funds agent wallets, sets limits |
| **Escrow Wallet** | System | Temporary hold during job execution |

## Pricing Model

Each peer sets its own pricing. Agents choose based on price, latency, and reputation.

### Indicative Costs

| Service | Unit | Cost |
|---------|------|------|
| LLM Inference (small, 7B-13B) | 1K tokens | 0.5 PCLAW |
| LLM Inference (medium, 30B-70B) | 1K tokens | 2.0 PCLAW |
| LLM Inference (large, 70B+) | 1K tokens | 5.0 PCLAW |
| Embedding Generation | 1K tokens | 0.2 PCLAW |
| Web Fetch | per request | 0.1 PCLAW |
| Web Search | per query | 0.5 PCLAW |
| Vector Search | per query | 0.05 PCLAW |
| Storage Write | per MB | 0.01 PCLAW |
| Storage Read | per MB | 0.005 PCLAW |
| WASM Tool Execution | per call | 0.02 PCLAW |

### Earning Rates

| Resource | Unit | Rate |
|----------|------|------|
| CPU | core-hour | 2.0 PCLAW |
| GPU (consumer) | GPU-hour | 15.0 PCLAW |
| GPU (datacenter) | GPU-hour | 40.0 PCLAW |
| Storage | GB-month | 0.5 PCLAW |
| Bandwidth/Relay | GB | 0.3 PCLAW |
| Uptime Bonus | per day | 1.0 PCLAW |

## Payment Flow

```
1. Agent signs JobRequest with budget and SLA
2. Matching peer accepts → tokens moved to Escrow
3. Peer executes job
4. Result delivered → Agent verifies
5a. Success → Escrow released, provider credited
5b. Failure → Escrow refunds to agent
5c. Timeout → Expired escrows swept every 60s, tokens auto-refunded
```

### Current Implementation Status

| Component | Status |
|-----------|--------|
| Wallet (credit, debit, balance, persistence) | Implemented |
| Escrow (create, release, refund) | Implemented |
| Expired escrow sweep (periodic refund) | Implemented |
| Job marketplace (request, bid, accept, settle) | Implemented |
| Provider settlement (local) | Implemented |
| Agent budget enforcement | Implemented |
| Initial balance (1,000 PCLAW for new wallets) | Implemented |
| Reputation-weighted bid scoring | Partial (scoring works, reputation tracking stub) |
| Provider sharing earnings | Planned (rate limits work, no token credit yet) |
| P2P cross-node settlement | Planned (local settlement works) |
| On-chain settlement | Future (v1.0) |
| Slashing | Future (v1.0) |

## Wallet Configuration

```toml
[wallet]
keyfile = "~/.peerclaw/wallet/default.key"

[wallet.spending]
max_daily_spend = 2000.0
reserve_balance = 1000.0
max_single_transaction = 100.0

[wallet.staking]
amount = 5000.0
auto_restake_rewards = true
```

### Agent Budget

```toml
[budget]
per_request = 10.0    # Max PCLAW per single task (~20K tokens at small model rate)
per_hour = 100.0
per_day = 500.0
total = 5000.0
```

Budget enforcement runs inside the agent ReAct loop — each LLM call estimates cost as `(tokens / 1000) × 0.5 PCLAW` and checks against all four limits before proceeding.

## Reputation System

Reputation affects earning potential and job assignment:

| Factor | Weight |
|--------|--------|
| Job Completion Rate | 30% |
| Result Accuracy | 25% |
| Latency Performance | 15% |
| Uptime | 15% |
| Stake Weight | 10% |
| Age | 5% |

### Reputation Tiers

| Score | Tier | Effect |
|-------|------|--------|
| 0.0–0.3 | Untrusted | Full verification, low priority |
| 0.3–0.6 | Standard | Sampled verification |
| 0.6–0.8 | Trusted | Optimistic execution, 1.2× rewards |
| 0.8–1.0 | Elite | Skip verification, 1.5× rewards |

## Slashing

Misbehavior results in stake loss:

| Offense | Penalty |
|---------|---------|
| Failed delivery | 1% of stake |
| Incorrect result | 2% of stake |
| Repeated failures (>5/24h) | 10% + 24h suspension |
| Malicious behavior | 100% + permanent ban |

## Local Accounting

Tokens are tracked locally per-node with persistent wallet state (redb). Current settlement model:

- **Local jobs** — Escrow created on requester wallet, released on success (tokens credited back to same wallet as "job_payment"), refunded on failure
- **Remote jobs** — Provider node credits itself upon receiving a signed settlement message (planned — currently providers on remote peers are not credited)
- **Expired escrows** — Background sweep every 60 seconds auto-refunds tokens locked in timed-out escrows
- **New wallets** — Credited 1,000 PCLAW on first creation (not on restart)

Future: on-chain settlement, payment channels between frequent peers, HTLC for atomic job payment.

---

*Updated March 2026 — pricing unified across EconomyConfig, job marketplace, and agent budget at 0.5 PCLAW/1K tokens base rate*
