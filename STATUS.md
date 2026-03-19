# Agora — Project Status

> **Last updated**: 2026-03-08 by Claude (Local Dev, Phase 6 complete)

## Current Phase

**Phase 1: Core Implementation** — COMPLETE.
**Phase 2: Social Layer** — COMPLETE (friend requests, owner identity, trust management).
**Phase 3: Project Collaboration** — COMPLETE (all 7 steps implemented).
**Phase 4: Security & Polish** — COMPLETE (hardening, CLI, roles, oversight).
**Phase 5: Shipping Readiness** — COMPLETE (conversations, GitHub, dashboard UX, infrastructure).
**Phase 6: Public Release** — COMPLETE (NAT traversal, packaging, docs, marketplace, reputation, coordinator).

## What Exists

### Phase 1 (Complete)
- [x] Rust daemon (`agora`) — TLS 1.3 networking, length-prefixed JSON framing
- [x] Cross-machine connection (macOS <-> Ubuntu, verified via LAN)
- [x] HTTP API (60+ endpoints), CLI (40+ subcommands), MCP bridge (26 tools)
- [x] Friend graph with trust levels 0-4, trust-gated wake-up
- [x] Auto-reconnect with exponential backoff
- [x] Conversation threading (UUID v5 deterministic IDs, reply_to chains)
- [x] Fan-out consumer model, per-consumer message buffers
- [x] Thread/sub-group system (CRUD, routing, wire protocol)
- [x] Dashboard MVP (React 19, sidebar + main content, dark-mode, chat bubbles)
- [x] Persistent daemon mode, auto-connect to known friends
- [x] Connection policies (min-trust threshold)
- [x] Agent-agnostic wake system (claude, openai, ollama, custom backends)

### Phase 2 (Complete)
- [x] DID-based agent identity (Ed25519 keypairs, `did:agora:` format, Hello signing)
- [x] Owner identity (multi-device ownership, attestation, auto-trust, TOFU pinning)
- [x] Bilateral friend request protocol (`friend.request`/`accept`/`reject`/`revoke`)
- [x] Asymmetric trust (`their_trust` field — you see what trust they assigned you)
- [x] Dashboard: overview screen, friend requests UI, agent detail with trust management
- [x] 22-test cross-machine validation suite (all pass)

### Phase 3 (Complete)
- [x] Per-message Ed25519 signing — all outbound messages signed, verified on receive
- [x] Project data model — Project, ProjectAgent, 7 roles, ProjectStatus, persistent stores
- [x] Project invitations — bilateral invite/accept/decline, auto-accept for trust >= 3
- [x] Clock-in/out — agents clock in with focus text, status propagated cross-machine
- [x] Wire protocol — 28 message types (project, task, stage, friend, thread, marketplace, reputation, coordinator)
- [x] Duplicate connection fix — RegisterResult enum with session ID dedup
- [x] Task Board — Task/TaskStatus/TaskPriority, auto-unblock dependencies, 6 API endpoints
- [x] Audit Trail — append-only Ed25519-signed entries, auto-logged on all mutations
- [x] Stage-Gated Workflows — 5 stages, per-role permission matrix, can_advance() guard

### Phase 4 (Complete)
- [x] **Security**: Unsigned message rejection, wake hook injection prevention, temp file permissions (0600), input validation
- [x] **CLI Polish**: Format module (ANSI colors, tables, TTY detection), `--format table|json`, new commands (`peers`, `messages`, `send`), rich output for all commands
- [x] **Role Enforcement**: `check_permission()` enforced on all API and P2P handlers, stage-aware permissions, 403 errors with audit logging
- [x] **Audit Replication**: Audit entries broadcast to peers via `project.audit` wire message, merge with dedup by UUID
- [x] **Human Oversight**: Suspend/unsuspend agents (API + CLI + MCP + wire protocol), suspended agents fail all permission checks
- [x] **MCP**: `agora_project_oversight` tool for suspend/unsuspend

### Phase 5 (Complete)
- [x] **TOML Config File**: `~/.agora/config.toml` — name, ports, auto_connect, min_trust, wake_command, connect targets. CLI flags override.
- [x] **Graceful Shutdown**: `ctrl_c` handler sends Close to all peers, saves state, removes PID file.
- [x] **Project-Conversation Linkage**: `project_id` on messages, auto-tagging, `GET /projects/{id}/conversations`, CLI + MCP tool.
- [x] **GitHub Integration**: Bidirectional sync (issues <-> tasks), `POST /projects/{id}/github/sync`, token management, CLI + MCP + dashboard sync button. Uses `octocrab`.
- [x] **Dashboard UX**: Toast notifications, message compose bar, project creation form, task filter bar, inline clock-in, project conversation viewer, agent search, onboarding card, error handling.
- [x] **Rate Limiting**: Token-bucket middleware (100 req/s) on all API routes.
- [x] **Integration Tests**: 13 end-to-end HTTP API tests (status, friends, projects, tasks, stages, clock, audit, GitHub, rate limiting).
- [x] **Library Crate**: `lib.rs` re-exports for integration test access.

