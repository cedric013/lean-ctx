# Addon Bootstrap Engine — design (Phase 2)

> Status: **design / not implemented**. This document specifies the next step
> after the Phase-1 "add == install" work for ephemeral runners. It is the
> reference for the GitLab epic *Addon Bootstrap Engine* (`root/lean-ctx#1105`,
> subtasks `#1106`–`#1110`).

## Problem

Today `lean-ctx addon add` is **declarative**: it appends a
`[[gateway.servers]]` entry to the global config and records the install — it
never fetches or installs a package. That is sufficient for **ephemeral
runners** (`npx`, `uvx`), which download and execute their package lazily on the
first tool call. So `repomix` and `serena` already install on add.

It is **not** sufficient for tools that need a real, one-time bootstrap before a
runnable command exists:

| Tool      | Why a runner can't do it                                              |
|-----------|----------------------------------------------------------------------|
| Headroom  | Ships as `headroom-ai[all]`; `uvx --from` breaks on its entry points — needs `uv tool install`. |
| Graphify  | `uv tool install "graphifyy[mcp]"` **and** a pre-built `graph.json`.  |
| Cognee    | Clone + `uv sync`; no single-command runner.                         |
| Letta     | `npm i -g` plus a long-running server instance.                       |

For these the registry stays **listed** (homepage + manual instructions). The
bootstrap engine closes that gap: a declarative `[install]` block that lean-ctx
can execute idempotently, with a clean uninstall path and the same security bar
as the rest of the addon system.

## Goals / non-goals

**Goals**
- One declarative `[install]` block per addon, pinned and auditable.
- Idempotent install + reliable uninstall (no orphaned global packages).
- Reuse the existing trust/audit pipeline — no new ad-hoc shell-outs.
- Keep Phase-1 behaviour unchanged when no `[install]` block is present.

**Non-goals**
- No arbitrary script execution (no `curl | sh`, no inline shell).
- No secret provisioning (Mem0 / Claude-Context keys stay the user's job; the
  engine only documents and validates the required env names).
- No new network fetch of the registry itself (still compiled-in / signed override).

## The `[install]` manifest block

```toml
[install]
manager = "uv"                     # one of: pip | uv | cargo | npm | brew
package = "headroom-ai[all]"       # the package spec the manager understands
version = "1.4.2"                  # MANDATORY exact pin (no ranges, no "latest")
verify  = ["headroom", "--version"]# argv run after install; non-zero ⇒ install failed
# bin = "headroom"                 # optional: resolved binary the [mcp] command expects
```

Rules (enforced by the validator, see *Security gates*):
- `manager` ∈ a fixed allowlist. Each manager maps to a **fixed argv template**
  the engine owns — the manifest never supplies raw shell.
- `version` is required and must be an exact pin; reuse the `is_pinned`
  heuristic from `trust.rs`.
- `verify` is an **argv array** (never a shell string) and must exit zero.
- The block is only meaningful together with a runnable `[mcp]` block whose
  `command` is produced by the install (e.g. `headroom`).

### Manager → argv templates (engine-owned)

| `manager` | install argv (conceptual)                       | uninstall argv                  |
|-----------|-------------------------------------------------|---------------------------------|
| `uv`      | `uv tool install "{package}=={version}"`        | `uv tool uninstall {bin}`       |
| `pip`     | `pip install --user "{package}=={version}"`     | `pip uninstall -y {package}`    |
| `cargo`   | `cargo install {package} --version {version}`   | `cargo uninstall {bin}`         |
| `npm`     | `npm i -g "{package}@{version}"`                | `npm rm -g {package}`           |
| `brew`    | `brew install {package}` (+ pin)                | `brew uninstall {package}`      |

The manifest chooses a manager + package + pin; it **cannot** influence the flags
or inject extra argv. This is what keeps the surface auditable.

## Install lifecycle (in `addons/install.rs`)

```mermaid
flowchart TD
  add["addon add <name>"] --> has{has [install]?}
  has -- no --> wire["wire [[gateway.servers]] (Phase 1)"]
  has -- yes --> gate["bootstrap gate: validate + consent"]
  gate --> present{already present? (verify argv)}
  present -- yes --> wire
  present -- no --> run["run engine-owned install argv (pinned)"]
  run --> verify["run verify argv → must exit 0"]
  verify -- ok --> record["record install receipt (manager, package, version, bin)"]
  record --> wire
  verify -- fail --> rollback["best-effort uninstall + abort, no wiring"]
```

- **Idempotency**: run `verify` *first*; if it already succeeds at the pinned
  version, skip the install and just wire. Re-running `add` is safe.
- **Receipt**: extend `<data_dir>/addons/installed.json` with an `install`
  record (manager, package, version, resolved bin, timestamp-free/contentful so
  it stays determinism-friendly per #498). `remove` reads it to uninstall.
- **Uninstall**: `addon remove` runs the manager's uninstall argv for packages
  *this engine installed* (tracked by receipt) — never something the user had
  already. Shared/global packages: only remove if the receipt owns them.
- **Failure**: any non-zero step aborts the whole `add` and leaves no
  `[[gateway.servers]]` entry; partial installs get a best-effort rollback.

## Security gates (extend `addons/audit.rs` + `trust.rs`)

New findings, mirroring the existing wiring audit:

- `bootstrap_unpinned` (**danger**): `[install].version` missing or not an exact
  pin → blocks installable/verified.
- `bootstrap_unknown_manager` (**danger**): `manager` not in the allowlist.
- `bootstrap_shell_meta` (**danger**): shell metacharacters in `package` /
  `verify` / `bin` → reject (defends against `package = "x; rm -rf ~"`).
- `bootstrap_network` capability coherence: a `[install]` block inherently needs
  outbound network + a writable package cache. If the addon declares
  `[capabilities] network = "none"`, flag `cap_net_underdeclared` exactly like
  the runner case in `wiring_uses_network`.
- Consent: the `add` preview prints the **exact** install + uninstall argv it
  will run, the manager, the package and the pin — before doing anything — and
  requires the same yes/no (`--yes` to skip in CI).
- Policy floor: honour `addons.policy` / `block_risky`; add
  `addons.allow_bootstrap` (default keeps current behaviour) so teams can forbid
  any package-manager execution by policy.

## What this unlocks

Once shipped, these flip from *listed* to *installable on add* (each gains an
`[install]` block + keeps its existing `[mcp]` + `integration` slug):

- **Headroom** → `manager = "uv"`, `compression` adapter.
- **Graphify** → `manager = "uv"` (+ a documented `graph.json` build step),
  `code-graph` adapter.
- **Cognee**, **Letta** → `memory` adapter (Letta still needs a running server;
  the receipt installs the client, the user runs the server).

Mem0 and Claude-Context remain key-gated: the engine can install their MCP
package but cannot provision `MEM0_API_KEY` / `OPENAI_API_KEY` + Milvus — those
stay documented prerequisites.

## Rollout

1. `[install]` parsing + validator gates (no execution yet) — safe to ship.
2. Install/uninstall executor behind `addons.allow_bootstrap` (default off).
3. Migrate Headroom → Graphify → Cognee/Letta registry entries, one MR each,
   each gated on a green `bundled_registry_passes_security_validator`.

## Open questions

- Per-manager cache/location detection for accurate "already present" checks.
- Whether to vendor a minimal `uv`/`npm` presence check into `doctor`.
- Windows support for the manager templates (Phase-1 runners already assume a
  POSIX-ish `PATH`).
