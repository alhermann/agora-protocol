#!/usr/bin/env bash
# demo-infra.sh — Robust demo infrastructure for Agora dashboards
# Ensures SSH tunnel, both Vite dev servers, and daemon connectivity stay alive.
#
# Usage: ./scripts/demo-infra.sh [start|stop|status|health]

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DAEMON_BIN="$PROJECT_DIR/daemon/target/debug/agora"
DASHBOARD_DIR="$PROJECT_DIR/dashboard"
PID_DIR="$PROJECT_DIR/.demo-pids"

# Config
BOB_HOST="${BOB_HOST:?Set BOB_HOST to the remote machine IP}"
BOB_SSH_USER="${BOB_SSH_USER:?Set BOB_SSH_USER to the remote SSH username}"
BOB_SSH_PASS="${BOB_SSH_PASS:?Set BOB_SSH_PASS to the remote SSH password}"
BOB_P2P_PORT=7312
LOCAL_API_PORT=7313
TUNNEL_LOCAL_PORT=7323
ALICE_DASHBOARD_PORT=5173
BOB_DASHBOARD_PORT=5174

mkdir -p "$PID_DIR"

log() { echo "[$(date '+%H:%M:%S')] $*"; }

# --- SSH Tunnel (with auto-reconnect) ---

start_tunnel() {
    # Kill any existing tunnel
    pkill -f "ssh.*-L.*${TUNNEL_LOCAL_PORT}" 2>/dev/null || true
    sleep 1

    # Start tunnel with ServerAliveInterval for auto-detection of dead connections
    sshpass -p "$BOB_SSH_PASS" ssh -f -N \
        -o StrictHostKeyChecking=no \
        -o ServerAliveInterval=10 \
        -o ServerAliveCountMax=3 \
        -o ExitOnForwardFailure=yes \
        -L "${TUNNEL_LOCAL_PORT}:127.0.0.1:${LOCAL_API_PORT}" \
        "${BOB_SSH_USER}@${BOB_HOST}"

    # Store the PID
    pgrep -f "ssh.*-L.*${TUNNEL_LOCAL_PORT}" > "$PID_DIR/tunnel.pid" 2>/dev/null
    log "SSH tunnel started (localhost:${TUNNEL_LOCAL_PORT} → Bob:${LOCAL_API_PORT})"
}

