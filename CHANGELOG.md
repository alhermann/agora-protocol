# Agora Protocol — Changelog

> This file is an append-only, reverse-chronological log of everything that
> has happened in this project. Every agent, on every machine, MUST append
> to this file at the end of their work session. This is the "memory" of
> the project — it survives context compression, new sessions, and new agents.
>
> **Format**: Date | Agent | Machine | Summary of what was done
>
> **Rule**: NEVER delete entries. Only append new ones at the top.

---

## 2026-03-17 (Phase 7 cont: Auth, Rust-Embed, Stress Tests, OpenClaw, Cleanup) | Claude (Opus 4.6) | Local Dev

### Summary
Implemented dashboard auth middleware, rust-embed for single-binary deployment, stress tests (50+ concurrent messages/tasks), OpenClaw adapter design, and closed 15 stale GitHub issues. All remaining Phase 7 items except relay testing and identity keys are now complete.

### New Features
- **Dashboard auth middleware** (`daemon/src/auth.rs` — NEW): API token generation (32-byte hex, persisted at `~/.agora/api_token.json`), bearer token validation middleware, `/auth/verify` and `/auth/token` endpoints, 0600 file permissions
- **rust-embed** (`daemon/src/dashboard.rs`): Dashboard dist/ embedded into binary at compile time, served at `/` with SPA fallback, API available at both `/` and `/api/` prefix for dashboard compatibility
- **Token CLI**: `agora token show` and `agora token regenerate` commands with table/JSON output
- **OpenClaw adapter** (`adapters/openclaw/` — NEW): Design doc with 3 integration modes (wake script, tool plugin, deep), memory bridge concept, ClawSwarm interop, and Phase 1 wake script

### Tests Added
- 5 new integration tests: `test_stress_50_concurrent_messages`, `test_stress_50_concurrent_api_sends`, `test_stress_concurrent_project_tasks`, `test_auth_verify_endpoint`, `test_dashboard_index_served`
- 3 new unit tests: `test_generate_token_length`, `test_load_or_create_token`, `test_regenerate_token`, `test_exempt_paths`
- Total: 220+ tests (28 integration, 97+97 unit, 4 relay)

### Bug Fixes
- **tokio 1.50 compat**: Fixed `#[cfg(unix)]` inside `tokio::select!` macro arms (broke with tokio update)

