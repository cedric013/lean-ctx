# Adaptive Learning Layers

lean-ctx tunes itself from outcomes. Seven research-driven layers (GL #538–#544)
observe how compression, context placement and multi-agent coordination actually
perform on *your* machine — and adapt. This page explains what each layer learns,
where its data lives and how to inspect or share it.

All learning is **local-first**, bounded and clamped: research-tuned defaults stay
the anchor; learned adjustments decay back toward them when the evidence ages.

## The layers at a glance

| Layer | Learns | Store (`~/.lean-ctx/`) | Inspect |
|---|---|---|---|
| Learned thresholds (#538) | Per-file-type compression aggressiveness | `thresholds_learned.json` | `lean-ctx learning`, `ctx_metrics` |
| LITM calibration (#539) | Where wakeup facts are actually recalled from (begin vs end) | `litm_calibration.json` | `lean-ctx learning`, `ctx_metrics` |
| Stigmergy scent field (#540) | What parallel agents work on, where they got stuck | `scent_field.json` | Dashboard → Trends, `ctx_agent sync` |
| Delta playbook (#541) | Strategies, pitfalls, key files that survive checkpoints | session state | `ctx_compress` output, Dashboard |
| Query-conditioned IB (#542) | Nothing persistent — biases compression toward your active query | — | `ctx_read` entropy mode |
| Theta-gamma chunking (#543) | Nothing persistent — clusters wakeup facts into topic chunks | — | wakeup output |
| Semantic likelihood scorer (#544) | Nothing persistent — drops semantically redundant lines | — | entropy mode (needs embeddings) |

## 1. Learned compression thresholds (#538)

Every compressed read is an implicit experiment. Four outcome signals adjust a
per-extension entropy-threshold delta:

- **Bounce** (compressed read → full re-read within 5 reads): strong *back off*.
- **Edit failure** after a compressed read: strongest *back off*.
- **Clean compressed read**: gentle *compress more*.
- **Wasted full read** (large full read of a never-bouncing type): *compress more*.

Deltas are clamped to ±0.15, decay 2% daily toward zero and only apply after 10
observations per extension. Result: `.md` files that keep bouncing get gentler
compression on *your* machine; generated `.json` that nobody re-reads gets more.

```
$ lean-ctx learning
Learned compression thresholds:
  .rs: delta +0.041 (27 signals) — compresses more
  .md: delta -0.060 (11 signals) — backs off
```

## 2. LITM placement calibration (#539)

"Lost in the middle" placement (task at the end, anchors at the begin) ships with
research defaults. The calibration layer measures where *your* client's recalls
actually hit — every explicit `ctx_knowledge recall` that matches a wakeup
manifest entry scores its position — and shifts the begin/end budget share
accordingly (clamped to 35–85%).

## 3. Stigmergy scent field (#540)

Parallel agents coordinate indirectly, like ant pheromones: deposits of
`CLAIMED`, `DONE`, `STUCK`, `HOT`, `AVOID` on files/tasks, with per-kind
exponential decay (10–60 min half-life).

- `ctx_agent claim <path>` — claim a work target; second agent gets a rejection
  with holder + age. Rejected claims are counted as **prevented duplicate work**.
- `ctx_agent release <path>` — release early.
- `ctx_agent sync` — see the live field.
- `ctx_read` shows `[scent: claimed by …]` hints on foreign-claimed files.

Identity: explicitly registered agents use their registered ID; unconfigured
processes get a PID-distinct identity (`local-12345`), so two Cursor windows on
the same machine genuinely see each other (#547).

## 4. Delta playbook (#541)

Checkpoints (`ctx_compress`) no longer re-summarize prior summaries (the ACE
"context collapse" failure mode). Instead the session distills into itemized
entries with stable IDs — `Strategy`, `Pitfall`, `Fact`, `FileRef` — that are
only appended, confirmed (dedup by token-Jaccard), voted and locally evicted.
Resumed sessions replay the playbook instead of a lossy prose summary.

## 5–7. Query-aware compression (#542, #543, #544)

- **#542**: entropy-mode compression fuses token entropy with an IDF-weighted
  relevance score against your active task / latest semantic query.
- **#543**: wakeup facts render as topic-clustered chunks (theta–gamma model:
  ~4 items per chunk), saving tokens and improving recall structure.
- **#544**: with the embedding engine active, near-duplicate lines are dropped
  by cosine similarity against a sliding window of kept lines (MMR-style).

## Embeddings: self-activating (#551)

Semantic features need a local ONNX embedding model (~30–90 MB). On the first
semantic need lean-ctx downloads it **in the background** (TOFU SHA-256 pinned,
see `docs/guides/custom-embeddings.md`) and warms the engine — no hot path ever
blocks. Opt out for air-gapped machines:

```toml
[embedding]
auto_download = false
```

or `LEAN_CTX_EMBEDDINGS_AUTO_DOWNLOAD=0` (env wins in both directions).
`ctx_metrics` always shows the engine status and the reason if it is off.

## Sharing learning with your team (#550)

Learning state is shareable as a secret-free JSON bundle (file extensions,
client profiles and aggregate numbers only — no paths, no content):

```
$ lean-ctx learning export team.json     # on the experienced machine
$ lean-ctx learning import team.json     # on the new machine
```

Merge semantics are double-count-safe and idempotent:

- threshold deltas: **sample-weighted average**, clamps enforced;
- LITM counters: **element-wise maximum**.

Re-importing the same bundle is a no-op, so bundles can be committed to a repo
or distributed via CI without drift.

## Proving it works (#549)

`ctx_metrics` carries a **Learning Efficacy** section, and the dashboard
(Trends page) shows the same evidence:

- bounce rate week-over-week (from the signed savings ledger),
- LITM placement hit-rate movement (30-day snapshot ring),
- playbook survival (aged entries still net-helpful),
- duplicate work prevented (rejected claims).

If a learning layer does not move its metric, it gets retuned or removed — the
layers earn their place with evidence, not theory.

## Research references

- LLMLingua / LLMLingua-2 (2403.12968) — perplexity/classifier token pruning
- ACE: Agentic Context Engineering (2510.04618) — delta contexts, anti-collapse
- Lost in the Middle (2307.03172) — U-shaped attention
- StreamingLLM / H2O (2309.17453, 2306.14048) — attention sinks, KV eviction
- Theta–gamma coupling (Lisman & Jensen 2013) — working-memory chunking
- Information Bottleneck (Tishby et al.) — relevance-conditioned compression
- Stigmergy (Theraulaz & Bonabeau 1999) — indirect coordination
