//! Privacy-safe health and efficiency views for project knowledge stores.

/// Privacy-safe health assessment of project knowledge stores.
/// Contains only counts and ratios — never content or file paths.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeHealthReport {
    /// Total number of assessed facts.
    pub total_facts: usize,
    /// Number of facts marked fresh.
    pub fresh_facts: usize,
    /// Number of facts not marked fresh.
    pub stale_facts: usize,
    /// Number of facts marked contradicted.
    pub contradicted_facts: usize,
    /// Fraction of facts marked fresh.
    pub freshness_score: f64,
    /// Fraction of facts marked contradicted.
    pub contradiction_rate: f64,
    /// Fraction of facts not marked fresh.
    pub stale_ratio: f64,
    /// Number of knowledge areas with no coverage.
    pub coverage_gaps: usize,
    /// Total number of episodes in the project store.
    pub total_episodes: usize,
    /// Total number of procedures in the project store.
    pub total_procedures: usize,
}

/// Aggregated efficiency metrics for org-wide dashboards.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EfficiencyView {
    /// Fraction of original tokens avoided before sending context.
    pub compression_ratio: f64,
    /// Fraction of reads served from cache.
    pub cache_hit_rate: f64,
    /// Fraction of plans accepted.
    pub plan_accept_rate: f64,
    /// Average kernel planning latency in milliseconds.
    pub kernel_overhead_ms: f64,
    /// Total tokens avoided before sending context.
    pub total_tokens_saved: u64,
    /// Total tokens sent after context compression.
    pub total_tokens_sent: u64,
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_u64(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// Assesses aggregate freshness, contradiction, coverage, and store counts.
#[must_use]
pub fn assess_health(
    facts: &[(bool, bool)],
    episodes: usize,
    procedures: usize,
    coverage_areas: usize,
    covered_areas: usize,
) -> KnowledgeHealthReport {
    let total_facts = facts.len();
    let fresh_facts = facts.iter().filter(|(fresh, _)| *fresh).count();
    let stale_facts = total_facts - fresh_facts;
    let contradicted_facts = facts
        .iter()
        .filter(|(_, contradicted)| *contradicted)
        .count();

    KnowledgeHealthReport {
        total_facts,
        fresh_facts,
        stale_facts,
        contradicted_facts,
        freshness_score: ratio(fresh_facts, total_facts),
        contradiction_rate: ratio(contradicted_facts, total_facts),
        stale_ratio: ratio(stale_facts, total_facts),
        coverage_gaps: coverage_areas.saturating_sub(covered_areas),
        total_episodes: episodes,
        total_procedures: procedures,
    }
}

/// Builds aggregate compression, cache, acceptance, and latency metrics.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn build_efficiency_view(
    original_tokens: u64,
    sent_tokens: u64,
    cache_hits: u64,
    total_reads: u64,
    accepted_plans: u64,
    total_plans: u64,
    kernel_overhead_ms: f64,
) -> EfficiencyView {
    EfficiencyView {
        compression_ratio: if original_tokens == 0 {
            0.0
        } else {
            1.0 - ratio_u64(sent_tokens, original_tokens)
        },
        cache_hit_rate: ratio_u64(cache_hits, total_reads),
        plan_accept_rate: ratio_u64(accepted_plans, total_plans),
        kernel_overhead_ms,
        total_tokens_saved: original_tokens.saturating_sub(sent_tokens),
        total_tokens_sent: sent_tokens,
    }
}

/// Formats a multi-line summary containing only aggregate counts and metrics.
#[must_use]
pub fn format_org_summary(health: &KnowledgeHealthReport, efficiency: &EfficiencyView) -> String {
    format!(
        "Knowledge health\n\
         Facts: {} total, {} fresh, {} stale, {} contradicted\n\
         Freshness: {:.1}%\n\
         Contradictions: {:.1}%\n\
         Stale: {:.1}%\n\
         Coverage gaps: {}\n\
         Episodes: {}\n\
         Procedures: {}\n\
         Efficiency\n\
         Compression: {:.1}%\n\
         Cache hits: {:.1}%\n\
         Plan acceptance: {:.1}%\n\
         Kernel overhead: {:.1} ms\n\
         Tokens saved: {}\n\
         Tokens sent: {}",
        health.total_facts,
        health.fresh_facts,
        health.stale_facts,
        health.contradicted_facts,
        health.freshness_score * 100.0,
        health.contradiction_rate * 100.0,
        health.stale_ratio * 100.0,
        health.coverage_gaps,
        health.total_episodes,
        health.total_procedures,
        efficiency.compression_ratio * 100.0,
        efficiency.cache_hit_rate * 100.0,
        efficiency.plan_accept_rate * 100.0,
        efficiency.kernel_overhead_ms,
        efficiency.total_tokens_saved,
        efficiency.total_tokens_sent,
    )
}

#[cfg(test)]
mod tests {
    use super::{assess_health, build_efficiency_view, format_org_summary};

    #[test]
    fn empty_facts_returns_zero_scores() {
        let report = assess_health(&[], 0, 0, 3, 0);
        assert_eq!(report.freshness_score, 0.0);
        assert_eq!(report.contradiction_rate, 0.0);
        assert_eq!(report.stale_ratio, 0.0);
        assert_eq!(report.coverage_gaps, 3);
    }

    #[test]
    fn all_fresh_facts_score_one() {
        let report = assess_health(&[(true, false); 10], 0, 0, 0, 0);
        assert_eq!(report.freshness_score, 1.0);
        assert_eq!(report.stale_facts, 0);
    }

    #[test]
    fn stale_facts_reduce_score() {
        let mut facts = [(true, false); 10];
        facts[5..].fill((false, false));
        let report = assess_health(&facts, 0, 0, 0, 0);
        assert_eq!(report.freshness_score, 0.5);
        assert_eq!(report.stale_ratio, 0.5);
    }

    #[test]
    fn contradiction_rate_correct() {
        let mut facts = [(true, false); 10];
        facts[..2].fill((true, true));
        let report = assess_health(&facts, 0, 0, 0, 0);
        assert_eq!(report.contradiction_rate, 0.2);
        assert_eq!(report.contradicted_facts, 2);
    }

    #[test]
    fn efficiency_view_compression() {
        let view = build_efficiency_view(1_000, 300, 7, 10, 4, 5, 2.5);
        assert!((view.compression_ratio - 0.7).abs() < f64::EPSILON);
        assert_eq!(view.total_tokens_saved, 700);
        assert_eq!(view.total_tokens_sent, 300);
    }

    #[test]
    fn format_summary_privacy_safe() {
        let health = assess_health(&[(true, false)], 2, 3, 4, 1);
        let efficiency = build_efficiency_view(100, 25, 3, 4, 4, 5, 1.5);
        let summary = format_org_summary(&health, &efficiency);

        assert!(summary.lines().count() > 1);
        assert!(!summary.contains("alice@example.com"));
        assert!(!summary.contains('/'));
        assert!(summary.contains("75.0%"));
    }
}
