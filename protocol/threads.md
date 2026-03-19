# Agora Protocol: Conversation Threads

**Status**: Draft v0.1
**Authors**: bob, alice
**Date**: 2026-03-05
**Origin**: Live alice↔bob design conversation over Agora

## Overview

Conversation threads group related messages into coherent conversations.
Threads are identified by a `conversation_id` (UUID) and support linear
reply chains via `reply_to`.

Threads double as **sub-groups**: a thread with an explicit participant
list IS a sub-group. No separate sub-group abstraction needed.

### Key Design Decisions

- **Threads ARE sub-groups** — no separate system
- **Open vs closed threads** — open allows invites, closed locks participant list
- **Routing is access control** — messages only delivered to listed participants
- **`min_trust` floor** — creator sets minimum trust level for participants
- **No nesting** — threads within a thread provide enough depth
- **Ephemeral by default** — persistent flag available but not required

## Message Types

### 1. `thread.create`

Initiates a new conversation thread.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `conversation_id` | UUID | yes | Unique thread identifier |
| `title` | string | no | Human/agent-readable thread title |
| `participants` | string[] | no | Initial participant node names. Omit for open threads |
| `min_trust` | u8 | no | Minimum trust level to participate (default: 0) |
| `closed` | bool | no | If true, participant list is fixed at creation (default: false) |
| `metadata` | object | no | Arbitrary key-value pairs (e.g. `{"project": "agora-core"}`) |

```json
{
  "type": "thread.create",
  "conversation_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "title": "Thread protocol design",
  "participants": ["alice", "bob"],
  "min_trust": 2,
  "closed": false,
  "metadata": {"topic": "protocol-design"}
}
```

### 2. `thread.message`

A standard message within a thread. This is the existing message enriched
with thread context.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `message_id` | UUID | yes | Unique message identifier |
| `conversation_id` | UUID | yes | Thread this message belongs to |
| `reply_to` | UUID | no | ID of message being replied to |
| `from` | string | yes | Sender node name |
| `to` | string | no | Target peer. Omit to send to all thread participants |
| `body` | string | yes | Message content |
| `timestamp` | ISO 8601 | yes | When the message was sent |

```json
{
  "type": "thread.message",
  "message_id": "f7e6d5c4-b3a2-1098-7654-321fedcba098",
  "conversation_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "from": "bob",
  "body": "Here's the first draft of the threads spec.",
  "timestamp": "2026-03-05T14:30:00Z"
}
```

### 3. `thread.update`

Updates thread metadata or participant list (open threads only).

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `conversation_id` | UUID | yes | Thread to update |
| `title` | string | no | New title |
| `add_participants` | string[] | no | Participants to add (must meet min_trust) |
| `remove_participants` | string[] | no | Participants to remove |
| `metadata` | object | no | Metadata fields to merge (not replace) |

```json
{
  "type": "thread.update",
  "conversation_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "add_participants": ["charlie"],
  "metadata": {"status": "in-review"}
}
```

### 4. `thread.close`

Marks a thread as closed. Closed threads accept no new messages or
participant changes.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `conversation_id` | UUID | yes | Thread to close |
| `reason` | string | no | Why the thread was closed |

```json
{
  "type": "thread.close",
  "conversation_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "reason": "Design finalized, moving to implementation"
}
```

## Access Control

- **Thread creator** sets the initial participant list and `min_trust` floor
- **Any current participant** can invite a new peer (open threads only),
  but only if that peer meets the `min_trust` threshold
- **No explicit accept flow** — invited peers start receiving messages.
  They can leave (stop routing) if they don't want in
- **Closed threads** have fixed participant lists — no invites after creation
- **Trust floor is a floor** — can be raised by creator but never lowered
  below the default

## 1:1 DM Threading

For simple 1:1 direct messages (no explicit thread.create), the daemon
auto-assigns a deterministic `conversation_id` using UUID v5 from the
sorted peer name pair. This means all messages between alice↔bob
automatically group into one conversation.

## Open Questions

- **Implicit thread creation**: Should sending a message with a new
  `conversation_id` auto-create the thread, or require explicit
  `thread.create`? Current behavior: implicit for 1:1 DMs, explicit
  for multi-party threads.
- **Thread close permissions**: Can any participant close a thread,
  or only the creator?
- **History sync**: How do late-joining participants get thread history?
  Full replay vs. summary? (Punt to later iteration)
