# lean-ctx vs Mem0

> **Last updated:** May 2026 | Both tools give AI agents persistent memory — but for very different use cases.

## Overview

| | lean-ctx | Mem0 |
|---|---|---|
| **Approach** | Local-first context layer for coding agents | Universal memory layer for all AI agents |
| **GitHub Stars** | 2,600+ | 55,000+ |
| **Language** | Rust (single binary) | Python |
| **License** | Apache 2.0 | Apache 2.0 |
| **Focus** | Code-specific (files, shells, repos) | General-purpose (conversations, preferences, entities) |
| **Architecture** | 100% local, no external dependencies | Cloud service or self-hosted (requires LLM + vector DB) |
| **MCP Tools** | 68+ | 9 (cloud-hosted MCP server) |

## The Core Difference

**Mem0** is a universal memory layer for AI applications. It remembers user preferences, conversation history, and entity relationships across any AI system — chatbots, customer support, autonomous agents. It's backed by a $23.5M Series A and targets enterprise AI at scale with SOC2 compliance.

**lean-ctx** is a domain-specific context layer built for AI *coding* agents. It remembers code architecture, session decisions, and task progress — but it also compresses file reads, shell output, and builds a structural code graph. It's not a general-purpose memory system; it's an engineering tool for engineering workflows.

The distinction: Mem0 remembers that "the user prefers dark mode and lives in Berlin." lean-ctx remembers that "auth is in `src/auth/`, uses JWT, the last refactoring broke the session middleware, and `cargo test` passes on main."

## Feature Comparison

| Feature | lean-ctx | Mem0 |
|---------|:--------:|:----:|
| **Memory** | | |
| Knowledge graph | Temporal facts with validity windows | Entity-linked memories with relations |
| Session persistence | Findings, decisions, blockers, progress | User, session, and agent state |
| Temporal reasoning | `was_valid_at()`, validity windows | Temporal memory (April 2026 algorithm) |
| Multi-level memory | Session + knowledge + episodic | User + session + agent levels |
| Entity linking | Via property graph (code entities) | Cross-memory entity linking + embedding |
| **Retrieval** | | |
| Semantic search | Hybrid BM25 + dense vector + graph proximity | Multi-signal (semantic + BM25 + entity) |
| LoCoMo benchmark | Not evaluated | 91.6 (April 2026) |
| LongMemEval | Not evaluated | 93.4 (April 2026) |
| **Code-Specific** | | |
| File read compression | 10 modes (map, signatures, diff, ...) | No |
| Cached re-reads | ~13 tokens | No |
| Shell output compression | 95+ patterns | No |
| Tree-sitter AST analysis | 26 languages | No |
| Call graph | Multi-hop BFS + risk classification | No |
| Blast radius / impact | ctx_impact (6 actions) | No |
| Architecture overview | ctx_architecture (9 actions) | No |
| PageRank repo-map | ctx_repomap (session-aware) | No |
| Repo packing | ctx_pack (.ctxpkg, PR packs) | No |
| Property graph | 8 node types, 14 edge types | No |
| **Operations** | | |
| Multi-agent support | ctx_agent, ctx_handoff, diary, sync | Agent state management |
| Observability | Real-time dashboard, budgets, SLOs | Platform dashboard (cloud) |
| Context proof | Cryptographic verification | No |
| Plugin system | Hook-based extensibility | No |
| **Infrastructure** | | |
| Privacy | 100% local, no external calls | Cloud-hosted or self-hosted |
| LLM required | No | Yes (default: gpt-4o-mini) |
| Vector DB required | No (built-in SQLite) | Yes (Qdrant, Pinecone, etc.) |
| API key required | No | Yes (for embedding + LLM) |
| Installation | Single binary | pip install + infrastructure setup |
| SOC2 compliance | Local-first (your responsibility) | SOC2 certified (managed service) |

## Shared Strengths

Despite different scopes, both tools address the same fundamental problem — AI agents losing context between sessions:

- **Temporal memory**: both track when facts were true and support time-based queries
- **Knowledge graph**: both build structured representations of entity relationships
- **Session persistence**: both survive chat restarts and editor relaunches
- **Multi-agent awareness**: both support multiple agents accessing shared memory
- **Semantic retrieval**: both use hybrid search (BM25 + vector) for relevant recall
- **MCP support**: both expose tools via the Model Context Protocol

## Where Mem0 Leads

### General-Purpose Memory at Scale
Mem0 handles any kind of memory — not just code. User preferences, conversation history, entity relationships, temporal facts across domains. If you're building a customer support bot or a personalized assistant, Mem0 is purpose-built for that.

### Retrieval Quality (Benchmarked)
Mem0's April 2026 algorithm achieves 91.6 on LoCoMo and 93.4 on LongMemEval — state-of-the-art for memory retrieval. These benchmarks measure conversational memory recall, entity linking, and temporal reasoning. lean-ctx hasn't been evaluated on these benchmarks (they measure general conversation, not code-specific recall).

