use super::model_pricing::{ModelCost, ModelQuote};

/// Billable cost caused by recovering context after lossy compression.
///
/// Recovered content is new input at the full input rate. The stable per-turn
/// prefix is a prompt-cache read; output is zero because no output-token count
/// is observable at the tool boundary and lean-ctx never invents billing data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BounceCost {
    pub recovered_input_tokens: u64,
    pub turn_cache_read_tokens: u64,
    pub total_tokens: u64,
    pub usd: f64,
}

pub const BOUNCE_WARNING: &str = "Compression zu aggressiv, consider mode=full";

#[must_use]
pub fn bounce_rate_pct(bounce_events: u64, compressed_reads: u64) -> f64 {
    if compressed_reads == 0 {
        0.0
    } else {
        bounce_events as f64 / compressed_reads as f64 * 100.0
    }
}

#[must_use]
pub fn bounce_warning(rate_pct: f64) -> Option<String> {
    (rate_pct > 5.0).then(|| BOUNCE_WARNING.to_string())
}

impl BounceCost {
    #[must_use]
    pub fn calculate(
        cost: &ModelCost,
        recovered_input_tokens: u64,
        turn_cache_read_tokens: u64,
    ) -> Self {
        let total_tokens = recovered_input_tokens.saturating_add(turn_cache_read_tokens);
        let usd = cost.estimate_usd(recovered_input_tokens, 0, 0, turn_cache_read_tokens);
        Self {
            recovered_input_tokens,
            turn_cache_read_tokens,
            total_tokens,
            usd,
        }
    }

    #[must_use]
    pub fn for_quote(
        quote: &ModelQuote,
        recovered_input_tokens: u64,
        turn_cache_read_tokens: u64,
    ) -> Self {
        Self::calculate(&quote.cost, recovered_input_tokens, turn_cache_read_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opus_cost() -> ModelCost {
        ModelCost {
            input_per_m: 5.00,
            output_per_m: 25.00,
            cache_write_per_m: 6.25,
            cache_read_per_m: 0.50,
        }
    }

    #[test]
    fn recovered_original_is_charged_as_new_input() {
        let cost = BounceCost::calculate(&opus_cost(), 100_000, 0);
        assert!((cost.usd - 0.50).abs() < 1e-12);
    }

    #[test]
    fn turn_overhead_is_charged_at_cache_read_rate() {
        let cost = BounceCost::calculate(&opus_cost(), 0, 100_000);
        assert!((cost.usd - 0.05).abs() < 1e-12);
    }

    #[test]
    fn real_opus_mix_prices_usd_exactly() {
        let cost = BounceCost::calculate(&opus_cost(), 80_000, 20_000);
        assert_eq!(cost.total_tokens, 100_000);
        assert!((cost.usd - 0.41).abs() < 1e-12);
    }

    #[test]
    fn new_input_is_ten_times_costlier_than_cache_read() {
        let input = BounceCost::calculate(&opus_cost(), 1_000_000, 0);
        let cached = BounceCost::calculate(&opus_cost(), 0, 1_000_000);
        assert!((input.usd / cached.usd - 10.0).abs() < 1e-12);
    }

    #[test]
    fn counts_saturate_without_wrapping() {
        let cost = BounceCost::calculate(&opus_cost(), u64::MAX, 1);
        assert_eq!(cost.total_tokens, u64::MAX);
        assert!(cost.usd.is_finite());
    }

    #[test]
    fn zero_token_bounce_has_zero_cost() {
        let cost = BounceCost::calculate(&opus_cost(), 0, 0);
        assert_eq!(cost.total_tokens, 0);
        assert_eq!(cost.usd, 0.0);
    }

    #[test]
    fn warning_starts_strictly_above_five_percent() {
        assert!(bounce_warning(bounce_rate_pct(5, 100)).is_none());
        assert_eq!(
            bounce_warning(bounce_rate_pct(6, 100)).as_deref(),
            Some(BOUNCE_WARNING)
        );
    }

    #[test]
    fn bounce_rate_without_reads_is_deterministic_zero() {
        assert_eq!(bounce_rate_pct(7, 0), 0.0);
        assert!(bounce_warning(bounce_rate_pct(7, 0)).is_none());
    }
}
