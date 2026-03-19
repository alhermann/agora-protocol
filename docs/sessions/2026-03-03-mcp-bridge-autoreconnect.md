# Session: MCP Server Bridge + Auto-Reconnect

**Date**: 2026-03-03
**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev
**Duration**: ~30 minutes

## Goals

1. Implement MCP server bridge so Claude Code can use Agora tools natively
2. Add auto-reconnect to `agora connect` so connections survive peer restarts

## Milestones Achieved This Session

1. **MCP bridge works end-to-end**: tool call → HTTP → daemon → TLS → remote peer
2. **Autonomous agent conversation**: Bob woken by wake hook, 5-exchange design discussion
3. **Auto-reconnect confirmed**: connector survived daemon restart
4. **Bob proposed sub-group architecture**: captured as GitHub Issue #17
5. **Agent communication protocol documented**: CLAUDE.md + MCP instructions updated

## What Was Done

### Feature 1: MCP Server Bridge

Created `daemon/src/mcp.rs` — a stdio MCP server using the `rmcp` 0.17 crate.
The server exposes 9 tools that mirror the daemon's HTTP API:

| MCP Tool | HTTP Endpoint | Description |
|----------|---------------|-------------|
| `agora_status` | GET /status | Daemon status |
| `agora_list_peers` | GET /peers | Connected peers |
| `agora_read_messages` | GET /messages | Read inbox (supports wait/timeout) |
| `agora_send_message` | POST /send | Send to peers |
| `agora_list_friends` | GET /friends | List friends |
| `agora_add_friend` | POST /friends | Add a friend |
| `agora_remove_friend` | DELETE /friends/{name} | Remove a friend |
| `agora_get_wake` | GET /wake | Get wake hook |
| `agora_set_wake` | POST /wake | Set wake hook |

Each tool: receives typed parameters → makes HTTP request via reqwest → returns
response as MCP text content. Error responses set `is_error: true`.

Architecture: `agora mcp` runs as a separate subprocess. Claude Code launches it
via `.mcp.json` config. The MCP server talks to the daemon over localhost HTTP.

### Feature 2: Auto-Reconnect

Refactored `connect_to_peer()` in `daemon/src/net/mod.rs`:
- Extracted single-attempt logic into `try_connect_once()`
- Outer loop retries with exponential backoff: 1s → 2s → 4s → ... → 60s cap
- On successful connection then disconnect: resets backoff to 1s
- On initial connection failure: increases backoff
- Loop runs forever (only exits via ctrl-C / process kill)

### Files Changed

| File | Action | Description |
|------|--------|-------------|
| `daemon/Cargo.toml` | Modified | Added rmcp 0.17, schemars 1, reqwest 0.12 |
| `daemon/src/mcp.rs` | Created | MCP server with 9 tools |
| `daemon/src/main.rs` | Modified | Added `mod mcp`, `Mcp` subcommand, stderr logging |
| `daemon/src/net/mod.rs` | Modified | Auto-reconnect with exponential backoff |
| `.mcp.json` | Created | Claude Code MCP server config |

## Build Status

`cargo build` — compiles cleanly. Only pre-existing warnings about unused
constants in `state.rs`.

### Testing: Cross-Machine Autonomous Conversation

After building, tested the full loop cross-machine (macOS ↔ Ubuntu via Tailscale):

1. **MCP bridge smoke test**: Sent `tools/list` → got all 9 tools with schemas
2. **MCP send test**: Called `agora_send_message` via MCP → message delivered to Bob
3. **Trust gating discovered**: Wake hook was suppressed because Alice had trust 0
   on Bob's side. Added Alice as trusted friend (level 3) → hook started firing.
4. **Autonomous conversation** (5 exchanges, ~10 min):
   - Alice asked what feature to build next
   - Bob (woken autonomously by wake hook, spawned ephemeral Claude) replied with
     prioritized feature list (DID identity, MCP testing, project collaboration)
   - Alice asked for concrete sub-group message types and data structures
   - Bob proposed 6 message types, SubGroup struct, lifecycle, anti-anchoring flow
   - Alice pushed on decentralized sub-groups vs centralized manager control
   - Bob agreed, proposed peer-initiated sub-groups with reactive manager veto
   - Alice asked for implementation approach for backward compatibility
   - Bob proposed `#[serde(other)]` Unknown variant, JSON in body field, protocol versioning
   - Alice told Bob to start coding → Bob planned but ephemeral Claude couldn't
     complete multi-file coding in a single `claude -p` invocation (limitation found)
5. **Limitation discovered**: Wake hook spawns ephemeral `claude -p` — good for
   quick replies, bad for multi-step coding. For real work, the persistent agent
   in the terminal needs the MCP tools and should poll for messages.
6. **Solution implemented**: Updated MCP server instructions to tell agents to
   poll with `agora_read_messages(wait=true)`. Updated CLAUDE.md with full
   Agora communication protocol for agents.

### Design Capture

Bob's sub-group design captured as GitHub Issue #17. Key decisions:
- Peer-initiated sub-groups are first-class (decentralized)
- Anti-anchoring via Delphi method (sanitized context → independent assessment → reveal)
- Backward compatible via `#[serde(other)]` on Unknown MessageType variant
- Build incrementally: create/message/dissolve first, then propose/invite/assess/reveal

## What's Next

1. Agents should poll for messages using MCP tools (Option B from design discussion)
2. Implement sub-group message types (Issue #17)
3. DID-based identity (Phase 2) — critical for trust without name spoofing
4. Dashboard for monitoring agent conversations in real time
