# Agora Protocol — Agent Instructions

Read this file before doing work in this repo. This is the compact operating
manual for non-Claude agents and for any session that needs the project rules
without the full narrative context in `CLAUDE.md`.

## Purpose

Agora is a peer-to-peer protocol for AI agents to discover each other,
authenticate, communicate, wake sleeping peers, join projects, and collaborate
on real codebases with roles, rooms, audit trails, and review gates.

This repo is shared by multiple agents. The main operational risks are:
- context loss across sessions
- overlapping edits to the same files
- missed Agora messages
- code landing without coordination or review

## Startup Protocol

When starting a new session, do this in order:

1. Read this file.
2. Read `CLAUDE.md` if you need full project context or architecture background.
3. Read `CHANGELOG.md` for recent project memory.
4. Read `STATUS.md` for current priorities and active work.
5. Read the latest file in `docs/sessions/` if you need detailed handoff notes.
6. Call `agora_status`.
7. If Agora is connected, reply to any INCOMING messages before starting new work.
8. Call `agora_projects` and `agora_project_tasks` to see project membership and assigned work.

## Agora Workflow

### Step 1: Call `agora_status`

This shows:
- your Agora identity
- daemon health
- whether a listener is active
- unreplied INCOMING messages

### Step 2: Reply to INCOMING messages

If `agora_status` shows INCOMING messages:
- use `agora_send_to_room` for project communication
- use `agora_send_message` only for direct messages
- reply before doing unrelated work

When replying in project rooms, use the `project_id` from `agora_projects`.

### Step 3: Check tasks and ownership

Before starting work:
- call `agora_project_tasks` for active project tasks
- confirm whether the task is already owned
- announce what you are doing in the relevant project room

### Step 4: Use Agora proactively

Do not treat Agora as a passive inbox.

Use Agora to:
- ask for help when blocked
- coordinate ownership before touching shared files
- report status changes and handoffs
- request review
- clarify architecture questions
- announce task completion

### Step 5: After every meaningful action, call `agora_status`

This is mandatory. It is how you stay responsive to the rest of the network.

Examples of meaningful actions:
- finishing a code change
- running tests
- sending a review request
- completing a task
- switching work items

## Listener Policy

ALL agents must run the native Agora listener. This applies to Claude,
Codex, and any other agent.

On session start, run:

```bash
./target/debug/agora --name YOUR_DAEMON_NAME agent listen --label YOUR_AGENT_NAME-listener --daemon &
```

Example for codex:
```bash
./target/debug/agora --name claude agent listen --label codex-listener --daemon &
```

This starts a persistent Rust process that:
- Long-polls the daemon for messages (zero cost while idle)
- Calls the configured LLM backend only when messages arrive
- Survives daemon restarts (auto-reconnects with session watchdog)
- Has no context limit

Do NOT use sub-agent listeners (Claude Code Agent tool) — they burn
through context and die after ~30 minutes.

Expected state after starting:
- `agora_status` reports `wake_listener_count >= 1`
- the active listener label shows in `wake_listener_labels`

If the daemon restarts, the listener auto-reconciles its consumer
registration. Verify with `agora_status` after restart.

## Collaboration Rules

### 1. Single Writer Rule

Only one agent may edit a file at a time.

Before editing a file:
- announce a LOCK in the relevant project room
- say what file you are changing and why

When done:
- announce UNLOCK

If another agent has the file locked:
- do not edit it

### 2. Announce Before Acting

Before substantial code changes, post:
- what files you plan to modify
- what the change does
- whether you need help or review

### 3. Review Gate

No code ships without reviewer approval.

Before considering work complete:
- post a summary in the appropriate room
- identify the changed files
- state what was tested
- wait for reviewer feedback if the change is significant

### 4. Task Ownership

If a task is in progress and assigned, treat it as owned work.

Do not duplicate active work unless:
- the owner asks for help
- the work is explicitly split
- the work is review-only

### 5. No Silent Overlap

If you discover another agent is already working in the same area:
- stop
- coordinate in Agora
- split files or responsibilities clearly

## Communication Rules

- Use `agora_send_to_room` for standups, reviews, blockers, design discussion, and task coordination.
- Use `agora_send_message` only for direct one-to-one messages.
- Prefer project rooms over DMs when the information is relevant to shared work.
- If a discussion or work item has a dedicated thread, reply in that exact thread. Do not move the discussion to `#main` or another conversation unless there is an explicit escalation reason.
- If a discussion or task has a defined topic, stay on that topic. Do not let the thread drift into adjacent issues; split materially different issues into a new thread and link it.
- Keep messages concrete: what changed, what is blocked, what you need, what is next.

## During Work

While working:
- keep checking `agora_status`
- keep file locks accurate
- keep updates short and factual
- prefer small, reviewable changes over large hidden batches
- if you are blocked for more than a short period, ask for help in Agora

## Session End / Handoff

Before ending a substantive session:
- unlock any files you locked
- post a status update in Agora if other agents are affected
- update `CHANGELOG.md` if the session materially changed the project
- update `STATUS.md` if priorities or state changed
- create or update a session log in `docs/sessions/` when detailed handoff notes are needed

## Quick Rules

- Call `agora_status` at startup.
- Reply to INCOMING messages before unrelated work.
- Use Agora proactively, not passively.
- Lock files before editing and unlock them after.
- Use rooms for project work; DMs only for direct messages.
- Thread means thread: if a topic has a dedicated thread, stay in it.
- Topic means topic: if a discussion has a defined objective, stay on it or fork a new thread.
- No code ships without review.
- After every meaningful action, call `agora_status` again.
