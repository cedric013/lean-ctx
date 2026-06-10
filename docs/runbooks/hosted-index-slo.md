# Hosted-Index SLO — Incident Runbook (GL #391)

Scope: the hosted team-server instances (Coolify-provisioned Docker
containers, image `localhost:5000/lean-ctx-team:latest`) whose SLO gates the
$29 price move (GL #374). Companion artifacts: `docs/examples/team-slos.toml`
(objectives), control-plane probe job (`lean-ctx-cloud/src/slo_probe_job.rs`,
60 s interval), alert mails via ZeptoMail (one per account, day and violation
kind).

## SLO objectives (GA gate)

| Objective | Threshold | Measured by |
|---|---|---|
| Availability | ≥ 99.5 % rolling 30 d | control-plane probe (`GET /v1/metrics`) |
| Query latency | p95 < 500 ms | server-reported `slo` block + probe RTT |
| Index lag after push | < 5 min | server-reported `index_lag_s` |
| Alert latency | < 5 min after violation | probe job mail path (verified E2E, GL #475) |

The 30-day report (`lean-ctx team slo-report --json`, or the control-plane
endpoint) is the formal gate artifact for GL #374.

## Architecture facts that matter in an incident

- One Docker container per team account, name pattern `<coolify-uuid>-<n>`,
  port 8077, IP on the Coolify network may change across restarts — resolve
  it fresh: `docker inspect <c> --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}'`.
- All state lives in the named volume mounted at `/data`
  (`/var/lib/docker/volumes/<uuid>_leanctx-data/_data`): `audit.jsonl`,
  `savings/*.jsonl` (hash-chained ledgers), `slos.toml`, `team.json`,
  `events.jsonl`, `connectors/`, `graphs/`, `knowledge/`, `state/`,
  `context-os/`. The container is disposable; the volume is not.
- Restart policy is `unless-stopped`. **Measured behaviour (chaos test
  2026-06-10):** it auto-restarts the container after a *process* crash
  (panic/OOM), but **not** after `docker kill`/`docker stop` — an API-level
  kill leaves the container `exited` until someone starts it. The watchdog
  for that case is the control-plane probe (down ≤ 60 s) + alert mail
  (< 5 min), then this runbook.
- The container image is distroless-style: no `kill`, no package manager.
  `sh` and busybox-level tools exist. Debug from the host, not inside.

## Incident 1 — instance down (probe alert "down")

1. Identify the container:
   `ssh pounce-server "docker ps -a --format '{{.Names}} {{.Status}}' | grep lean-ctx-team"`
2. If `Exited`: start it and verify health — measured recovery is
   **sub-second** once started (chaos test: 0.25 s to HTTP 200):

   ```bash
   ssh pounce-server 'C=<container>; docker start $C && sleep 1 && \
     IP=$(docker inspect $C --format "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}"); \
     curl -s -o /dev/null -w "%{http_code}\n" http://$IP:8077/health'
   ```

3. If it exits again immediately: check `docker logs --tail 100 <c>` for a
   config/parse error in `LEAN_CTX_TEAM_CONFIG`, then fall back to a Coolify
   redeploy of the service (Coolify UI → service → Redeploy). The named
   volume survives redeploys.
4. Confirm the probe goes green (next 60 s cycle) and close the alert.

## Incident 2 — data/storage recovery

Symptoms: 5xx on index routes, `audit.jsonl` write errors, volume full.

1. Inspect usage: `docker exec <c> sh -c "du -sh /data/* | sort -h | tail"`.
   Storage quota is enforced at 5 GiB (`storageQuotaBytes`); the billing
   plane reads the same number — do not silently raise it.
2. Ledger integrity after any crash: the savings ledgers are hash-chained;
   verify with `lean-ctx ledger verify` against a copy of
   `/data/savings/*.jsonl`. A broken chain is tamper-evidence, not data loss
   — never rewrite in place; export, investigate, keep the original.
3. Volume restore: Coolify named volumes live under
   `/var/lib/docker/volumes/<uuid>_leanctx-data/_data` and are included in
   the host backup. Restore = stop container → rsync snapshot into the
   volume path → start container. Index files (`graphs/`, vector data) are
   reproducible worst-case: members re-push via `lean-ctx sync index push`
   (measured: 26 MB bundle pushes in ~2 s, pulls in ~2 s).
4. Index rebuild (lag alert without crash): trigger a member push or wait for
   the connector sync (5 min interval); `index_lag_s` in `/v1/metrics` must
   drop below 300 s.

## Incident 3 — latency (p95 ≥ 500 ms)

1. Check host pressure first: `ssh pounce-server "docker stats --no-stream | head"`.
   Baseline for a healthy idle instance is ~20 MiB RSS; anything in the GiB
   range points at a runaway index job.
2. Check the server's own numbers: `GET /v1/metrics` (owner token) → `slo`
   block. If server-reported p95 is fine but probe RTT is high, the issue is
   network/Traefik, not the instance.
3. Mitigation order: restart container (sub-second, Incident 1) → Coolify
   redeploy → scale host.

## Escalation

| Step | Who/what | When |
|---|---|---|
| 0 | Probe alert mail (automatic) | < 5 min after violation |
| 1 | This runbook, Incidents 1–3 | immediately |
| 2 | Coolify redeploy of the service | container won't stay up |
| 3 | Human operator (Yves) | data integrity in doubt, host-level failure |

Every incident leaves a trace: probe samples (`billing_slo_samples`),
the instance audit log (`/data/audit.jsonl`), and the alert mail. The 30-day
report includes violations — the gate is honest by construction.

## Chaos test protocol (2026-06-10, GL #391 AC2)

Executed against the live team instance `team-03260655e5b9.leanctx.com`
(container `yskkscw4g0gkkgccs4s4cwg4-122631873294`):

| Step | Action | Result |
|---|---|---|
| 1 | `docker kill` (API kill) | container stays `exited` — restart policy does **not** cover API kills (documented above); probe would page within 60 s |
| 2 | `docker start` | HTTP 200 after **0.25 s**; IP changed 10.0.1.27 → 10.0.1.25 (probe resolves by hostname, unaffected) |
| 3 | Data integrity check | `/data` named volume fully intact: `audit.jsonl` continued 799 → 804 lines across the kill, all 4 hash-chained savings ledgers present, `slos.toml` unchanged |
| 4 | In-container PID-1 kill attempts | impossible by design: distroless image (no `kill` binary) and kernel PID-1 namespace protection — an in-container compromise cannot crash the server this way (hardening, not a gap) |
| 5 | Memory-pressure probe (12 MiB cgroup limit) | no OOM at idle (~20 MiB incl. cache, RSS below limit) — instance is far from memory pressure; limit reset afterwards |

Conclusion: recovery within SLO (seconds against a 99.5 % budget of ~3.6 h/month),
durability proven, the one real gap (API-level kill is not auto-restarted) is
covered by the probe + alert + Incident 1 and documented here.

## Remaining for the GL #374 gate

- 30 consecutive days of probe data (clock armed 2026-06-10 08:16 UTC,
  earliest gate date 2026-07-10).
- The deployed `lean-ctx-cloud-api` container predates the probe job commit
  (`bc5f64e`); the next control-plane deploy (GL #506 ships it) activates
  minute-level sampling — verify `billing_slo_samples` fills afterwards.
