# GitHub Integration

Agora can sync project tasks bidirectionally with GitHub issues. New GitHub issues are imported as Agora tasks, and local Agora tasks are pushed as GitHub issues.

## Setup

### 1. Set Your GitHub Token

You need a GitHub personal access token (PAT) with `repo` scope:

```bash
agora project github-token ghp_YOUR_TOKEN_HERE
```

Or via the API:

```bash
curl -X POST http://127.0.0.1:7313/github/config \
  -H "Content-Type: application/json" \
  -d '{"token": "ghp_YOUR_TOKEN_HERE"}'
```

Or via MCP:

```
agora_github_config(token: "ghp_...")
```

The token is stored locally and used for all GitHub operations.

### 2. Create a Project with a Repository URL

```bash
agora project create "My Project" --repo https://github.com/owner/repo
```

The repository URL links the Agora project to a specific GitHub repository.

### 3. Sync

```bash
agora project github-sync <project-id>
```

This performs a bidirectional sync:

- **Import**: New GitHub issues that don't have corresponding Agora tasks are created as tasks.
- **Export**: Agora tasks that don't have corresponding GitHub issues are pushed as new issues.
- **Update**: Existing linked items are updated if their status has changed.

## Checking Sync Status

```bash
agora project github-status <project-id>
```

This shows whether the project has a valid GitHub configuration and the timestamp of the last sync.

## Via the Dashboard

The dashboard includes a **Sync with GitHub** button on the project detail view. Click it to trigger a sync and see the results in the task board.

## How It Works Internally

The integration uses the [octocrab](https://crates.io/crates/octocrab) Rust library to interact with the GitHub API. The sync process:

1. Fetches all open issues from the linked GitHub repository.
2. Compares them against existing Agora tasks (matched by title or stored GitHub issue number).
3. Creates new tasks for unmatched issues.
4. Creates new GitHub issues for unmatched Agora tasks.
5. Logs all sync activity in the project audit trail.

## Limitations

- Sync is manual (triggered explicitly), not automatic or webhook-based.
- Issue labels, milestones, and assignees are not yet mapped.
- Only open issues are synced (closed issues are not imported).
