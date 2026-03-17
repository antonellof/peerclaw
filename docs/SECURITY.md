# Security Model

PeerClaw implements defense-in-depth security across multiple layers: sandboxed execution, credential protection, encryption, and runtime safety checks.

## Sandboxed Execution

### WASM Sandbox (Wasmtime)

All untrusted tools run in isolated WASM sandboxes:

- **Capability-based isolation** — Tools have no access by default
- **No filesystem/network** — Unless explicitly granted
- **Fuel metering** — Prevents infinite loops and resource exhaustion
- **Memory limits** — Configurable maximum memory per execution
- **Timeout enforcement** — Hard limits on execution time
- **Tool hashing** — BLAKE3 hash identifies tool versions

```toml
[wasm]
max_fuel = 1_000_000        # Fuel units per execution
max_memory_mb = 64          # Max memory allocation
timeout_ms = 30_000         # 30 second timeout
```

### Capability Grants

Tools must declare required capabilities:

```toml
[tool.capabilities]
http = ["api.example.com", "*.wikipedia.org"]  # Allowlisted hosts
filesystem = ["read:/data", "write:/tmp"]      # Path-scoped access
shell = false                                   # No shell access
secrets = ["API_KEY"]                          # Specific secrets only
```

### MicroVM Isolation (Planned)

Future releases will support Firecracker microVMs for heavy workloads:
- <125ms boot time
- Strict memory limits
- Read-only rootfs
- Network namespace isolation

## Safety Layer (`src/safety/`)

![Safety Flow](images/safety-flow.svg)

Runtime protection against common attack vectors:

### Leak Detection

Automatically detects and redacts credentials in outbound data:

```rust
// Detected patterns:
- API keys (sk-*, AKIA*, ghp_*, etc.)
- Private keys (-----BEGIN PRIVATE KEY-----)
- JWT tokens
- Database connection strings
- OAuth tokens
- High-entropy secrets (configurable threshold)
```

Configuration:
```toml
[safety.leak_detection]
enabled = true
redact_string = "[REDACTED]"
entropy_threshold = 4.5
```

### Prompt Injection Defense

Sanitizes untrusted content before injection into prompts:

- **Content escaping** — Special characters neutralized
- **Boundary markers** — Clear separation of user/system content
- **Instruction stripping** — Removes embedded directives
- **Length validation** — Prevents context overflow

### Content Policy

Configurable rules for content filtering:

```toml
[safety.policy]
enabled = true

[[safety.policy.rules]]
name = "no_code_execution"
pattern = "exec\\(|eval\\(|system\\("
severity = "block"
message = "Code execution patterns blocked"

[[safety.policy.rules]]
name = "no_pii"
pattern = "\\b\\d{3}-\\d{2}-\\d{4}\\b"  # SSN pattern
severity = "warn"
message = "Potential PII detected"
```

Severity levels:
- `block` — Reject the request
- `warn` — Log warning, continue
- `redact` — Remove matched content

### Input Validation

All inputs validated at system boundaries:

- **Length checks** — Maximum prompt/response sizes
- **Character validation** — Unicode normalization, control char filtering
- **Rate limiting** — Per-user and per-endpoint limits
- **Schema validation** — Structured inputs validated against schemas

## Credential Protection

### Secret Management

Secrets are never exposed to agent code:

1. **Host-only injection** — Secrets injected at the host boundary
2. **Environment isolation** — WASM tools cannot read host env vars
3. **Scoped access** — Tools declare which secrets they need
4. **Audit logging** — All secret accesses logged

### Secret Storage

```
~/.peerclaw/
├── secrets/
│   ├── .env              # Encrypted at rest
│   └── keyring/          # OS keychain integration
└── wallet/
    └── default.key       # Argon2id-encrypted Ed25519 key
```

### Leak Prevention

Outbound data scanned before transmission:

```
Agent Output → Leak Detector → [Clean] → Network
                    │
                    └→ [Secrets Found] → Redact/Block + Alert
```

## Encryption

### P2P Communication

All peer-to-peer traffic encrypted:

| Layer | Protocol |
|-------|----------|
| Transport | QUIC (TLS 1.3) or TCP |
| Session | Noise Protocol (XX handshake) |
| Identity | Ed25519 signatures |
| Content | BLAKE3 integrity hashes |

### Identity

- **Ed25519 keypairs** — Generated locally, never transmitted
- **Peer ID** — Derived from public key (libp2p PeerId format)
- **Message signing** — All network messages signed
- **Signature verification** — Invalid signatures rejected

### Content Hashing

- **BLAKE3** — Fast, secure hashing for all content
- **Tool verification** — WASM modules identified by hash
- **Storage chunks** — Content-addressed storage

## Endpoint Allowlisting

HTTP requests restricted to approved hosts:

```toml
[web_access]
enabled = true
allowed_hosts = [
    "*.wikipedia.org",
    "api.github.com",
    "huggingface.co"
]
blocked_hosts = [
    "*.onion",
    "localhost",
    "127.*",
    "10.*",
    "192.168.*"
]
max_requests_per_minute = 30
max_response_size_mb = 10
timeout_seconds = 30
```

## Skill Trust Levels

Skills have tiered trust based on source:

| Trust Level | Source | Capabilities |
|-------------|--------|--------------|
| **Local** | User-created in `~/.peerclaw/skills/` | Full tool access |
| **Installed** | Explicitly installed from network | Read-only tools |
| **Network** | Discovered from peers, not installed | Minimal (echo, time) |

```toml
[skills.trust]
local_tools = ["*"]                    # All tools
installed_tools = ["web_fetch", "file_read"]
network_tools = ["echo", "time"]
```

## Audit Logging

Security-relevant events logged:

```
[2024-03-17T10:15:32Z] [SECURITY] secret_access tool=web_fetch secret=API_KEY
[2024-03-17T10:15:33Z] [SECURITY] leak_detected type=api_key action=redacted
[2024-03-17T10:15:34Z] [SECURITY] policy_violation rule=no_pii severity=warn
[2024-03-17T10:15:35Z] [SECURITY] wasm_timeout tool=slow_tool fuel=1000000
```

Configuration:
```toml
[logging.security]
enabled = true
level = "info"
output = "~/.peerclaw/logs/security.log"
max_size_mb = 100
rotate_count = 5
```

## Best Practices

1. **Minimize capabilities** — Grant only what tools need
2. **Review skills** — Inspect network skills before installing
3. **Rotate secrets** — Regular key rotation for API keys
4. **Monitor logs** — Watch security audit logs
5. **Update regularly** — Keep PeerClaw updated for security patches
6. **Use allowlists** — Prefer allowlists over blocklists for HTTP
7. **Limit budgets** — Set conservative spending limits for agents

---

*v0.2 — March 2026*
