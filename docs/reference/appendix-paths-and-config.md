# Appendix — Paths, Env Vars & Config

Where lean-ctx stores everything, every environment variable that changes its
behavior, and every config section. Source: `rust/src/core/data_dir.rs`,
`rust/src/core/config/`.

---

## 1. Data directory

### Resolution order (`lean_ctx_data_dir()`)

| Priority | Path | Condition |
|----------|------|-----------|
| 1 | `$LEAN_CTX_DATA_DIR` | set and non-empty → always wins |
| 2 | `~/.lean-ctx` | exists **and** contains a marker (`stats.json`, `config.toml`, or `sessions/`) |
| 3 | `$XDG_CONFIG_HOME/lean-ctx` (default `~/.config/lean-ctx`) | exists **and** contains a marker |
| 4 | fallback | empty `~/.lean-ctx` if present, else new `~/.config/lean-ctx` |

An empty `~/.lean-ctx/` without markers does **not** trigger legacy mode (this
prevents the stats-split problem). Unix dir permissions: `0700`.
`migrate_if_split()` merges stats if two `stats.json` sources are found.

> **Do not hardcode `LEAN_CTX_DATA_DIR`** in editor MCP configs unless you
> intentionally relocate the dir — a wrong value splits your stats. lean-ctx
> auto-detects correctly.

### A second, separate directory (daemon IPC)

`~/.local/share/lean-ctx/` (via `dirs::data_local_dir()`) holds only runtime IPC:
`daemon.pid`, `daemon.sock`, `daemon-*.log`. This is **not** the data dir and is
intentionally separate.

### What lives in the data dir

| Area | Path | Contents |
|------|------|----------|
| Config | `config.toml` | Global config (see §3) |
| Stats | `stats.json`, `mcp-live.json`, `mode_stats.json`, `heatmap.json` | Token/tool stats, live MCP metrics |
| Sessions | `sessions/<id>.json`, `sessions/latest.json` | CCP session snapshots |
| Knowledge | `knowledge/<project-hash>/{knowledge,gotchas,embeddings}.json` | Per-project facts |
| Search | `vectors/<project-hash>/bm25_index.bin.zst`, `embeddings.json` | BM25 + dense vectors |
| Graph | `graphs/<project-hash>/index.json.zst` | Property graph |
| Archive | `archives/<id>/…`, `archives/index.db` | Zero-loss tool-output archive (FTS) |
| Memory | `memory/{episodes,procedures,archive}/` | Episodic + procedural memory |
| Reports | `report/`, `setup/`, `doctor/`, `status/latest.json` | Last command reports |
| Packages | `packages/`, `package-index.json` | Context packages |
| Agents | `agents/`, `handoffs/`, `keys/` | Multi-agent state + identity keys |
| Misc | `filters/`, `tee/`, `audit/trail.jsonl`, `models/`, `cloud/` | Filters, tee logs, audit, embedding models, cloud creds |
| Auth | `session_token` (0600) | Proxy/HTTP auth token |

Project-local lean-ctx data (in the repo, not the data dir): `.lean-ctx.toml`
(project config override), `.lean-ctx-id`, `.lean-ctx/`.

---

## 2. Environment variables

There are ~120 env vars; the ones you'll actually touch are below. The full list
is in `rust/src/core/config/`. Most have a matching `config.toml` key — the env
var always wins.

### The ones you'll use

| Variable | Purpose | Default |
|----------|---------|---------|
| `LEAN_CTX_DISABLED=1` | Bypass ALL compression + disable shell hook | unset |
| `LEAN_CTX_RAW=1` | Uncompressed output for one command | unset |
| `LEAN_CTX_DATA_DIR` | Explicit data dir | auto-detected |
| `LEAN_CTX_PROJECT_ROOT` | Explicit project root | auto-detected |
| `LEAN_CTX_TOOL_PROFILE` | `minimal\|standard\|power` | config / power |
| `LEAN_CTX_PROFILE` | Active context profile | config / `coder` |
| `LEAN_CTX_COMPRESSION` | `off\|lite\|standard\|max` | config / `lite` |
| `LEAN_CTX_MEMORY_PROFILE` | `low\|balanced\|performance` | `performance` |
| `LEAN_CTX_PROXY_PORT` | Proxy port | `4444` |
| `LEAN_CTX_NO_UPDATE_CHECK=1` | Disable update check | unset |
| `LEAN_CTX_ALLOW_PATH` | Extra PathJail roots (path list; see §5) | unset |
| `LEAN_CTX_EXTRA_ROOTS` | Multi-root workspace roots (path list; see §5) | unset |

### Provider tokens (for `ctx_provider`)

`GITHUB_TOKEN` / `GH_TOKEN`, `GITLAB_TOKEN` / `CI_JOB_TOKEN`, `JIRA_URL` +
`JIRA_EMAIL` + `JIRA_TOKEN`, `DATABASE_URL`. Optional LLM enhance:
`OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`.

### Internal (set by lean-ctx itself — don't set these)

`LEAN_CTX_MCP_SERVER`, `LEAN_CTX_ACTIVE`, `LEAN_CTX_HOOK_CHILD`,
`LEAN_CTX_HEADLESS`, `LEAN_CTX_PLUGIN_DIR`, etc.

---

## 3. Config file (`config.toml`)

Global at `<DATA_DIR>/config.toml`; per-project override at `<repo>/.lean-ctx.toml`
(merged, project wins). Manage with `lean-ctx config` (`set`, `schema`,
`validate`, `show`).

### Sections

