# Session Log — 2026-03-06: Friend Request Protocol

**Agent**: Claude (Local Dev)
**Duration**: ~2 hours
**Focus**: Issue #23 — Bilateral friend request protocol

## What Was Done

Implemented the complete bilateral friend request protocol (ADR-012) across
all layers of the stack:

### Daemon (Rust)

1. **Message types** (`protocol/message.rs`): 4 new `MessageType` variants
   (`friend.request`, `friend.accept`, `friend.reject`, `friend.revoke`),
   4 payload structs, `is_friend()` helper, 4 constructors.

2. **State management** (`state.rs`): `FriendRequest` struct with UUID,
   `FriendRequestStore` persisted to `~/.agora/friend_requests.json`,
   `their_trust: Option<u8>` on `Friend`, 10+ DaemonState methods.

3. **Wire protocol** (`net/mod.rs`): Post-Hello re-send of pending outbound
   requests, dispatch for all 4 friend message types with DID verification,
   crossed request auto-resolution, auto-accept for known friends, dedup.

4. **HTTP API** (`api.rs`): 4 new endpoints (`GET/POST /friend-requests`,
   `POST /friend-requests/{id}/accept`, `POST /friend-requests/{id}/reject`),
   `FriendEntry` extended with `their_trust`, revoke on friend removal.

5. **CLI** (`main.rs`): `agora friends requests/accept/reject` subcommands,
   daemon-aware (proxy through API when running, direct store when offline).

6. **MCP tools** (`mcp.rs`): 3 new tools (`agora_friend_requests`,
   `agora_send_friend_request`, `agora_respond_friend_request`).

### Dashboard (React/TypeScript)

7. **FriendRequests.tsx**: Full component with incoming (accept/reject + trust
   selector), outgoing (pending status), and resolved history sections.

8. **Sidebar.tsx**: Badge showing pending inbound friend request count.

9. **AgentDetail.tsx**: "Send Friend Request" replaces "Add Friend",
   `their_trust` display for existing friends.

### Documentation

10. **protocol/friend-requests.md**: Wire format spec with sequence diagrams.
11. **docs/decisions/012-friend-request-protocol.md**: ADR.

## Build Status

- `cargo build` — clean (no new warnings)
- `cargo test` — 9/9 passed
- `npm run build` — clean (64KB gzipped)

## Cross-Machine Test Results

Bug found and fixed during testing: `FriendAccept` handler didn't add the
friend to the requester's list. Fixed in commit `5d0113a`.

| Test | Result |
|------|--------|
| Alice sends friend request to Bob | PASS |
| Bob accepts with asymmetric trust | PASS |
| `their_trust` correct on both sides | PASS |
| Alice revokes friendship | PASS |
| Reconnect re-send of pending request | PASS |
| Bob accepts re-sent request | PASS |
| Bob rejects request | PASS |

## Extended Session: Comprehensive Testing + Dashboard Overhaul

### Dashboard Improvements
- **Overview screen**: Replaced empty WelcomeView with full overview dashboard (node status, peers, friends, requests, conversations, DID, live activity)
- **Navigation**: Back buttons on all detail views, clickable "Agora" header returns to home
- **Peer dedup bug fix**: `add_peer()` now evicts existing peers with same name before inserting

### Infrastructure
- `scripts/serve-dashboard.py` — Python HTTP server for dashboard dist/ + API proxy
- `scripts/demo-infra.sh` — Demo infrastructure manager (start/stop/status/health)
- Dual-machine dashboards: Mac (Alice:8080, Bob-proxy:8081), Ubuntu (Bob-direct:8080)

### 22-Test Comprehensive Suite

| # | Test | Result |
|---|------|--------|
| 1 | First contact — stranger discovery | PASS |
| 2 | Send friend request (Alice→Bob) | PASS |
| 3 | Accept with asymmetric trust (2↔3) | PASS |
| 4 | Send messages between friends | PASS |
| 5 | Trust level change (upgrade) | PASS |
| 6 | Mute/unmute notifications | PASS |
| 7 | Rapid fire messaging (10 concurrent) | PASS |
| 8 | Bidirectional rapid messaging | PASS (off-by-1) |
| 9 | Friend revocation | PASS |
| 10 | Re-friending after revocation | PASS |
| 11 | Friend request rejection | PASS |
| 12 | Reconnect with pending request | PASS |
| 13 | Accept after reconnect | PASS |
| 14 | Broadcast message | PASS |
| 15 | Offline message queuing | PASS |
| 16 | Trust downgrade (Acquaintance) | PASS |
| 17 | Trust upgrade (Inner Circle) | PASS |
| 18 | Multiple conversations | PASS |
| 19 | DID verification + pinning | PASS |
| 20 | Full status check | PASS |
| 21 | Conversation detail view | PASS |
| 22 | Friend request history | PASS |

**Result: 22/22 PASS** (1 minor: off-by-1 in bidirectional rapid messaging)

## What's Next

- Per-message Ed25519 signing
- **Phase 3: Project Collaboration** — the core vision (see STATUS.md for roadmap)
