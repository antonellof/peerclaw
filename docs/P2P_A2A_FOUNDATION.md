# P2P and A2A foundation (PeerClaw)

## Stack

| Component | Location | Role |
|-----------|----------|------|
| libp2p behaviours | `src/p2p/behaviour.rs` | Kademlia, mDNS, GossipSub, Identify, **request-response** `/peerclaw/a2a-rpc/1.0.0` |
| Gossip topics | `src/runtime.rs`, `src/a2a/gossip.rs` | Jobs, provider, skills, **`peerclaw/a2a/v1`**, **`peerclaw/resources/v1`** |
| A2A HTTP | `src/web/mod.rs` | `GET /.well-known/agent-card.json`, `POST /a2a`, `GET /api/a2a/peers` |
| Shared state | `src/a2a/state.rs` | Tasks + discovered agent cards (same `Arc` on `Runtime` and `Network`) |

## Manual checks

1. Start two nodes with web on different ports; ensure both subscribe (default `peerclaw serve --web 127.0.0.1:PORT`).
2. After ~45s with peers connected, check logs for gossip publish; on peer A: `curl http://127.0.0.1:PORT_B/api/a2a/peers` should list peer B’s card.
3. JSON-RPC: `curl -s -X POST http://127.0.0.1:8080/a2a -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"tasks/create","params":{"message":"hi"}}'`

## Tests

- `cargo test a2a::jsonrpc` — unit tests on JSON-RPC dispatch.
