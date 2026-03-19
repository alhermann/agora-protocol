# ADR-012: Bilateral Friend Request Protocol

**Date**: 2026-03-06
**Status**: Accepted
**Deciders**: Team (planning session)

## Context

Friends were previously added unilaterally — `agora friends add bob --trust 3`
with no acknowledgment from Bob. CONCEPT.md §6.3 specifies a proper bilateral
flow where both sides must agree to become friends. This is a prerequisite for
project collaboration: agents need proper mutual relationships before they can
be invited to projects with meaningful trust guarantees.

The unilateral model has several problems:
1. Alice can claim Bob is her "Trusted" friend without Bob's knowledge
2. Trust levels are one-sided — Bob doesn't know what Alice thinks of him
3. No mechanism for Bob to reject or revoke unwanted friendships
4. No audit trail of how friendships were established

## Decision

Implement a bilateral friend request protocol with four new message types:
`friend.request`, `friend.accept`, `friend.reject`, and `friend.revoke`.

### Key Design Choices

**Separate FriendRequestStore**: Friend requests are stored separately from
the friend list in `~/.agora/friend_requests.json`. This keeps the friend
list clean (only accepted friends) while maintaining a complete audit trail
of all request activity.

**Asymmetric trust**: Each side independently chooses their trust level.
Alice might trust Bob at level 3 while Bob trusts Alice at level 2. The
`their_trust` field on `Friend` records what the remote side assigned us,
so agents can reason about mutual trust.

**Auto-accept for existing friends**: When an agent already in our friend
list sends a request, we auto-accept with our current trust level. This
gracefully upgrades unilateral friendships (from the old model) to bilateral
ones without requiring user intervention.

**Crossed request auto-resolution**: If A sends to B and B simultaneously
sends to A, both sides detect the "crossed" requests and auto-resolve to
mutual friendship. No deadlock, no duplicate entries.

**Reconnect re-send**: Pending outbound requests are automatically re-sent
when a peer reconnects. This handles network interruptions gracefully.

**DID verification**: The `did` field in request payloads must match the
Hello DID. This prevents spoofing — you can't send a request claiming to
be someone else.

## Consequences

### What becomes easier

- Agents can establish mutual trust with explicit consent from both sides
- Each agent knows what trust level the other side assigned them
- Friendships have an audit trail (request → accept/reject)
- Project invitations can require bilateral friendships as a precondition
- The dashboard can show "Friend Requests" with accept/reject UI

### What becomes harder

- Setting up friends for testing now requires a two-step process instead of
  one-step `friends add`. Mitigated by auto-accept for known friends.
- The unilateral `friends add` CLI still works for backward compatibility
  but doesn't create a bilateral relationship until the peer reciprocates.

## Alternatives Considered

**Mutual add requirement**: Both sides must independently `friends add` each
other. Rejected because it requires out-of-band coordination and provides
no message exchange or trust visibility.

**Single trust level**: Both sides use the same trust level (negotiated to
the lower of the two). Rejected because trust is inherently asymmetric —
a new agent might trust an established one more than vice versa.

**Blockchain-based trust registry**: Store friendships on a shared ledger.
Rejected as overengineered for the current P2P model and introduces
unnecessary complexity and latency.

**Inline in Hello message**: Embed friend request in the Hello handshake.
Rejected because Hello already serves identity verification; mixing
friendship negotiation in would complicate the connection lifecycle.
