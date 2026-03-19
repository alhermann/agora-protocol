#!/usr/bin/env bash
# OpenClaw adapter for Agora Protocol — wake script
#
# This script is called by the Agora daemon when a message arrives from a
# trusted peer. It reads the message, builds a prompt, calls OpenClaw, and
# sends the reply back through the daemon.
#
# Environment variables set by the daemon:
#   AGORA_FROM             - Name of the sender
#   AGORA_PREVIEW          - First 200 chars of the message
#   AGORA_MESSAGE_COUNT    - Number of messages in this batch
#   AGORA_CONVERSATION_ID  - Conversation thread UUID
#   AGORA_MESSAGES_FILE    - Path to temp file with full message JSON
#   AGORA_API_PORT         - HTTP API port (default: 7313)
#   AGORA_API_URL          - Full API URL

set -euo pipefail

API_PORT="${AGORA_API_PORT:-7313}"
API_URL="${AGORA_API_URL:-http://127.0.0.1:${API_PORT}}"
OPENCLAW_CMD="${OPENCLAW_CMD:-openclaw}"

# Read all pending messages
MESSAGES=$(curl -sf "${API_URL}/messages" 2>/dev/null || echo "[]")
MSG_COUNT=$(echo "$MESSAGES" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")

if [ "$MSG_COUNT" = "0" ]; then
    echo "[agora-openclaw] No messages to process."
    exit 0
fi

echo "[agora-openclaw] Processing $MSG_COUNT message(s) from ${AGORA_FROM:-unknown}"

# Build prompt with message context
PROMPT=$(python3 -c "
import json, sys

messages = json.loads('''$MESSAGES''')
lines = []
lines.append('You received messages on the Agora peer-to-peer agent network.')
lines.append('Reply to each message thoughtfully. Send replies using the agora tools.')
lines.append('')
for msg in messages:
    lines.append(f'From: {msg.get(\"from\", \"unknown\")}')
    lines.append(f'Message: {msg.get(\"body\", \"\")}')
    if msg.get('conversation_id'):
        lines.append(f'Conversation: {msg[\"conversation_id\"]}')
    lines.append('---')
print('\n'.join(lines))
")

# Call OpenClaw to generate a reply
REPLY=$($OPENCLAW_CMD chat --message "$PROMPT" 2>/dev/null)

if [ -z "$REPLY" ]; then
    echo "[agora-openclaw] OpenClaw returned empty reply, skipping."
    exit 0
fi

# Send the reply back through the daemon
SENDER=$(echo "$MESSAGES" | python3 -c "import sys,json; msgs=json.load(sys.stdin); print(msgs[0].get('from','') if msgs else '')" 2>/dev/null || echo "")
CONV_ID=$(echo "$MESSAGES" | python3 -c "import sys,json; msgs=json.load(sys.stdin); print(msgs[0].get('conversation_id','') if msgs else '')" 2>/dev/null || echo "")

PAYLOAD=$(python3 -c "
import json
payload = {'body': '''$REPLY'''}
sender = '''$SENDER'''
conv_id = '''$CONV_ID'''
if sender:
    payload['to'] = sender
if conv_id:
    payload['conversation_id'] = conv_id
print(json.dumps(payload))
")

curl -sf -X POST "${API_URL}/send" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD" >/dev/null 2>&1

echo "[agora-openclaw] Replied to ${SENDER:-broadcast}"
