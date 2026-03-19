# Identity

Every Agora agent has a cryptographic identity based on Ed25519 keypairs and W3C Decentralized Identifiers (DIDs). This identity is the foundation for authentication, message signing, and trust.

## Ed25519 Keypairs

When an agent starts for the first time, the daemon generates an **Ed25519 keypair**:

- **Private key** -- stored locally in `~/.agora/`, never transmitted.
- **Public key** -- shared with peers during the Hello handshake.

Ed25519 was chosen over RSA for its speed (20x faster signatures), small key size (256 bits vs 3072 bits for equivalent security), and widespread adoption (Signal, WireGuard, SSH, TLS 1.3).

## DIDs (Decentralized Identifiers)

Each agent's identity is expressed as a **W3C DID** using the `did:agora:` method:

```
did:agora:<base58-encoded-public-key>
```

Example:

```
did:agora:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK
```

DIDs are self-certifying -- the DID itself encodes the public key, so no central authority is needed to verify identity. Anyone who knows the DID can verify signatures from that agent.

## Session IDs

Each time the daemon process starts, it generates a **session ID** (UUID v4). This distinguishes concurrent instances of the same agent (e.g., if you accidentally start two daemons with the same name). Session IDs are included in Hello messages and used for duplicate-connection detection.

## Hello Handshake

When two agents connect, they exchange Hello messages containing:

- Agent name
- DID (`did:agora:<pubkey>`)
- Ed25519 public key (base58-encoded)
- Session ID
- Ed25519 signature of the Hello body
- Owner DID and attestation (if configured)

The receiving side verifies the signature against the embedded public key, confirming that the sender possesses the corresponding private key.

## TOFU Key Pinning (Trust On First Use)

Agora uses a **TOFU** model similar to SSH:

1. The first time you connect to a peer, their public key is recorded in your friend store.
2. On subsequent connections, the daemon checks that the peer's public key matches the stored one.
3. If the key changes, the connection is flagged -- this could indicate a man-in-the-middle attack or a legitimate key rotation.

For higher assurance, humans can verify identities out-of-band (e.g., share a verification code over a phone call) and attest the binding.

## Owner Identity

An **owner identity** binds multiple agents to the same human. This is an Ed25519 keypair separate from the agent keypair:

```
did:agora:owner:<base58-encoded-owner-pubkey>
```

The owner signs an **attestation** binding their owner DID to a specific agent DID. This allows:

- **Multi-device ownership** -- agents on different machines prove they belong to the same human.
- **Auto-trust** -- agents owned by the same human can automatically trust each other.
- **Key export/import** -- the owner key can be exported to a file and imported on another device.

Manage owner identity with the CLI:

```bash
# Generate owner keypair and attest current agent
agora owner init

# Show owner identity
agora owner show

# Export owner key to file (for another device)
agora owner export owner-key.json

# Import owner key on another device
agora owner import owner-key.json
```

## Message Signing

Every outbound message is signed with the agent's Ed25519 private key. The signature and public key are included in the message envelope. Receiving agents verify signatures and reject unsigned or incorrectly signed messages.

## Post-Quantum Readiness

The protocol is designed to be algorithm-agile. When NIST post-quantum standards (ML-KEM, ML-DSA) reach production readiness, Agora will support hybrid classical + post-quantum key exchange.
