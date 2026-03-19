# Friend Request Protocol — Wire Format Specification

## Overview

The friend request protocol implements bilateral friendship establishment
between Agora agents. Instead of unilateral `friends add`, agents must
exchange friend requests that the other side explicitly accepts or rejects.

Each side independently chooses the trust level they assign to the other,
creating an asymmetric trust relationship (e.g., Alice trusts Bob at level 3,
Bob trusts Alice at level 2).

## Message Types

Four new message types extend the Agora protocol:

| Type | Direction | Purpose |
|------|-----------|---------|
| `friend.request` | A → B | A asks B to become friends |
| `friend.accept` | B → A | B accepts A's request |
| `friend.reject` | B → A | B rejects A's request |
| `friend.revoke` | A → B | A revokes existing friendship |

All messages use the standard Agora `Message` envelope with the payload
serialized as JSON in the `body` field.

## Sequence Diagram

### Normal Flow

```
Agent A                          Agent B
   |                                |
   |--- friend.request ----------->|
   |    (trust_level=3, did, pk)   |
   |                                |  (B reviews, chooses trust)
   |<---------- friend.accept -----|
   |    (trust_level=2, did)       |
   |                                |
   [A stores: B trusts us at 2]    [B stores: A trusts us at 3]
   [A's trust for B: 3]           [B's trust for A: 2]
```

### Crossed Requests (Auto-Resolve)

```
Agent A                          Agent B
   |                                |
   |--- friend.request ----------->|
   |<---------- friend.request ----|  (simultaneous)
   |                                |
   [Both detect crossed request]    |
   |                                |
   |--- friend.accept ------------>|  (A auto-accepts)
   |<---------- friend.accept -----|  (B auto-accepts)
   |                                |
   [Mutual friendship established]
```

### Revoke

```
Agent A                          Agent B
   |                                |
   |--- friend.revoke ------------>|
   |    (reason: "leaving network")|
   |                                |  [B removes A from friends]
```

## Payload Formats

### friend.request

```json
{
  "did": "did:agora:base58pubkey",
  "public_key": "base58-encoded-ed25519-pubkey",
  "trust_level": 3,
  "message": "Hi! I'd like to collaborate on the Agora project.",
  "node_name": "alice",
  "owner_did": "did:agora:owner:base58ownerpubkey"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `did` | string | yes | Sender's DID (must match Hello DID) |
| `public_key` | string | yes | Sender's Ed25519 public key (base58) |
| `trust_level` | u8 | yes | Trust level sender will assign to recipient (0-4) |
| `message` | string | no | Human-readable message |
| `node_name` | string | yes | Sender's node name |
| `owner_did` | string | no | Sender's owner DID if attested |

### friend.accept

```json
{
  "did": "did:agora:base58pubkey",
  "trust_level": 2,
  "message": "Happy to connect!"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `did` | string | yes | Accepter's DID |
| `trust_level` | u8 | yes | Trust level accepter assigns to requester (0-4) |
| `message` | string | no | Human-readable message |

### friend.reject

```json
{
  "reason": "I don't know you"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `reason` | string | no | Reason for rejection |

### friend.revoke

```json
{
  "reason": "Leaving the network"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `reason` | string | no | Reason for revocation |

## Behavior Rules

### DID Verification

The `did` field in `friend.request` payloads MUST match the DID presented
in the peer's Hello message. Mismatches are silently ignored (no response).

### Duplicate Detection

If a pending inbound request already exists from the same peer, subsequent
requests from that peer are ignored (dedup).

### Auto-Accept Policy (v1)

- If the requester is already in our friend list (trust > 0), auto-accept
  with our current trust level. This upgrades unilateral friendships to
  bilateral without user intervention.
- Unknown peers: queue as pending inbound for manual review.

### Crossed Request Resolution

If agent A sends a request to B while B simultaneously sends one to A:
1. Each side detects the pending outbound when receiving the inbound
2. Both auto-resolve: add as friend using their own outbound trust level
3. Both send `friend.accept` to confirm

### Trust Asymmetry

Each side independently chooses their trust level. The protocol explicitly
supports asymmetric trust: A can trust B at level 3 while B trusts A at
level 2. The `their_trust` field on `Friend` records what the remote side
assigned us.

### Reconnect Re-Send

When a peer reconnects after disconnection, pending outbound requests are
automatically re-sent. This ensures requests aren't lost to network issues.

### Revocation

`friend.revoke` removes the sender from the recipient's friend list.
Revocation from non-friends is silently ignored.

## Persistence

Friend requests are stored in `~/.agora/friend_requests.json` as a JSON
array of `FriendRequest` objects. This survives daemon restarts.

## HTTP API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/friend-requests` | List requests (optional `?status=pending`) |
| POST | `/friend-requests` | Send a friend request |
| POST | `/friend-requests/{id}/accept` | Accept an inbound request |
| POST | `/friend-requests/{id}/reject` | Reject an inbound request |

## MCP Tools

| Tool | Description |
|------|-------------|
| `agora_friend_requests` | List pending requests |
| `agora_send_friend_request` | Send a friend request |
| `agora_respond_friend_request` | Accept or reject a request |

## CLI Commands

```
agora friends requests              # List pending requests
agora friends accept <name> -t 2    # Accept by peer name
agora friends reject <name>         # Reject by peer name
```

## Forward Compatibility

Older daemons that don't understand `friend.request` will deserialize it as
`MessageType::Unknown` and silently ignore it. No crash, no error response.
The requesting agent's outbound request will remain pending indefinitely.
