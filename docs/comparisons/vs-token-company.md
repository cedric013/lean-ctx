# lean-ctx vs The Token Company

> **Last updated:** June 2026 | Both tools cut LLM token costs by compressing
> context — but from opposite ends: TTC compresses **prose with a trained model
> in the cloud**, lean-ctx compresses **code with deterministic rules, locally.**

## Overview

| | lean-ctx | The Token Company (TTC) |
|---|---|---|
| **Approach** | Local, rule/algorithm-based context layer for coding agents | Cloud gateway with a trained delete-only model |
| **Core engine** | Entropy + information-bottleneck + AST (deterministic) | "Bear-2" ML token classifier (keep/delete) |
| **Best at** | Code: files, shell output, repos | Unstructured prose: chat, docs, RAG |
| **Determinism** | Pure function of input — no model, no drift | Deterministic per (model version, setting) |
| **Runs** | 100% local, single Rust binary, no egress | Cloud API / gateway |
| **Integration** | MCP tools + CLI + API proxy (base-URL swap) | Gateway (base-URL swap) + API |
| **Privacy** | Data never leaves the machine | Content sent to TTC's service |
| **License** | Apache 2.0 (OSS) | Commercial SaaS |

## The Core Difference

**The Token Company** trained a model ("Bear-2") that classifies each token as
*keep* or *delete* — it never paraphrases, only removes. That makes it excellent
at squeezing **unstructured natural language** (conversation history, retrieved
documents, system prompts) where rule-based methods struggle. It ships as a
drop-in gateway: swap your provider base URL and prose is compressed
transparently, with a per-role *aggressiveness* dial and `<ttc_safe>` markers to
protect spans.

**lean-ctx** compresses **code and tool output** using deterministic algorithms —
Shannon-entropy line filtering, query-conditioned information bottleneck,
tree-sitter AST signatures, and 95+ shell-output pattern modules. It runs
entirely on your machine and guarantees that identical inputs always produce
byte-identical output, with **no model and therefore no drift over time**.

The distinction: TTC makes *a paragraph of chat history* smaller. lean-ctx makes
*`cargo build` output, a 2,000-line source file, and a repo map* smaller — and
proves it never changed the answer.

## Feature Comparison

