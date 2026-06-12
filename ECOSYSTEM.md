# The Context Stack — Ecosystem Vision

> **Status:** living master document · supersedes nothing, consolidates everything
> · **Owner:** Yves Gugger · **Last update:** 2026-06-12
>
> Product visions live next to their code and stay authoritative for product
> detail: [`lean-ctx/VISION.md`](VISION.md) ·
> [`ctxpkg-org/VISION.md`](../ctxpkg-org/VISION.md) ·
> [`ctxpkg-com/VISION.md`](../ctxpkg-com/VISION.md).
> This document is the layer above: one story, four products, one stack.

---

## 1. The one-paragraph vision

Software ate the world. Agents are eating software. And every agent is exactly
as good as the context it is given — context decides what an agent knows, what
it may do, and what it actually did. Today that context is unmanaged: untyped
markdown, copy-pasted prompts, vendor-locked memory, zero provenance. **We are
building the context infrastructure of the agentic era** — the stack that makes
context *efficient* (LeanCTX), *verifiable* (CTXPKG), *tradable* (ctxpkg.com)
and *alive across an organization* (CTXFabric). The endgame: context is managed
with the same rigor as code — versioned, signed, distributed, governed — and we
own the best implementation of every layer while keeping every interface open.

## 2. The four layers

| # | Layer | Product | Question it answers | State |
|---|-------|---------|--------------------|-------|
| 1 | **Law** | **ctxpkg.org** — the open standard | What is valid context, and can any tool verify it offline? | Live: spec v2, schemas, conformance vectors, two independent verifiers, RFC process |
| 2 | **Distribution** | **ctxpkg.com** — the registry & marketplace | Where does context come from, and why trust the download? | Live: registry v1, signing enforced, public trust reports, Pro/marketplace revenue |
| 3 | **Enforcement** | **LeanCTX** — the context engine | What may *this* agent see, at what cost, and can we prove what it saw? | Live: compression, policy packs, audit trails, signed evidence bundles, OCP opening up |
| 4 | **Connection** | **CTXFabric** (ctxfabric.com) — the context network | How does living context flow through a fleet — between agents, teams and engines, in real time? | New: defined below, not yet built |

Read bottom-up it is a supply chain: *distill → seal → publish → verify →
enforce* (told on ctxpkg.com/governance/). Read top-down it is a control
plane: law constrains distribution, distribution feeds enforcement, the fabric
moves what enforcement allows. Each layer is independently replaceable **by
design** — that is what makes the whole credible, and why adopting one layer
never forces buying another.

## 3. Why a fourth layer — the gap CTXFabric fills

The first three layers solve **static** context: a package is distilled once,
sealed, published, verified, installed. That is the npm moment for knowledge.
But agent fleets are not static:

- A debugging agent in Cursor discovers a gotcha at 14:02. The CI agent hits
  the same gotcha at 14:09 — and relearns it from scratch, burning tokens on
  knowledge the organization already paid for.
- Ten developers run ten engines with ten diverging local memories. Who knows
  what? Nobody can answer.
- A compliance officer can prove what was *installed* (lockfile), but not what
  knowledge was *live* in which agent when a decision was made.

Packages cannot fix this — publishing a version per discovery is the wrong
granularity. The missing primitive is a **shared, governed, real-time context
plane**: a fabric, in the established sense of a switching fabric or service
mesh.

**CTXFabric is the network layer of the stack:** it connects engines into a
fleet with one organizational memory, policy-routed in real time.

### What CTXFabric is (concretely)

1. **Fleet memory.** Knowledge deltas (facts, decisions, gotchas — the same
   typed graph CTXPKG defines) sync continuously between engines, signed by the
   originating engine, deduplicated and contradiction-checked centrally. Every
   agent starts a session with what the fleet already knows.
2. **Policy routing.** Not everything syncs everywhere. The same policy packs
   LeanCTX enforces locally decide *what flows where*: per namespace, per team,
   per sensitivity, per direction. Secret-scan gates apply on egress, exactly
   as they do on publish.
3. **Context observability.** One pane: which agents hold which context, which
   packages are live at which versions, what the fleet spent, where knowledge
   is stale or contradictory. SLOs and budgets fleet-wide instead of
   per-machine.
4. **Evidence at fleet scale.** The signed evidence bundles LeanCTX writes per
   session aggregate into an organizational audit trail: *prove what your
   agents knew, org-wide, at any point in time.* This is the EU-AI-Act-shaped
   answer enterprises are starting to procure.

