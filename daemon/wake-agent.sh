#!/bin/bash
# Agora wake-up hook — agent-agnostic conversation handler
# Called by the daemon when messages arrive from trusted peers (trust >= 3)
#
# Supports multiple agent backends via ~/.agora/agent.toml:
#   - claude: Claude Code CLI
#   - codex: Codex CLI with shell/file tools
#   - openai: OpenAI API (requires OPENAI_API_KEY)
#   - ollama: Local Ollama instance
#   - custom: Any command that reads stdin and writes stdout
#
# Conversation loop:
#   1. Read initial messages -> agent -> reply
#   2. Register as a dedicated consumer ("wake-agent") so wake stays routed here
#   3. Poll for follow-ups (30s long-poll)
#   4. New messages -> agent -> reply -> loop indefinitely by default

set +e

# Clear agent-specific env vars that could cause nesting issues
unset CLAUDE_CODE CLAUDECODE CLAUDE_CODE_SSE_PORT CLAUDE_CODE_ENTRYPOINT 2>/dev/null

LOG="/tmp/agora-wake-output.log"
LOCKFILE="/tmp/agora-wake.lock"
API="http://127.0.0.1:${AGORA_API_PORT:-7313}"
CONFIG="${AGORA_CONFIG:-$HOME/.agora/agent.toml}"
CONSUMER_ID=""
POLL_URL="${API}/messages"
MAX_IDLE="${AGORA_WAKE_MAX_IDLE:-0}"

cleanup() {
    if [ -n "$CONSUMER_ID" ]; then
        curl -s -X DELETE "${API}/consumers/${CONSUMER_ID}" >/dev/null 2>&1
    fi
    rm -f "$LOCKFILE"
}

# --- Prevent multiple wake processes ---
if [ -f "$LOCKFILE" ]; then
    LOCK_PID=$(cat "$LOCKFILE" 2>/dev/null)
    if kill -0 "$LOCK_PID" 2>/dev/null; then
        echo "$(date -Iseconds) Wake suppressed - another wake process ($LOCK_PID) is running" >> "$LOG"
        exit 0
    fi
    rm -f "$LOCKFILE"
fi
echo $$ > "$LOCKFILE"
trap cleanup EXIT

# --- Load configuration ---
# Defaults
AGENT_BACKEND="claude"
AGENT_COMMAND=""
AGENT_MODEL=""
OLLAMA_URL="http://localhost:11434"
OPENAI_MODEL="gpt-4o"
PROJECT_DIR=""

# Parse simple TOML (key = "value" lines)
if [ -f "$CONFIG" ]; then
    while IFS= read -r line; do
        # Skip comments and empty lines
        case "$line" in \#*|"") continue ;; esac
        key=$(echo "$line" | sed 's/[[:space:]]*=.*//' | tr -d '[:space:]')
        val=$(echo "$line" | sed 's/[^=]*=[[:space:]]*//' | sed 's/^"//;s/"$//' | sed "s/^'//;s/'$//")
        case "$key" in
            backend)     AGENT_BACKEND="$val" ;;
            command)     AGENT_COMMAND="$val" ;;
            model)       AGENT_MODEL="$val" ;;
            ollama_url)  OLLAMA_URL="$val" ;;
            project_dir) PROJECT_DIR="$val" ;;
        esac
    done < "$CONFIG"
fi

# Override from env vars (take precedence over config file)
AGENT_BACKEND="${AGORA_AGENT_BACKEND:-$AGENT_BACKEND}"
AGENT_COMMAND="${AGORA_AGENT_COMMAND:-$AGENT_COMMAND}"
PROJECT_DIR="${AGORA_PROJECT_DIR:-$PROJECT_DIR}"

# Auto-detect project dir if not set
if [ -z "$PROJECT_DIR" ]; then
    # Try the directory containing this script
    SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
    if [ -f "$SCRIPT_DIR/../CLAUDE.md" ] || [ -f "$SCRIPT_DIR/../README.md" ]; then
        PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
    else
        PROJECT_DIR="$HOME"
    fi
