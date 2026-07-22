//! Request-scoped wiring for client profiles, identity, and live ETPAO metrics.

use super::client_profile::{self, ClientEfficiencyProfile};
use super::context_broker::{BrokerBudget, ContextBroker};
use super::coverage_class::{self, CoverageClass};
use super::etpao_live::{EtpaoLive, OutcomeMetrics, RequestMetrics};
use super::identity::CallerIdentity;
use super::identity_resolver;

/// Client and caller metadata resolved for one request.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Identity used for attribution and cost-center accounting.
    pub identity: CallerIdentity,
    /// Client capabilities detected from transport headers.
    pub profile: ClientEfficiencyProfile,
    /// Effective integration coverage for this request path.
    pub coverage: CoverageClass,
    /// Token allocations computed for the detected client.
    pub broker_budget: BrokerBudget,
}

/// Amount of context optimization available for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationLevel {
    /// Compress, route, cache, and measure inline traffic.
    Full,
    /// Compress and cache context delivered through MCP.
    #[default]
    Partial,
    /// Collect metrics without modifying request context.
    ObserveOnly,
    /// Leave unmanaged traffic unchanged.
    None,
}

/// Resolves request identity, coverage, profile, and broker token allocations.
#[must_use]
pub fn build_request_context(
    headers: &[(String, String)],
    has_proxy: bool,
    has_mcp: bool,
    has_hooks: bool,
) -> RequestContext {
    let identity = identity_resolver::resolve_with_defaults(headers);
    let coverage = coverage_class::detect_coverage(has_proxy, has_mcp, has_hooks);
    let mut profile = client_profile::detect_from_headers(headers);
    profile.coverage = coverage;
    let broker_budget = ContextBroker::new(profile.clone()).compute_budget();

    RequestContext {
        identity,
        profile,
        coverage,
        broker_budget,
    }
}

/// Returns whether lean-ctx can modify context for the request.
#[must_use]
pub fn should_optimize(ctx: &RequestContext) -> bool {
    coverage_class::is_addressable(ctx.coverage)
}

/// Maps request coverage to the supported optimization level.
#[must_use]
pub fn optimization_level(ctx: &RequestContext) -> OptimizationLevel {
    match ctx.coverage {
        CoverageClass::FullInline => OptimizationLevel::Full,
        CoverageClass::ContextControlled => OptimizationLevel::Partial,
        CoverageClass::ObserveOnly => OptimizationLevel::ObserveOnly,
        CoverageClass::Unmanaged => OptimizationLevel::None,
    }
}

/// Records request token usage with detected client and coverage metadata.
pub fn record_request_etpao(
    etpao: &mut EtpaoLive,
    ctx: &RequestContext,
    input_tokens: usize,
    output_tokens: usize,
) {
    etpao.record_request(RequestMetrics {
        input_tokens,
        output_tokens,
        reasoning_tokens: 0,
        schema_tokens: 0,
        cache_write_tokens: 0,
        retry_count: 0,
        client_id: ctx.profile.client_id.clone(),
        coverage_class: ctx.coverage,
    });
}

/// Records an evaluated outcome with detected client attribution.
pub fn record_outcome_etpao(
    etpao: &mut EtpaoLive,
    ctx: &RequestContext,
    accepted: bool,
    quality: f64,
) {
    etpao.record_outcome(OutcomeMetrics {
        accepted,
        quality_score: quality,
        first_pass: accepted,
        client_id: ctx.profile.client_id.clone(),
    });
}

/// Formats a compact request summary for diagnostics.
#[must_use]
pub fn format_request_summary(ctx: &RequestContext) -> String {
    format!(
        "[{}] user={} team={} budget={}",
        coverage_class::coverage_label(ctx.coverage),
        ctx.identity.user_id.as_deref().unwrap_or("-"),
        ctx.identity.team_id.as_deref().unwrap_or("-"),
        ctx.broker_budget.context_tokens
    )
}

#[cfg(test)]
mod tests {
    use super::{
        OptimizationLevel, build_request_context, format_request_summary, optimization_level,
        record_outcome_etpao, record_request_etpao, should_optimize,
    };
    use crate::core::context_kernel::coverage_class::CoverageClass;
    use crate::core::context_kernel::etpao_live::EtpaoLive;

    fn context(coverage: CoverageClass) -> super::RequestContext {
        let (has_proxy, has_mcp, has_hooks) = match coverage {
            CoverageClass::FullInline => (true, false, false),
            CoverageClass::ContextControlled => (false, true, false),
            CoverageClass::ObserveOnly => (false, false, true),
            CoverageClass::Unmanaged => (false, false, false),
        };
        build_request_context(&[], has_proxy, has_mcp, has_hooks)
    }

    #[test]
    fn build_context_from_empty_headers() {
        let ctx = build_request_context(&[], false, false, false);
        assert_eq!(ctx.profile.client_id, "unknown");
        assert_eq!(ctx.profile.coverage, CoverageClass::Unmanaged);
        assert_eq!(ctx.coverage, CoverageClass::Unmanaged);
        assert!(ctx.broker_budget.context_tokens > 0);
    }

    #[test]
    fn build_context_with_proxy() {
        let ctx = build_request_context(&[], true, false, false);
        assert_eq!(ctx.coverage, CoverageClass::FullInline);
        assert_eq!(ctx.profile.coverage, CoverageClass::FullInline);
    }

    #[test]
    fn should_optimize_full_inline() {
        assert!(should_optimize(&context(CoverageClass::FullInline)));
    }

    #[test]
    fn should_optimize_unmanaged() {
        assert!(!should_optimize(&context(CoverageClass::Unmanaged)));
    }

    #[test]
    fn optimization_level_mapping() {
        for (coverage, expected) in [
            (CoverageClass::FullInline, OptimizationLevel::Full),
            (CoverageClass::ContextControlled, OptimizationLevel::Partial),
            (CoverageClass::ObserveOnly, OptimizationLevel::ObserveOnly),
            (CoverageClass::Unmanaged, OptimizationLevel::None),
        ] {
            assert_eq!(optimization_level(&context(coverage)), expected);
        }
    }

    #[test]
    fn format_summary_includes_identity() {
        let ctx = build_request_context(&[], false, true, false);
        let expected_user = format!("user={}", ctx.identity.user_id.as_deref().unwrap_or("-"));
        let expected_team = format!("team={}", ctx.identity.team_id.as_deref().unwrap_or("-"));
        let summary = format_request_summary(&ctx);
        assert!(summary.contains(&expected_user));
        assert!(summary.contains(&expected_team));
        assert!(summary.contains("budget="));
    }

    #[test]
    fn etpao_recording_works() {
        let ctx = context(CoverageClass::FullInline);
        let mut etpao = EtpaoLive::new();
        record_request_etpao(&mut etpao, &ctx, 100, 25);
        record_outcome_etpao(&mut etpao, &ctx, true, 0.9);

        let summary = etpao.summary();
        assert_eq!(etpao.request_count(), 1);
        assert_eq!(etpao.outcome_count(), 1);
        assert_eq!(summary.total_tokens, 125);
        assert_eq!(summary.accepted_outcomes, 1);
    }
}
