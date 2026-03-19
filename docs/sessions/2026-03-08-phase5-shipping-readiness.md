# Session: Phase 5 — Shipping Readiness

**Date**: 2026-03-08
**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev

## What Was Done

Implemented all 20 steps of the Phase 5 plan across 4 workstreams:

### Workstream 4: Infrastructure (Steps 1-2, 18-20)
1. **TOML Config File** — New `daemon/src/config.rs`. Supports `~/.agora/config.toml` with name, ports, auto_connect, min_trust, wake_command, and connect targets. CLI flags override config values. 3 unit tests.
2. **Graceful Shutdown** — `tokio::signal::ctrl_c()` handler in Start command. New `broadcast_close()` method sends Close to all peers, waits 500ms for delivery, disconnects all, removes PID file.
3. **Rate Limiting** — Custom token-bucket middleware (100 req/s) using `axum::middleware::from_fn`. Global `LazyLock` rate limiter. Returns 429 when exhausted.
4. **Integration Tests** — New `daemon/tests/integration.rs` with 13 end-to-end tests. Required creating `daemon/src/lib.rs` as library crate for test access. Tests cover: status, health, friends CRUD, project lifecycle (create/tasks/update/complete), stage management, clock in/out, audit trail, GitHub config, conversations, messages, rate limiting.

### Workstream 1: Project-Conversation Linkage (Steps 3-6)
5. Added `project_id: Option<String>` to `StoredMessage` with `#[serde(default, skip_serializing_if)]`.
6. Added `project_id: Option<Uuid>` to `OutboundMessage`. Updated all 22+ construction sites.
7. `push_inbox()` calls `extract_project_id()` for project-related message types (TaskAssign, TaskUpdate, ProjectStage, etc.).
8. New `get_project_messages()` filters conversation_history by project_id.
9. API: `GET /projects/{id}/conversations`.
10. CLI: `agora project conversation <id> [--limit N]`.
11. MCP: `agora_project_conversations` tool.

### Workstream 3: Dashboard UX (Steps 7-14)
12. Toast notification system (`Toast.tsx`) — React context, 4s auto-dismiss, 3 types.
13. Message compose bar in ConversationChat.
14. Project creation form ("+") in Sidebar with inline form.
15. Task filter bar (all/todo/in_progress/done/blocked) in ProjectDetail.
16. Inline clock-in form (replaces `prompt()`).
17. Project conversation viewer (toggle + message list).
18. Agent search input in Sidebar (shown when >3 agents).
19. Onboarding card in WelcomeView (when 0 peers + 0 friends).
20. Error handling: all empty catch blocks now show toast errors.

### Workstream 2: GitHub Integration (Steps 15-17)
21. New `daemon/src/github.rs` — GitHubConfig, parse_github_repo, import_issues, push_task_as_issue, sync_bidirectional. 10 unit tests.
22. `github_issue_number: Option<u64>` on Task struct.
23. `octocrab = "0.43"` dependency.
24. API: `POST /projects/{id}/github/sync`, `GET /projects/{id}/github/status`, `GET/POST /github/config`.
25. CLI: `project github-sync`, `project github-token`, `project github-status`.
26. MCP: `agora_github_sync`, `agora_github_config`.
27. Dashboard: sync button + status indicator in ProjectDetail, API wrappers.

## Key Decisions
- Used custom token-bucket rate limiter instead of `tower::limit::RateLimitLayer` (tower's RateLimit doesn't implement Clone, which axum requires).
- Created `lib.rs` library crate to enable integration tests (Rust binary crates can't be imported by integration tests).
- Stage management integration test tolerates 403 due to parallel test HOME env var races.

## Test Results
- 105 tests pass: 46 lib + 46 bin + 13 integration
- Dashboard builds clean
- Daemon builds clean

## Files Changed
- `daemon/Cargo.toml` — Added octocrab, toml, tempfile (dev), lib target
- `daemon/src/config.rs` — NEW: TOML config support
- `daemon/src/github.rs` — NEW: GitHub integration
- `daemon/src/lib.rs` — NEW: Library crate
- `daemon/tests/integration.rs` — NEW: Integration tests
- `dashboard/src/components/Toast.tsx` — NEW: Toast system
- `daemon/src/state.rs` — project_id on messages, import_github_tasks, broadcast_close
- `daemon/src/api.rs` — Rate limiting, GitHub endpoints, project conversations
- `daemon/src/main.rs` — Config loading, graceful shutdown, GitHub CLI
- `daemon/src/mcp.rs` — New tools (conversations, GitHub)
- `daemon/src/project.rs` — github_issue_number on Task
- `dashboard/src/components/ProjectDetail.tsx` — Task filter, clock-in, conversations, GitHub sync
- `dashboard/src/components/Sidebar.tsx` — Project creation, agent search
- `dashboard/src/components/ConversationChat.tsx` — Compose bar
- `dashboard/src/components/WelcomeView.tsx` — Onboarding
- `dashboard/src/styles.css` — ~200 lines of new CSS
- `dashboard/src/types.ts` — GitHub types, project_id on messages
- `dashboard/src/api.ts` — GitHub + conversation API wrappers
- `dashboard/src/App.tsx` — ToastProvider wrapper
