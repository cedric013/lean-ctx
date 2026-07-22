//! Cache-aware compression decisions using provider billing economics (#1195).
//!
//! Compression of an already-sent prompt prefix can turn cheap cache reads into
//! cache writes.  This module compares that penalty with the actual token
//! reduction and chooses the candidate with the highest expected USD benefit.

use crate::core::gain::model_pricing::{ModelPricing, PricingMatchKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionDepth {
    Conservative,
    Balanced,
    Aggressive,
}

#[derive(Debug, Clone, Copy)]
pub struct CompressionCandidate {
    pub depth: CompressionDepth,
    pub compressed_tokens: u64,
    /// Probability that the already-sent prefix remains byte-stable, in 0..=1.
    pub prefix_stability: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct CompressionContext<'a> {
    pub original_tokens: u64,
    /// Tokens in the cacheable prefix before compression.
    pub prefix_tokens: u64,
    pub current_turn: u64,
    pub predicted_total_turns: u64,
    /// Measured probability of a compressed read causing an expensive retry.
    pub bounce_rate: f64,
    pub model: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct CandidateEconomics {
    pub depth: CompressionDepth,
    pub gross_benefit_usd: f64,
    pub cache_break_risk_usd: f64,
    pub net_benefit_usd: f64,
    pub cache_break_probability: f64,
}

#[derive(Debug, Clone)]
pub struct CompressionDecision {
    pub selected: Option<CandidateEconomics>,
    pub pricing_match: PricingMatchKind,
}

impl CompressionDecision {
    #[must_use]
    pub fn should_compress(&self) -> bool {
        self.selected.is_some()
    }
}

/// Compare candidates using all provider input token classes affected here.
///
/// `gross_benefit` prices removed tokens once as `cache_write`, then as
/// `cache_read` on every predicted reuse. `cache_break_risk` prices the
/// remaining compressed prefix at the write-minus-read premium on each reuse,
/// weighted by the measured probability that the prefix changes.
#[must_use]
pub fn decide(
    context: CompressionContext<'_>,
    candidates: &[CompressionCandidate],
) -> CompressionDecision {
    let quote = ModelPricing::load().quote(context.model);
    let remaining_turns = context
        .predicted_total_turns
        .saturating_sub(context.current_turn);
    let write_rate = quote.cost.cache_write_per_m;
    let read_rate = quote.cost.cache_read_per_m;
    let write_premium = (write_rate - read_rate).max(0.0);

    let selected = candidates
        .iter()
        .filter(|candidate| candidate.compressed_tokens < context.original_tokens)
        .map(|candidate| {
            let saved = context
                .original_tokens
                .saturating_sub(candidate.compressed_tokens);
            let stability = finite_probability(candidate.prefix_stability);
            let bounce_rate = finite_probability(context.bounce_rate);
            let break_probability = 1.0 - stability * (1.0 - bounce_rate);
            let gross_rate = write_rate + remaining_turns as f64 * read_rate;
            let gross_benefit_usd = usd(saved, gross_rate);
            let compressed_prefix = context.prefix_tokens.saturating_sub(saved);
            let cache_break_risk_usd = usd(
                compressed_prefix,
                remaining_turns as f64 * break_probability * write_premium,
            );
            CandidateEconomics {
                depth: candidate.depth,
                gross_benefit_usd,
                cache_break_risk_usd,
                net_benefit_usd: gross_benefit_usd - cache_break_risk_usd,
                cache_break_probability: break_probability,
            }
        })
        .filter(|economics| economics.net_benefit_usd > 0.0)
        .max_by(|a, b| {
            a.net_benefit_usd
                .total_cmp(&b.net_benefit_usd)
                .then_with(|| depth_rank(a.depth).cmp(&depth_rank(b.depth)))
        });

    CompressionDecision {
        selected,
        pricing_match: quote.match_kind,
    }
}

/// Ratio of the previous prefix preserved at the start of the current prefix.
/// Appending content is perfectly stable; a mutation near the front scores low.
#[must_use]
pub fn prefix_stability_score(previous: &[u8], current: &[u8]) -> f64 {
    if previous.is_empty() {
        return 1.0;
    }
    let common = previous
        .iter()
        .zip(current)
        .take_while(|(a, b)| a == b)
        .count();
    common as f64 / previous.len() as f64
}

/// Deterministic session-length estimate used when no explicit forecast exists.
/// Short sessions retain a small horizon; established long sessions reserve a
/// further 50% of their current length for cache-reuse economics.
#[must_use]
pub fn predict_session_length(current_turn: u64) -> u64 {
    match current_turn {
        0..=3 => 4,
        4..=11 => 12,
        _ => current_turn.saturating_add(current_turn / 2),
    }
}

fn finite_probability(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn usd(tokens: u64, rate_per_m: f64) -> f64 {
    tokens as f64 / 1_000_000.0 * rate_per_m
}

fn depth_rank(depth: CompressionDepth) -> u8 {
    match depth {
        CompressionDepth::Conservative => 0,
        CompressionDepth::Balanced => 1,
        CompressionDepth::Aggressive => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(turn: u64, predicted: u64) -> CompressionContext<'static> {
        CompressionContext {
            original_tokens: 1_000_000,
            prefix_tokens: 1_000_000,
            current_turn: turn,
            predicted_total_turns: predicted,
            bounce_rate: 0.0,
            model: Some("claude-opus-4.5"),
        }
    }

    #[test]
    fn stable_prefix_prices_write_and_future_reads_exactly() {
        let decision = decide(
            context(1, 2),
            &[CompressionCandidate {
                depth: CompressionDepth::Balanced,
                compressed_tokens: 500_000,
                prefix_stability: 1.0,
            }],
        );
        let economics = decision.selected.unwrap();
        assert!((economics.gross_benefit_usd - 3.375).abs() < 1e-12);
        assert_eq!(economics.cache_break_risk_usd, 0.0);
        assert!((economics.net_benefit_usd - 3.375).abs() < 1e-12);
    }

    #[test]
    fn unstable_long_prefix_costs_real_write_minus_read_premium() {
        let decision = decide(
            context(1, 13),
            &[CompressionCandidate {
                depth: CompressionDepth::Balanced,
                compressed_tokens: 500_000,
                prefix_stability: 0.0,
            }],
        );
        assert!(!decision.should_compress());
        let risk = 0.5 * 12.0 * (6.25 - 0.50);
        assert!((risk - 34.5_f64).abs() < 1e-12);
    }

    #[test]
    fn short_session_selects_largest_real_saving() {
        let candidates = [
            CompressionCandidate {
                depth: CompressionDepth::Conservative,
                compressed_tokens: 800_000,
                prefix_stability: 0.98,
            },
            CompressionCandidate {
                depth: CompressionDepth::Aggressive,
                compressed_tokens: 300_000,
                prefix_stability: 0.95,
            },
        ];
        let selected = decide(context(3, 4), &candidates).selected.unwrap();
        assert_eq!(selected.depth, CompressionDepth::Aggressive);
    }

    #[test]
    fn long_session_can_prefer_conservative_stable_candidate() {
        let candidates = [
            CompressionCandidate {
                depth: CompressionDepth::Conservative,
                compressed_tokens: 800_000,
                prefix_stability: 0.999,
            },
            CompressionCandidate {
                depth: CompressionDepth::Aggressive,
                compressed_tokens: 300_000,
                prefix_stability: 0.75,
            },
        ];
        let selected = decide(context(20, 40), &candidates).selected.unwrap();
        assert_eq!(selected.depth, CompressionDepth::Conservative);
    }

    #[test]
    fn non_saving_candidate_never_compresses() {
        let candidate = CompressionCandidate {
            depth: CompressionDepth::Aggressive,
            compressed_tokens: 1_000_000,
            prefix_stability: 1.0,
        };
        assert!(!decide(context(1, 1), &[candidate]).should_compress());
    }

    #[test]
    fn appended_prefix_is_fully_stable() {
        assert_eq!(prefix_stability_score(b"stable", b"stable + new"), 1.0);
    }

    #[test]
    fn front_mutation_has_zero_stability() {
        assert_eq!(prefix_stability_score(b"abc", b"xbc"), 0.0);
    }

    #[test]
    fn prediction_distinguishes_short_and_long_sessions() {
        assert_eq!(predict_session_length(2), 4);
        assert_eq!(predict_session_length(8), 12);
        assert_eq!(predict_session_length(40), 60);
    }

    #[test]
    fn nan_stability_fails_closed_as_cache_break() {
        let candidate = CompressionCandidate {
            depth: CompressionDepth::Balanced,
            compressed_tokens: 500_000,
            prefix_stability: f64::NAN,
        };
        assert!(!decide(context(1, 13), &[candidate]).should_compress());
    }

    #[test]
    fn measured_bounces_raise_expected_cache_risk() {
        let candidate = CompressionCandidate {
            depth: CompressionDepth::Balanced,
            compressed_tokens: 500_000,
            prefix_stability: 1.0,
        };
        let clean = decide(context(1, 13), &[candidate]).selected.unwrap();
        let mut retrying = context(1, 13);
        retrying.bounce_rate = 0.5;
        assert!(decide(retrying, &[candidate]).selected.is_none());
        assert_eq!(clean.cache_break_probability, 0.0);
    }
}