| Section | What it controls |
|---------|------------------|
| (root keys) | compression, cache, shell hook, profiles, memory caps, savings footer, proxy tri-state |
| `[tools]` | `profile` (minimal/standard/power), explicit `enabled` list |
| `[setup]` | `auto_inject_rules`, `auto_inject_skills`, `auto_update_mcp` |
| `[archive]` | Zero-loss tool-output archive: `enabled`, `threshold_chars` (800), `max_age_hours` (48), `max_disk_mb` (500) |
| `[search]` | BM25/dense/splade weights + candidate counts |
| `[autonomy]` | Auto preload/dedup/consolidate, cognition loop |
| `[providers]` | GitHub/GitLab/Jira/Postgres + MCP bridges |
| `[loop_detection]` | Per-tool call limits to prevent agent loops |
| `[updates]` | `auto_update`, `check_interval_hours` (6), `notify_only` |
| `[boundary_policy]` | Cross-project search/import + universal gotchas |
| `[secret_detection]` | Secret redaction in output |
| `[cloud]` | `contribute_enabled` + sync timestamps |
| `[proxy]` | Upstream URLs for Anthropic/OpenAI/Gemini |
| `[memory.*]` | Knowledge/episodic/procedural/lifecycle/gotcha/embeddings caps |
| `[llm]` | Optional local LLM enhance (Ollama) |

Key defaults worth knowing:
- `compression_level = "lite"` (root) — light compression on by default.
- `savings_footer = "always"` config default, but the **`SavingsFooter` enum
  default is `Never`** so no inline footer tokens are emitted unless enabled.
- `memory_profile = "performance"`, `memory_cleanup = "aggressive"`.
- `[memory.knowledge] max_facts = 200` — the source of doctor's "facts at
  capacity" warning.

---

## 4. Files written outside the data dir

| Category | Examples | Written by |
|----------|----------|-----------|
| Shell hook | `~/.zshenv`, `~/.bashenv`, fish, PowerShell profile | `setup` step 1 / `init --global` |
| Agent aliases | `~/.zshrc`, `~/.bashrc` (lean-ctx-on/off/mode/status) | `setup` / `init --global` |
| MCP configs | `~/.cursor/mcp.json`, `~/.claude.json`, ~30 editors | `setup` step 3 / `init --agent` |
| Agent rules (opt-in) | `~/.cursor/rules/lean-ctx.mdc`, `AGENTS.md` blocks | `setup` step 4 |
| Skills (opt-in) | `~/.claude/skills/lean-ctx/`, … | `setup` step 6 |
| Proxy env (opt-in) | RC exports, `~/.claude/settings.json`, Codex `config.toml` | `proxy enable` |
| Autostart | `~/Library/LaunchAgents/com.leanctx.{proxy,daemon,autoupdate}.plist`; systemd user units on Linux | setup steps 5/9 |
| Binary | `~/.local/bin/lean-ctx` | installer / `dev-install` |

Every edit to an existing file goes through `config_io::write_atomic`, which
writes a `*.lean-ctx.bak` backup first. Rules injection only rewrites content
between `<!-- lean-ctx -->` markers — your own content is preserved.
`lean-ctx uninstall` reverses all of the above.

---

## 5. Filesystem boundary — `path_jail`, `allow_paths`, `extra_roots` (GH #392)

All tool file access (`ctx_read`, `ctx_edit`, `ctx_tree`, …) is jailed under the
current `project_root` (**PathJail**). Three knobs widen or remove that boundary —
they overlap, so here is exactly what each one does:

| Knob | Effect | Use when |
|------|--------|----------|
| `allow_paths = ["…"]` (root key) | **Adds** directories to PathJail's whitelist. Tools may read/edit under them, but `ctx_tree`/`ctx_search` do **not** scan them. | One extra directory needs to be readable/editable (e.g. a shared skills folder). |
| `extra_roots = ["…"]` (root key) | Same whitelist effect as `allow_paths` **plus** multi-root scanning: `ctx_tree`, `ctx_search`, overview treat them as additional project roots. | Multi-repo workspaces. |
| `path_jail = false` (root key) | **Disables PathJail entirely** — every absolute path is allowed. | Sandboxed environments (bwrap, containers, VMs) where the OS is the boundary. |

Env equivalents (path-list syntax, `:` on Unix / `;` on Windows):
`LEAN_CTX_ALLOW_PATH` (= `allow_paths`), `LEAN_CTX_EXTRA_ROOTS` (= `extra_roots`).

Notes that save debugging time:

- **`~`, `$VAR` and `${VAR}` are expanded** in `allow_paths` / `extra_roots` /
  the env vars (since v3.8.1). On older versions `"$HOME/code"` was matched
  literally and silently never applied.
- `allow_paths = ["/"]` technically whitelists everything; prefer the explicit
  `path_jail = false` — `lean-ctx doctor` flags the `"/"` pattern.
- Config changes are picked up on the next tool call (mtime-based reload); no
  MCP server restart needed. If a change appears to do nothing, run
  `lean-ctx doctor`: it reports config parse errors (a broken `config.toml`
  silently falls back to defaults) and dead `allow_paths` entries (unset
  `$VAR`, missing directory), plus the effective jail state.
- **Compile-time off-switch:** building with the `no-jail` cargo feature
  removes the jail entirely (for trusted single-user builds).
- **Removed:** the `LEAN_CTX_NO_JAIL=1` env var (≤ 3.7.3). It was replaced by
  the `path_jail = false` config key and the `no-jail` compile feature; setting
  the old env var has no effect on current versions.
- Home-level IDE config dirs (`~/.cursor`, `~/.claude`, …) are excluded from
  the jail's whitelist by default; opt in with `allow_ide_config_dirs = true`
  (they expose other projects' sessions and credentials).