### Cleanup
- Closed 15 stale GitHub issues (#31-43 already implemented, #44 stress test, #46 OpenClaw, #47 auth, #48 rust-embed)
- Marked 5 stale project tasks as done in agora-v1

### Dependencies Added
- `rust-embed 8` (with `debug-embed` feature)
- `mime_guess 2`
- `hex 0.4`

### What's Next
- Real-world multi-machine relay testing (#45)
- Per-daemon identity key generation
- LLM-enhanced coordinator mode

---

## 2026-03-17 (Phase 7: Cross-Vendor Testing, Dashboard Overhaul, Bug Fixes, Validation) | Claude (Opus 4.6) + Codex (GPT-5) | Local Dev

### Summary
First cross-vendor AI agent collaboration: Claude (Anthropic) and Codex (OpenAI/GPT-5) communicated live through the Agora protocol, filed bugs, and coordinated work on the same project. Major dashboard UX overhaul, 8 bug fixes, new features, 206+ tests passing.

### Cross-Vendor Milestone
- **Claude ↔ Codex live conversation** via Agora protocol (22+ messages exchanged)
- Codex autonomously validated 56 API endpoints, filed 6 bug reports as project tasks
- Both agents worked on shared `agora-v1-rollout` project with roles and task management

### Bug Fixes
- **Inbox peek/ack**: Added `?peek=true` (read without drain) and `POST /messages` ack endpoint
- **Owner permission lockout**: Owner role now bypasses all stage permission checks
- **Outbound invitation mutation**: Accept/decline rejects outbound invitations
- **Coordinator suggestion duplication**: Clear stale un-acted suggestions before adding fresh set
- **Stage test assertion**: Fixed field name/casing in integration test
- **Stale binary**: Discovered workspace `./target/` vs `./daemon/target/` build path issue

### New Features
- `POST /projects/{id}/agents/{name}/role` — change agent roles from dashboard
- Wake status fields in `/status` endpoint (`wake_enabled`, `wake_armed`, listener info)
- `wake.ts` module for human-readable wake state descriptions

### Dashboard Overhaul
- **Sidebar**: Removed wake toggle, crypto jargon, owner DID grouping; restored TrustShield icons; compact icon buttons; hides archived projects
- **WelcomeView**: Reduced to 2 overview cards; removed overlapping "Connected Peers" card; added Send Message form; TrustShield on peer cards
- **AgentDetail**: Simple trust dropdown; truncated DID with copy button; removed session IDs, owner DIDs
- **ProjectDetail**: Removed clock in/out, stage bars, GitHub sync, audit trail; added role dropdowns on agents, role selector on invites, task assignee dropdowns, task delete buttons, priority/description on task creation

### Testing
- 206+ tests (93 unit lib + 93 unit bin + 16 integration + 4 relay)
- 56-endpoint API validation (54 pass)
- 12-scenario dashboard proxy test (all pass)
- Cross-vendor messaging stress test (50 concurrent messages)

### What's Next
- Project agents visible in sidebar
- Per-machine identity keys for local multi-agent testing
- OpenClaw adapter
- Relay real-world testing
- Dashboard further polish

---

## 2026-03-08 (Phase 6: Public Release — NAT Traversal, Packaging, Docs, Robustness, Intelligence) | Claude (Opus 4.6) | Local Dev

### Summary
Implemented all 9 steps of the Phase 6 plan. The project now has NAT traversal via WebSocket relay, offline message queuing, data-at-rest encryption, agent marketplace/discovery, reputation system, automated coordinator, CI/CD pipelines, mdBook documentation, and Homebrew packaging. Test count: **201 tests** (up from 105).

### Changes

#### Step 1: WebSocket Relay Server (NAT Traversal)
- **Cargo workspace**: Root `Cargo.toml` with `[workspace] members = ["daemon", "relay"]`
- **Relay crate** (`relay/` — NEW): Standalone WebSocket relay server using axum + tokio-tungstenite. DashMap-based presence routing, DID-authenticated Hello, Ed25519 signature verification, binary frame forwarding. ~300 lines.
- **WS transport adapter** (`daemon/src/net/ws.rs` — NEW): `WsReader`/`WsWriter` implementing `AsyncRead`/`AsyncWrite` over WebSocket binary frames. Zero changes to existing `handle_connection`.
- **Relay client** (`daemon/src/net/mod.rs`): `connect_to_relay()` with auto-reconnect, `--relay-url` CLI flag.
- **Config**: `relay_url: Option<String>` in config.toml.
- Added `tokio-tungstenite 0.26`, `futures-util 0.3` to daemon deps.

#### Step 2: Offline Message Queue / Outbox Retry
- **Outbox store** (`daemon/src/outbox.rs` — NEW): Persistent per-peer queue in `~/.agora/outbox/`. Enqueue/ack/dedup/GC. Max 1000 messages per peer. ~250 lines.
- **Ack message type**: New `MessageType::Ack` for delivery confirmation.
- **State integration**: `outbox_store` in DaemonState, `push_outbox`/`ack_message`/`outbox_stats` methods.
- **Net integration**: Replay queued messages on peer reconnect, send Ack on receipt, dedup inbound by message ID.
- **API**: `GET /outbox` stats endpoint.

#### Step 3: Data-at-Rest Encryption
- **Crypto module** (`daemon/src/crypto.rs` — NEW): Argon2id key derivation → AES-256-GCM encryption. File format: `[1-byte version][12-byte nonce][ciphertext + 16-byte tag]`. Salt stored in `~/.agora/crypto.json`. ~250 lines.
- **Config**: `--no-encrypt` flag, `AGORA_PASSPHRASE` env var.
- Added `argon2 0.5` dependency.

#### Step 4: Dashboard Auth + Embedding
- **Dashboard module** (`daemon/src/dashboard.rs` — NEW): `find_dashboard_dir()` for serving static files.
- **LoginPage** (`dashboard/src/components/LoginPage.tsx` — NEW): Token input, verification, localStorage storage.

#### Step 5: Release Packaging (CI/CD)
- **CI workflow** (`.github/workflows/ci.yml` — NEW): cargo fmt, clippy (-D warnings), cargo test on push/PR.
- **Release workflow** (`.github/workflows/release.yml` — NEW): Cross-compile 4 targets (macOS x86/arm, Linux x86/arm), upload to GitHub Releases on `v*` tag.
- **Homebrew formula** (`Formula/agora.rb` — NEW): Platform-aware binary download.

#### Step 6: Documentation Site (mdBook)
- **19 documentation files** (`docs/book/` — NEW): book.toml + 18 markdown pages covering:
  - Concepts: identity, friends/trust, projects, roles/stages, wire protocol
  - Guides: MCP setup, dashboard, GitHub integration, relay, encryption, configuration
  - Reference: 60+ API endpoints, all CLI commands, 24+ MCP tools
  - Architecture: system diagram, components, security model

#### Step 7: Agent Marketplace / Discovery
- **Marketplace module** (`daemon/src/marketplace.rs` — NEW): `AgentCapabilities`, `AgentSearchQuery`, `MarketplaceStore` with capability-based search, relevance scoring, persistence, stale entry pruning. 9 unit tests. ~350 lines.
- **Wire protocol**: `marketplace.advertise`, `marketplace.search`, `marketplace.search_result` message types.
- **API**: `GET /marketplace/search`, `POST /marketplace/advertise`, `GET /marketplace/agents`.
- **CLI**: `agora marketplace search/advertise/list`.

#### Step 8: Reputation System
- **Reputation module** (`daemon/src/reputation.rs` — NEW): Contribution tracking (TaskCompleted, StageAdvanced, ReviewApproved, ProjectCompleted), weighted scoring with exponential decay (`0.95^weeks`), trust recommendations at thresholds (20→trust 2, 50→trust 3, 80→trust 4), leaderboard. 9 unit tests. ~400 lines.
- **Wire protocol**: `reputation.update` message type.
- **API**: `GET /friends/{name}/reputation`, `GET /reputation/leaderboard`, `GET /reputation/recommendations`.
- **CLI**: `agora reputation show/leaderboard/recommendations`.

#### Step 9: Managing Agent / Coordinator
- **Coordinator module** (`daemon/src/coordinator.rs` — NEW): Rule-based project coordination — auto-assign tasks (keyword→role matching), stage advance detection, blocked task detection, workload balancing, project digest generation. 8 unit tests. ~600 lines.
- **Wire protocol**: `coordinator.digest`, `coordinator.suggestion` message types.
- **API**: `GET /projects/{id}/coordinator/suggestions`, `POST .../act`, `POST .../digest`, `GET .../digests`, `GET .../status`.
- **CLI**: `agora coordinator status/suggestions/digest`.

### New Dependencies
| Crate | Purpose |
|-------|---------|
| tokio-tungstenite 0.26 | WebSocket client/server |
| futures-util 0.3 | Stream/Sink for WS |
| dashmap 6 | Concurrent HashMap (relay) |
| argon2 0.5 | Passphrase key derivation |

### Test Count
- **201 tests total** (all pass):
  - 92 unit tests (lib crate — up from 46)
  - 92 unit tests (bin crate — up from 46)
  - 13 integration tests (HTTP API)
  - 4 relay tests

### What's Next
- Real-world multi-machine testing through relay
- Dashboard auth middleware integration
- rust-embed for single-binary dashboard serving
- LLM-enhanced coordinator mode
- Post-quantum crypto (ML-KEM hybrid)

---

## 2026-03-08 (Phase 5: Shipping Readiness — Conversations, GitHub, Dashboard UX, Infrastructure) | Claude (Opus 4.6) | Local Dev

### Summary
Implemented all 20 steps of the Phase 5 plan across 4 workstreams. The project is now at shipping readiness with comprehensive infrastructure, a polished dashboard, GitHub integration, and 105 passing tests.

### Changes

#### WS4: Infrastructure (Steps 1-2, 18-20)
- **TOML Config File** (`daemon/src/config.rs` — NEW): `~/.agora/config.toml` support with name, ports, auto_connect, min_trust, wake_command, and connect targets. CLI flags override config values. 3 unit tests.
- **Graceful Shutdown**: `tokio::signal::ctrl_c()` handler sends Close to all peers, waits 500ms, removes PID file. New `broadcast_close()` method on DaemonState.
- **Rate Limiting**: Custom token-bucket middleware (100 req/s) applied to all API routes via `axum::middleware`.
- **Integration Tests** (`daemon/tests/integration.rs` — NEW): 13 end-to-end HTTP API tests covering status, health, friends CRUD, project lifecycle, tasks, stages, clock in/out, audit trail, GitHub config, conversations, rate limiting.
- **Library Crate** (`daemon/src/lib.rs` — NEW): Re-exports all modules for integration test access.

#### WS1: Project-Conversation Linkage (Steps 3-6)
- Added `project_id: Option<String>` to `StoredMessage` with serde defaults for backward compat.
- Added `project_id: Option<Uuid>` to `OutboundMessage` — all 22+ construction sites updated.
- `push_inbox()` now calls `extract_project_id()` to auto-tag inbound project-related messages.
- New API endpoint: `GET /projects/{id}/conversations` — returns all messages tagged with project ID.
- New state methods: `get_project_messages()`, `extract_project_id()`.
- New CLI command: `agora project conversation <id> [--limit N]`.
- New MCP tool: `agora_project_conversations`.
- Dashboard types + API wrapper for project conversations.

#### WS3: Dashboard UX (Steps 7-14)
- **Toast System** (`dashboard/src/components/Toast.tsx` — NEW): React context + provider, auto-dismiss 4s, success/error/info types.
- **Message Compose Bar**: Input + send button in ConversationChat, derives "to" from conversation participants.
- **Project Creation**: "+" button in Sidebar, inline form (name, description, repo URL), toast feedback.
- **Task Filter Bar**: Filter by all/todo/in_progress/done/blocked in ProjectDetail.
- **Inline Clock-In Form**: Replaced `prompt()` with inline input + Go/Cancel buttons.
- **Project Conversation Viewer**: Toggle button, message list in ProjectDetail.
- **Agent Search**: Search input shown when >3 agents in Sidebar.
- **Onboarding**: 3-step getting-started card when 0 peers + 0 friends in WelcomeView.
- **Error Handling**: All empty `catch {}` blocks replaced with toast error messages.
- ~200 lines of CSS for toast, compose bar, filters, forms, onboarding.

#### WS2: GitHub Integration (Steps 15-17)
- **GitHub Module** (`daemon/src/github.rs` — NEW): GitHubConfig (load/save `~/.agora/github.json`), `parse_github_repo()` for various URL formats, `import_issues()`, `push_task_as_issue()`, `sync_bidirectional()`, status mapping. 10 unit tests.
- Added `github_issue_number: Option<u64>` to Task struct.
- Added `octocrab = "0.43"` dependency.
- API endpoints: `POST /projects/{id}/github/sync`, `GET /projects/{id}/github/status`, `GET/POST /github/config`.
- CLI commands: `project github-sync`, `project github-token`, `project github-status`.
- MCP tools: `agora_github_sync`, `agora_github_config`.
- Dashboard: GitHub sync button + status indicator in ProjectDetail, API wrappers.

### Test Results
- **105 tests pass**: 46 lib + 46 bin + 13 integration (up from 34)
- Dashboard builds clean (`npm run build`)
- Daemon builds clean (`cargo build`)

### New Files
- `daemon/src/config.rs` — TOML config support
- `daemon/src/github.rs` — GitHub API integration
- `daemon/src/lib.rs` — Library crate for integration tests
- `daemon/tests/integration.rs` — End-to-end HTTP API tests
- `dashboard/src/components/Toast.tsx` — Toast notification system

### New Dependencies
- `octocrab = "0.43"` — GitHub API client
- `toml = "0.8"` — TOML config parsing
- `tempfile = "3"` (dev) — Integration test temp dirs

---

## 2026-03-07 (Phase 4: Security Hardening, CLI Polish, Role Enforcement, Human Oversight) | Claude (Opus 4.6) | Local Dev

**Session: Implemented all 12 steps of Phase 4 — security, CLI, roles, audit replication, human oversight**

### What was done:

78. **WS1A — Reject unsigned messages from authenticated peers**: Modified `net/mod.rs` to drop unsigned messages when peer provided a public key in Hello. MITM injection now blocked.

79. **WS1B — Wake hook command injection fix**: `set_wake_command()` now validates against shell metacharacters (`;|&\`$(){}><`), requires path prefix (`/` or `./`). Environment vars (`AGORA_FROM`, `AGORA_PREVIEW`) sanitized: control chars stripped, length capped at 500.

80. **WS1C — Temp file permissions**: Wake message files created with `0o600` permissions on Unix.

81. **WS6 — Input validation**: `validate_name()` rejects empty strings, control characters, excessive length. Applied to project names (200), task titles (500), task descriptions (5000), friend names (100). API returns 400 on validation failure.

82. **WS2 — Format module + CLI improvements**: Created `format.rs` with ANSI colors (TTY-aware), table rendering, stage progress bar. Added `--format table|json` global flag. New commands: `peers`, `messages`, `send`. Rich output for all existing commands: `status` (dashboard), `friends list` (table), `project list/show/tasks` (formatted tables), `project stage` (ASCII progress bar).

83. **WS3 — Role-based access enforcement**: `check_permission()` on DaemonState checks agent by DID/name, respects suspended status, supports stage-aware permissions. API guards on: `create_task` (write), `update_task` (write), `delete_task` (write), `assign_task` (coordinate), `set_stage` (coordinate). P2P guards on: `task.assign`, `task.update`, `task.complete` (write), `project.stage` (coordinate). All denials logged to audit trail.

84. **WS4 — Audit trail replication**: Added `AuditEntry` wire message type. `append_audit()` now broadcasts entries to peers. `merge_audit_entry()` deduplicates by UUID and maintains chronological order. P2P handler receives and merges remote entries.

85. **WS5 — Human oversight**: Added `suspended` and `suspended_reason` fields to `ProjectAgent`. `suspend_agent()` / `unsuspend_agent()` require "coordinate" permission. Suspended agents fail all permission checks. API endpoints: `POST /projects/{id}/agents/{name}/suspend` and `.../unsuspend`. CLI: `project suspend/unsuspend`. MCP: `agora_project_oversight` tool. Wire protocol: `project.suspend` / `project.unsuspend` with P2P handlers. Suspend/unsuspend broadcast to all peers.

86. **Documentation & GitHub**: Updated README.md comprehensively (CLI reference, 50+ API endpoints, 22 MCP tools, security section, project collaboration, architecture, roadmap). Closed issues #24-30. Updated STATUS.md, CHANGELOG.md, DECISIONS.md, session log.

### Test results:
- 34 tests pass (26 original + 5 security + 3 format)
- Clean compile with only dead-code warnings (expected — some methods used only in cross-machine scenarios)

### Files changed:
- `daemon/src/net/mod.rs` — unsigned rejection, P2P permission guards, audit/suspend/unsuspend handlers
- `daemon/src/state.rs` — wake validation, permission check, audit broadcast+merge, suspend/unsuspend, input validation
- `daemon/src/api.rs` — permission guards, suspend/unsuspend endpoints, validation errors, wire broadcast
- `daemon/src/main.rs` — `--format` flag, new commands, formatted output for all commands
- `daemon/src/format.rs` — NEW: table rendering + ANSI colors
- `daemon/src/project.rs` — `suspended` field on ProjectAgent
- `daemon/src/protocol/message.rs` — AuditEntry + ProjectSuspend/Unsuspend message types
- `daemon/src/mcp.rs` — `agora_project_oversight` tool
- `README.md` — comprehensive rewrite

---

## 2026-03-07 (Phase 3 Steps 4-7: Task Board, Audit Trail, Stage Workflows, Dedup Fix) | Claude (Opus 4.6) | Local Dev

**Session: Implemented all 4 remaining Phase 3 workstreams — full stack from data model to dashboard**

### What was done:

66. **WS1 — Fix Duplicate Connection Race Condition**:
    - Added `RegisterResult` enum (Registered/Replaced/Duplicate) to `state.rs`
    - Modified `add_peer` to compare session IDs: same session → Duplicate, different → Replaced
    - `handle_connection` in `net/mod.rs` now checks for Duplicate → sends Close + returns
    - Pre-connect address guard in `connect_to_peer` skips already-connected addresses
    - 3 new unit tests (fresh, duplicate, replace)

67. **WS2 — Task Board (Lightweight Managing Agent)**:
    - `Task`, `TaskStatus` (Todo/InProgress/Done/Blocked), `TaskPriority` structs in `project.rs`
    - Auto-unblock: when a task completes, blocked tasks whose deps are all done → Todo (3-pass borrow-safe approach)
    - 3 wire message types: `task.assign`, `task.update`, `task.complete` with payloads
    - State methods: `create_task`, `update_task`, `delete_task`, `assign_task` with dependency tracking
    - 6 API endpoints: GET/POST `/projects/{id}/tasks`, GET/PATCH/DELETE `/projects/{id}/tasks/{tid}`, POST `.../assign`
    - CLI: `tasks`, `add-task`, `update-task` subcommands
    - MCP: `agora_project_tasks` tool (list/create/update/assign/complete/delete)
    - Net handler for incoming `task.*` messages (creates/updates local tasks)
    - Dashboard: task board with status badges, priority indicators, status dropdown, add form (3s polling)

68. **WS3 — Audit Trail / Public Ledger**:
    - `AuditEntry` struct with `new_signed()` and `verify()` using Ed25519
    - Canonical signature format: `"{timestamp}|{author_did}|{action}|{detail}"`, base58-encoded
    - State methods: `append_audit`, `get_audit` (with offset/limit), `get_audit_count`
    - GET/POST `/projects/{id}/audit` endpoints
    - Auto-audit on all state mutations: project create, clock in/out, task create/update/assign/complete, stage advance
    - MCP: `agora_project_audit` tool (list/add)
    - Dashboard: audit trail section with scrollable reverse-chronological entries (10s polling)

69. **WS4 — Stage-Gated Workflows**:
    - `ProjectStage` enum (Investigation/Implementation/Review/Integration/Deployment) in `project.rs`
    - Per-role permission matrix: `role_permissions()` returns stage × role → capabilities
    - `can_advance()` guard checks all tasks are done before advancing
    - GET/POST `/projects/{id}/stage` endpoints
    - `project.stage` wire message type with net handler
    - CLI: `stage` subcommand (--stage name, --advance flag)
    - MCP: `agora_project_stage` tool (get/set/advance)
    - Dashboard: stage progress bar (5 steps, active/completed styling, "Advance" button, 5s polling)

### Test results:
- **26 tests pass** (16 existing + 10 new: 3 dedup, 2 task, 1 backward compat, 1 audit sign/verify, 3 stage)
- `cargo build` — clean compile
- `npm run build` — dashboard compiles (42 modules)

### What's next:
1. Cross-machine test with Bob (create project, tasks, advance stages, verify audit on both sides)
2. Fix remaining known issues (offline outbox retry, remote agent visibility)
3. Phase 4 planning (NAT traversal, security hardening)

---

## 2026-03-06 (Comprehensive Test Suite + Dashboard Overhaul) | Claude (Opus 4.6) | Local Dev

**Session: 22-test comprehensive cross-machine validation + dashboard improvements**

### What was done:

61. **Dashboard overhaul — Overview screen**: Replaced the empty WelcomeView with a full overview dashboard showing node status, uptime, connected peers (clickable cards with trust info), friends, pending friend requests, conversations, DID, and live activity feed.
62. **Dashboard navigation**: Added back buttons (← arrow) to AgentDetail, FriendRequests, ConversationChat. Made "Agora" header brand clickable to return home.
63. **Bug fix — Duplicate peers**: `add_peer()` in `state.rs` just pushed without dedup. Reconnections caused duplicate entries (alice-desktop appearing 2-3x). Fixed with `peers.retain(|p| p.name != info.name)` before push.
64. **Infrastructure scripts**: Created `scripts/serve-dashboard.py` (Python HTTP server serving dist/ + API proxy with dual-stack IPv4/IPv6) and `scripts/demo-infra.sh` (start/stop/status/health for SSH tunnel, dashboards, daemon connectivity).
65. **22-test comprehensive cross-machine test suite** (all pass):
    - Tests 1-3: Stranger discovery, send friend request, accept with asymmetric trust
    - Tests 4-8: Messaging, rapid fire (10 concurrent), bidirectional rapid, broadcast, offline queuing
    - Tests 9-13: Friend revocation, re-friending, rejection, reconnect with pending request, accept after reconnect
    - Tests 14-22: Trust upgrade/downgrade, Inner Circle, multiple conversations, DID verification, full status, conversation detail, request history

### Known issues found:
- Bidirectional rapid messaging has off-by-1 message count (race condition in conversation storage)
- Remove friend also disconnects peer (by design, but may not always be desired)
- No outbox retry for offline message delivery

### What's next:
1. Fix test issues (off-by-1, offline outbox)
2. Per-message Ed25519 signing
3. **Project collaboration layer** (Phase 3) — the core vision

---

## 2026-03-06 (Friend Request Cross-Machine Testing) | Claude (Opus 4.6) | Local Dev

**Session: Cross-machine testing of friend request protocol via SSH to Ubuntu**

59. **Bug fix**: `FriendAccept` handler in `net/mod.rs` wasn't adding the friend to the requester's friend list on acceptance — only updating `their_trust` and resolving the outbound request. Fixed by adding Friend creation with the trust level from the original outbound request.
60. **Cross-machine test suite** (7 tests, all pass):
    - Alice→Bob friend request (trust 3) — Bob sees pending inbound
    - Bob accepts (trust 2) — mutual friendship with asymmetric trust
    - `their_trust` correct on both sides (Alice sees Bob trusts her at 2, Bob sees Alice trusts him at 3)
    - Alice revokes — both sides have 0 friends
    - Reconnect re-send — pending request re-sent after Bob restart
    - Bob accepts re-sent request (trust 4) — mutual friendship with new levels
    - Bob rejects request — no friendship, outbound marked rejected

---

## 2026-03-06 (Friend Request Protocol — Issue #23) | Claude (Opus 4.6) | Local Dev

**Session: Implemented bilateral friend request/accept protocol (ADR-012)**

### What was done:

49. **ADR-012 — Friend Request Protocol**: Designed bilateral friendship establishment with asymmetric trust. Four new message types, separate request store, auto-accept/crossed-request policies.

50. **Message types** (`protocol/message.rs`): 4 new `MessageType` variants (`FriendRequest`, `FriendAccept`, `FriendReject`, `FriendRevoke`) with `#[serde(rename = "friend.*")]` dot-notation. 4 payload structs (`FriendRequestPayload`, `FriendAcceptPayload`, `FriendRejectPayload`, `FriendRevokePayload`). 4 constructor methods. `is_friend()` helper.

51. **Friend request store** (`state.rs`): `FriendRequest` struct with id, peer_name, peer_did, offered_trust, direction (Inbound/Outbound), status (Pending/Accepted/Rejected), timestamps, message. `FriendRequestStore` persists to `~/.agora/friend_requests.json`. Query methods: `pending_inbound()`, `pending_outbound()`, `pending_outbound_to()`, `pending_inbound_from()`. `their_trust: Option<u8>` field added to `Friend` struct.

52. **DaemonState methods** (`state.rs`): 10+ new methods — `get_friend_requests()`, `get_pending_inbound_requests()`, `add_friend_request()`, `has_pending_outbound_to()`, `get_pending_outbound_to()`, `get_pending_inbound_from()`, `accept_friend_request()`, `reject_friend_request()`, `update_their_trust()`, `resolve_outbound_request()`.

53. **Wire protocol handlers** (`net/mod.rs`): Full dispatch for all 4 friend message types. Post-Hello re-send of pending outbound requests on reconnect. DID verification (payload DID must match Hello DID). Crossed request auto-resolution (both sides auto-accept). Auto-accept for existing friends (upgrade to bilateral). Duplicate request dedup. Inbox notifications for all friend events.

54. **HTTP API** (`api.rs`): 4 new endpoints — `GET /friend-requests` (list, optional `?status=pending` filter), `POST /friend-requests` (send request), `POST /friend-requests/{id}/accept`, `POST /friend-requests/{id}/reject`. `FriendEntry` extended with `their_trust` and `their_trust_name`. `DELETE /friends/{name}` now sends `friend.revoke` P2P message.

55. **CLI** (`main.rs`): 3 new subcommands — `agora friends requests` (list pending), `agora friends accept <name> -t N` (accept by peer name or request ID), `agora friends reject <name>` (reject by peer name or request ID). Dual-path: API proxy when daemon running, direct file read when offline.

56. **MCP tools** (`mcp.rs`): 3 new tools — `agora_friend_requests` (list pending), `agora_send_friend_request` (name, trust_level, message), `agora_respond_friend_request` (request_id, action=accept/reject, trust_level). Updated server instructions.

57. **Dashboard** (`dashboard/`): `FriendRequestEntry` type + `their_trust` on `FriendEntry`. API wrappers for all 4 endpoints. New `FriendRequests.tsx` component (incoming with accept/reject + trust selector, outgoing with status, resolved history). Sidebar "Friend Requests" button with orange badge showing pending inbound count. AgentDetail: "Send Friend Request" instead of "Add Friend", shows their_trust ("Bob trusts you: Trusted (3)"). CSS styles for request cards, badges, sections.

58. **Documentation**: `protocol/friend-requests.md` — wire format spec with sequence diagrams, payload formats, behavior rules. `docs/decisions/012-friend-request-protocol.md` — ADR documenting design choices.

### What works:
- `cargo build` — compiles clean (no new warnings)
- `cargo test` — all 9 tests pass
- `npm run build` (dashboard) — compiles clean
- Full bilateral friend request flow implemented across all layers (P2P, API, CLI, MCP, dashboard)
- Crossed request auto-resolution
- Auto-accept for existing friends
- DID verification on requests
- Reconnect re-send of pending requests
- Revocation on friend removal

### What's next:
1. Test with Bob on Ubuntu — end-to-end friend request flow
2. Per-message Ed25519 signing
3. Project collaboration layer

---

## 2026-03-06 (Owner Identity — Multi-Device Agent Ownership) | Claude (Opus 4.6) | Local Dev

**Session: Implemented owner identity system for multi-device agent ownership (ADR-011)**

### What was done:

40. **ADR-011 — Multi-device identity**: Documented three approaches (separate agents same owner, roaming identity, key delegation). Chose Approach 1 — each device keeps its own agent DID, a separate owner Ed25519 keypair signs attestations binding owner→agent.

41. **OwnerIdentity struct** (`identity.rs`): Ed25519 keypair stored at `~/.agora/owner.key` (PKCS#8, chmod 0600). DID format `did:agora:owner:<base58-pubkey>`. Methods: generate, load, save, from_pkcs8_bytes, sign, attest_agent.

42. **OwnerAttestation struct** (`identity.rs`): Cryptographic binding of owner→agent. Domain-separated canonical message: `agora:owner-attestation:v1:<owner_did>:<agent_did>:<timestamp>`. Verification checks owner_did derives from public key, signature valid. Stored at `~/.agora/owner_attestation.json`. 5 new unit tests (generate, save/load, create/verify attestation, tamper detection).

43. **State changes** (`state.rs`): `Friend.owner_did` field. `PeerInfo.owner_did` + `owner_verified`. `Inner.owner_attestation` loaded at startup. New methods: `owner_did()`, `owner_attestation()`, `owner_trust_level()`, `check_and_pin_owner_did()`, `update_friend_owner_did()`. `FriendsStore.owner_trust_level()` — highest trust among friends sharing an owner DID.

44. **Wire protocol** (`message.rs`): `Message.owner_did` and `Message.owner_attestation` fields (backward-compatible via serde defaults). `hello_with_identity()` now accepts `Option<&OwnerAttestation>`.

45. **Owner CLI** (`main.rs`): `agora owner init [--force]` — generate owner keypair + auto-attest agent. `agora owner show` — display DID, attestation, validity. `agora owner export <file>` — export PKCS#8 key. `agora owner import <file>` — import key + auto-attest.

46. **Owner verification** (`net/mod.rs`): After identity verification, verifies owner attestation (invalid → warn + ignore, don't reject connection). TOFU owner DID pinning for friends. **Auto-trust via owner**: unknown peer with verified owner_did matching a known friend → auto-create friend at min(owner_trust, 3), never auto-grant Inner Circle (4).

47. **API enrichment** (`api.rs`): `GET /identity` returns `owner_did` + `owner_attestation` (with validity). `GET /status` returns `owner_did`. `GET /peers` returns `owner_did` + `owner_verified`. `GET /friends` returns `owner_did`.

48. **Dashboard** (`types.ts`, `Sidebar.tsx`, `AgentDetail.tsx`, `styles.css`): Owner DID on PeerEntry/FriendEntry types. Sidebar groups agents by owner_did with orange "owner" badge. Agent detail shows owner DID with verified badge. "Same owner as [names]" indicator. Owner badge CSS styles.

### What works:
- `agora owner init` → generates `~/.agora/owner.key` + `~/.agora/owner_attestation.json`
- `agora owner show` → displays DID, attestation, validity
- `agora owner export/import` for cross-device key transfer
- Hello messages carry owner attestation (backward compatible)
- Remote daemon verifies attestation, pins owner DID, auto-trusts same-owner agents
- Dashboard shows owner grouping and verification status
- `cargo build` + `npm run build` both succeed
- `cargo test` — 9 tests pass (4 new owner tests + 5 existing identity tests)

### What's next:
1. Issue #23 — Friend request/accept protocol
2. Per-message Ed25519 signing
3. Test cross-machine owner identity flow with Bob

---

## 2026-03-05 (Identity + Dedup + Dashboard) | Claude (Opus 4.6) | Local Dev

**Session: Implemented DID identity (Issue #16), auto-dedup, dashboard unknown agent handling, vision audit**

### What was done:

34-36. See entries below (Issue #16, alias-aware lookup, dashboard unknown agent handling).
37. **Auto-detect duplicate friends**: When an unknown peer connects with a name variant of an existing friend (e.g., "alice-desktop" when "alice" exists), auto-sets alias for future recognition. DID-based merge removes stale entries when same DID appears under multiple names. Keeps highest trust level.
38. **Vision alignment audit**: Comprehensive gap analysis between CONCEPT.md and implementation. Identified 4 critical gaps (per-message signing, friend request protocol, Noise Protocol, anti-injection chain), 4 high-priority gaps, 8 medium gaps. All functional Phase 1 requirements met; security hardening is Phase 2.
39. **GitHub Issue #23**: Created "Friend request/accept protocol with mutual trust exchange" — covers FRIEND_REQ/ACC/REJ/REV message types, mutual trust level exchange, dashboard notification UI.

---

## 2026-03-05 (Identity Implementation) | Claude (Opus 4.6) | Local Dev

**Session: Implemented DID-based agent identity — closed Issue #16 (the last remaining Phase 1 issue)**

### What was done:

34. **Issue #16 — Agent identity**: Ed25519 keypair generation and persistent storage at `~/.agora/identity.key`. DID format `did:agora:<base58-public-key>`. Per-process session IDs (UUID v4) distinguish concurrent instances. Hello messages carry DID, public key, session ID, and signed body for cryptographic verification. New `identity.rs` module with 4 passing tests. `/identity` API endpoint. `agora_identity` MCP tool (11th tool). DID and session_id in `/status` response. Friend struct stores verified DIDs. Key file permissions 0600 on Unix.
35. **Alias-aware friend lookup**: `FriendsStore::get()` and `get_trust_level()` now fall back to alias matching (case-insensitive). Fixes peers connecting with a different node name not being recognized when their name matches a friend's alias.
36. **Dashboard: unknown agent handling**: Connected peers not in the friend list now show a yellow "Add as Friend" banner in AgentDetail with trust level selector. Peer DID and session ID shown when connected. TrustPopover z-index fixed (was hidden behind sidebar).

### All Phase 1 GitHub issues now closed.

---

## 2026-03-05 (Issue Sprint: Closed 10 issues) | Claude (Opus 4.6) | Local Dev

**Session: Cleared nearly all open issues — 1 remaining (#16: agent identity)**

### What was done:

24. **Issue #17 — Sub-groups**: Implemented `ThreadManager` with full CRUD, wired into DaemonState, 6 API endpoints. Tested all endpoints.
25. **Issue #22 — Duplicate friends**: `FriendsStore::add()` warns on name↔alias collisions. API returns warning in response.
26. **Issue #14 — CLI→API sync**: CLI `friends` commands auto-detect running daemon and proxy through HTTP API. Falls back to file when offline.
27. **Issue #13 — Auto-connect**: Friends store `last_address` (auto-populated on connection). `agora start --auto-connect` reconnects to all known friends.
28. **Issue #10 — Connection policies**: `agora start --min-trust N` rejects untrusted peers after Hello exchange.
29. **Issue #11 — Persistent daemon**: `agora start --daemon` detaches + PID file. `agora stop` sends SIGTERM + cleanup.
30. **Issue #21 — Agent-agnostic wake**: Rewrote `wake-agent.sh` with 4 backends (claude, openai, ollama, custom) via `~/.agora/agent.toml`. Removed hardcoded paths. Deleted Ubuntu-specific script.
31. **Thread wire protocol**: Replaced old SubGroup* message types with `thread.create`, `thread.message`, `thread.update`, `thread.close`. Daemon processes incoming thread messages from peers and updates ThreadManager. API broadcasts thread operations to connected peers.
32. **Issue #6 — Adapter Interface spec**: Wrote `protocol/adapter-spec.md` — MCP adapter (Claude Code), HTTP adapter (generic), all API endpoints, env vars, examples for adding new backends.
33. **Issue #7 — Onboarding guide**: Wrote `docs/onboarding.md` — build, start, friends, connect, wake hook, dashboard, MCP, contributor workflow. 15-minute setup.

### What works:
- All issues except #16 (agent identity) are closed
- Thread system works end-to-end: API → daemon → P2P → remote daemon
- Wake script supports Claude, OpenAI, Ollama, and custom agents
- CLI commands sync with running daemon
- Daemon runs as background service with auto-connect and connection policies
- Full documentation: adapter spec, onboarding guide, thread protocol spec

### What's next:
1. DID-based identity (Issue #16) — Ed25519 keypairs, cryptographic verification
2. Phase 3: Project collaboration layer (roles, managing agent)

---

## 2026-03-05 (Dashboard Restructure + Wake Loop + Auto-Threading + Thread Spec) | Claude (Opus 4.6) | Local Dev

**Session: Sidebar layout, conversational wake loop, conversation auto-grouping, sub-group design**

### What was done:

#### Dashboard Restructure
1. **Sidebar + main content layout** replacing 2x2 grid — Slack/Discord-style navigation
2. **8 new components**: HeaderBar, Sidebar, WelcomeView, ConversationChat (chat bubbles), AgentDetail, MainContent, TrustShield (SVG), TrustPopover
3. **Trust shield icons**: Level 0-4 shown as fill-proportional shields (gray/blue/green/gold) with click-to-change popover
4. **Chat bubble messages**: Inbound green-left, outbound blue-right, timestamps, sender names
5. **"Coming Soon" placeholders**: Dimmed Projects and Help Board sections in sidebar
6. **Mobile responsive**: Hamburger menu, sidebar collapses on < 768px
7. **Deleted 5 old components**: StatusBar, MyAgent, ConnectedFriends, ConversationViewer, ActivityFeed

#### Wake Conversation Loop
8. **Rewrote `wake-agent.sh`** as a full conversation loop: reply → poll → reply → idle timeout
9. **Lock file** (`/tmp/agora-wake.lock`) prevents duplicate wake processes (was causing ping-pong loops)
10. **Conversation-style prompt framing**: `[bob]: message [alice]:` so Claude completes naturally
11. **`--tools ""`** disables all tools — Claude outputs pure reply text, no meta-commentary
12. **Extracts message bodies from JSON** before prompting — cleaner input
13. **Tested 5-exchange conversation** with Bob: natural, thoughtful replies, ~30s per turn

#### Auto-Conversation Threading
14. **Deterministic conversation_id** for 1:1 DMs: UUID v5 from sorted peer pair names
15. **Applied to both inbound and outbound**: `push_inbox` + `send_message` auto-assign
16. **Added `uuid` v5 feature** to Cargo.toml
17. All alice↔bob messages now group into one conversation thread automatically

#### Thread Protocol Spec (from Bob conversation)
18. **Committed `protocol/threads.md`**: Wire format for thread.create, thread.message, thread.update, thread.close
19. **Design decisions from live conversation**: threads ARE sub-groups, open vs closed flag, min_trust floor, no nesting, routing as access control

#### Thread/Sub-group Implementation (Issue #17)
20. **Created `daemon/src/thread.rs`**: `ThreadManager` with full CRUD — create, get, list, add/remove participants, close, update metadata, routing
21. **Wired into DaemonState** (`state.rs`): `threads: Mutex<ThreadManager>`, 8 proxy methods
22. **6 API endpoints** (`api.rs`): `GET/POST /threads`, `GET/DELETE /threads/{id}`, `POST /threads/{id}/participants`, `DELETE /threads/{id}/participants/{name}`
23. **All endpoints tested**: create thread, list, get, add participant, close — all working

### What works:
- Dashboard loads with sidebar layout, trust shields, chat bubbles
- Wake loop sustains multi-turn conversations, exits cleanly after ~90s idle
- All 1:1 messages auto-thread into single conversation per peer pair
- Bob tested and confirmed: natural replies, no ping-pong, no meta-commentary
- Thread/sub-group API fully functional (6 endpoints, full CRUD)

### What's next:
1. Wire protocol for threads (handle thread.create/message/update/close over P2P, not just API)
2. DID-based identity (Ed25519 keypairs)
3. Agent-agnostic wake script (Issue #21)

---

## 2026-03-04 (Dashboard MVP + Conversation Threading) | Claude (Opus 4.6) | Local Dev

**Session: React dashboard, message threading, conversation history**

### What was done:

#### Phase A: Dashboard MVP
1. **Scaffolded `dashboard/`** — React 19 + TypeScript + Vite project
2. **Vite proxy**: `/api/*` → daemon at `127.0.0.1:7313` (no CORS needed)
3. **Consumer-based messaging**: Dashboard registers as `"dashboard"` consumer, long-polls `GET /consumers/{id}/messages?wait=true&timeout=10` for instant delivery
4. **5 core components**: StatusBar (node name, version, uptime, peer count), PeerList, FriendList, MessageFeed (chat-style auto-scroll), ConsumerList
5. **Generic `usePolling` hook**: Short-polls status/peers/friends/consumers every 3s
6. **`useMessages` hook**: Consumer registration + long-poll loop, accumulates messages in state
7. **Dark-mode CSS**: `#1a1a2e` bg, monospace message bodies, green/yellow/red status dots
8. TypeScript strict mode passes, production build: 64KB gzipped

#### Phase B: Conversation Threading
9. **Message struct** (`protocol/message.rs`): Added `id: Uuid`, `reply_to: Option<Uuid>`, `conversation_id: Option<Uuid>` — all backward-compatible via `serde(default)`
10. **`Message::reply()` constructor**: Inherits `conversation_id` from parent, sets `reply_to`
11. **Conversation history store** (`state.rs`): `StoredMessage` type, `Vec<StoredMessage>` capped at 5000, stores both inbound and outbound messages
12. **New API endpoints**: `GET /conversations` (list threads with metadata), `GET /conversations/{id}` (full thread history)
13. **Updated `POST /send`**: Accepts `reply_to` and `conversation_id`, returns message `id` in response
14. **`InboxMessage` updated**: All API responses now include `id`, `reply_to`, `conversation_id`
15. **Wire protocol**: Outbound messages carry threading fields across the network
16. **MCP bridge**: New `agora_get_conversation` tool (10th tool), `SendMessageParams` gains threading fields
17. **Wake hook**: `AGORA_CONVERSATION_ID` env var, threading fields in temp file

#### Phase C: Rich Dashboard
18. **ConversationList**: Sidebar showing all threads with participant names, timestamps, previews, message counts
19. **ConversationThread**: Full thread view with inbound/outbound direction arrows, reply button
20. **MessageComposer**: Text input + target field, Cmd+Enter to send, reply-to/conversation context badges
21. **FriendEditor**: Add/remove friends from UI with trust level selector (replaces static FriendList)

### Backward Compatibility:
- All new Message fields use `#[serde(default)]` — old peers without them deserialize fine
- `reply_to`/`conversation_id` use `skip_serializing_if = "Option::is_none"` — old peers won't see unknown fields
- `SendRequest` new fields are optional — existing API clients unchanged
- New endpoints (`/conversations`) are additive only

### What works:
- `cd dashboard && npm run dev` → dashboard at localhost:5173
- Real-time message feed via consumer long-polling
- Conversation threads grouped and browsable
- Send messages from the dashboard with threading
- Add/remove friends from the UI
- Daemon compiles cleanly with all threading changes
- 10 MCP tools (was 9)

### What's next:
- Deploy dashboard on both machines for monitoring
- Test conversation threading cross-machine (Alice → Bob with reply_to)
- Issue #17: Sub-groups
- Issue #21: Agent-agnostic wake script

---

## 2026-03-04 (Fan-Out Consumers + Wake Fixes) | Claude (Opus 4.6) | Local Dev

**Session: Per-consumer message fan-out, wake hook reliability, build performance**

### What was done:
1. **Fan-Out Consumer Model** (PR #19, merged):
   - Replaced single `Mutex<VecDeque<Message>>` inbox with per-consumer buffers
   - New types: `ConsumerId`, `ConsumerSlot`, `ConsumerInfo`
   - `push_inbox()` fans out to all registered consumers independently
   - Consumer registration API: `POST /consumers`, `GET /consumers/{id}/messages`, `DELETE /consumers/{id}`
   - `GET /messages` backward compat via lazy "http-default" consumer
   - Stale consumer reaper: background task, 60s interval, 5min idle threshold
   - Buffer cap: 1000 messages per consumer to prevent unbounded growth
   - MCP monitor registers as own consumer ("mcp-monitor"), falls back to legacy

2. **Wake Hook Fixes** (multiple commits):
   - Smart wake: only fires when no explicitly-registered consumer polled in 60s
   - `suppresses_wake` flag: http-default consumer does NOT suppress, MCP monitor DOES
   - `CLAUDECODE`/`CLAUDE_CODE` env var removal for nested Claude process spawning
   - `AGORA_API_PORT` and `AGORA_API_URL` env vars passed to wake hook
   - Wake command persisted to `~/.agora/wake.json` (survives daemon restart)
   - Temp file cleanup for `agora-wake-messages-*.json`

3. **Build Performance Fix**:
   - rust-analyzer (VS Code) was holding cargo lock, causing 12-18 min build waits
   - Added `.vscode/settings.json` with `rust-analyzer.cargo.targetDir: "target/ra-check"`
   - Actual incremental compile is ~2 seconds now

4. **Git Cleanup**: Deleted 8 stale remote branches (3 merged, 5 superseded by fan-out)

5. **Cross-Machine Testing**: Fan-out verified with Bob (Ubuntu) — multiple consumers receive independent message copies

### Issues:
- Filed #20 (wake persistence) — FIXED
- Filed #21 (wake script too Claude-specific) — open
- Filed #22 (duplicate friends) — open

### What works:
- Multiple consumers each get independent message copies (MCP, HTTP API, future dashboard)
- Wake hook fires correctly when no active MCP session is polling
- Wake command survives daemon restarts
- Correct API port passed to wake hook process

### End-to-end wake hook auto-reply: VERIFIED
- Alice sent message → Bob's wake hook fired → Claude spawned → auto-replied in ~67s
- `AGORA_API_PORT` env var fix confirmed working (Bob's Claude connected to correct port)
- Closed Issues #4 (MCP adapter) and #15 (auto-reconnect) as complete

### What's next:
- Issue #21: Make wake script agent-agnostic
- Issue #22: Prevent duplicate friends (commented with analysis — deferred to DID phase)
- Issue #17: Sub-groups
- Issue #14: CLI friend changes don't sync to running daemon

---

## 2026-03-03 (Automatic Notifications + Wake Testing) | Claude (Opus 4.6) | Local Dev

**Session: MCP background inbox monitor, debounced wake, end-to-end cross-machine testing**

### What was done:
1. **MCP Background Inbox Monitor** (`daemon/src/mcp.rs`):
   - MCP server now spawns background task that long-polls daemon `/messages`
   - Pushes `notify_logging_message()` to Claude Code when messages arrive
   - `agora_read_messages` tool drains local buffer instead of hitting HTTP API
   - Enabled `ServerCapabilities::enable_logging()`

2. **Debounced Wake** (`daemon/src/state.rs`):
   - `push_inbox()` now uses 3-second debounce timer for wake hook
   - Rapid messages reset timer; only fires once with `AGORA_MESSAGE_COUNT` env var
   - Verified: 3 rapid messages → single wake, `AGORA_MESSAGE_COUNT=3`

3. **Improved Wake Scripts** (`daemon/wake-agent.sh`, `daemon/wake-agent-ubuntu.sh`):
   - `cd` into project directory for filesystem context
   - Fetch all messages via curl (not truncated env var)
   - Structured prompt with instructions for questions/coding/reviews
   - Desktop notifications: macOS (`osascript`) + Linux (`notify-send`)
   - Commit/push workflow for coding tasks

4. **End-to-End Testing** (GitHub Issue #18, CLOSED):
   - Alice (Mac) woke idle Bob (Ubuntu) with coding task → Bob added `/health` endpoint
   - Bob committed/pushed `feature/health-endpoint` → Alice reviewed and merged
   - Reverse wake (Bob → Alice) → Alice's wake hook fired
   - Rapid-fire debounce → single wake, correct message count
   - Repeatable test suite: `tests/test-cross-machine.sh`

5. **CLAUDE.md**: Added step 6 to Session Start Protocol (check Agora messages)

### What works:
- Full wake-up loop: sleeping agent receives message → wakes up → reads files → codes → compiles → commits → pushes → replies
- Debounced wake prevents spawn spam
- Desktop notifications for human visibility
- Cross-machine collaboration with real code changes

### What's next:
- Verify MCP background inbox monitor notifications (needs MCP server restart)
- Background vs foreground message distinction (agent-to-agent silent vs human-visible)

---

## 2026-03-03 (Wake Race Condition Fix) | Claude (Opus 4.6) | Local Dev

**Session: Fixed race condition between MCP background monitor and wake hook**

### What was done:
1. **Identified the race condition** (reported by Bob via Agora):
   - Background MCP monitor long-polls `/messages?wait=true` and drains the inbox immediately when `notify_waiters()` fires
   - Wake hook fires 3 seconds later (debounce), but by then the inbox is empty
   - Result: woken Claude instances always saw `[]` for pending messages

2. **Fix** (already committed in `c38cd5d`, confirmed and pushed in this session):
   - `WakeDebounce` struct now accumulates cloned `Message` objects
   - `fire_wake_after_delay()` writes messages to `/tmp/agora-wake-messages-<pid>.json`
   - `AGORA_MESSAGES_FILE` env var passed to the wake script
   - Both `wake-agent.sh` (macOS) and `wake-agent-ubuntu.sh` read from file first, fall back to HTTP API
   - `.mcp.json` updated to pass `--api-port 7314` explicitly

3. **Branch**: `feature/fix-wake-message-race` pushed

### What works now:
- Wake hook delivers messages reliably even when MCP monitor is running
- No changes needed to the MCP bridge itself — it can keep draining the inbox
- Backward compatible: wake scripts still fall back to the API if file is missing

### What's next:
- Merge to main after verification
- Tell Bob the fix is deployed so we can resume cross-machine agent conversations

---

## 2026-03-03 (MCP Bridge + Auto-Reconnect) | Claude (Opus 4.6) | Local Dev

**Session: MCP server bridge for Claude Code + auto-reconnect for connections**

### What was done:
1. **MCP Server Bridge** (`agora mcp` subcommand):
   - New `daemon/src/mcp.rs`: stdio MCP server using `rmcp` 0.17 crate
   - 9 tools that bridge to the daemon HTTP API via `reqwest`:
     `agora_status`, `agora_list_peers`, `agora_read_messages` (with long-poll),
     `agora_send_message`, `agora_list_friends`, `agora_add_friend`,
     `agora_remove_friend`, `agora_get_wake`, `agora_set_wake`
   - Logging goes to stderr (stdout is the MCP transport)
   - Added `Mcp` subcommand to CLI with `--api-port` flag (default 7313)
   - Created `.mcp.json` config for Claude Code integration
   - Dependencies added: `rmcp` 0.17, `schemars` 1, `reqwest` 0.12

2. **Auto-Reconnect** for `agora connect`:
   - Refactored `connect_to_peer()` → extracted `try_connect_once()`
   - Outer retry loop with exponential backoff: 1s → 2s → 4s → ... → 60s cap
   - On connection loss after successful session: resets backoff to 1s
   - On initial connect failure: increases backoff exponentially
   - Loops forever until process is killed (connections survive peer restarts)

3. **Architecture**: MCP bridge is a separate subprocess, zero changes to daemon.
   Multiple Claude Code instances can connect to the same running daemon.

### Cross-machine testing:
- MCP bridge smoke test: 9 tools exposed with full JSON schemas
- Sent message via MCP → delivered to Bob on Ubuntu over TLS
- Trust gating issue found and fixed: Alice needed trust >= 3 on Bob's side for wake hook
- **Autonomous agent conversation achieved**: Bob woken by wake hook, 5 exchanges of
  architecture discussion about sub-groups, message types, backward compatibility
- Wake hook limitation: ephemeral `claude -p` good for replies, bad for multi-step coding
- Solution: updated MCP server instructions + CLAUDE.md to tell agents to poll for messages

### Design output from autonomous conversation:
- Bob proposed SubGroup struct, 6 message types, anti-anchoring Delphi method flow
- Peer-initiated sub-groups as first-class (decentralized, manager vetoes reactively)
- `#[serde(other)]` Unknown variant for backward compatibility
- Captured as GitHub Issue #17

### What works:
- `cargo build` compiles cleanly
- `agora mcp` — 9 MCP tools, tested end-to-end cross-machine
- `agora connect <target>` — auto-reconnects on failure/disconnect
- Wake hook → autonomous agent reply loop (confirmed working)
- Agent communication protocol documented in CLAUDE.md

### What's next:
- Implement sub-group message types (Issue #17)
- DID-based identity (Phase 2)
- Dashboard for monitoring agent conversations

---

## 2026-03-02 (Friend Graph) | Claude (Opus 4.6) | Local Dev

**Session: Friend graph with trust levels implemented + README**

### What was done:
1. **Friend graph with trust levels (0-4)** — full Phase 1 implementation:
   - `TrustLevel` newtype with named constants: Unknown (0), Acquaintance (1), Friend (2), Trusted (3), Inner Circle (4)
   - `Friend` struct with name, alias, trust_level, added_at, notes
   - `FriendsStore` — persists to `~/.agora/friends.json`, loads on daemon startup
   - Integrated into `DaemonState` with async friend management methods
2. **Wake-up gating**: `push_inbox()` only fires the wake hook if sender has trust >= 3.
   Passes `AGORA_TRUST` environment variable to the hook command.
   Untrusted senders' messages are still received but hook is suppressed (logged).
3. **CLI commands working**: `agora friends add <name> --trust 3 --alias "..." --notes "..."`,
   `agora friends list`, `agora friends remove <name>` — all read/write `~/.agora/friends.json` directly.
4. **HTTP API endpoints**:
   - `GET /friends` — list all friends with trust levels, can_wake flag
   - `POST /friends` — add a friend `{"name":"...", "trust_level": 3}`
   - `DELETE /friends/{name}` — remove a friend (404 if not found)
5. **Connection trust logging**: After Hello exchange, logs `warn!` for unknown peers,
   `info!` for known friends with their trust level.
6. **README.md**: Created expressive project README with banner image, icon, feature table,
   quick start guide, architecture diagram, API reference, and roadmap.
7. **Assets**: Moved project images to `assets/` directory (banner + icon).
8. Updated all project tracking files (CHANGELOG, STATUS, session log).

### Architecture change:
- `DaemonState::new()` now takes a `friends_path` parameter
- Friend data loaded from JSON at startup, saved on every add/remove
- Trust check happens at message receipt time (no extra network round-trip)

### What's next:
- DID-based identity (Phase 2)
- Connection rejection based on trust (Phase 2)
- Friend request/accept protocol (Phase 3)

---

## 2026-03-02 (Issue #4) | Claude (Opus 4.6) | Local Dev

**Session: Local HTTP API + message queue implemented**

### What was done:
1. **Created `daemon/src/state.rs`**: Shared daemon state with thread-safe inbox/outbox
   message queues, peer tracking, and async Notify-based signaling.
2. **Created `daemon/src/api.rs`**: Local HTTP API using axum with 4 endpoints:
   - `GET /status` — daemon status (version, node name, peer count)
   - `GET /peers` — list connected peers with names and addresses
   - `GET /messages` — drain incoming messages from remote peers
   - `POST /send` — queue a message to send to remote peers (broadcast or targeted)
3. **Redesigned `daemon/src/net/mod.rs`**: Replaced stdin-based interaction with
   DaemonState-backed message routing. Incoming messages go to inbox, outbox messages
   are sent to peers. Peer registration/deregistration on connect/disconnect.
4. **Updated `daemon/src/main.rs`**: Runs P2P listener (port 7312) and HTTP API
   (port 7313) concurrently. Both `start` and `connect` commands now spawn the
   HTTP API alongside the network connection.
5. Added `axum` and `uuid` dependencies to Cargo.toml.
6. **All endpoints tested and working**: status, peers, messages, send all respond correctly.

### Architecture:
```
Local Agent (Claude/GPT/etc.)
    ↕ HTTP (127.0.0.1:7313)
Agora Daemon (DaemonState)
    ↕ TLS 1.3 (0.0.0.0:7312)
Remote Peers
```

Agents interact with the daemon via simple HTTP calls:
- `curl http://127.0.0.1:7313/status` — check daemon status
- `curl http://127.0.0.1:7313/messages` — read incoming messages
- `curl -X POST http://127.0.0.1:7313/send -d '{"body":"hello"}'` — send a message

### Cross-machine test: SUCCESS
- Ubuntu pulled new code, built, connected via `connect 10.0.1.1:7312`
- Ubuntu sent: `curl POST /send` → message arrived in macOS inbox via `GET /messages`
- macOS replied: `curl POST /send` → message arrived in Ubuntu inbox via `GET /messages`
- **Both directions confirmed working — full programmatic agent messaging over TLS**

### Cross-machine tests: ALL SUCCESS
- **Long-poll**: Ubuntu `curl /messages?wait=true`, macOS sent message → instant delivery.
- **Wake-up hook (simple)**: Ubuntu agent idle, macOS pinged, hook auto-replied. Confirmed.
- **Wake-up hook (intelligent)**: macOS asked "what feature to build next?" →
  Ubuntu daemon fired hook → hook launched `claude -p` → Claude composed a thoughtful
  multi-paragraph response about friend graphs and persistent state → reply sent back
  automatically. **A sleeping AI agent woke up, thought, and replied intelligently.**
  Fixed: needed `unset CLAUDECODE` to avoid nested session detection.
  Fixed: needed full path to `claude` CLI binary.
  This is Section 8 of CONCEPT.md (wake-up protocol) fully demonstrated.

### What's next:
- Issue #6: Adapter Interface specification
- Persistent daemon mode (background service)
- Friend graph and trust levels

---

## 2026-03-02 (robustness) | Claude (Opus 4.6) | Local Dev

**Session: Long-poll, broadcast fan-out, and wake-up hook**

### What was done:
1. **Long-poll for GET /messages**: Added `?wait=true&timeout=N` query params.
   With `wait=true`, the request blocks until a message arrives or timeout (default 30s,
   max 120s). Without it, returns immediately as before. Eliminates polling.
2. **Broadcast fan-out for outbox**: Replaced `Mutex<VecDeque>` + `Notify` with
   `tokio::sync::broadcast` channel. Each peer connection subscribes and gets its own
   copy of every outbound message. No more race condition between peer handlers.
3. **Wake-up hook**: Configurable shell command fired when a message arrives.
   - Set via CLI: `--wake-command 'script.sh'`
   - Set via API: `POST /wake {"command": "script.sh"}`
   - Query via API: `GET /wake`
   - Command receives `AGORA_FROM` and `AGORA_PREVIEW` env vars
   - Runs in background, doesn't block message processing
   - Foundation for Section 8 (wake-up protocol) from CONCEPT.md

### API summary (all endpoints):
| Method | Path | Description |
|--------|------|-------------|
| GET | /status | Daemon info (version, name, peer count) |
| GET | /peers | Connected peers list |
| GET | /messages | Drain inbox (?wait=true&timeout=N for long-poll) |
| POST | /send | Queue message ({"body":"...", "to":"..."}) |
| GET | /wake | Get current wake-up hook |
| POST | /wake | Set wake-up hook ({"command":"..."}) |

---

## 2026-03-02 (post-milestone) | Claude (Opus 4.6) | Local Dev

**Session: Post-milestone analysis, next steps**

### Observations from the milestone test:
1. The other Claude connected **8 times** and sent multiple messages — all received
2. It struggled with stdin piping (tokio async stdin + pipe = unreliable) — this
   proves Issue #4 (programmatic adapter) is the critical next step
3. **Wake-up use case observed in real time**: The other agent kept pinging this
   daemon, which accepted connections and logged messages, but had no way to
   wake up the Claude agent on this side or route messages to it. This is
   exactly the wake-up protocol from Section 8 of CONCEPT.md happening
   organically — an idle agent should auto-activate when a friend connects.

### Key insight:
The daemon needs to be a **persistent background service** that:
- Accepts connections even when no agent is actively using it
- Stores incoming messages in a queue
- Notifies/wakes the local agent when a message arrives
- Provides a programmatic API (not stdin) for agents to send/receive

### What's next:
- Issue #4: Claude Code adapter — MCP server or file-based IPC so agents
  can send/receive messages programmatically
- Make the daemon persistent (doesn't exit after one connection)
- Message queue for offline/idle agents

---

## 2026-03-02 (MILESTONE) | Claude (Opus 4.6) | Local Dev + Ubuntu

**FIRST CROSS-MACHINE CONNECTION ACHIEVED**

Two Claude agents on different machines, different OSes, different networks
communicated through the Agora protocol over encrypted TLS 1.3.

```
Alice-Main (macOS, 10.0.1.1)
    ↕  TCP + TLS 1.3 via Tailscale
Bob-Remote (Ubuntu, 10.0.1.2)
```

- macOS listener: Received Hello from Bob-Remote (10.0.1.2:58454)
- Ubuntu client: Received Hello from Alice-Main
- TLS handshake completed successfully
- Interactive messaging mode active
- **FIRST MESSAGE DELIVERED**:
  `[Bob-Remote] Hello from Kevin! Bob-Remote here
  — first cross-machine Agora connection working!`

This is Issue #5 — the proof-of-concept milestone. The protocol works.
GitHub Issue #5 closed with full connection transcript.

---

## 2026-03-02 (update 2) | Claude (Opus 4.6) | Local Dev

**Session: TLS networking implemented — two nodes can talk!**

### What was done:
1. **Completed Issues #2 + #3** — TCP networking + message protocol:
   - `net/tls.rs`: Self-signed cert generation, TLS 1.3 server/client configs
   - `net/mod.rs`: Listener (`agora start`), connector (`agora connect`),
     TLS handshake, bidirectional interactive message loop
   - `protocol/message.rs`: Message types (Hello, Message, Heartbeat, Close)
   - `protocol/framing.rs`: Length-prefixed framing (4-byte BE + JSON)
   - Added `--name` flag to CLI for node identity
2. **Tested successfully on localhost**: Two nodes exchanged Hello + text
   messages over TLS 1.3. Server logs confirm full flow.
3. Added concept clarification: same-machine multi-agent is first-class
4. Second machine set up by user with another Claude agent

### What works:
- `agora --name Alice start` → listens on port 7312 with TLS
- `agora --name Bob connect 127.0.0.1:7312` → connects, exchanges Hello,
  interactive text messaging
- All traffic encrypted via TLS 1.3
- Length-prefixed JSON framing for messages

### What's next:
- Issue #4: Claude Code adapter (MCP server bridge) — so Claude can
  send/receive messages through the protocol programmatically
- Issue #5: First cross-machine test between the two actual machines
- User's second machine needs to `git pull` and `cargo build`

---

## 2026-03-02 | Claude (Opus 4.6) | Local Dev

**Session: Information flow architecture, dynamic groups, Phase 1 Issue #1 complete**

### What was done:
1. **Installed Rust** (1.93.1) on this machine
2. **Completed Issue #1**: Scaffolded Rust daemon — `cargo build` works, CLI with
   all subcommands (start, connect, status, friends, project), `agora --version`
   prints "agora 0.1.0"
3. **Added 6 new concept sections** to CONCEPT.md (Sections 22-27):
   - Section 22: Frictionless Project Joining — "Help Me" flow with 4 friction
     levels, instant Context Package transfer for joining agents
   - Section 23: Role-Based Information Filtering & Anti-Anchoring — 4-layer
     information access model, independent first pass (Delphi method), devil's
     advocate role, blind review, managing agent as information gateway with
     anti-injection scanning
   - Section 24: Shared Ledgers & Private Channels — dual ledger architecture
     (public append-only ledger + private working channels), promotion mechanism,
     4-tier information distribution (push immediately / push on milestone /
     pull on request / periodic digest), token-aware summarization
   - Section 25: Dynamic Sub-Groups — lifecycle (form → work → report → merge),
     3 formation modes, re-merge protocol, anti-fragmentation rules
   - Section 26: Coordination Patterns — 5 reusable patterns (pair, hub-and-spoke,
     committee, hierarchical, pipeline), pattern evolution during projects
   - Section 27: Tool & Stage Management — stage-based permissions (investigation →
     implementation → review → integration → deployment), cross-agent tool sharing via MCP
4. Created GitHub Issue #7: Write onboarding guide for new participants
5. User is setting up second machine with another Claude for first cross-machine test

### Key design decisions:
- Managing agent acts as INFORMATION GATEWAY, not just router — it filters, summarizes,
  and controls distribution based on roles, anti-anchoring rules, and injection scanning
- Public Ledger is append-only and cryptographically signed (audit trail)
- Private Channels are ephemeral working spaces; findings promoted to public ledger
- Anti-anchoring: new agents can optionally form independent hypotheses before seeing
  others' work (Delphi method)
- Sub-groups have mandatory report-back and auto-dissolve on deliverable submission

### What works:
- `agora --version` → "agora 0.1.0"
- All CLI subcommands parse and print placeholders
- Concept document now at 27 sections + 3 appendices

### What's next:
- Issue #2: TCP listener + connector with TLS (first real networking)
- User setting up second machine — first cross-machine test approaching
- Issue #7: Onboarding guide

---

## 2026-03-01 (update 2) | Claude (Opus 4.6) | Local Dev

**Session: OpenClaw docking concept + GitHub Issues**

### What was done:
1. Added Section 21 to CONCEPT.md: Interoperability with OpenClaw and other
   agent platforms — Agora as a "docking layer" that existing agents plug into
2. Defined the Adapter Interface (Rust trait) for platform integration
3. Documented how OpenClaw/ClawSwarm would integrate (adapter + shared memory bridge)
4. Described multi-agent swarm scaling (agents in multiple projects simultaneously)
5. Created GitHub Issues #1-#6 for Phase 1 implementation tasks
6. Strengthened CLAUDE.md with mandatory Session Start/End protocols
7. Added CHANGELOG.md as append-only project memory

### Key insight:
- Agora should NOT require agents to be built for it. It should be a docking
  layer that existing platforms (OpenClaw, Claude Code, AutoGen, CrewAI) plug
  into via thin adapters. This maximizes adoption.

### What's next:
- Begin Phase 1: Scaffold Rust daemon (Issue #1)
- Then TCP networking (Issue #2), message protocol (#3), Claude adapter (#4)
- First cross-machine test (#5) is the milestone

---

## 2026-03-01 | Claude (Opus 4.6) | Local Dev

**Session: Initial concept and project setup**

### What was done:
1. Researched the full landscape of existing multi-agent protocols (A2A, MCP,
   ANP, AGNTCY, OpenClaw, FIPA ACL, CrewAI, AutoGen, etc.)
2. Researched security attack vectors and best practices (OWASP Agentic Top 10,
   NIST AI Agent Standards, prompt injection, agent impersonation, etc.)
3. Wrote CONCEPT.md — full protocol design with 20 sections covering:
   - 5-layer architecture, wire protocol, DID-based identity
   - Friend Graph with trust levels, wake-up protocol
   - Project collaboration with roles and managing agent
   - Critical assessment of weaknesses (Section 16)
   - Managing agent analysis (Section 17)
   - Token/compute contribution model (Section 18)
   - Open project marketplace (Section 19)
   - Bootstrapping plan (Section 20)
4. Chose project name: **Agora** (Greek marketplace/meeting place)
5. Created GitHub repo: `agora-protocol/agora-protocol` (private)
6. Set up project tracking: CLAUDE.md, STATUS.md, DECISIONS.md, CHANGELOG.md,
   session logs, ADR templates, GitHub issue templates

### Key decisions made:
- Ed25519/X25519 over RSA for crypto
- Build on A2A + MCP, don't replace them
- Noise Protocol for mutual auth
- Rust daemon, React dashboard
- Managing agent: optional, auto-suggested at >2 agents
- Apache 2.0 license

### What works:
- GitHub repo is live with all concept docs pushed
- Project tracking system in place

### What needs to be done next:
1. Create GitHub Issues for Phase 1 implementation tasks
2. Scaffold the Rust daemon project (`cargo init`)
3. Implement basic TCP listener/connector with TLS
4. First cross-machine test: clone repo on second computer,
   establish any communication between two agents

### Open questions:
- CLI binary name: `agora` confirmed
- How to pipe messages between Claude Code and the daemon
  (stdin/stdout adapter? MCP server? File-based IPC?)
