use anyhow::{Result, anyhow};
use clap::ArgMatches;

use crate::core::savings_ledger::{event::SavingsEvent, store};

const MECHANISMS: [&str; 3] = ["compression", "routing", "caching"];
const P5_FIELDS: [&str; 19] = [
    "intent_tag",
    "outcome",
    "model_original",
    "model_routed",
    "routing_savings",
    "response_original_tokens",
    "response_delivered_tokens",
    "agent_chain_id",
    "chain_depth",
    "measurement_method",
    "evidence_class",
    "confidence",
    "quality_signal",
    "attribution_group",
    "attribution_id",
    "baseline_ref",
    "price_version",
    "customer_approval",
    "settlement_status",
];

/// Handle `lean-ctx ocla ledger ...` inspection commands.
pub fn handle_ledger(matches: &ArgMatches) -> Result<()> {
    let path = store::default_path().ok_or_else(|| anyhow!("savings ledger path unavailable"))?;
    let action = matches.subcommand_name().unwrap_or("summary");

    match action {
        "summary" => {
            let summary = store::summarize(&path);
            let events = store::load(&path);
            println!("Ledger events: {}", summary.total_events);
            for (mechanism, count, tokens, usd) in mechanism_breakdown(&events, &summary) {
                println!("{mechanism}: {count} events, {tokens} tokens, ${usd:.6}");
            }
        }
        "verify" => {
            let result = store::verify(&path);
            if result.valid {
                println!("Ledger valid: {} events", result.total);
            } else {
                println!(
                    "Ledger invalid at event {} ({} events read)",
                    result.first_invalid_at.unwrap_or(result.total),
                    result.total
                );
            }
        }
        "query" => {
            let mechanism = matches
                .get_one::<String>("mechanism")
                .ok_or_else(|| anyhow!("query requires --mechanism <M>"))?;
            let limit = matches.get_one::<usize>("limit").copied().unwrap_or(10);
            for event in events_for_mechanism(&store::load(&path), mechanism, limit) {
                println!(
                    "{} {} {} tokens={} usd=${:.6} hash={}",
                    event.ts,
                    event.mechanism,
                    event.tool,
                    event.saved_tokens,
                    event.saved_usd,
                    event.entry_hash
                );
            }
        }
        "p5-coverage" => {
            let events = store::load(&path);
            let events_with_p5 = events
                .iter()
                .filter(|event| p5_presence(event).iter().any(|populated| *populated))
                .count();
            println!("P5 coverage: {} events", events.len());
            println!("Events with any P5 field: {events_with_p5}/{}", events.len());
            for (field, populated) in P5_FIELDS.iter().zip(p5_counts(&events)) {
                println!("{field}: {populated}/{}", events.len());
            }
        }
        other => return Err(anyhow!("unknown ledger subcommand: {other}")),
    }
    Ok(())
}

fn mechanism_breakdown(
    events: &[SavingsEvent],
    summary: &store::LedgerSummary,
) -> Vec<(&'static str, usize, u64, f64)> {
    MECHANISMS
        .iter()
        .map(|mechanism| {
            let count = events.iter().filter(|event| event.mechanism == *mechanism).count();
            let (tokens, usd) = summary
                .by_mechanism
                .iter()
                .find(|row| row.0 == *mechanism)
                .map_or((0, 0.0), |row| (row.1, row.2));
            (*mechanism, count, tokens, usd)
        })
        .collect()
}

fn events_for_mechanism<'a>(
    events: &'a [SavingsEvent],
    mechanism: &str,
    limit: usize,
) -> impl Iterator<Item = &'a SavingsEvent> {
    events
        .iter()
        .filter(move |event| event.mechanism == mechanism)
        .rev()
        .take(limit)
}

fn p5_counts(events: &[SavingsEvent]) -> [usize; 19] {
    let mut counts = [0; 19];
    for event in events {
        let populated = p5_presence(event);
        for (count, is_populated) in counts.iter_mut().zip(populated) {
            *count += usize::from(is_populated);
        }
    }
    counts
}

fn p5_presence(event: &SavingsEvent) -> [bool; 19] {
    [
        event.intent_tag.is_some(),
        event.outcome.is_some(),
        event.model_original.is_some(),
        event.model_routed.is_some(),
        event.routing_savings.is_some(),
        event.response_original_tokens.is_some(),
        event.response_delivered_tokens.is_some(),
        event.agent_chain_id.is_some(),
        event.chain_depth.is_some(),
        event.measurement_method.is_some(),
        event.evidence_class.is_some(),
        event.confidence.is_some(),
        event.quality_signal.is_some(),
        event.attribution_group.is_some(),
        event.attribution_id.is_some(),
        event.baseline_ref.is_some(),
        event.price_version.is_some(),
        event.customer_approval.is_some(),
        event.settlement_status.is_some(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::savings_ledger::event::MECHANISM_COMPRESSION;

    fn event(mechanism: &str, saved_tokens: u64) -> SavingsEvent {
        serde_json::from_value(serde_json::json!({
            "ts": "2026-07-20T00:00:00Z",
            "tool": "ctx_read",
            "mechanism": mechanism,
            "model_id": "test",
            "tokenizer": "o200k_base",
            "baseline_tokens": saved_tokens + 10,
            "actual_tokens": 10,
            "saved_tokens": saved_tokens,
            "bounce_adjustment": 0,
            "unit_price_per_m_usd": 1.0,
            "saved_usd": 0.001,
            "repo_hash": "repo",
            "agent_id": "agent",
            "prev_hash": "genesis",
            "entry_hash": "hash",
            "version": "5"
        }))
        .expect("valid test event")
    }

    #[test]
    fn p5_counts_only_populated_fields() {
        let mut populated = event(MECHANISM_COMPRESSION, 10);
        populated.intent_tag = Some("coding".into());
        populated.confidence = Some(0.9);
        let counts = p5_counts(&[populated, event("routing", 0)]);
        assert_eq!(counts[0], 1);
        assert_eq!(counts[11], 1);
        assert!(counts.iter().skip(1).take(10).all(|count| *count == 0));
    }

    #[test]
    fn query_returns_newest_matching_events_and_honors_limit() {
        let events = vec![
            event(MECHANISM_COMPRESSION, 1),
            event("routing", 2),
            event(MECHANISM_COMPRESSION, 3),
        ];
        let result: Vec<u64> = events_for_mechanism(&events, MECHANISM_COMPRESSION, 1)
            .map(|event| event.saved_tokens)
            .collect();
        assert_eq!(result, vec![3]);
    }
}
