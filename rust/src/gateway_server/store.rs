//! `usage_events` Postgres store (enterprise#17, baseline fields enterprise#18).
//!
//! One row per measured LLM turn: who (person/team/project, enterprise#11),
//! what (provider/model/tokens), what it cost (priced with the shared
//! `ModelPricing` table) and the counterfactual-baseline inputs that make the
//! success fee provable (`uncompressed_input_tokens`, `reference_model`,
//! `reference_cost_usd`, `is_local` — Doc 08 §2).
//!
//! Schema management follows the repo rule: `init_schema` is idempotent
//! `batch_execute` DDL (`CREATE TABLE IF NOT EXISTS …`), no migration files.
//!
//! The writer consumes the `proxy::usage_sink` stream: bounded channel, spawned
//! task, INSERT per event. Fail-open (enterprise#12): insert errors are logged
//! and counted, never propagated to the request path.

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;

use crate::core::gain::model_pricing::ModelPricing;
use crate::proxy::usage::RealUsage;

/// Buffered events between the proxy choke-point and the Postgres writer.
/// Sized for bursts (a full channel drops events, counted in `usage_sink`).
pub const WRITER_QUEUE: usize = 4096;

pub fn pool_from_database_url(database_url: &str) -> anyhow::Result<Pool> {
    let pg_cfg: tokio_postgres::Config = database_url.parse()?;
    let mgr = Manager::from_config(
        pg_cfg,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    Ok(Pool::builder(mgr).max_size(8).build()?)
}

/// Idempotent DDL (Doc 08 §2): `IF NOT EXISTS` only, run on every start.
const USAGE_EVENTS_DDL: &str = r"
CREATE TABLE IF NOT EXISTS usage_events (
  id                 BIGSERIAL PRIMARY KEY,
  ts                 TIMESTAMPTZ      NOT NULL DEFAULT now(),
  person             TEXT             NOT NULL,
  team               TEXT,
  project            TEXT             NOT NULL,
  tool               TEXT,
  provider           TEXT             NOT NULL,
  model              TEXT             NOT NULL,
  routed_from        TEXT,
  input_tokens       BIGINT           NOT NULL,
  output_tokens      BIGINT           NOT NULL,
  cache_read_tokens  BIGINT           NOT NULL DEFAULT 0,
  cache_write_tokens BIGINT           NOT NULL DEFAULT 0,
  reasoning_tokens   BIGINT           NOT NULL DEFAULT 0,
  cost_usd           DOUBLE PRECISION NOT NULL,
  saved_tokens       BIGINT           NOT NULL DEFAULT 0,
  saved_usd          DOUBLE PRECISION NOT NULL DEFAULT 0,
  -- Avoided-cost baseline for the success fee (enterprise#18, Doc 04 §6):
  uncompressed_input_tokens BIGINT    NOT NULL DEFAULT 0,
  reference_model    TEXT,
  reference_cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
  is_local           BOOLEAN          NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS idx_usage_events_person_ts  ON usage_events (person, ts);
CREATE INDEX IF NOT EXISTS idx_usage_events_project_ts ON usage_events (project, ts);
CREATE INDEX IF NOT EXISTS idx_usage_events_model_ts   ON usage_events (model, ts);
";

/// Applies the usage-store DDL. Safe to run on every start (idempotent).
pub async fn init_schema(pool: &Pool) -> anyhow::Result<()> {
    let client = pool.get().await?;
    client.batch_execute(USAGE_EVENTS_DDL).await?;
    Ok(())
}

/// One `usage_events` row, fully derived from a finalized [`RealUsage`].
#[derive(Debug, Clone, PartialEq)]
pub struct UsageEvent {
    pub person: String,
    pub team: Option<String>,
    pub project: String,
    pub provider: String,
    pub model: String,
    pub routed_from: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost_usd: f64,
    pub saved_tokens: i64,
    pub saved_usd: f64,
    pub uncompressed_input_tokens: i64,
    pub reference_model: Option<String>,
    pub reference_cost_usd: f64,
    pub is_local: bool,
}

/// Identity fallbacks when a request carried no gateway key/tags: the row must
/// still be attributable (`NOT NULL`), and "anonymous/default" is honest about
/// what the gateway knew. Strict deployments make keys mandatory via
/// `proxy_require_token` + gateway-keys, so these appear only in solo mode.
const ANONYMOUS_PERSON: &str = "anonymous";
const DEFAULT_PROJECT: &str = "default";

impl UsageEvent {
    /// Derives the row from a measured turn, pricing both the actual cost and
    /// the compression saving with the shared pricing table.
    ///
    /// `saved_usd` here is the SEE (compression) component only: saved request
    /// tokens priced at the *served* model's input rate. Routing/baseline
    /// attribution (`reference_cost_usd` vs `cost_usd`) is computed by the
    /// ledger against the frozen `reference_model` (wave 3, enterprise#15).
    #[must_use]
    pub fn from_usage(usage: &RealUsage, pricing: &ModelPricing) -> Self {
        let wire = usage.wire.as_deref();
        let quote = pricing.quote(Some(&usage.model));
        let cost_usd = quote.cost.estimate_usd(
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_write_tokens,
            usage.cache_read_tokens,
        );
        let saved_tokens = wire.map_or(0, |w| w.saved_tokens);
        // Input-side saving: input-rate USD per token × saved request tokens.
        #[allow(clippy::cast_precision_loss)]
        let saved_usd = quote.cost.input_per_m / 1_000_000.0 * saved_tokens as f64;

        Self {
            person: wire
                .and_then(|w| w.person.clone())
                .unwrap_or_else(|| ANONYMOUS_PERSON.to_string()),
            team: wire.and_then(|w| w.team.clone()),
            project: wire
                .and_then(|w| w.project.clone())
                .unwrap_or_else(|| DEFAULT_PROJECT.to_string()),
            provider: wire.map_or_else(String::new, |w| w.provider.clone()),
            model: usage.model.clone(),
            routed_from: wire.and_then(|w| w.routed_from.clone()),
            input_tokens: to_i64(usage.input_tokens),
            output_tokens: to_i64(usage.output_tokens),
            cache_read_tokens: to_i64(usage.cache_read_tokens),
            cache_write_tokens: to_i64(usage.cache_write_tokens),
            reasoning_tokens: to_i64(usage.reasoning_tokens),
            cost_usd,
            saved_tokens: to_i64(saved_tokens),
            saved_usd,
            uncompressed_input_tokens: to_i64(wire.map_or(0, |w| w.uncompressed_input_tokens)),
            reference_model: None, // frozen per deployment; stamped in wave 3 (enterprise#15)
            reference_cost_usd: 0.0,
            is_local: wire.is_some_and(|w| w.is_local),
        }
    }
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Inserts one event. Errors bubble to the writer loop, which logs and moves on.
pub async fn insert_event(
    client: &deadpool_postgres::Client,
    e: &UsageEvent,
) -> anyhow::Result<()> {
    client
        .execute(
            "INSERT INTO usage_events \
             (person, team, project, provider, model, routed_from, \
              input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
              reasoning_tokens, cost_usd, saved_tokens, saved_usd, \
              uncompressed_input_tokens, reference_model, reference_cost_usd, is_local) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
            &[
                &e.person,
                &e.team,
                &e.project,
                &e.provider,
                &e.model,
                &e.routed_from,
                &e.input_tokens,
                &e.output_tokens,
                &e.cache_read_tokens,
                &e.cache_write_tokens,
                &e.reasoning_tokens,
                &e.cost_usd,
                &e.saved_tokens,
                &e.saved_usd,
                &e.uncompressed_input_tokens,
                &e.reference_model,
                &e.reference_cost_usd,
                &e.is_local,
            ],
        )
        .await?;
    Ok(())
}

/// Wires the usage stream into Postgres: installs the process-wide sink
/// (`proxy::usage_sink`) and spawns the writer task. Call once at gateway
/// startup, after `init_schema`.
///
/// Returns `false` when a sink was already installed (double start).
pub fn spawn_writer(pool: Pool) -> bool {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RealUsage>(WRITER_QUEUE);
    if !crate::proxy::usage_sink::install(tx) {
        return false;
    }
    tokio::spawn(async move {
        // One pricing table for the writer's lifetime: rows are priced at
        // insert time (the ledger re-values against frozen references).
        let pricing = ModelPricing::load();
        while let Some(usage) = rx.recv().await {
            let event = UsageEvent::from_usage(&usage, &pricing);
            match pool.get().await {
                Ok(client) => {
                    if let Err(e) = insert_event(&client, &event).await {
                        tracing::warn!("usage_events insert failed (fail-open): {e:#}");
                    }
                }
                Err(e) => {
                    tracing::warn!("usage_events pool unavailable (fail-open): {e:#}");
                }
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::usage::WireContext;

    fn usage_with_wire(wire: Option<Box<WireContext>>) -> RealUsage {
        RealUsage {
            model: "claude-sonnet-4-5".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_tokens: 100,
            reasoning_tokens: 50,
            cohort: None,
            wire,
        }
    }

    #[test]
    fn event_carries_identity_and_baseline_fields() {
        let usage = usage_with_wire(Some(Box::new(WireContext {
            provider: "Anthropic".into(),
            person: Some("yves".into()),
            team: Some("platform".into()),
            project: Some("ai-gateway".into()),
            saved_tokens: 4000,
            uncompressed_input_tokens: 5000,
            is_local: false,
            routed_from: Some("claude-opus-4-5".into()),
        })));
        let event = UsageEvent::from_usage(&usage, &ModelPricing::load());

        assert_eq!(event.person, "yves");
        assert_eq!(event.team.as_deref(), Some("platform"));
        assert_eq!(event.project, "ai-gateway");
        assert_eq!(event.provider, "Anthropic");
        assert_eq!(event.model, "claude-sonnet-4-5");
        assert_eq!(event.routed_from.as_deref(), Some("claude-opus-4-5"));
        assert_eq!(event.input_tokens, 1000);
        assert_eq!(event.saved_tokens, 4000);
        assert_eq!(event.uncompressed_input_tokens, 5000);
        assert!(!event.is_local);
        assert!(event.cost_usd > 0.0, "known model must be priced");
        assert!(
            event.saved_usd > 0.0,
            "saved tokens on a priced model must yield saved USD"
        );
        // Wave 3 stamps these; until then the columns hold their defaults.
        assert_eq!(event.reference_model, None);
        assert_eq!(event.reference_cost_usd, 0.0);
    }

    #[test]
    fn event_without_wire_context_uses_honest_fallbacks() {
        let event = UsageEvent::from_usage(&usage_with_wire(None), &ModelPricing::load());
        assert_eq!(event.person, ANONYMOUS_PERSON);
        assert_eq!(event.project, DEFAULT_PROJECT);
        assert_eq!(event.team, None);
        assert_eq!(event.saved_tokens, 0);
        assert_eq!(event.saved_usd, 0.0);
        assert_eq!(event.uncompressed_input_tokens, 0);
        assert!(!event.is_local);
    }

    #[test]
    fn schema_ddl_is_idempotent_by_construction() {
        // The gateway runs this DDL on every start against a live database, so
        // every CREATE must carry IF NOT EXISTS.
        for stmt in ["CREATE TABLE", "CREATE INDEX"] {
            for (i, _) in USAGE_EVENTS_DDL.match_indices(stmt) {
                let tail = &USAGE_EVENTS_DDL[i..(i + stmt.len() + 14).min(USAGE_EVENTS_DDL.len())];
                assert!(
                    tail.contains("IF NOT EXISTS"),
                    "non-idempotent DDL statement: {tail}"
                );
            }
        }
        // And the baseline fields (enterprise#18) are part of the schema.
        for col in [
            "uncompressed_input_tokens",
            "reference_model",
            "reference_cost_usd",
            "is_local",
        ] {
            assert!(
                USAGE_EVENTS_DDL.contains(col),
                "baseline column {col} missing from schema"
            );
        }
    }
}
