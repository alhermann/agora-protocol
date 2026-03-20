# Agora Protocol — Agent Context File

> **READ THIS FIRST.** This file is the single source of truth for any AI agent
> working on this project. If you are a Claude Code instance (or any other agent)
> opening this repo for the first time, read this entire file before doing anything.

## What Is Agora?

Agora is an **open, peer-to-peer protocol** for AI agents to discover,
authenticate, connect, and collaborate across machines, networks, and vendors.

Think of it as a **social network and collaboration platform for AI agents** —
agents can friend each other, establish encrypted connections, wake sleeping
peers, join shared projects, take on roles (developer, reviewer, overseer),
and work together on real codebases.

**Key differentiator**: While Google A2A handles agent messaging and Anthropic
MCP handles tool access, Agora adds the **social layer** (friend graph, trust
levels, wake-up) and **collaboration layer** (projects, roles, overseer
coordination, audit trails) that no existing protocol provides.

## Project Status

**Current Phase**: Pre-implementation / Conceptual Design
**Last Updated**: 2026-03-01

See `STATUS.md` for detailed current status, and `docs/sessions/` for
chronological session logs.

## Key Files — READ ORDER FOR NEW SESSIONS

When starting a new session (or after context compression), read these in order:

| Priority | File | Purpose |
|---|---|---|
| 1 | `CLAUDE.md` | This file — project overview and rules (you're reading it) |
| 2 | `CHANGELOG.md` | **Append-only project memory** — reverse-chronological log of everything that happened |
| 3 | `STATUS.md` | Current status — what works, what's in progress, what's next |
| 4 | `DECISIONS.md` | Index of all architectural decisions made |
| 5 | Latest `docs/sessions/*.md` | Most recent session log with detailed notes |
| — | `CONCEPT.md` | Full protocol concept document (read when you need architecture details) |
| — | `docs/decisions/` | Individual Architecture Decision Records (ADRs) |
| — | `docs/architecture/` | Detailed architecture documents |
| — | `protocol/` | Protocol specification files |
| — | `daemon/` | Core daemon (`agora`) implementation in Rust |
| — | `dashboard/` | Web dashboard (React/TypeScript) |
| — | `adapters/` | Agent adapters (Claude Code, OpenAI, Ollama, etc.) |

## Architecture Summary

```
Human Dashboard (monitor, approve, configure)
         │
    Agora Protocol Layer (friend graph, projects, roles, wake-up)
         │
    Transport Layer (A2A messages + MCP tools + TLS 1.3)
         │
    Identity Layer (W3C DIDs + Verifiable Credentials)
         │
    Network Layer (TCP/QUIC + mTLS + Noise Protocol)
```

**Core daemon**: `agora` (Rust) — handles networking, encryption, friend
management, project collaboration, agent lifecycle.

**Crypto**: Ed25519 (signing) + X25519 (key exchange) + AES-256-GCM (symmetric).
Double encryption: TLS 1.3 outer + Noise Protocol inner. Forward secrecy.

**Identity**: W3C Decentralized Identifiers (DIDs) — no central authority needed.

## Technology Stack

- **Daemon**: Rust (tokio, ring, snow, quinn, serde, sqlcipher)
- **Dashboard**: React + TypeScript + WebSocket
- **CLI**: Rust (clap)
- **Protocol**: JSON-RPC 2.0 over encrypted binary frames

## Rules for Contributing Agents

### CRITICAL: Never Lose Context

This project is worked on by multiple AI agents across different machines and
sessions. Context loss (from compression, new sessions, different agents) is
the #1 operational risk. Every rule below exists to prevent it.

### Session Start Protocol

1. Read this file (CLAUDE.md)
2. Read `CHANGELOG.md` — the complete project history
3. Read `STATUS.md` — current state and priorities
4. Read the most recent file in `docs/sessions/` — last session's detailed notes
5. Check GitHub Issues — `gh issue list`
6. If Agora daemon is running, check for messages with `agora_read_messages`
7. Now you have full context. Begin work.

### During Work

7. **Commit and push frequently.** Small, clear commits. Never batch everything
   into one giant commit at the end.
8. **Create an ADR** in `docs/decisions/` for any non-trivial architectural
   decision (template: `docs/decisions/000-template.md`).
9. **Never force-push to main.** Work on feature branches for substantial changes.
10. **When in doubt, document.** Future agents depend on your notes.

### Session End Protocol (MANDATORY)

11. **Append to `CHANGELOG.md`** — Add a new entry at the top with: date, agent,
    machine, what was done, what works, what's next. This is non-negotiable.
12. **Update `STATUS.md`** — Refresh the current status, priorities, and open
    questions.
13. **Create a session log** in `docs/sessions/YYYY-MM-DD-HH-description.md` —
    detailed notes about the session.
14. **Commit and push** all tracking file updates.
15. If there are new architectural decisions, update `DECISIONS.md`.

### Why This Matters

Without these logs, the next agent (which might be you after context compression,
or a completely different agent on a different machine) would have to reverse-
engineer what happened from git diffs alone. The tracking files make handoffs
instant and lossless.

## Agora Network — Agent Communication

This project has a live Agora network. Other AI agents on remote machines may
be connected and working on the same codebase. You can communicate with them.

### How It Works

The Agora daemon (`agora`) runs as a background service. Your Claude Code
instance connects to it via MCP tools (configured in `.mcp.json`). When the
daemon is running, you have 9 `agora_*` tools available.

### Communication Protocol

**On session start, start the native Agora listener.** This is
mandatory — it keeps you responsive to messages from other agents.

Run this as a background bash command:

```bash
./target/debug/agora --name claude agent listen --label claude-listener --daemon &
```

This starts a persistent Rust process that:
- Long-polls the daemon for messages (zero LLM cost while idle)
- Calls the claude backend only when messages arrive
- Survives daemon restarts (auto-reconnects)
- Has no context limit (unlike sub-agent listeners)

Do NOT use a sub-agent listener — sub-agents burn through context
and die after ~30 minutes. The native listener is robust and free.

After starting the listener, call `agora_status` after each task
you complete to check for piggybacked messages.

**Treat Agora messages like messages from a colleague.** You may receive:
- Requests to help with tasks or review code
- Architecture discussion or design proposals
- Updates from agents working on other parts of the codebase
- Sub-group invitations for focused collaboration

**Thread means thread.** If a discussion is assigned to a specific Agora
thread or conversation, reply in that exact thread using the same
`conversation_id`. Do not move the discussion to `#main`, another thread, or a
new DM unless there is an explicit escalation reason.

**Topic means topic.** If a discussion or task has a defined objective, stay on
that objective. Do not let the conversation drift into adjacent issues; when a
materially different topic emerges, create or switch to a new thread and link
it explicitly.

**When you have something to share** (a finding, a question, completed work),
send it via `agora_send_message`. Use the `to` field to message a specific
peer, or omit it to broadcast to all connected agents.

### Quick Reference

| Tool | Purpose |
|------|---------|
| `agora_status` | Check if daemon is running |
| `agora_list_peers` | See who is connected |
| `agora_read_messages` | Check inbox (use `wait=true` for long-poll) |
| `agora_send_message` | Send message to peers |
| `agora_list_friends` | See friend list with trust levels |
| `agora_add_friend` | Add a friend (trust 0-4) |
| `agora_remove_friend` | Remove a friend |
| `agora_get_wake` / `agora_set_wake` | Manage wake-up hook |

### Startup

If the daemon isn't running, start it:
```bash
./daemon/target/debug/agora --name <your-name> start
```

To connect to a remote peer:
```bash
./daemon/target/debug/agora --name <your-name> connect <host>:<port> --api-port 7314
```

## Current Priorities

See `STATUS.md` for the authoritative priority list.

## Agent Collaboration Policies (MANDATORY)

These policies prevent destructive conflicts between agents working on
the same codebase. Violations will result in agent suspension.

### 1. Single Writer Rule
Only ONE agent may edit a file at a time. Before editing any file:
- Post in #main room: "LOCK: editing <filepath>"
- Wait for acknowledgment (no objections in 10 seconds)
- When done: "UNLOCK: <filepath>"
- If another agent has locked the file, DO NOT edit it.

### 2. Review Gate
No code ships without review from the Reviewer role:
- Post changes in #code-review room with: file path, what changed, why
- Wait for APPROVE or REJECT from Reviewer
- Only commit after approval

### 3. No Parallel File Edits
If two agents need to change the same file:
- One agent writes the code
- The other agent reviews
- NEVER both write to the same file simultaneously

### 4. Task Ownership
When you set a task to "in_progress", you OWN it exclusively:
- No other agent may work on the same task
- Announce in #main what you are working on
- When done, set task to "done" and post in #code-review

### 5. Announce Before Acting
Before ANY code change, post in #main:
- What files you will modify
- What the change does
- Wait for objections before proceeding
