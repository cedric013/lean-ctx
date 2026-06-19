#!/usr/bin/env bash
#
# demo-great-filter.sh — live, end-to-end walkthrough of "The Great Filter"
# (CISO product, Epic #678), using the REAL lean-ctx binary. No mock data:
# every signature is real Ed25519, every enforcement decision runs the same
# guards as the MCP agent path and is recorded in the tamper-evident audit
# trail that the signed compliance report attests.
#
# It does NOT start the MCP server (which manages host processes); enforcement
# is exercised server-free via `lean-ctx policy enforce`, the identical guard
# sequence used by the agent pipeline (role + policy deny, egress DLP, output
# redaction + input filters).
#
# What it proves, in order:
#   1. an admin mints an org signing key and signs a policy pack into a portable,
#      Ed25519-signed artifact;
#   2. the artifact verifies OFFLINE (what an auditor/endpoint checks);
#   3. an endpoint trust-pins the key and installs it as an un-bypassable FLOOR;
#   4. the regulated built-in packs ship real PII filters + egress DLP;
#   5. the active policy actually enforces — a denied tool, a secret-bearing
#      action, and PII in tool output are blocked/redacted and AUDITED;
#   6. a signed compliance report aggregates that enforcement;
#   7. the report verifies OFFLINE (no lean-ctx needed).
#
# Usage:
#   scripts/demo-great-filter.sh            # build (if needed) + run
#   LEANCTX_BIN=/path/to/lean-ctx scripts/demo-great-filter.sh
#   KEEP=1 scripts/demo-great-filter.sh     # keep the temp dir + artifacts

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── Resolve a real lean-ctx binary that supports `policy enforce` ─────────────
binary_has_enforce() { "$1" policy 2>&1 | grep -q "policy enforce"; }

if [[ -n "${LEANCTX_BIN:-}" ]]; then
    BIN="$LEANCTX_BIN"
elif [[ -x "rust/target/release/lean-ctx" ]] && binary_has_enforce "rust/target/release/lean-ctx"; then
    BIN="rust/target/release/lean-ctx"
else
    echo "→ building lean-ctx (release)…"
    (cd rust && cargo build --release --quiet)
    BIN="rust/target/release/lean-ctx"
fi
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
binary_has_enforce "$BIN" || { echo "ERROR: $BIN lacks 'policy enforce' (rebuild)"; exit 1; }
echo "→ using binary: $BIN"

# ── Isolated, throwaway dirs so the demo never touches real config/audit ─────
DEMO_DIR="$(mktemp -d "${TMPDIR:-/tmp}/leanctx-greatfilter-demo.XXXXXX")"
export LEAN_CTX_DATA_DIR="$DEMO_DIR/data"
export LEAN_CTX_CONFIG_DIR="$DEMO_DIR/config"
export LEAN_CTX_STATE_DIR="$DEMO_DIR/state"
export LEAN_CTX_CACHE_DIR="$DEMO_DIR/cache"
export LEAN_CTX_AGENT_ID="ciso-pilot-demo"
PROJ="$DEMO_DIR/proj"
mkdir -p "$LEAN_CTX_DATA_DIR" "$LEAN_CTX_CONFIG_DIR" "$LEAN_CTX_STATE_DIR" "$LEAN_CTX_CACHE_DIR" "$PROJ"

cleanup() { [[ "${KEEP:-0}" == "1" ]] || rm -rf "$DEMO_DIR"; }
trap cleanup EXIT

hr() { printf '\n\033[1;36m── %s ─────────────────────────────────────────\033[0m\n' "$1"; }

# RFC-3339 helper that works on both BSD (macOS) and GNU date.
rfc3339_offset_hours() {
    local h="$1"
    date -u -v"${h}"H +%Y-%m-%dT%H:%M:%SZ 2>/dev/null \
        || date -u -d "${h} hours" +%Y-%m-%dT%H:%M:%SZ
}

# ── The CISO policy pack (the org's signed security floor) ────────────────────
PACK="$DEMO_DIR/acme-ciso.toml"
cat > "$PACK" <<'TOML'
name = "acme-ciso-floor"
version = "1.0.0"
description = "ACME security baseline: no web egress, PII redaction, secret egress DLP"