### Phase 6 (Complete)
- [x] **WebSocket Relay**: Standalone relay crate for NAT traversal. Agents connect outbound, relay forwards by DID. Direct P2P still works; relay is fallback.
- [x] **Offline Message Queue**: Persistent per-peer outbox (`~/.agora/outbox/`), Ack message type, replay on reconnect, dedup by UUID, max 1000/peer.
- [x] **Data-at-Rest Encryption**: Argon2id key derivation → AES-256-GCM. Encrypted file format with versioning. `--no-encrypt` for dev, `AGORA_PASSPHRASE` for automation.
- [x] **Dashboard Auth**: LoginPage component, token verification, localStorage. `find_dashboard_dir()` for static serving.
- [x] **CI/CD**: GitHub Actions — CI (fmt/clippy/test on push/PR), Release (cross-compile 4 targets, GitHub Releases on tag). Homebrew formula.
- [x] **mdBook Documentation**: 19 files — concepts, guides, API/CLI/MCP reference, architecture. Ready for GitHub Pages.
- [x] **Agent Marketplace**: Capability-based discovery. Agents advertise domains/tools/availability. Relevance-scored search. Persistent store.
- [x] **Reputation System**: Contribution tracking with weighted scoring + exponential decay. Trust recommendations at score thresholds. Leaderboard.
- [x] **Coordinator**: Rule-based project coordination — auto-assign tasks, stage advance detection, blocked task detection, workload balancing, digest generation.

### Test Suite
- 201 tests total (all pass):
  - 92 unit tests (lib crate)
  - 92 unit tests (bin crate)
  - 13 integration tests (HTTP API end-to-end)
  - 4 relay tests
- 22-test cross-machine validation suite (Phase 2)
- 12-turn cross-machine project collaboration test (Phase 3)

### Codebase Size
- ~22,500 LOC total:
  - ~17,800 daemon (Rust)
  - ~4,750 dashboard (TypeScript/React)
  - ~300 relay (Rust)
- 60+ API endpoints, 40+ CLI commands, 26+ MCP tools, 28 wire protocol message types
- 5 new modules in Phase 6: outbox, crypto, marketplace, reputation, coordinator

### Phase 7 (In Progress)
- [x] **Cross-vendor testing**: Claude (Anthropic) ↔ Codex (OpenAI/GPT-5) live collaboration
- [x] **Inbox peek/ack**: `?peek=true` reads without drain, `POST /messages` ack by ID
- [x] **Owner permission fix**: Owner role bypasses all stage permission checks
- [x] **Outbound invitation guard**: Accept/decline rejects outbound invitations
- [x] **Coordinator dedup**: Idempotent suggestion generation
- [x] **Agent role management**: `POST /projects/{id}/agents/{name}/role` endpoint + dashboard UI
- [x] **Wake status**: `/status` returns `wake_enabled`, `wake_armed`, listener info
- [x] **Dashboard overhaul**: Simplified sidebar, welcome, agent detail, project detail
- [x] **Project agent visibility**: `agent_names` in project list API, sidebar shows all collaborators
- [x] **Task management**: Assignee dropdown, delete button, priority selector, description field
- [x] **Dashboard auth middleware**: Bearer token auth, `/auth/verify` endpoint, token CLI (`agora token show/regenerate`)
- [x] **rust-embed**: Dashboard dist/ embedded into binary, served at `/` with SPA fallback, API at both `/` and `/api/` prefix
- [x] **Stress tests**: 50 concurrent messages, 50 concurrent API sends, 50 concurrent task creations — all pass
- [x] **OpenClaw adapter design**: 3 integration modes, memory bridge, ClawSwarm interop, wake script
- [x] **Stale issue cleanup**: Closed GitHub issues #31-43 (all previously implemented)
- [x] **tokio 1.50 compat**: Fixed `#[cfg(unix)]` in `tokio::select!` arms

### Known Issues (Non-blocking)
- [ ] Per-machine identity keys needed for local multi-agent testing (same DID on shared machine)
- [ ] Project conversations only track inter-agent messages, not local API mutations
- [ ] Coordinator LLM-enhanced mode not yet implemented (rule-based only)
- [ ] Relay server has no TLS termination (relies on external reverse proxy)
- [ ] Pre-existing flaky test: `test_wake_status_tracks_readiness_and_listeners` (race condition)

## What's Next

### Remaining Phase 7
- [ ] Real-world multi-machine relay testing (requires remote machine)
- [ ] Per-daemon identity key generation
- [ ] LLM-enhanced coordinator mode

### Phase 8: Federation & Scale
- [ ] TLS on relay server
- [ ] Federation / relay mesh
- [ ] Post-quantum crypto (ML-KEM hybrid)
- [ ] Mobile/web agent adapters

### Test Suite
- 220+ tests total:
  - 97 unit tests (lib crate, 96 pass + 1 pre-existing flaky)
  - 97 unit tests (bin crate, 96 pass + 1 pre-existing flaky)
  - 28 integration tests (HTTP API end-to-end, all pass)
  - 4 relay tests
- 56-endpoint API validation
- Cross-vendor messaging validation (Claude ↔ Codex)

## Active Contributors

| Agent | Machine | Last Active | Current Focus |
|---|---|---|---|
| Claude (Opus 4.6) | Local Dev | 2026-03-17 | Phase 7 — auth, rust-embed, stress tests, OpenClaw |
| Codex (GPT-5) | Local Dev | 2026-03-17 | API validation, bug reporting |
| Claude (Bob) | Remote Dev | 2026-03-07 | Available for collaboration |

---

*Update this file at the end of every work session.*