### Enterprise Features
Mem0 offers a managed service with SOC2 compliance, a platform dashboard, cross-platform SDKs, and a cloud-hosted MCP server. For enterprises that need managed infrastructure and compliance certifications, Mem0 has a clear advantage.

### Community and Ecosystem
With 55k+ stars, 310+ contributors, and integrations with LangChain, CrewAI, LangGraph, and more, Mem0 has a large ecosystem. lean-ctx's ecosystem is smaller but growing.

## Where lean-ctx Leads

### Code-Specific Intelligence

lean-ctx understands code at a structural level that Mem0 doesn't attempt:

```bash
# Tree-sitter AST analysis
lean-ctx read src/auth/middleware.ts -m map    # dependency graph + exports

# Call graph traversal
# "Show me everything that calls authenticate() up to 3 hops"

# Impact analysis
# "What breaks if I change the User model?"

# PageRank repo-map
lean-ctx repomap . --max-tokens 2048          # most important code symbols
```

These capabilities require deep understanding of code structure — not something a general-purpose memory system provides.

### Token Compression (Every Interaction)

lean-ctx's core value is compressing every file read and shell command. This directly reduces costs and extends useful context window:

```bash
# File reads: 10 modes from full to aggressive
lean-ctx read src/main.rs -m signatures  # ~98% reduction

# Shell output: 95+ pattern modules
lean-ctx -c "git status"                 # ~85% reduction
lean-ctx -c "cargo test"                 # ~92% reduction
lean-ctx -c "npm install"               # ~93% reduction

# Cached re-reads
lean-ctx read src/main.rs               # ~13 tokens (unchanged)
```

Mem0 doesn't compress file reads or shell output — it's not designed for that workflow.

### 100% Local, No API Keys

lean-ctx runs entirely on your machine with zero external dependencies:

```bash
curl -fsSL https://leanctx.com/install.sh | sh
lean-ctx setup
# Done. No OpenAI key, no vector DB, no Docker.
```

Mem0 requires an LLM (default: gpt-4o-mini via OpenAI API) for memory extraction and a vector database for storage. The managed service simplifies this but requires a cloud account and API key. The self-hosted option requires significant infrastructure.

### Observability and Governance

lean-ctx provides real-time visibility into context window usage:

```bash
lean-ctx gain --live       # real-time token savings
lean-ctx dashboard         # browser-based context manager
lean-ctx wrapped --week    # weekly summary
```

This includes budget controls, SLO policies, and cryptographic context proofs — features specific to managing AI coding agent context windows.

## Architecture Comparison

```
Mem0:
  Conversations → LLM extraction → Memories
                                      ↓
                                Entity Linking → Graph DB
                                      ↓
                                Vector Embeddings → Vector DB
                                      ↓
                                Retrieval: semantic + BM25 + entity fusion

lean-ctx:
  Code Files → tree-sitter → Property Graph (SQLite)
      ↓                          ↓
  Compression → Session Cache → Knowledge Facts (temporal)
      ↓                          ↓
  Shell Output → Pattern Match → Compressed Output
      ↓                          ↓
  Embeddings → ONNX (local) → Hybrid Search (BM25 + dense + graph)
                                      ↓
                                Observability Dashboard
```

## When to Use Which

### Choose Mem0 if you...

- Build general-purpose AI applications (chatbots, assistants, customer support)
- Need memory for non-code conversations (preferences, history, entities)
- Want enterprise-grade managed infrastructure with SOC2
- Need proven retrieval quality on standard memory benchmarks
- Integrate with LangChain, CrewAI, or other AI frameworks

### Choose lean-ctx if you...

- Use AI coding agents daily (Cursor, Claude Code, Codex, ...)
- Need code-specific intelligence (call graphs, impact analysis, repo-maps)
- Want token compression on file reads and shell output
- Require 100% local operation with no API keys or external services
- Want 68+ specialized coding tools, not just memory

### Can You Use Both?

Yes. Mem0 and lean-ctx operate at different levels and don't conflict. You could use Mem0 for cross-application user memory (remembering preferences across tools) and lean-ctx for code-specific context within your AI coding workflow. The tools serve complementary purposes.

## Summary

Mem0 is the leading general-purpose memory layer for AI, with 55k+ stars, state-of-the-art benchmarks, and enterprise backing. It's the right choice for building AI applications that need to remember conversations, preferences, and entities across sessions.

lean-ctx is a domain-specific tool built for one thing: making AI coding agents more effective. It provides code-aware memory alongside compression, structural intelligence, and observability — all running locally with no external dependencies.

The choice comes down to your use case: general AI memory vs. coding agent context engineering.

---

*Both projects are open source under Apache 2.0.*

[Get started with lean-ctx](https://leanctx.com/docs/getting-started) | [Mem0 on GitHub](https://github.com/mem0ai/mem0) | [Mem0 Docs](https://docs.mem0.ai)

