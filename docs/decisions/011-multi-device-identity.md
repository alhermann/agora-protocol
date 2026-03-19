# ADR-011: Multi-Device Agent Ownership

**Status**: Accepted
**Date**: 2026-03-05
**Decider**: Team (Opus 4.6)

## Context

Each Agora agent generates a machine-local Ed25519 keypair (`~/.agora/identity.key`).
If a human runs agents on multiple machines, each gets a different DID. There is no
concept of "Developer's agent" spanning devices — remote peers see unrelated strangers.

**CONCEPT.md alignment:**
- §5.1: DID documents contain "Human owner identifier (optional, privacy-preserving)"
- §5.2: Key hierarchy — Master Key → Identity Key → Connection Keys → Session Keys
- §6.2: Friend records store `"owner": "Bob <bob@example.com>"`
- §11.4: "Human attestation: humans can verify each other out-of-band and attest the
  binding between a human identity and an agent DID"
- §6.1: Trust Level 4 = "Can act on behalf of the owner (delegated authority)"

## Decision

**Approach 1 — Separate agents, same owner.** Each device keeps its own agent DID.
A separate "owner" Ed25519 keypair represents the human. The owner key signs each
agent's DID to create a verifiable attestation. Remote peers recognize same-owner
agents and can auto-trust them.

## Approaches Considered

| Approach | Description | Pros | Cons | Status |
|----------|-------------|------|------|--------|
| 1. Separate agents, same owner | Each device has own DID; owner keypair signs each | Secure (no key sharing needed for agent keys), simple per-device, no single point of failure | Friends see multiple entries per human | **Chosen** |
| 2. Roaming identity | One keypair synced across devices | One DID for everything, simplest mental model | Key sync is hard; one compromise = all devices; no forward secrecy per-device | Deferred |
| 3. Key delegation (certificates) | Master key issues time-limited device certificates | Best of both; one identity, no key sharing | Complex certificate infrastructure, revocation lists, expiry management | Future goal |

## Why Approach 1

1. **No key sharing** — Agent private keys never leave the machine. The only key
   that gets exported is the owner key, which is a deliberate human-initiated action.
2. **No single point of failure** — Compromising one device's agent key doesn't
   compromise others (unlike Approach 2).
3. **Simple implementation** — Fits naturally into the existing identity system.
   Just adds a new key type and attestation structure.
4. **Incrementally adoptable** — Agents without owners work exactly as before
   (backward compatible).
5. **Foundation for Approach 3** — The owner key can later evolve into a
   certificate authority for time-limited delegations.

## Design

### Owner Identity
- Ed25519 keypair stored at `~/.agora/owner.key` (PKCS#8 DER, chmod 0600)
- DID format: `did:agora:owner:<base58-pubkey>` (distinct prefix from agent DIDs)
- Never auto-generated — requires explicit `agora owner init` or `import`
- Exportable for cross-device transfer

### Owner Attestation
Cryptographic binding of owner → agent:
```
agora:owner-attestation:v1:<owner_did>:<agent_did>:<timestamp>
```
Signed with owner's Ed25519 key. Stored at `~/.agora/owner_attestation.json`.

### Auto-Trust
When a verified owner_did on a connecting peer matches a known friend's owner_did:
- Auto-create friend entry at `min(owner_trust, 3)` — capped at Trusted
- Never auto-grant Inner Circle (trust 4) — that requires human decision

### Wire Protocol
Hello messages carry optional `owner_did` and `owner_attestation` fields.
Old daemons ignore unknown JSON fields; new daemons default to None.

## Consequences

- Friends list may show multiple entries for the same human (one per device).
  The dashboard can group them visually by owner_did.
- Owner key export/import is a manual step — acceptable for the security gain.
- Future: Approach 3 can be layered on top without breaking changes.
