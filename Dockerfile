# ── Stage 1: Build frontend ─────────────────────────────────────────
FROM node:22-alpine AS frontend
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm ci --ignore-scripts
COPY web/ .
RUN npm run build

# ── Stage 2: Build Rust binary ─────────────────────────────────────
FROM rust:1.83-bookworm AS builder
RUN apt-get update && apt-get install -y \
    cmake pkg-config libssl-dev protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Cache dependencies first
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY prompts/ prompts/
COPY templates/ templates/
RUN cargo build --release

# ── Stage 3: Runtime ───────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates python3 python3-pip \
    curl jq poppler-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /peerclaw

# Copy binary
COPY --from=builder /app/target/release/peerclaw /usr/local/bin/peerclaw

# Copy frontend dist
COPY --from=frontend /app/web/dist /peerclaw/web/dist

# Copy templates and prompts
COPY templates/ /peerclaw/templates/
COPY prompts/ /peerclaw/prompts/

# Data directory
RUN mkdir -p /data/.peerclaw
ENV PEERCLAW_HOME=/data/.peerclaw
ENV PEERCLAW_WEB_DIST=/peerclaw/web/dist

# Default port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -sf http://localhost:8080/api/status || exit 1

# Default: serve with web dashboard, connect to Ollama on host
ENTRYPOINT ["peerclaw"]
CMD ["serve", "--web", "0.0.0.0:8080", "--ollama"]
