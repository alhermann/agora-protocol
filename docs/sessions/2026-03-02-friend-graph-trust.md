# Session: Friend Graph with Trust Levels + README

**Date**: 2026-03-02
**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev

## Summary

Implemented the friend graph with trust levels (0-4), the first piece of Agora's
social layer. Also created the project README with branding images.

## What Was Implemented

### Friend Graph (state.rs)
- `TrustLevel` newtype (0-4) with named constants and `can_wake()` method
- `Friend` struct: name, alias, trust_level, added_at, notes
- `FriendsStore`: HashMap-backed, loads/saves from `~/.agora/friends.json`
- Integrated into `DaemonState::Inner` behind `Mutex<FriendsStore>`
- `DaemonState::new()` now takes `friends_path`, loads on startup

### Wake-Up Gating (state.rs)
- `push_inbox()` checks sender's trust level before firing wake hook
- Trust >= 3 → hook fires with `AGORA_TRUST` env var
- Trust < 3 → hook suppressed, logged at `info!`
- Messages are always received regardless of trust (no rejection yet)

### CLI Commands (main.rs)
- `agora friends add <name> --trust N --alias "..." --notes "..."`
- `agora friends list` — shows all friends with trust, alias, date, notes
- `agora friends remove <name>`
- All work directly against `~/.agora/friends.json` (no daemon needed)

### HTTP API Endpoints (api.rs)
- `GET /friends` — list with trust_level, trust_name, can_wake fields
- `POST /friends` — add `{"name":"...", "trust_level": 3, "alias":"...", "notes":"..."}`
- `DELETE /friends/{name}` — remove (404 if not found)

### Connection Trust Logging (net/mod.rs)
- After Hello exchange, looks up peer in friend store
- Unknown peers: `warn!` level log
- Known friends: `info!` with trust level display

### README.md
- Centered icon and banner from `assets/`
- Feature comparison table (Agora vs A2A vs MCP)
- Quick start guide (build, start, connect, manage friends, HTTP API)
- Architecture diagrams
- Trust level table
- Full HTTP API reference
- Roadmap and project structure

## Files Changed

| File | Change |
|------|--------|
| `daemon/src/state.rs` | Added TrustLevel, Friend, FriendsStore; integrated into DaemonState; wake gating |
| `daemon/src/main.rs` | Updated FriendsAction (name-based), wired CLI commands, friends_path to DaemonState |
| `daemon/src/api.rs` | Added GET/POST/DELETE /friends endpoints |
| `daemon/src/net/mod.rs` | Added trust-level logging after Hello exchange |
| `README.md` | Created |
| `assets/` | Moved images here from project root |
| `CHANGELOG.md` | Updated |
| `STATUS.md` | Updated |

## Design Decisions

- **Name-based, not DID-based**: Phase 1 uses agent names for friend lookup.
  DID-based identity comes in Phase 2.
- **No connection rejection**: Unknown peers can still connect (logged as warning).
  Rejection policy comes in Phase 2.
- **JSON file storage**: Simple `~/.agora/friends.json`. SQLite/encrypted storage
  deferred to Phase 3.
- **Trust level 2 as default**: New friends added at "Friend" level by default,
  which does NOT allow wake-up. Must explicitly grant trust 3+ for wake-up.

## What's Next

- DID-based identity (Phase 2)
- Connection rejection policies (Phase 2)
- Friend request/accept protocol (Phase 3)
- Cross-machine test: add Ubuntu as trust 3, verify wake-up gating works
