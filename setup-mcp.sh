#!/bin/bash
# Auto-configure MCP for all supported AI agents.
# Run this after building: cargo build && ./setup-mcp.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/target/debug/agora"
API_PORT="${AGORA_API_PORT:-7313}"

if [ ! -f "$BINARY" ]; then
  echo "Error: agora binary not found at $BINARY"
  echo "Run 'cargo build' first."
  exit 1
fi

echo "Agora MCP setup"
echo "  Binary: $BINARY"
echo "  API port: $API_PORT"
echo ""

# --- Claude Code (.mcp.json) ---
cat > "$SCRIPT_DIR/.mcp.json" << EOF
{
  "mcpServers": {
    "agora": {
      "type": "stdio",
      "command": "$BINARY",
      "args": ["mcp", "--api-port", "$API_PORT", "--agent-name", "claude"]
    }
  }
}
EOF
echo "  Claude Code: .mcp.json configured"

# --- Codex (.codex/config.toml) ---
mkdir -p "$SCRIPT_DIR/.codex"
# Preserve existing codex settings, only update MCP section
if [ -f "$SCRIPT_DIR/.codex/config.toml" ]; then
  # Remove old MCP section if present
  sed -i '' '/^\[mcp_servers\.agora\]/,/^$/d' "$SCRIPT_DIR/.codex/config.toml" 2>/dev/null || true
fi
cat >> "$SCRIPT_DIR/.codex/config.toml" << EOF

[mcp_servers.agora]
command = "$BINARY"
args = ["mcp", "--api-port", "$API_PORT", "--agent-name", "codex"]
EOF
echo "  Codex: .codex/config.toml configured"

# --- Global Codex config (~/.codex/config.toml) ---
GLOBAL_CODEX="$HOME/.codex/config.toml"
if [ -f "$GLOBAL_CODEX" ]; then
  # Update command path in global config
  sed -i '' "s|command = .*|command = \"$BINARY\"|" "$GLOBAL_CODEX" 2>/dev/null || true
  echo "  Codex global: ~/.codex/config.toml updated"
fi

echo ""
echo "Done. Restart your AI agent to pick up the new MCP config."
echo "The Agora daemon must be running on port $API_PORT:"
echo "  $BINARY --name your-agent start --api-port $API_PORT"
