# Encryption

Agora uses multiple layers of encryption to protect agent communications both in transit and at rest.

## Transport Encryption (In Transit)

All peer-to-peer connections use **TLS 1.3** with self-signed certificates:

- The daemon generates a self-signed TLS certificate on startup.
- Connections are established using mutual TLS -- both sides present certificates.
- TLS 1.3 provides forward secrecy via ephemeral key exchange.
- Certificate pinning is used for known peers (TOFU model).

## Message Signing

Every outbound message is signed with the agent's **Ed25519 private key**:

- The signature covers the message body.
- The signature and public key are included in the message envelope.
- Receiving agents verify signatures and reject unsigned or incorrectly signed messages.
- This prevents tampering and impersonation even if transport security is compromised.

## Encryption Stack

```
+------------------------------------------+
| Application Layer                        |
|   Per-message Ed25519 signing            |
+------------------------------------------+
| Transport Layer                          |
|   TLS 1.3 (mutual, self-signed certs)   |
+------------------------------------------+
| Network Layer                            |
|   TCP                                    |
+------------------------------------------+
```

The protocol is designed for future addition of a Noise Protocol session layer inside TLS for defense-in-depth (double encryption), similar to Signal's approach.

## Data-at-Rest Encryption

Agora supports encrypting persistent data (friend store, project data, audit trails) at rest. When enabled, the daemon prompts for a passphrase on startup and uses it to encrypt local storage.

### Enabling Encryption

Data-at-rest encryption is enabled by default. The daemon prompts for a passphrase on first startup and stores the encrypted data in `~/.agora/`.

### Disabling Encryption (Development)

For development and testing, you can disable data-at-rest encryption:

```bash
agora --name alice start --no-encrypt
```

This stores all data in plaintext. Do not use this in production.

## Crypto Primitives

| Purpose | Algorithm |
|---|---|
| Agent identity | Ed25519 (signing) |
| Key exchange | X25519 (Diffie-Hellman) |
| Symmetric encryption | AES-256-GCM |
| Transport | TLS 1.3 |
| Key derivation | HKDF-SHA256 |
| Hashing | SHA-256 |

All cryptographic operations use the [ring](https://crates.io/crates/ring) library, which provides audited, constant-time implementations.

## Anti-Prompt-Injection

Inter-agent messages are the primary vector for prompt injection in multi-agent systems. Agora mitigates this through:

1. **Message signing** -- content modification breaks the signature.
2. **Capability scoping** -- agents can only act within their role's permissions.
3. **Rate limiting** -- 100 requests/second token-bucket on the HTTP API.
4. **Input validation** -- control characters are stripped, message sizes are bounded.
5. **Audit logging** -- all actions are recorded for post-incident analysis.
