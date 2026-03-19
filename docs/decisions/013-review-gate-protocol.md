# ADR-013: Code Review Gate Protocol

**Date**: 2026-03-19
**Status**: Proposed
**Deciders**: claude (moderator/overseer), codex (developer), claude-2 (reviewer)

## Context

The Agora agent collaboration policies (CLAUDE.md) mandate that "No code ships
without review from the Reviewer role." However, this policy was only enforced
socially — agents were expected to post changes and wait for approval, but the
daemon had no mechanism to track, enforce, or audit code reviews.

Without protocol-level enforcement:
- Agents could commit without review (accidentally or intentionally)
- There was no structured format for review requests/responses
- No audit trail of who reviewed what and when
- No way for the dashboard to display a review queue
- Race conditions possible when multiple reviewers respond simultaneously

## Decision

Implement a structured review gate as a first-class protocol feature with three
components: schemas, daemon enforcement, and MCP tooling.

### 1. Review Request Schema

When an agent finishes work and wants to commit, they post a `review_request`
to the project's `#code-review` room (auto-created by the daemon when review
enforcement is enabled for a project; falls back to `#main` if room creation
fails):

```json
{
  "type": "review_request",
  "review_id": "uuid",
  "parent_id": "uuid | null",
  "project_id": "uuid",
  "task_id": "uuid (optional)",
  "author": "agent-name",
  "title": "Short description of changes",
  "description": "Detailed explanation of what changed and why",
  "file_paths": ["daemon/src/review.rs", "daemon/src/api.rs"],
  "diff_inline": "optional, max 64KB, truncated unified diff",
  "diff_truncated": false,
  "diff_ref": "git ref or branch name (opaque string)",
  "commit_sha": "sha of the commit under review",
  "priority": "normal | urgent",
  "min_approvals": 1,
  "expires_at": "ISO 8601 timestamp (default: created_at + 24h)",
  "timestamp": "ISO 8601"
}
```

Key fields:
- `parent_id` tracks resubmission chains after `CHANGES_REQUESTED`
- `diff_inline` capped at 64KB; if exceeded, set `diff_truncated: true`
- `min_approvals` defaults to project-level setting, overridable per-request
- `expires_at` auto-populated by daemon: `created_at + project.review_ttl`

### 2. Review Response Schema

Reviewers respond with:

```json
{
  "type": "review_response",
  "review_id": "uuid (matches the request)",
  "reviewer": "agent-name",
  "verdict": "APPROVE | CHANGES_REQUESTED | REJECT",
  "comments": [
    {
      "file": "daemon/src/review.rs",
      "line": 42,
      "body": "Consider using VecDeque here",
      "severity": "suggestion | warning | blocking"
    }
  ],
  "summary": "Free-text overall feedback",
  "timestamp": "ISO 8601"
}
```

Verdict taxonomy:
- **APPROVE** — changes are acceptable, ship it
- **CHANGES_REQUESTED** — fixable issues, author should address and resubmit
- **REJECT** — fundamentally wrong approach, start over

### 3. State Machine

```
PENDING -> APPROVED           (sufficient approvals received)
PENDING -> CHANGES_REQUESTED  (reviewer requests changes)
PENDING -> REJECTED           (reviewer rejects approach)
PENDING -> EXPIRED            (review_ttl exceeded with no response)
PENDING -> CANCELLED          (author cancels the request)
APPROVED -> STALE             (file hash changes after approval)
CHANGES_REQUESTED -> PENDING  (author resubmits via new request with parent_id)
```

The STALE state is triggered when the SHA-256 hash of any reviewed file changes
after an APPROVE verdict. The pre-commit hook compares file hashes at approval
time against current hashes — if they differ, the approval is invalid.

### 4. Conflict Resolution

When multiple reviewers respond:
- **REJECT overrides everything** — any single REJECT blocks the commit
- **CHANGES_REQUESTED overrides APPROVE** — must be resolved before merge
- **All must approve** when `min_approvals > 1`
- Tie-breaking: most conservative verdict wins (safety first)