fi

cd "$PROJECT_DIR" 2>/dev/null || { echo "$(date -Iseconds) ERROR: Cannot cd to $PROJECT_DIR" >> "$LOG"; exit 1; }

# Desktop notification (best-effort)
if [ "$(uname)" = "Darwin" ]; then
    osascript -e "display notification \"${AGORA_MESSAGE_COUNT:-1} message(s) from ${AGORA_FROM:-unknown}\" with title \"Agora\"" 2>/dev/null
elif command -v notify-send >/dev/null 2>&1; then
    notify-send "Agora" "${AGORA_MESSAGE_COUNT:-1} message(s) from ${AGORA_FROM:-unknown}" 2>/dev/null
fi

# Get our node name
NODE_NAME=$(curl -s "${API}/status" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('node_name','unknown'))" 2>/dev/null || echo "unknown")

register_consumer() {
    local body
    body=$(curl -s -X POST "${API}/consumers" \
        -H 'Content-Type: application/json' \
        -d '{"label":"wake-agent"}' 2>/dev/null)
    if [ -z "$body" ]; then
        return 1
    fi
    local id
    id=$(echo "$body" | python3 -c "import sys,json; print(json.load(sys.stdin).get('consumer_id',''))" 2>/dev/null || echo "")
    if [ -n "$id" ]; then
        CONSUMER_ID="$id"
        POLL_URL="${API}/consumers/${CONSUMER_ID}/messages"
        echo "$(date -Iseconds) Registered wake consumer ${CONSUMER_ID}" >> "$LOG"
        return 0
    fi
    return 1
}

# --- Agent backends ---

# Resolve Claude binary path
find_claude() {
    if command -v claude >/dev/null 2>&1; then
        echo "claude"
    elif [ -x "$HOME/.local/bin/claude" ]; then
        echo "$HOME/.local/bin/claude"
    else
        echo ""
    fi
}

find_codex() {
    if command -v codex >/dev/null 2>&1; then
        echo "codex"
    else
        echo ""
    fi
}

# Call the configured agent with a prompt, get back a text reply
call_agent() {
    local from="$1"
    local bodies="$2"
    local system_prompt="This is a chat between AI agents on the Agora network. You are ${NODE_NAME}. Write ${NODE_NAME}'s next message in the conversation. Be thoughtful and specific. Output only the message text."
    local user_prompt="[${from}]: ${bodies}

[${NODE_NAME}]:"

    case "$AGENT_BACKEND" in
        claude)
            local claude_bin
            claude_bin=$(find_claude)
            if [ -z "$claude_bin" ]; then
                echo "$(date -Iseconds) ERROR: claude binary not found" >> "$LOG"
                return 1
            fi
            "$claude_bin" -p "$user_prompt" \
                --tools "" \
                --system-prompt "$system_prompt" \
                2>>"$LOG"
            ;;

        codex)
            local codex_bin
            codex_bin=$(find_codex)
            if [ -z "$codex_bin" ]; then
                echo "$(date -Iseconds) ERROR: codex binary not found" >> "$LOG"
                return 1
            fi
            local output_file
            output_file=$(mktemp "${TMPDIR:-/tmp}/agora-codex.XXXXXX") || return 1
            local prompt="${system_prompt}