check_tunnel() {
    if curl -sf --connect-timeout 3 "http://127.0.0.1:${TUNNEL_LOCAL_PORT}/status" >/dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

ensure_tunnel() {
    if ! check_tunnel; then
        log "SSH tunnel down — restarting..."
        start_tunnel
        sleep 2
        if check_tunnel; then
            log "SSH tunnel recovered"
        else
            log "WARNING: SSH tunnel failed to recover"
            return 1
        fi
    fi
}

# --- Vite Dev Servers ---

start_vite() {
    local port=$1
    local api_port=$2
    local label=$3

    # Kill existing on that port
    lsof -ti ":${port}" 2>/dev/null | xargs kill 2>/dev/null || true
    sleep 1

    cd "$DASHBOARD_DIR"
    AGORA_API_PORT="$api_port" npx vite --port "$port" &
    local pid=$!
    echo "$pid" > "$PID_DIR/vite-${label}.pid"
    log "Dashboard '${label}' started on port ${port} (PID ${pid}, API→${api_port})"
}

check_vite() {
    local port=$1
    if curl -sf --connect-timeout 2 "http://localhost:${port}/" >/dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

ensure_vite() {
    local port=$1
    local api_port=$2
    local label=$3

    if ! check_vite "$port"; then
        log "Dashboard '${label}' on port ${port} is down — restarting..."
        start_vite "$port" "$api_port" "$label"
        sleep 3
        if check_vite "$port"; then
            log "Dashboard '${label}' recovered"
        else
            log "WARNING: Dashboard '${label}' failed to start"
        fi
    fi
}

# --- Daemon Health ---

check_daemon_local() {
    curl -sf --connect-timeout 2 "http://127.0.0.1:${LOCAL_API_PORT}/status" >/dev/null 2>&1
}

check_daemon_bob() {
    curl -sf --connect-timeout 3 "http://127.0.0.1:${TUNNEL_LOCAL_PORT}/status" >/dev/null 2>&1
}

check_peer_connection() {
    local count
    count=$(curl -sf --connect-timeout 2 "http://127.0.0.1:${LOCAL_API_PORT}/peers" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo "0")
    [ "$count" -gt 0 ]
}

ensure_peer_connection() {
    if ! check_peer_connection; then
        log "No peer connection — reconnecting Alice to Bob..."
        curl -sf -X POST "http://127.0.0.1:${LOCAL_API_PORT}/connect" \
            -H 'Content-Type: application/json' \
            -d "{\"address\":\"${BOB_HOST}:${BOB_P2P_PORT}\"}" >/dev/null 2>&1
        sleep 3
        if check_peer_connection; then
            log "Peer connection restored"
        else
            log "WARNING: Could not reconnect to Bob"
        fi
    fi
}

# --- Commands ---

cmd_start() {
    log "Starting Agora demo infrastructure..."

    # 1. Check local daemon
    if ! check_daemon_local; then
        log "ERROR: Local daemon not running. Start with: $DAEMON_BIN --name alice-desktop start --daemon"
        exit 1
    fi
    log "Local daemon (alice-desktop): OK"

    # 2. SSH tunnel
    start_tunnel
    sleep 2

    if ! check_daemon_bob; then
        log "WARNING: Bob's daemon not reachable through tunnel"
    else
        log "Bob's daemon: OK (via tunnel)"
    fi

    # 3. Peer connection
    ensure_peer_connection

    # 4. Dashboards
    start_vite "$ALICE_DASHBOARD_PORT" "$LOCAL_API_PORT" "alice"
    sleep 2
    start_vite "$BOB_DASHBOARD_PORT" "$TUNNEL_LOCAL_PORT" "bob"
    sleep 2

    log ""
    log "=== Demo Infrastructure Ready ==="
    log "  Alice dashboard: http://localhost:${ALICE_DASHBOARD_PORT}"
    log "  Bob dashboard:   http://localhost:${BOB_DASHBOARD_PORT}"
    log ""
    log "Run './scripts/demo-infra.sh health' to check everything"
    log "Run './scripts/demo-infra.sh stop' to tear down"
}

cmd_stop() {
    log "Stopping demo infrastructure..."
    pkill -f "ssh.*-L.*${TUNNEL_LOCAL_PORT}" 2>/dev/null || true
    lsof -ti ":${ALICE_DASHBOARD_PORT}" 2>/dev/null | xargs kill 2>/dev/null || true
    lsof -ti ":${BOB_DASHBOARD_PORT}" 2>/dev/null | xargs kill 2>/dev/null || true
    rm -f "$PID_DIR"/*.pid
    log "All stopped"
}

cmd_status() {
    echo "=== Agora Demo Status ==="
    echo ""
    printf "  %-30s %s\n" "Alice daemon (localhost:${LOCAL_API_PORT}):" "$(check_daemon_local && echo 'UP' || echo 'DOWN')"
    printf "  %-30s %s\n" "SSH tunnel (localhost:${TUNNEL_LOCAL_PORT}):" "$(check_tunnel && echo 'UP' || echo 'DOWN')"
    printf "  %-30s %s\n" "Bob daemon (via tunnel):" "$(check_daemon_bob && echo 'UP' || echo 'DOWN')"
    printf "  %-30s %s\n" "Peer connection:" "$(check_peer_connection && echo 'CONNECTED' || echo 'DISCONNECTED')"
    printf "  %-30s %s\n" "Alice dashboard (:${ALICE_DASHBOARD_PORT}):" "$(check_vite $ALICE_DASHBOARD_PORT && echo 'UP' || echo 'DOWN')"
    printf "  %-30s %s\n" "Bob dashboard (:${BOB_DASHBOARD_PORT}):" "$(check_vite $BOB_DASHBOARD_PORT && echo 'UP' || echo 'DOWN')"
    echo ""

    # Show friend/request counts
    local alice_friends bob_friends alice_reqs bob_reqs
    alice_friends=$(curl -sf "http://127.0.0.1:${LOCAL_API_PORT}/friends" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo "?")
    bob_friends=$(curl -sf "http://127.0.0.1:${TUNNEL_LOCAL_PORT}/friends" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo "?")
    alice_reqs=$(curl -sf "http://127.0.0.1:${LOCAL_API_PORT}/friend-requests?status=pending" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo "?")
    bob_reqs=$(curl -sf "http://127.0.0.1:${TUNNEL_LOCAL_PORT}/friend-requests?status=pending" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['count'])" 2>/dev/null || echo "?")

    echo "  Alice: ${alice_friends} friends, ${alice_reqs} pending requests"
    echo "  Bob:   ${bob_friends} friends, ${bob_reqs} pending requests"
}

cmd_health() {
    local all_ok=true

    ensure_tunnel || all_ok=false
    ensure_vite "$ALICE_DASHBOARD_PORT" "$LOCAL_API_PORT" "alice" || all_ok=false
    ensure_vite "$BOB_DASHBOARD_PORT" "$TUNNEL_LOCAL_PORT" "bob" || all_ok=false
    ensure_peer_connection || all_ok=false

    if $all_ok; then
        log "All systems healthy"
    else
        log "Some systems needed recovery — check status"
    fi
}

case "${1:-status}" in
    start)  cmd_start ;;
    stop)   cmd_stop ;;
    status) cmd_status ;;
    health) cmd_health ;;
    *)      echo "Usage: $0 [start|stop|status|health]"; exit 1 ;;
esac