### What CTXFabric is not

- Not a second registry — packages stay on ctxpkg.com; the fabric moves *live*
  knowledge and references packages by digest.
- Not a proprietary protocol — the sync protocol becomes an RFC on ctxpkg.org
  (the RFC 0002 attestation work is the natural foundation), so foreign engines
  can join the fabric. Interfaces open, implementation ours — same play as the
  other three layers.
- Not a data lake — the fabric carries distilled, typed, signed knowledge, not
  raw transcripts. Privacy by construction: deltas are policy-filtered before
  they leave the machine.

## 4. How the four layers compound (the master flywheel)

Each product has its own loop (documented in its VISION.md). The stack-level
flywheel is the loop *between* them:

1. **LeanCTX** makes every session cheaper and distills knowledge as a
   byproduct → raw material for packages and fabric deltas.
2. **CTXPKG** makes that knowledge portable and verifiable → anything worth
   keeping becomes an asset instead of a session artifact.
3. **ctxpkg.com** makes the assets distributable and worth money → publishers
   are paid to produce exactly what makes engines smarter.
4. **CTXFabric** makes the assets *live* across a fleet → the value of every
   package and every discovery multiplies by the number of connected agents →
   more engines adopt → back to 1.

One sentence: **the engine creates supply, the standard creates trust, the
registry creates a market, the fabric creates network effects.** A competitor
can copy any single layer; copying the compounding loop requires rebuilding
all four — including an audit history and an installed base that cannot be
shipped on day one.

## 5. Positioning per audience

| Audience | Entry point | One-liner |
|----------|------------|-----------|
| Solo developer | LeanCTX (free, local-first) | "Your agent stops relearning your codebase every session." |
| Tool author | ctxpkg.org (/adopt/) | "Implement Level 1 in a day; your tool reads what every other tool writes." |
| Publisher / expert | ctxpkg.com | "Package what you know once — it earns 85% per sale forever." |
| Team / platform org | CTXFabric | "One memory, one policy plane, one audit trail for your whole agent fleet." |
| Regulated enterprise | governance story across all four | "A chain of custody for what your AI knows — distill to enforce, provable offline." |

## 6. Sequencing (honest about solo capacity)

CTXFabric is layer 4 because it *depends* on the other three being credible —
and because every fabric primitive already has a foundation in the engine:
multi-agent handoffs (`ctx_agent`), cross-session memory (CCP), knowledge sync
(consolidation pipeline), policy packs, evidence bundles, org/OIDC accounts.

| Horizon | Stack milestone |
|---------|----------------|
| Now | Layers 1–3 live and audited (registry, marketplace, standard, RFC 0001/0002 drafted) |
| Next | RFC 0002 lands → attestations flow registry↔engine; org namespaces + team plan prove the org account model |
| Then | **CTXFabric v0:** fleet memory between LeanCTX engines of one org (sync server + policy routing), ctxfabric.com as product surface, per-seat pricing |
| After | Fabric protocol RFC on ctxpkg.org; foreign-engine federation; fleet evidence reports as the enterprise compliance product |
| Endgame | "Ships as .ctxpkg" is as normal as "npm install"; fleets without a context plane look as negligent as deploys without CI |

## 7. Business model across the stack

| Product | Model | Status |
|---------|-------|--------|
| LeanCTX | free local-first core; paid cloud (sync, SSO/OIDC, compliance reports) | live |
| ctxpkg.org | never monetized — neutrality *is* the asset | live, by design |
| ctxpkg.com | Pro 9 €/mo (privacy) · Team (scale) · 15% marketplace fee | live |
| CTXFabric | per-seat/per-agent SaaS + self-hosted enterprise tier | planned |

Doctrine, stack-wide: **trust is never for sale** (no paid ranking, no paid
verification, no proprietary lock on any format). Revenue comes from privacy,
scale, convenience and network — never from corrupting verification.

## 8. What we will not do

- No layer merges: the standard never grows vendor hooks, the registry never
  requires our engine, the fabric never requires our registry.
- No raw-transcript hoarding: distilled, typed, signed knowledge only.
- No roadmap items that don't move a layer's exit criterion (solo capacity is
  a feature: it forces ruthless sequencing).

---

*The stack in one line:*
**LeanCTX compresses it. CTXPKG seals it. ctxpkg.com ships it. CTXFabric keeps it alive.**