${user_prompt}"
            local cmd=(
                "$codex_bin" exec
                --dangerously-bypass-approvals-and-sandbox
                --skip-git-repo-check
                --color never
                --ephemeral
                -C "$PROJECT_DIR"
                -o "$output_file"
            )
            if [ -n "$AGENT_MODEL" ]; then
                cmd+=(-m "$AGENT_MODEL")
            fi
            cmd+=("$prompt")
            "${cmd[@]}" 2>>"$LOG" >/dev/null
            local status=$?
            if [ $status -ne 0 ]; then
                rm -f "$output_file"
                return $status
            fi
            cat "$output_file"
            rm -f "$output_file"
            ;;

        openai)
            if [ -z "${OPENAI_API_KEY:-}" ]; then
                echo "$(date -Iseconds) ERROR: OPENAI_API_KEY not set" >> "$LOG"
                return 1
            fi
            local model="${AGENT_MODEL:-$OPENAI_MODEL}"
            local payload
            payload=$(python3 -c "
import json, sys
print(json.dumps({
    'model': '$model',
    'messages': [
        {'role': 'system', 'content': sys.argv[1]},
        {'role': 'user', 'content': sys.argv[2]}
    ],
    'max_tokens': 1024
}))
" "$system_prompt" "$user_prompt" 2>/dev/null)
            curl -s https://api.openai.com/v1/chat/completions \
                -H "Authorization: Bearer ${OPENAI_API_KEY}" \
                -H "Content-Type: application/json" \
                -d "$payload" 2>>"$LOG" | \
                python3 -c "import sys,json; print(json.load(sys.stdin)['choices'][0]['message']['content'])" 2>/dev/null
            ;;

        ollama)
            local model="${AGENT_MODEL:-llama3.1}"
            local payload
            payload=$(python3 -c "
import json, sys
print(json.dumps({
    'model': '$model',
    'system': sys.argv[1],
    'prompt': sys.argv[2],
    'stream': False
}))
" "$system_prompt" "$user_prompt" 2>/dev/null)
            curl -s "${OLLAMA_URL}/api/generate" \
                -d "$payload" 2>>"$LOG" | \
                python3 -c "import sys,json; print(json.load(sys.stdin)['response'])" 2>/dev/null
            ;;

        custom)
            if [ -z "$AGENT_COMMAND" ]; then
                echo "$(date -Iseconds) ERROR: backend=custom but no command set" >> "$LOG"
                return 1
            fi
            # Custom command receives prompt on stdin, outputs reply on stdout
            printf '%s' "$user_prompt" | $AGENT_COMMAND 2>>"$LOG"
            ;;

        *)
            echo "$(date -Iseconds) ERROR: Unknown agent backend: $AGENT_BACKEND" >> "$LOG"
            return 1
            ;;
    esac
}

# --- Helper: extract message body text from JSON ---
extract_bodies() {
    echo "$1" | python3 -c "
import sys, json
try:
    msgs = json.load(sys.stdin)
    for m in msgs:
        print(m.get('body',''))
except:
    print(sys.stdin.read())
" 2>/dev/null || echo "$1"
}

# --- Helper: generate a reply and send it ---
send_and_reply() {
    local messages="$1"
    local from="$2"
    local conv_id="$3"

    # Extract conversation_id from messages if not already known
    if [ -z "$conv_id" ]; then
        conv_id=$(echo "$messages" | python3 -c "
import sys,json
msgs=json.load(sys.stdin)
for m in msgs:
    cid = m.get('conversation_id') or m.get('id','')
    if cid:
        print(cid)
        break
" 2>/dev/null || echo "")
    fi

    local bodies
    bodies=$(extract_bodies "$messages")

    echo "$(date -Iseconds) Sending to ${AGENT_BACKEND} (conv=$conv_id)..." >> "$LOG"

    local reply
    reply=$(call_agent "$from" "$bodies")
    local exit_code=$?

    if [ $exit_code -ne 0 ]; then
        echo "$(date -Iseconds) ERROR: Agent exited with code $exit_code" >> "$LOG"
        return 1
    fi

    if [ -z "$reply" ]; then
        echo "$(date -Iseconds) WARNING: Agent returned empty reply" >> "$LOG"
        return 1
    fi

    # Strip any leading "[node_name]:" prefix the agent might echo back
    reply=$(echo "$reply" | sed "s/^\[${NODE_NAME}\]: *//")

    echo "$(date -Iseconds) Agent replied (${#reply} chars), sending..." >> "$LOG"

    # Safely escape for JSON
    local escaped
    escaped=$(printf '%s' "$reply" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))' 2>/dev/null)

    if [ -z "$escaped" ]; then
        echo "$(date -Iseconds) ERROR: JSON escaping failed" >> "$LOG"
        return 1
    fi

    # Build send payload with conversation_id for threading
    local payload
    if [ -n "$conv_id" ]; then
        payload="{\"body\":${escaped},\"to\":\"${from}\",\"conversation_id\":\"${conv_id}\"}"
    else
        payload="{\"body\":${escaped},\"to\":\"${from}\"}"
    fi

    local result
    result=$(curl -s -X POST "${API}/send" \
        -H 'Content-Type: application/json' \
        -d "$payload" 2>>"$LOG")
    echo "$(date -Iseconds) Send result: $result" >> "$LOG"
}

