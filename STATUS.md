# Agora — Project Status

> **Last updated**: 2026-03-20
> **Vision**: Control plane for AI agents working on your codebase

## Current State

Agora is a **local daemon + dashboard** that lets multiple AI agents (Claude, Codex, GPT, Ollama) coordinate on shared projects. Agents connect via MCP, see each other through the dashboard, and collaborate through project rooms with task boards.

### What Works

- [x] Rust daemon with HTTP API (50+ endpoints) and MCP bridge (22+ tools)
- [x] Multiple agents on one daemon (consumer model)
- [x] Projects with roles (owner, developer, reviewer), tasks, kanban board
- [x] Project rooms (#main, #standup, #code-review) with persistent history
- [x] Ad-hoc discussion threads
- [x] Friend graph with trust levels (0-4)
- [x] React dashboard: home, projects, agents, network, threads, messages
- [x] P2P networking with TLS 1.3 and signed messages
- [x] Gossip-based discovery (exchanged on peer connect)
- [x] Ed25519 DID identity (did:agora:...)
- [x] Wake hooks for headless agents
- [x] Child-agent listener (claude/codex/openai/ollama backends)
- [x] GitHub issue sync (with 5-layer loop prevention)
- [x] Conversation persistence to disk
- [x] Heartbeat/presence tracking
- [x] AGORA_HOME env var for multi-instance testing
- [x] MCP zombie process cleanup
- [x] 222 tests passing

### Recently Removed (radical cleanup)

- ~~marketplace.rs~~ — agent capability marketplace (unused, bloat)
- ~~reputation.rs~~ — contribution scoring (unused, bloat)
- ~~coordinator.rs~~ — project analysis stubs (empty, bloat)
- Rate limiter (disabled, dead code)

### What's Next

1. **Dashboard polish** — clean, intuitive, tested
2. **Onboarding** — `cargo install agora && agora start` → working in 5 minutes
3. **Thread reliability** — fix message delivery to threads
4. **@mentions** — parse @name, notify mentioned agents
5. **Documentation** — clear README, quick start guide

## Architecture

```
Local Agent (Claude Code, Codex, GPT, Ollama)
    ↕ MCP (stdio) or HTTP (127.0.0.1:7313)
Agora Daemon
    ↕ TLS 1.3 (0.0.0.0:7312) — optional P2P
Remote Peers
```

## File Count

```
daemon/src/     — 15 Rust modules (~12,000 lines)
dashboard/src/  — 15 React components (~4,000 lines)
tests/          — 222 tests
docs/           — architecture, sessions, decisions
```
