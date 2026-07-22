//! Wiring between MCP tool registries and kernel schema optimization.

use std::sync::{Mutex, MutexGuard, OnceLock};

use super::coverage_class;
use super::kernel_config;
use super::mcp_coverage;
use super::mcp_schema_opt::{self, SchemaEntry};

const ESTIMATED_TOKENS_PER_PARAMETER: usize = 8;

/// Result of optimizing the MCP tool list.
#[derive(Debug, Clone)]
pub struct OptimizedToolList {
    /// Optimized tool entries (name, description, parameter count).
    pub tools: Vec<(String, String, usize)>,
    /// Tokens before optimization.
    pub tokens_before: usize,
    /// Tokens after optimization.
    pub tokens_after: usize,
    /// Number of tools dropped.
    pub dropped_count: usize,
    /// Number of descriptions compressed.
    pub compressed_count: usize,
    /// Whether optimization was applied (false if disabled).
    pub optimized: bool,
}

/// Cumulative process-wide schema optimization savings.
#[derive(Debug, Clone, Copy, Default)]
pub struct SchemaSavings {
    /// Number of tool-list optimizations applied.
    pub optimizations_applied: usize,
    /// Total estimated tokens removed by optimization.
    pub total_tokens_saved: usize,
    /// Saved tokens divided by tokens seen before optimization.
    pub avg_compression_ratio: f64,
}

#[derive(Debug, Default)]
struct SavingsState {
    optimizations_applied: usize,
    total_tokens_before: usize,
    total_tokens_saved: usize,
}

static SAVINGS: OnceLock<Mutex<SavingsState>> = OnceLock::new();

fn savings_guard() -> MutexGuard<'static, SavingsState> {
    SAVINGS
        .get_or_init(|| Mutex::new(SavingsState::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn estimated_schema_tokens(name: &str, description: &str, param_count: usize) -> usize {
    mcp_schema_opt::estimate_tokens(name)
        .saturating_add(mcp_schema_opt::estimate_tokens(description))
        .saturating_add(param_count.saturating_mul(ESTIMATED_TOKENS_PER_PARAMETER))
}

fn unchanged(tools: &[(String, String, usize)]) -> OptimizedToolList {
    let tokens = tools.iter().fold(0usize, |total, (name, desc, count)| {
        total.saturating_add(estimated_schema_tokens(name, desc, *count))
    });
    OptimizedToolList {
        tools: tools.to_vec(),
        tokens_before: tokens,
        tokens_after: tokens,
        dropped_count: 0,
        compressed_count: 0,
        optimized: false,
    }
}

fn optimize_tool_list_with_feature(
    tools: &[(String, String, usize)],
    client_name: &str,
    enabled: bool,
) -> OptimizedToolList {
    if !enabled {
        return unchanged(tools);
    }

    let coverage = mcp_coverage::detect_mcp_coverage(client_name, false, false);
    let budget = mcp_schema_opt::budget_for_coverage(coverage);
    let entries = tools
        .iter()
        .map(|(name, description, param_count)| SchemaEntry {
            name: name.clone(),
            description: description.clone(),
            param_count: *param_count,
            estimated_tokens: estimated_schema_tokens(name, description, *param_count),
            essential: false,
        })
        .collect::<Vec<_>>();
    let result = mcp_schema_opt::optimize_schemas(&entries, &budget);
    let tokens_saved = result.tokens_before.saturating_sub(result.tokens_after);

    {
        let mut savings = savings_guard();
        savings.optimizations_applied = savings.optimizations_applied.saturating_add(1);
        savings.total_tokens_before = savings
            .total_tokens_before
            .saturating_add(result.tokens_before);
        savings.total_tokens_saved = savings.total_tokens_saved.saturating_add(tokens_saved);
    }

    OptimizedToolList {
        tools: result
            .entries
            .into_iter()
            .map(|entry| (entry.name, entry.description, entry.param_count))
            .collect(),
        tokens_before: result.tokens_before,
        tokens_after: result.tokens_after,
        dropped_count: result.dropped_count,
        compressed_count: result.compressed_count,
        optimized: true,
    }
}

/// Optimizes MCP tool descriptions and count for the client's coverage budget.
#[must_use]
pub fn optimize_tool_list(
    tools: &[(String, String, usize)],
    client_name: &str,
) -> OptimizedToolList {
    optimize_tool_list_with_feature(
        tools,
        client_name,
        kernel_config::features().schema_optimization,
    )
}

/// Returns cumulative savings produced by enabled schema optimization.
#[must_use]
pub fn schema_savings() -> SchemaSavings {
    let savings = savings_guard();
    let avg_compression_ratio = if savings.total_tokens_before == 0 {
        0.0
    } else {
        savings.total_tokens_saved as f64 / savings.total_tokens_before as f64
    };
    SchemaSavings {
        optimizations_applied: savings.optimizations_applied,
        total_tokens_saved: savings.total_tokens_saved,
        avg_compression_ratio,
    }
}

fn should_optimize_with_feature(client_name: &str, enabled: bool) -> bool {
    let coverage = mcp_coverage::detect_mcp_coverage(client_name, false, false);
    enabled && coverage_class::is_addressable(coverage)
}

/// Returns whether schema optimization is enabled and the client is addressable.
#[must_use]
pub fn should_optimize(client_name: &str) -> bool {
    should_optimize_with_feature(client_name, kernel_config::features().schema_optimization)
}

/// Clears cumulative schema optimization metrics.
pub fn reset_schema_state() {
    *savings_guard() = SavingsState::default();
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::{
        optimize_tool_list_with_feature, reset_schema_state, schema_savings,
        should_optimize_with_feature,
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_schema_state();
        guard
    }

    fn tools(description_len: usize) -> Vec<(String, String, usize)> {
        (0..20)
            .map(|index| (format!("tool_{index}"), "x".repeat(description_len), 3))
            .collect()
    }

    #[test]
    fn optimize_reduces_tokens() {
        let _guard = setup();
        let optimized = optimize_tool_list_with_feature(&tools(4_000), "cursor", true);
        assert!(optimized.tokens_after < optimized.tokens_before);
        assert!(optimized.compressed_count > 0);
    }

    #[test]
    fn optimize_disabled_returns_unchanged() {
        let _guard = setup();
        let original = tools(100);
        let optimized = optimize_tool_list_with_feature(&original, "cursor", false);
        assert_eq!(optimized.tools, original);
        assert!(!optimized.optimized);
    }

    #[test]
    fn cursor_gets_full_budget() {
        let _guard = setup();
        let optimized = optimize_tool_list_with_feature(&tools(1_000), "cursor", true);
        assert_eq!(optimized.tools.len(), 20);
    }

    #[test]
    fn unknown_client_gets_small_budget() {
        let _guard = setup();
        let optimized = optimize_tool_list_with_feature(&tools(1_000), "random", true);
        assert!(optimized.tools.len() < 20);
    }

    #[test]
    fn should_optimize_respects_config() {
        let _guard = setup();
        assert!(!should_optimize_with_feature("cursor", false));
        assert!(should_optimize_with_feature("cursor", true));
        assert!(!should_optimize_with_feature("random", true));
    }

    #[test]
    fn savings_accumulate() {
        let _guard = setup();
        let _ = optimize_tool_list_with_feature(&tools(4_000), "cursor", true);
        let savings = schema_savings();
        assert_eq!(savings.optimizations_applied, 1);
        assert!(savings.total_tokens_saved > 0);
        assert!(savings.avg_compression_ratio > 0.0);
    }
}
