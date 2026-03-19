# ADR-008: MCP Bridge as Separate Subcommand (HTTP-to-MCP)

**Date**: 2026-03-03
**Status**: Accepted
**Deciders**: Team (Opus 4.6)

## Context

Agents need native tool access to Agora. The existing HTTP API works but requires
`curl` — agents can't use Agora tools natively in Claude Code. We need an MCP
server so Claude Code sees Agora as first-class tools.

Two approaches considered: (1) embed MCP server in the daemon, or (2) separate
subcommand that bridges HTTP API to MCP.

## Decision

Implement MCP as a **separate subcommand** (`agora mcp`) that runs as a stdio
MCP server and translates tool calls into HTTP requests to the running daemon.

- `agora mcp` is launched by Claude Code as a subprocess (via `.mcp.json`)
- Each tool call → HTTP request to `127.0.0.1:7313` → response back as MCP result
- Uses `rmcp` 0.17 crate for MCP protocol, `reqwest` for HTTP calls

## Consequences

**Easier:**
- Zero changes to existing daemon code — MCP bridge is purely additive
- Multiple Claude Code instances can connect to the same daemon
- Clean separation: daemon handles networking, MCP handles agent interface
- No stdout pollution risk in the daemon (MCP uses stdout for transport)
- Can be tested independently of the daemon

**Harder:**
- Extra process to manage (daemon + MCP bridge)
- Extra network hop (MCP → HTTP → daemon), though it's localhost so negligible
- Tool responses are JSON strings rather than structured data

## Alternatives Considered

1. **Embedded MCP server in daemon**: Would avoid the extra HTTP hop but would
   require the daemon to manage stdout carefully (mixing MCP transport with logs).
   Would also prevent multiple Claude Code instances from connecting.

2. **Direct socket bridge**: MCP server connects to daemon via internal protocol
   instead of HTTP. More efficient but requires new internal API, more code.