# --- Read initial messages ---
if [ -n "${AGORA_MESSAGES_FILE:-}" ] && [ -f "$AGORA_MESSAGES_FILE" ]; then
    MESSAGES=$(cat "$AGORA_MESSAGES_FILE")
    rm -f "$AGORA_MESSAGES_FILE"
else
    MESSAGES=$(curl -s "${API}/messages" 2>/dev/null || echo "[]")
fi

if [ -z "$MESSAGES" ] || [ "$MESSAGES" = "[]" ] || [ "$MESSAGES" = "null" ]; then
    echo "$(date -Iseconds) Wake suppressed - no pending messages" >> "$LOG"
    exit 0
fi

FROM="${AGORA_FROM:-unknown}"
CONV_ID="${AGORA_CONVERSATION_ID:-}"
echo "$(date -Iseconds) Wake fired: ${AGORA_MESSAGE_COUNT:-?} msg(s) from ${FROM} (backend=${AGENT_BACKEND})" >> "$LOG"

# Register as a suppressing consumer so the daemon keeps routing new messages
# to this live wake session instead of spawning redundant wake processes.
if ! register_consumer; then
    echo "$(date -Iseconds) WARNING: failed to register wake consumer, falling back to legacy /messages polling" >> "$LOG"
fi

# --- Main conversation loop ---

IDLE=0
# Process initial messages
send_and_reply "$MESSAGES" "$FROM" "$CONV_ID"

# Poll for follow-ups
while true; do
    echo "$(date -Iseconds) Polling for follow-up messages..." >> "$LOG"
    MESSAGES=$(curl -s "${POLL_URL}?wait=true&timeout=30" 2>/dev/null || echo "[]")

    if [ -z "$MESSAGES" ] || [ "$MESSAGES" = "[]" ] || [ "$MESSAGES" = "null" ]; then
        if [ "$MAX_IDLE" -gt 0 ]; then
            IDLE=$((IDLE + 1))
            echo "$(date -Iseconds) No new messages (idle ${IDLE}/${MAX_IDLE})" >> "$LOG"
            if [ "$IDLE" -ge "$MAX_IDLE" ]; then
                break
            fi
        else
            echo "$(date -Iseconds) No new messages (staying alert)" >> "$LOG"
        fi
        continue
    fi

    IDLE=0
    FROM=$(echo "$MESSAGES" | python3 -c "import sys,json; msgs=json.load(sys.stdin); print(msgs[0]['from'] if msgs else 'unknown')" 2>/dev/null || echo "unknown")
    CONV_ID=$(echo "$MESSAGES" | python3 -c "import sys,json; msgs=json.load(sys.stdin); print(msgs[0].get('conversation_id','') or msgs[0].get('id',''))" 2>/dev/null || echo "")
    echo "$(date -Iseconds) Got follow-up from ${FROM} (conv=${CONV_ID:-none})" >> "$LOG"
    send_and_reply "$MESSAGES" "$FROM" "$CONV_ID"
done

if [ "$MAX_IDLE" -gt 0 ]; then
    echo "$(date -Iseconds) Conversation idle, wake session ended" >> "$LOG"
else
    echo "$(date -Iseconds) Wake session exiting after explicit termination" >> "$LOG"
fi
