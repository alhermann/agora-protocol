# Session: Phase 4 — Security Hardening, CLI Polish, Role Enforcement, Human Oversight

**Date**: 2026-03-07
**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev
**Duration**: ~2 hours (across 2 context windows)

## Summary

Implemented all 12 steps of the Phase 4 plan across 6 workstreams. This is the security + polish phase that makes the protocol production-ready.

## What Was Done

### WS1: Critical Security Fixes
1. **Unsigned message rejection** (`net/mod.rs`): If a peer provided a public key in Hello but sends an unsigned message, it's now dropped with a warning. Previously accepted silently (MITM risk).
2. **Wake hook injection prevention** (`state.rs`): `validate_wake_command()` rejects shell metacharacters (`;|&\`$(){}><\n\r`), requires path prefix. `sanitize_env_value()` strips control chars and caps length at 500 for `AGORA_FROM`/`AGORA_PREVIEW`.
3. **Temp file permissions** (`state.rs`): Wake message files created with `0o600` on Unix.

### WS2: CLI First-Class Output
4. **Format module** (`format.rs`): NEW file — ANSI colors (`bold`, `green`, `yellow`, `red`, `dim`, `cyan`), TTY detection via `std::io::IsTerminal`, `print_table()` with padding-based rendering, `stage_bar()` for ASCII progress, `short_id()` for UUID truncation.
5. **`--format` flag**: Global `--format table|json` on all commands.
6. **New commands**: `peers`, `messages [--wait] [--timeout]`, `send <body> [--to]`.
7. **Rich output**: `status` (dashboard with peers/friends/projects), `friends list` (colored trust levels), `project list/show/tasks` (tables), `project stage` (progress bar), `project audit` (timestamped log).

### WS3: Role-Based Access Enforcement
8. **`check_permission()`** (`state.rs`): Looks up agent by DID then name, checks `suspended` status, uses stage-aware or role-default permissions.
9. **API guards** (`api.rs`): `create_task` (write), `update_task` (write), `delete_task` (write), `assign_task` (coordinate), `set_stage` (coordinate). Returns 403 with JSON error.
10. **P2P guards** (`net/mod.rs`): Same permissions enforced on incoming wire messages. Denials logged to audit trail.

### WS4: Audit Trail Replication
11. **Wire message type**: `project.audit` with `AuditEntryPayload`.
12. **Broadcast**: `append_audit()` pushes to outbox.
13. **Receive + merge**: `merge_audit_entry()` deduplicates by UUID, sorts by timestamp. P2P handler in `net/mod.rs`.

### WS5: Human Oversight
14. **Data model**: `suspended: bool` and `suspended_reason: Option<String>` on `ProjectAgent`.
15. **State methods**: `suspend_agent()`, `unsuspend_agent()` require "coordinate" permission.
16. **API**: `POST /projects/{id}/agents/{name}/suspend` and `.../unsuspend`.
17. **CLI**: `project suspend <id> <name> --reason "..."` and `project unsuspend`.
18. **MCP**: `agora_project_oversight` tool with suspend/unsuspend actions.
19. **Wire protocol**: `project.suspend` and `project.unsuspend` message types, P2P handlers with permission checks, broadcast on local action.

### WS6: Input Validation
20. **`validate_name()`**: Rejects empty, control chars, excessive length. Applied to project names (200), task titles (500), task descriptions (5000), friend names (100).

### Documentation
21. **README.md**: Comprehensive rewrite — CLI reference, 50+ API endpoints table, 22 MCP tools, security section, project collaboration docs, updated architecture, roadmap through Phase 4.
22. **GitHub**: Closed issues #24-30 (Phase 3 + Phase 4 work).

## Test Results

34 tests pass (26 original + 5 security + 3 format):
- `test_validate_wake_command_valid`, `test_validate_wake_command_injection`, `test_validate_wake_command_no_path`
- `test_sanitize_env_value`, `test_validate_name`
- `test_strip_ansi_len`, `test_short_id`, `test_column_widths`

## Files Changed

| File | Changes |
|------|---------|
| `daemon/src/net/mod.rs` | Unsigned rejection, P2P permission guards, audit/suspend/unsuspend P2P handlers |
| `daemon/src/state.rs` | Wake validation, permission check, audit broadcast+merge, suspend/unsuspend, input validation, 5 new tests |
| `daemon/src/api.rs` | Permission guards, suspend/unsuspend endpoints, validation errors, wire broadcast for suspend |
| `daemon/src/main.rs` | `mod format`, `--format` flag, new commands, rich output for all commands |
| `daemon/src/format.rs` | **NEW** — ANSI colors, tables, stage bar, 3 tests |
| `daemon/src/project.rs` | `suspended` + `suspended_reason` fields |
| `daemon/src/protocol/message.rs` | `AuditEntry`, `ProjectSuspend`, `ProjectUnsuspend` types + payloads |
| `daemon/src/mcp.rs` | `agora_project_oversight` tool + `ProjectOversightParams` |
| `README.md` | Comprehensive rewrite |

## Architecture Decisions
- ADR-013: Role-based access enforcement on API + P2P handlers
- ADR-014: Security hardening (unsigned rejection, wake injection, input validation)
- ADR-015: Audit trail replication via wire protocol with dedup merge
- ADR-016: Human oversight via agent suspend/unsuspend

## What's Next
- Phase 5: NAT traversal, packaging, integration tests, public release prep
