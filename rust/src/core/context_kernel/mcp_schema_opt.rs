//! MCP tool-schema compression and budget enforcement.

use super::coverage_class::CoverageClass;

/// A single tool schema entry for optimization.
#[derive(Debug, Clone)]
pub struct SchemaEntry {
    /// Tool name.
    pub name: String,
    /// Tool description text.
    pub description: String,
    /// Number of parameters the tool accepts.
    pub param_count: usize,
    /// Estimated token count for the full schema.
    pub estimated_tokens: usize,
    /// Whether this tool is essential (never dropped).
    pub essential: bool,
}

/// Token budget constraints for tool schemas.
#[derive(Debug, Clone)]
pub struct SchemaBudget {
    /// Maximum total tokens for all tool schemas combined.
    pub max_total_tokens: usize,
    /// Maximum tokens per individual tool schema.
    pub max_per_tool_tokens: usize,
}

impl Default for SchemaBudget {
    fn default() -> Self {
        Self {
            max_total_tokens: 8_000,
            max_per_tool_tokens: 500,
        }
    }
}

/// Result of schema optimization.
#[derive(Debug, Clone)]
pub struct OptimizedSchemas {
    /// Optimized schema entries.
    pub entries: Vec<SchemaEntry>,
    /// Total tokens before optimization.
    pub tokens_before: usize,
    /// Total tokens after optimization.
    pub tokens_after: usize,
    /// Number of tools whose descriptions were compressed.
    pub compressed_count: usize,
    /// Number of tools dropped entirely.
    pub dropped_count: usize,
}

impl OptimizedSchemas {
    /// Returns the percentage of estimated schema tokens saved.
    pub fn savings_pct(&self) -> f64 {
        if self.tokens_before == 0 {
            return 0.0;
        }

        self.tokens_before.saturating_sub(self.tokens_after) as f64 / self.tokens_before as f64
            * 100.0
    }
}

/// Estimates token usage using a conservative four-characters-per-token ratio.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Collapses whitespace and truncates a description to the requested token budget.
pub fn compress_description(desc: &str, max_tokens: usize) -> String {
    if estimate_tokens(desc) <= max_tokens {
        return desc.to_owned();
    }

    let normalized = desc.split_whitespace().collect::<Vec<_>>().join(" ");
    if estimate_tokens(&normalized) <= max_tokens {
        return normalized;
    }

    let max_chars = max_tokens.saturating_mul(4);
    if max_chars == 0 {
        return String::new();
    }

    let mut compressed = normalized
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    compressed.push('…');
    compressed
}

/// Compresses and filters schemas until the supplied budget is met where possible.
pub fn optimize_schemas(entries: &[SchemaEntry], budget: &SchemaBudget) -> OptimizedSchemas {
    let tokens_before = entries.iter().fold(0usize, |total, entry| {
        total.saturating_add(entry.estimated_tokens)
    });
    let mut optimized = entries.to_vec();
    optimized.sort_unstable_by_key(|entry| (!entry.essential, entry.estimated_tokens));

    let mut compressed_count = 0;
    for entry in &mut optimized {
        let description_tokens = estimate_tokens(&entry.description);
        if description_tokens > budget.max_per_tool_tokens {
            let compressed = compress_description(&entry.description, budget.max_per_tool_tokens);
            let schema_overhead = entry.estimated_tokens.saturating_sub(description_tokens);
            entry.estimated_tokens = schema_overhead.saturating_add(estimate_tokens(&compressed));
            entry.description = compressed;
            compressed_count += 1;
        }
    }

    let mut tokens_after = optimized.iter().fold(0usize, |total, entry| {
        total.saturating_add(entry.estimated_tokens)
    });
    let mut dropped_count = 0;
    while tokens_after > budget.max_total_tokens {
        let Some(index) = optimized.iter().rposition(|entry| !entry.essential) else {
            break;
        };
        let dropped = optimized.remove(index);
        tokens_after = tokens_after.saturating_sub(dropped.estimated_tokens);
        dropped_count += 1;
    }

    OptimizedSchemas {
        entries: optimized,
        tokens_before,
        tokens_after,
        compressed_count,
        dropped_count,
    }
}

/// Returns schema limits suited to the client's integration coverage.
pub fn budget_for_coverage(coverage: CoverageClass) -> SchemaBudget {
    match coverage {
        CoverageClass::FullInline => SchemaBudget {
            max_total_tokens: 12_000,
            max_per_tool_tokens: 800,
        },
        CoverageClass::ContextControlled => SchemaBudget::default(),
        CoverageClass::ObserveOnly => SchemaBudget {
            max_total_tokens: 4_000,
            max_per_tool_tokens: 300,
        },
        CoverageClass::Unmanaged => SchemaBudget {
            max_total_tokens: usize::MAX,
            max_per_tool_tokens: usize::MAX,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CoverageClass, OptimizedSchemas, SchemaBudget, SchemaEntry, budget_for_coverage,
        compress_description, optimize_schemas,
    };

    fn entry(name: &str, tokens: usize, essential: bool) -> SchemaEntry {
        SchemaEntry {
            name: name.to_owned(),
            description: "short description".to_owned(),
            param_count: 1,
            estimated_tokens: tokens,
            essential,
        }
    }

    #[test]
    fn compress_under_budget_unchanged() {
        let description = "short\n description";
        assert_eq!(compress_description(description, 20), description);
    }

    #[test]
    fn compress_over_budget_truncates() {
        let compressed = compress_description(&"a".repeat(100), 10);
        assert!(compressed.ends_with('…'));
        assert!(compressed.chars().count() <= 40);
    }

    #[test]
    fn optimize_drops_non_essential() {
        let entries = vec![entry("small", 20, false), entry("large", 80, false)];
        let result = optimize_schemas(
            &entries,
            &SchemaBudget {
                max_total_tokens: 30,
                max_per_tool_tokens: 500,
            },
        );
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "small");
        assert_eq!(result.dropped_count, 1);
    }

    #[test]
    fn optimize_keeps_essential() {
        let entries = vec![entry("optional", 20, false), entry("essential", 80, true)];
        let result = optimize_schemas(
            &entries,
            &SchemaBudget {
                max_total_tokens: 10,
                max_per_tool_tokens: 500,
            },
        );
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "essential");
    }

    #[test]
    fn savings_pct_correct() {
        let result = OptimizedSchemas {
            entries: Vec::new(),
            tokens_before: 1_000,
            tokens_after: 600,
            compressed_count: 0,
            dropped_count: 0,
        };
        assert!((result.savings_pct() - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn budget_for_coverage_varies() {
        let full = budget_for_coverage(CoverageClass::FullInline);
        let controlled = budget_for_coverage(CoverageClass::ContextControlled);
        let observed = budget_for_coverage(CoverageClass::ObserveOnly);
        assert!(full.max_total_tokens > controlled.max_total_tokens);
        assert!(controlled.max_total_tokens > observed.max_total_tokens);
        assert!(full.max_per_tool_tokens > controlled.max_per_tool_tokens);
        assert!(controlled.max_per_tool_tokens > observed.max_per_tool_tokens);
    }
}
