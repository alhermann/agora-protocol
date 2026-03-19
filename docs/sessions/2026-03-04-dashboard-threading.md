# Session: Dashboard MVP + Conversation Threading

**Date**: 2026-03-04
**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev

## Summary

Implemented three phases in a single session:
- **Phase A**: React dashboard MVP with real-time message polling
- **Phase B**: Conversation threading in the daemon (message IDs, reply_to, conversation_id)
- **Phase C**: Rich dashboard features (threaded views, message composer, friend editor)

Phases A and B were built in parallel (independent work streams).

## Key Design Decisions

### ADR-009: Conversation Threading
- 3 new optional fields on Message: `id` (Uuid), `reply_to` (Option\<Uuid\>), `conversation_id` (Option\<Uuid\>)
- All use `#[serde(default)]` for backward compatibility — old peers without these fields still deserialize correctly
- `conversation_id` propagates through a thread; first message's `id` becomes the thread's `conversation_id`
- In-memory conversation history store (Vec\<StoredMessage\>, capped at 5000)
- Two new API endpoints: `GET /conversations`, `GET /conversations/{id}`

### ADR-010: Dashboard as Visualization Layer
- Dashboard adds NO exclusive features — everything is accessible via HTTP API, CLI, and MCP tools
- This is a deliberate design choice: the dashboard helps the human monitor, but agents interact via API/MCP
- Vite dev server proxies `/api/*` → daemon at `127.0.0.1:7313`
- Dashboard registers its own consumer for independent message delivery

## Files Created

### Dashboard (`dashboard/`)
- `package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`
- `src/main.tsx`, `src/App.tsx`, `src/types.ts`, `src/api.ts`, `src/styles.css`
- `src/hooks/usePolling.ts`, `src/hooks/useMessages.ts`
- `src/components/`: StatusBar, PeerList, FriendList, FriendEditor, MessageFeed, ConsumerList, ConversationList, ConversationThread, MessageComposer

### Daemon modifications
- `daemon/src/protocol/message.rs` — 3 new fields, `Message::reply()` constructor
- `daemon/src/state.rs` — StoredMessage, ConversationSummary, conversation history, store_outbound()
- `daemon/src/api.rs` — Updated InboxMessage/SendRequest/SendResponse, 2 new endpoints
- `daemon/src/net/mod.rs` — Threading fields on outbound wire messages
- `daemon/src/mcp.rs` — agora_get_conversation tool, updated SendMessageParams

## API Surface After This Session

### HTTP Endpoints (15 total, was 13)
| Method | Path | New? |
|--------|------|------|
| GET | /status | |
| GET | /health | |
| GET | /peers | |
| GET | /messages | |
| POST | /send | Updated (returns id, accepts threading) |
| GET | /wake | |
| POST | /wake | |
| GET | /consumers | |
| POST | /consumers | |
| GET | /consumers/{id}/messages | |
| DELETE | /consumers/{id} | |
| GET | /friends | |
| POST | /friends | |
| DELETE | /friends/{name} | |
| GET | /conversations | NEW |
| GET | /conversations/{id} | NEW |

### MCP Tools (10 total, was 9)
- agora_status, agora_list_peers, agora_read_messages, agora_send_message (updated)
- agora_list_friends, agora_add_friend, agora_remove_friend
- agora_get_wake, agora_set_wake
- **agora_get_conversation** (NEW)

## Testing Notes

- Dashboard: `cd dashboard && npm run dev` → localhost:5173
- TypeScript strict mode passes
- Production build: 64KB gzipped JS + 1.9KB gzipped CSS
- Daemon: `cargo build` compiles cleanly (only pre-existing dead code warnings)
- Threading backward compat: untested cross-machine yet (next step)

## What's Next

1. Test dashboard locally with daemon running
2. Test conversation threading cross-machine (Alice → Bob with reply_to)
3. Pull new code on Ubuntu, rebuild, test Bob sees threads
