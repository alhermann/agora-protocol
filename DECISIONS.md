# Agora — Architecture Decision Record Index

This file indexes all architectural decisions made for the Agora protocol.
Each decision has a dedicated file in `docs/decisions/`.

| ID | Date | Decision | Status |
|---|---|---|---|
| ADR-001 | 2026-03-01 | Use Ed25519/X25519 over RSA for cryptographic operations | Accepted |
| ADR-002 | 2026-03-01 | Use Noise Protocol Framework (XX pattern) for authentication | Accepted |
| ADR-003 | 2026-03-01 | Build on A2A + MCP rather than creating competing standards | Accepted |
| ADR-004 | 2026-03-01 | Use W3C DIDs for agent identity | Accepted |
| ADR-005 | 2026-03-01 | Implement daemon in Rust | Accepted |
| ADR-006 | 2026-03-01 | Project name: Agora | Accepted |
| ADR-007 | 2026-03-01 | Managing agent optional, auto-suggested at >2 agents | Proposed |
| ADR-008 | 2026-03-03 | MCP bridge as separate subcommand (HTTP-to-MCP) | Accepted |
| ADR-009 | 2026-03-04 | Conversation threading via optional UUID fields (backward-compatible) | Accepted |
| ADR-010 | 2026-03-04 | Dashboard as visualization-only layer (all features via CLI/API/MCP) | Accepted |
| ADR-011 | 2026-03-06 | Multi-device identity: separate agents, same owner (Approach 1) | Accepted |
| ADR-012 | 2026-03-06 | Bilateral friend request protocol with asymmetric trust | Accepted |
| ADR-013 | 2026-03-07 | Role-based access enforcement on API + P2P handlers | Accepted |
| ADR-014 | 2026-03-07 | Security hardening: unsigned rejection, wake injection, input validation | Accepted |
| ADR-015 | 2026-03-07 | Audit trail replication via wire protocol with dedup merge | Accepted |
| ADR-016 | 2026-03-07 | Human oversight: agent suspend/unsuspend with wire protocol broadcast | Accepted |
| ADR-017 | 2026-03-08 | TOML config file (~/.agora/config.toml) with CLI override semantics | Accepted |
| ADR-018 | 2026-03-08 | Project-conversation linkage via project_id field on StoredMessage | Accepted |
| ADR-019 | 2026-03-08 | GitHub integration via octocrab with bidirectional issue sync | Accepted |
| ADR-020 | 2026-03-08 | Token-bucket rate limiting middleware (100 req/s) | Accepted |
| ADR-021 | 2026-03-08 | Library crate (lib.rs) for integration test access to daemon internals | Accepted |
| ADR-022 | 2026-03-08 | WebSocket relay for NAT traversal (outbound-only, DID-routed) | Accepted |
| ADR-023 | 2026-03-08 | Argon2id + AES-256-GCM for data-at-rest encryption | Accepted |
| ADR-024 | 2026-03-08 | Offline message queue with Ack-based delivery confirmation | Accepted |
| ADR-025 | 2026-03-08 | Capability-based agent marketplace with relevance-scored search | Accepted |
| ADR-026 | 2026-03-08 | Reputation scoring with weighted contributions + exponential decay | Accepted |
| ADR-027 | 2026-03-08 | Hybrid coordinator: rule-based core + optional LLM enhancement | Accepted |