[context]
# No outbound web tool — the classic exfiltration sink.
deny_tools = ["ctx_url_read"]
max_context_tokens = 16000
audit_retention_days = 365

[redaction]
# Secrets/identifiers scrubbed from tool output (and matched by egress DLP).
api_token = '(?i)sk-[a-z0-9-]{10,}'
iban = '\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b'
employee_id = 'EMP-\d{4,}'

[filters]
# Inbound DLP on tool output before it reaches the model.
pii = "redact"
injection = "block"

[egress]
# Output DLP: the agent must not write/run anything carrying a detected secret.
block_secrets = true
max_writes_per_min = 120
TOML

hr "1. Admin signs the org policy (real Ed25519)"
"$BIN" policy org sign "$PACK" --org acme -o "$DEMO_DIR/artifact.json" | grep -E "Signed|signer"

hr "2. Verify the signed artifact OFFLINE"
"$BIN" policy org verify "$DEMO_DIR/artifact.json"

hr "3. Endpoint pins the key + installs the floor"
"$BIN" policy org install "$DEMO_DIR/artifact.json" --trust | grep -E "Installed|Org policy|trust|mode" | head -6

hr "4. Regulated built-in packs ship real filters + egress DLP"
echo "policy show finance-eu (resolved):"
"$BIN" policy show finance-eu | grep -E "filters|egress|classification|block_secrets|forbidden" | head -6

hr "5. The active policy ENFORCES (each decision is audited)"
echo "secret: contact john.doe@example.com IBAN DE89370400440532013000 ref EMP-4711" > "$PROJ/secret.txt"
echo "5a. denied tool (web egress):"
"$BIN" policy enforce ctx_url_read --project-root "$PROJ"
echo "5b. secret-bearing action (egress DLP, blocked before it runs):"
"$BIN" policy enforce ctx_shell --project-root "$PROJ" \
    --json '{"command":"echo export TOKEN=sk-live-abcdef123456"}'
echo "5c. PII in tool output (redaction + input filter):"
"$BIN" policy enforce ctx_search --project-root "$PROJ" --json '{"pattern":"IBAN","path":"."}'

hr "6. Signed CISO compliance report over the enforcement window"
FROM="$(rfc3339_offset_hours -1)"
TO="$(rfc3339_offset_hours +1)"
REPORT="$DEMO_DIR/compliance-report.json"
REPORT_OUT="$("$BIN" compliance report --from "$FROM" --to "$TO" \
    --framework eu-ai-act --pack "$PACK" --out "$REPORT")"
echo "$REPORT_OUT" | grep -E "written|Period|Blocked|Chain|Signer"

hr "7. Verify the compliance report OFFLINE"
"$BIN" compliance verify "$REPORT"

# ── Hard assertions: the demo fails loudly if enforcement regresses ──────────
blocked="$(printf '%s\n' "$REPORT_OUT" | sed -n 's/.*Blocked:[[:space:]]*\([0-9]\{1,\}\).*/\1/p' | head -1)"
redacted="$(printf '%s\n' "$REPORT_OUT" | sed -n 's/.*Redacted:[[:space:]]*\([0-9]\{1,\}\).*/\1/p' | head -1)"
: "${blocked:=0}" "${redacted:=0}"
if (( blocked < 2 )) || (( redacted < 1 )); then
    echo "FAIL: expected blocked>=2 and redacted>=1, got blocked=$blocked redacted=$redacted"
    exit 1
fi

hr "Done — real, signed, verifiable"
cat <<EOF
A signed org policy was distributed and enforced as an un-bypassable floor.
Enforcement was REAL (blocked=$blocked, redacted=$redacted) and recorded in a
SHA-256 audit chain, then attested by an Ed25519-signed compliance report that
verifies offline — exactly what a CISO hands to an auditor.

Artifacts (${KEEP:+kept }in $DEMO_DIR):
  - signed org policy : $DEMO_DIR/artifact.json
  - compliance report : $REPORT
EOF
[[ "${KEEP:-0}" == "1" ]] && echo "(KEEP=1 — temp dir preserved)" || echo "(temp dir cleaned up; re-run with KEEP=1 to inspect)"