| Feature | lean-ctx | The Token Company |
|---|:--:|:--:|
| **Compression target** | | |
| Source-code files | 10 modes (map, signatures, diff, …) | Generic text only |
| Shell / build / test output | 95+ pattern modules, errors preserved | Generic text only |
| Unstructured prose / chat / RAG | Information-bottleneck + dedup | **Trained model (strongest)** |
| Cached re-reads | ~13 tokens | — |
| **Control** | | |
| Intensity dial | **Single 0–1 `aggressiveness` knob** (#708) | **Single `aggressiveness` float** |
| Per-role control (system / user) | **Yes** — proxy, cache-safe (#710) | **Yes** |
| Protect spans | **`<lc_safe>` / `protect`** (#709) | **`<ttc_safe>` / `protect()`** |
| **Determinism** | Pure function, no drift (#498) | Per (model version, setting) |
| Prompt-cache preserving | **Cache-aware pruning + `cache_preservation_ratio`** (#732) | Assistant passthrough |
| **Code intelligence** | | |
| Tree-sitter AST | 18–26 languages | — |
| Call graph / impact / repo-map | Yes | — |
| Semantic search | Hybrid BM25 + vector + graph | — |
| **Infrastructure** | | |
| Runs locally | **100% local, no egress** | Cloud service |
| Integration | MCP + CLI + API proxy | Gateway + API |
| Signed savings / audit ledger | **Ed25519-signed ROI batches** | — |

## Shared Strengths

- **Same mission**: shrink the tokens an LLM has to pay for, without breaking the
  task.
- **Gateway model**: both offer a drop-in base-URL swap (lean-ctx's API proxy on
  `127.0.0.1`, TTC's cloud gateway).
- **Protect/aggressiveness UX**: both converge on a simple intensity dial plus
  explicit "don't touch this" markers.
- **Lossless philosophy**: TTC is delete-only (no paraphrase); lean-ctx is
  delete/transform-only with anti-inflation guards (never returns more tokens
  than the raw input).

## Where The Token Company Leads

### Unstructured-prose compression
A trained classifier beats hand-written rules on free-form natural language —
chat transcripts, retrieved RAG chunks, long system prompts. This is TTC's real
moat. lean-ctx has narrowed the gap with query-conditioned information-bottleneck
compression for prose (task-conditioned entropy mode) and cache-safe prose
squeezing in the proxy — but a trained model still leads on pure free-form prose.
A *local* delete-only model was evaluated and deliberately deferred: it cannot yet
meet lean-ctx's determinism + single-local-binary bar (see the prose-model spike).

### Public accuracy arena
TTC leads with blind evaluation (they report CoQA rising from 93.3 to 95.3 with
compression on) and a large public preference arena (they report 268K+ votes).
lean-ctx now ships its **own** deterministic accuracy proof — a curated needle /
long-context-QA / code-edit suite behind a CI gate (`eval ab --gate --margin`),
with a model-free test that the compressed context still contains the answer
(#712). What TTC still has and lean-ctx does not is that *public, at-scale* vote
count — a marketing asset, not a capability gap.

### Turn-key per-role UX
One `aggressiveness` float and per-role settings out of the box is a clean,
approachable interface for non-engineers.

## Where lean-ctx Leads

### Determinism without model drift
lean-ctx output is a **pure function of (file content, mode, CRP mode, task)** —
guaranteed byte-stable and CI-tested (issue #498). TTC is deterministic only for
a *fixed model version + setting*; when "Bear-2" is updated, output changes. For
reproducible builds, audits, and prompt-cache stability, a no-model guarantee is
the stronger one.

### Prompt-cache preservation
lean-ctx prunes history only at **frozen, cache-aware boundaries** and never
rewrites content the client marked with `cache_control` — so Anthropic/OpenAI
prompt caches keep hitting (cheap cached-prefix tokens instead of full-price
rewrites). Model-based prose rewriting is inherently harder to keep prefix-stable.

### Code & tool-output intelligence
File reads with AST-aware signature extraction, 95+ shell patterns that always
preserve compiler errors and test summaries, call graphs, impact analysis, and
PageRank repo-maps. None of this is in scope for a general-purpose prose
compressor.

### 100% local / no egress
Nothing leaves the machine — a hard requirement for security-sensitive teams
(the "Great Filter" / CISO use case). A cloud gateway that sees your prompts and
code is a non-starter under many data-governance regimes.

### Signed, auditable savings
Every saving is recorded in a hash-chained ledger and exported as Ed25519-signed
ROI batches — verifiable cost evidence, not a dashboard number.

## How Each Tool Ensures Deterministic Output

| | The Token Company | lean-ctx |
|---|---|---|
| Mechanism | Model argmax at a fixed setting | Pure algorithms, no model |
| Reproducible across time | Only within a model version | Always |
| Cache key | (model version, aggressiveness) | (content, mode, crp_mode, task) |
| Failure mode | Output shifts on model update | None (regression-tested) |
| Verification | Internal | Public byte-stability tests (#498) |

**Takeaway:** TTC achieves *settable* determinism and manages drift via model
versioning. lean-ctx achieves *absolute* determinism because there is no model
in the path. Both are valid; lean-ctx's is the stronger guarantee for audit and
caching, at the cost of weaker free-form-prose compression — now narrowed by a
query-conditioned IB prose path (shipped), with a local trained model evaluated
and deferred to keep the no-model guarantee intact.

## Which Tool Should I Use?

### "I'm compressing chatbot history, RAG context, or long prose prompts"
**Use The Token Company.** A trained model is the right tool for free-form
natural language, and their gateway makes it a one-line change.

### "I'm running a coding agent (Cursor, Claude Code, Codex, Copilot)"
**Use lean-ctx.** File reads, shell/build output, repo structure, and session
memory are exactly what it's built for — and it keeps your code on your machine.

### "I have strict data-governance / no third-party data processing"
**Use lean-ctx.** It is 100% local with no egress; a cloud gateway cannot meet a
no-egress requirement.

### "I need reproducible, audit-grade output and prompt-cache stability"
**Use lean-ctx.** Deterministic with no model drift, prefix-stable pruning, and
a signed savings ledger.

### "I want both code compression and best-in-class prose compression"
Run **lean-ctx locally** for code and tool output; its query-conditioned IB now
compresses prose locally too, so most prose no longer has to leave the machine.
For the last mile on pure free-form prose, TTC's trained model still leads — pair
them if a cloud prose pass is acceptable for your data-governance rules.

---

*Honest comparison policy: every page lists where the competitor leads. TTC's
prose model and accuracy evidence are genuine strengths; lean-ctx's locality,
determinism, code intelligence, and cache preservation are genuine strengths.
Numbers attributed to TTC are as reported on their site/docs (June 2026) and not
independently verified.*

[The Token Company →](https://thetokencompany.com/) · [lean-ctx docs →](https://leanctx.com/docs)