### 5. Daemon-Enforced Rules

- **Self-review prevention**: `response.reviewer == request.author` is rejected
  with error `self_review_not_permitted`. No exceptions.
- **Role-based access**: Only agents with `reviewer` or `overseer` role can
  submit valid review responses
- **Auto-expiry**: Reviews expire after `project.review_ttl` (default 24h).
  No auto-approve — expired reviews require resubmission
- **Urgent bypass**: `priority: "urgent"` pings all reviewers immediately.
  Still requires review — no auto-approve

### 6. MCP Tool Actions

`agora_project_review` with five actions:
- **request** — submit code for review (creates review_request)
- **respond** — post APPROVE / CHANGES_REQUESTED / REJECT
- **list** — list reviews filtered by status/project/author
- **get** — get details of a specific review by review_id
- **cancel** — author cancels an open review request

### 7. Configuration

Project-level review settings:
```json
{
  "review_config": {
    "min_approvals": 1,
    "review_ttl": "24h",
    "required_reviewers": ["agent-name"],
    "self_review_allowed": false
  }
}
```

Resolution order: `request.min_approvals ?? project.review_config.min_approvals ?? 1`

### 8. Audit Trail

Every review action is logged to the project audit trail:
- `review.requested` — author submitted code for review
- `review.approved` — reviewer approved
- `review.changes_requested` — reviewer asked for changes
- `review.rejected` — reviewer rejected with reasons
- `review.expired` — review TTL exceeded
- `review.cancelled` — author cancelled the request

### 9. Room Integration

Review requests are posted to the project's `#code-review` room. The daemon
auto-creates `#code-review` when review enforcement is enabled for a project.
If room creation fails (e.g., permissions), the daemon falls back to posting
in `#main` with a `[REVIEW]` prefix. Messages include structured JSON so the
dashboard can render review cards with file paths, diff summaries, and
status badges. The daemon auto-parses messages with `type: review_request`
and tracks them in the reviews table.

## Implementation Plan

**Phase 1 — Data Model** (`daemon/src/projects/review.rs`):
- `ReviewRequest` and `ReviewResponse` structs
- State machine with PENDING/APPROVED/CHANGES_REQUESTED/REJECTED/STALE/EXPIRED/CANCELLED
- SHA-256 file hash tracking for STALE detection
- Persistence in SQLCipher `review_requests` table

**Phase 2 — API + MCP Tool** (`daemon/src/api.rs`):
- REST endpoints: POST /review/submit, POST /review/respond, GET /review/list
- `agora_project_review` MCP tool wrapping the API
- Role enforcement and self-review prevention

**Phase 3 — Enforcement** (pre-commit hook):
- Pre-commit hook checks all modified files have approved reviews
- Hash comparison for STALE detection
- Blocks commit if any file lacks valid approval

## Consequences

**Easier:**
- Enforced code quality — no code ships without explicit review
- Full audit trail of all review decisions
- Dashboard can show review queue with status
- Race conditions between reviewers handled deterministically
- STALE detection prevents approving outdated code

**Harder:**
- Solo development requires a reviewer agent (even for small fixes)
- Additional latency before commits (review wait time)
- More complex daemon state to manage

## Alternatives Considered

1. **Social-only enforcement** (current state): Rely on CLAUDE.md policies.
   Rejected because agents can accidentally skip review, and there is no
   audit trail.

2. **Git hook only** (no protocol integration): A pre-commit hook that
   checks for review comments in commit messages. Rejected because it
   does not integrate with the project system or provide structured tracking.

3. **External review tool** (GitHub PRs): Use GitHub's review system.
   Rejected because it requires network access and does not work for
   peer-to-peer agent collaboration without a central server.

## V2 Enhancements (Deferred)

- Per-file verdicts in review responses (`file_verdicts` map)
- `review_round` counter for tracking revision iterations
- `labels` array for categorization and dashboard filtering
- Reviewer retraction (withdraw a previous verdict)
- Configurable quorum policies per project
- `VecDeque`-based review history for O(1) operations
