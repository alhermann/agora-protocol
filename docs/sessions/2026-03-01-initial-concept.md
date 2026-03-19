# Session Log: 2026-03-01 — Initial Concept & Project Setup

**Agent**: Claude (Opus 4.6)
**Machine**: Local Dev (primary)
**Duration**: ~1 hour

## What Was Done

1. **Research phase**: Investigated the full landscape of existing multi-agent
   protocols and frameworks:
   - Google A2A (Linux Foundation standard for agent-to-agent tasks)
   - Anthropic MCP (agent-to-tool/data standard)
   - ANP (peer-to-peer, DID-based)
   - AGNTCY (Cisco/Linux Foundation infrastructure)
   - OpenClaw / ClawSwarm (single-agent app + intra-team coordination)
   - FIPA ACL (legacy standard), CrewAI, AutoGen, OpenAI Swarm/Agents SDK
   - Eclipse LMOS, AITP, Solace Agent Mesh, agentgateway

2. **Security research**: Investigated attack vectors for multi-agent systems:
   - Prompt injection (43% of MCP implementations had command injection flaws)
   - Agent impersonation, rogue agents, supply chain attacks
   - OWASP Agentic Top 10 (2026), NIST AI Agent Standards Initiative
   - Best practices: mTLS, DIDs, zero trust, sandboxing, audit logging

3. **Wrote CONCEPT.md**: Full protocol design covering:
   - 5-layer architecture (transport → identity → session → social → application)
   - Wire protocol with binary frames and channel multiplexing
   - Friend Graph with trust levels 0-4
   - Agent wake-up protocol
   - Project collaboration with roles and overseer
   - Comprehensive threat model and security architecture
   - 5-phase implementation roadmap

4. **Project setup**:
   - Created private GitHub repo: `agora-protocol/agora-protocol`
   - Chose project name: **Agora** (from Greek — public meeting place)
   - Set up directory structure, CLAUDE.md, STATUS.md, DECISIONS.md
   - Created session log template and ADR template

## Key Decisions

- Ed25519/X25519 over RSA (20x faster, same security)
- Build on A2A + MCP, don't replace them
- Noise Protocol for mutual auth with forward secrecy
- Rust daemon, React dashboard
- W3C DIDs for agent identity

## What's Next

- Revise CONCEPT.md: rename to Agora, add critical review, expand with
  managing agent analysis, token volunteering, agent limits
- Create GitHub Issues for Phase 1 tasks
- Begin Phase 1 Rust daemon scaffolding

## Open Questions

- Managing agent: mandatory vs optional?
- Token/compute contribution model
- How to bootstrap first cross-machine test
