#!/bin/bash
# Spin up a PeerClaw P2P cluster with incremental node launches.
#
# Usage:
#   ./scripts/run_agents.sh              # 5 nodes, 3s between each
#   ./scripts/run_agents.sh 10           # 10 nodes
#   ./scripts/run_agents.sh 5 5          # 5 nodes, 5s between each
#
# The first node's dashboard opens at http://127.0.0.1:8080
# Press Ctrl+C to shut down the entire cluster.

set -e

NUM_NODES=${1:-5}
DELAY=${2:-3}
BASE_WEB_PORT=8080
BASE_P2P_PORT=9000

BOLD="\033[1m"
DIM="\033[2m"
GREEN="\033[32m"
YELLOW="\033[33m"
CYAN="\033[36m"
RESET="\033[0m"

# ── Build ────────────────────────────────────────────────────────────────────
BIN=./target/release/peerclaw
if [ ! -f "$BIN" ]; then
    echo -e "${BOLD}Building release binary…${RESET}"
    cargo build --release
fi

# ── Temp directory per cluster run ───────────────────────────────────────────
CLUSTER_DIR=$(mktemp -d -t peerclaw_cluster_XXXXXX)
PIDS=()

cleanup() {
    echo ""
    echo -e "${YELLOW}Shutting down ${#PIDS[@]} node(s)…${RESET}"
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    rm -rf "$CLUSTER_DIR"
    echo -e "${GREEN}Cluster stopped.${RESET}"
}
trap cleanup EXIT INT TERM

# ── Spawn nodes incrementally ────────────────────────────────────────────────
echo -e "${BOLD}=== PeerClaw P2P Cluster ===${RESET}"
echo -e "Nodes: ${CYAN}${NUM_NODES}${RESET}  Delay: ${CYAN}${DELAY}s${RESET}  Dashboard: ${CYAN}http://127.0.0.1:${BASE_WEB_PORT}${RESET}"
echo ""

BOOTSTRAP_ADDR="/ip4/127.0.0.1/tcp/${BASE_P2P_PORT}"

for i in $(seq 0 $((NUM_NODES - 1))); do
    NODE_DIR="$CLUSTER_DIR/node_$i"
    mkdir -p "$NODE_DIR"

    WEB_PORT=$((BASE_WEB_PORT + i))
    P2P_PORT=$((BASE_P2P_PORT + i))
    P2P_ADDR="/ip4/127.0.0.1/tcp/$P2P_PORT"

    # First node is the bootstrap; others connect to it
    BOOT_FLAG=""
    if [ "$i" -gt 0 ]; then
        BOOT_FLAG="--bootstrap $BOOTSTRAP_ADDR"
    fi

    PEERCLAWD_HOME="$NODE_DIR" \
    RUST_LOG=peerclaw=info \
    "$BIN" serve \
        --web "127.0.0.1:$WEB_PORT" \
        --listen "$P2P_ADDR" \
        $BOOT_FLAG \
        > "$NODE_DIR/output.log" 2>&1 &

    PID=$!
    PIDS+=($PID)

    echo -e "  ${GREEN}▸${RESET} Node ${BOLD}$i${RESET}  web=:${WEB_PORT}  p2p=:${P2P_PORT}  pid=${PID}"

    # Give the node a moment to start before launching the next one
    # so the dashboard shows peers appearing incrementally.
    if [ "$i" -lt $((NUM_NODES - 1)) ]; then
        sleep "$DELAY"
    fi
done

# ── Wait for first node to be ready, then print status ──────────────────────
echo ""
echo -e "${DIM}Waiting for nodes to discover each other…${RESET}"
sleep 4

echo ""
echo -e "${BOLD}=== Cluster Status ===${RESET}"
echo ""
printf "  %-6s %-24s %-14s %s\n" "Node" "Web" "P2P port" "Peer ID"
echo "  $(printf '%.0s─' {1..70})"

for i in $(seq 0 $((NUM_NODES - 1))); do
    NODE_DIR="$CLUSTER_DIR/node_$i"
    WEB_PORT=$((BASE_WEB_PORT + i))
    P2P_PORT=$((BASE_P2P_PORT + i))

    # Try to get peer ID from the API
    PEER_ID=$(curl -sf "http://127.0.0.1:${WEB_PORT}/api/status" 2>/dev/null \
        | grep -o '"peer_id":"[^"]*"' | head -1 | cut -d'"' -f4 || true)

    if [ -z "$PEER_ID" ]; then
        # Fall back to log scraping
        PEER_ID=$(grep -o 'Peer ID: [^ ]*' "$NODE_DIR/output.log" 2>/dev/null \
            | head -1 | cut -d' ' -f3 || echo "starting…")
    fi

    DISPLAY_ID="${PEER_ID:0:16}…"
    printf "  %-6s %-24s %-14s %s\n" "$i" "http://127.0.0.1:$WEB_PORT" ":$P2P_PORT" "$DISPLAY_ID"
done

# ── Check peer connections ───────────────────────────────────────────────────
echo ""
TOTAL_CONNS=0
for i in $(seq 0 $((NUM_NODES - 1))); do
    WEB_PORT=$((BASE_WEB_PORT + i))
    CONNS=$(curl -sf "http://127.0.0.1:${WEB_PORT}/api/status" 2>/dev/null \
        | grep -o '"connected_peers":[0-9]*' | head -1 | cut -d: -f2 || echo "0")
    TOTAL_CONNS=$((TOTAL_CONNS + CONNS))
    echo -e "  Node $i: ${CYAN}${CONNS}${RESET} connected peer(s)"
done

echo ""
echo -e "${GREEN}Cluster ready.${RESET} Total connections: ${BOLD}${TOTAL_CONNS}${RESET}"
echo -e "Open ${CYAN}http://127.0.0.1:${BASE_WEB_PORT}${RESET} to see the dashboard."
echo -e "${YELLOW}Press Ctrl+C to stop the cluster.${RESET}"
echo ""

# ── Keep alive ───────────────────────────────────────────────────────────────
# Periodically print connection counts so the user sees P2P activity.
while kill -0 "${PIDS[0]}" 2>/dev/null; do
    sleep 15
    CONNS=()
    for i in $(seq 0 $((NUM_NODES - 1))); do
        WEB_PORT=$((BASE_WEB_PORT + i))
        C=$(curl -sf "http://127.0.0.1:${WEB_PORT}/api/status" 2>/dev/null \
            | grep -o '"connected_peers":[0-9]*' | head -1 | cut -d: -f2 || echo "?")
        CONNS+=("$C")
    done
    TS=$(date +%H:%M:%S)
    echo -e "${DIM}[${TS}]${RESET} peers: ${CONNS[*]}"
done
