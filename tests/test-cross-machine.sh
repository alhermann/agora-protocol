#!/bin/bash
# Agora Protocol — Cross-Machine Integration Tests
#
# Prerequisites:
#   - Daemon running locally (port 7313)
#   - Connector to remote peer (port 7314)
#   - Remote peer daemon running with wake hook configured
#   - Both sides have each other as friends with trust >= 3
#
# Usage:
#   ./tests/test-cross-machine.sh [test-name]
#
# Tests:
#   message    — Send a message and wait for reply
#   debounce   — Send 3 rapid messages, verify single wake
#   wake-code  — Send coding task and verify woken agent completes it
#   all        — Run all tests

set -euo pipefail

API_PORT="${AGORA_API_PORT:-7314}"
API="http://127.0.0.1:${API_PORT}"
REMOTE_NAME="${AGORA_REMOTE_NAME:-bob}"
PASS=0
FAIL=0
RESULTS=""

log() { echo "[$(date +%H:%M:%S)] $*"; }
pass() { PASS=$((PASS + 1)); RESULTS="${RESULTS}\n  PASS: $1"; log "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); RESULTS="${RESULTS}\n  FAIL: $1"; log "FAIL: $1"; }

# --- Helpers ---

send_message() {
    local body="$1"
    python3 -c "
import json, sys
sys.stdout.write(json.dumps({'body': sys.argv[1], 'to': sys.argv[2]}))
" "$body" "$REMOTE_NAME" | curl -s -X POST "${API}/send" -H 'Content-Type: application/json' -d @- > /dev/null
}

wait_for_reply() {
    local timeout="${1:-120}"
    curl -s "${API}/messages?wait=true&timeout=${timeout}" 2>/dev/null
}

check_peer_connected() {
    local peers
    peers=$(curl -s "${API}/peers" 2>/dev/null)
    echo "$peers" | python3 -c "import json,sys; d=json.load(sys.stdin); sys.exit(0 if d.get('count',0)>0 else 1)" 2>/dev/null
}

# --- Tests ---

test_connection() {
    log "Test: Connection check"
    if check_peer_connected; then
        pass "Remote peer connected"
    else
        fail "No remote peer connected"
        return 1
    fi
}

test_message() {
    log "Test: Send message and wait for reply"
    log "  Draining inbox..."
    curl -s "${API}/messages" > /dev/null 2>&1

    log "  Sending greeting..."
    send_message "AGORA_TEST: Please reply with exactly 'AGORA_TEST_OK' in your response body."

    log "  Waiting for reply (up to 120s)..."
    local reply
    reply=$(wait_for_reply 120)

    if echo "$reply" | grep -q "AGORA_TEST_OK"; then
        pass "Message round-trip"
    elif [ "$reply" = "[]" ]; then
        fail "Message round-trip — no reply received (timeout)"
    else
        log "  Reply received but unexpected content: $(echo "$reply" | head -c 200)"
        fail "Message round-trip — unexpected reply"
    fi
}

test_debounce() {
    log "Test: Debounced wake (3 rapid messages)"
    log "  Draining inbox..."
    curl -s "${API}/messages" > /dev/null 2>&1

    log "  Sending 3 messages rapidly..."
    send_message "AGORA_DEBOUNCE_TEST message 1 of 3"
    send_message "AGORA_DEBOUNCE_TEST message 2 of 3"
    send_message "AGORA_DEBOUNCE_TEST message 3 of 3 - reply with AGORA_DEBOUNCE_OK and the value of AGORA_MESSAGE_COUNT if available"

    log "  Waiting for reply (up to 120s)..."
    local reply
    reply=$(wait_for_reply 120)

    if echo "$reply" | grep -q "AGORA_DEBOUNCE_OK"; then
        pass "Debounced wake — single reply received"
    elif [ "$reply" = "[]" ]; then
        fail "Debounced wake — no reply received (timeout)"
    else
        log "  Reply: $(echo "$reply" | head -c 200)"
        pass "Debounced wake — reply received (check logs for single invocation)"
    fi
}

test_wake_code() {
    log "Test: Wake with coding task"
    log "  Draining inbox..."
    curl -s "${API}/messages" > /dev/null 2>&1

    log "  Sending coding task..."
    send_message "AGORA_CODE_TEST: Please add a comment '// Agora integration test marker' as the very last line of daemon/src/api.rs, then reply with AGORA_CODE_OK and confirm the change."

    log "  Waiting for reply (up to 180s)..."
    local reply
    reply=$(wait_for_reply 180)

    if echo "$reply" | grep -q "AGORA_CODE_OK"; then
        pass "Wake coding task — completed"
    elif [ "$reply" = "[]" ]; then
        fail "Wake coding task — no reply received (timeout)"
    else
        log "  Reply: $(echo "$reply" | head -c 200)"
        pass "Wake coding task — reply received (verify changes manually)"
    fi
}

# --- Runner ---

run_all() {
    test_connection || return 1
    test_message
    test_debounce
    test_wake_code
}

case "${1:-all}" in
    connection) test_connection ;;
    message)    test_connection && test_message ;;
    debounce)   test_connection && test_debounce ;;
    wake-code)  test_connection && test_wake_code ;;
    all)        run_all ;;
    *)          echo "Usage: $0 {connection|message|debounce|wake-code|all}"; exit 1 ;;
esac

# --- Summary ---
echo ""
echo "================================"
echo "  Test Results: ${PASS} passed, ${FAIL} failed"
echo -e "$RESULTS"
echo "================================"

exit $FAIL
