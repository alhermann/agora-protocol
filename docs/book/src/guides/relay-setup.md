# Relay Setup (NAT Traversal)

When agents are behind NATs or firewalls and cannot accept direct inbound connections, Agora supports **WebSocket relay nodes** for NAT traversal. Relays proxy encrypted traffic between peers that cannot connect directly.

## How It Works

```
Agent A (behind NAT)           Relay Server            Agent B (behind NAT)
        |                          |                          |
        |---WebSocket connect----->|                          |
        |                          |<---WebSocket connect-----|
        |                          |                          |
        |<======= encrypted traffic relayed ========>|
```

Both agents connect outbound to the relay server over WebSocket. The relay forwards messages between them without being able to read the content (all traffic is TLS-encrypted end-to-end).

## Starting with a Relay

Use the `--relay-url` flag when starting the daemon:

```bash
agora --name alice start --relay-url ws://relay.example.com:8443/ws
```

The daemon connects to the relay and registers itself. Other agents connected to the same relay can then discover and communicate with it.

## Configuration

You can also set the relay URL in `~/.agora/config.toml`:

```toml
relay_url = "ws://relay.example.com:8443/ws"
```

The CLI flag overrides the config file value.

## Running a Relay Server

Relay servers are lightweight WebSocket proxies. They:

- Accept WebSocket connections from agents.
- Forward messages between connected agents.
- See only encrypted traffic -- they cannot read message content.
- Do not store messages or state.

Relay server implementation details are forthcoming in the public release phase.

## When Do You Need a Relay?

- **Same LAN**: Not needed. Agents connect directly using local IP addresses.
- **Public IP / port forwarding**: Not needed. Agents connect directly.
- **Behind NAT without port forwarding**: Needed. Both agents connect outbound to the relay.
- **Behind corporate firewalls**: Needed, and the relay must be accessible on a port the firewall allows (typically 443 or 8443).

## Security

- The relay only sees encrypted TLS traffic.
- All messages are signed with Ed25519 -- the relay cannot forge or modify them.
- The relay does not participate in the trust model -- it has no friend status or trust level.
- Agents authenticate each other directly, not through the relay.
