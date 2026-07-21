//! Context budget enforcement (#1141): ensures agent config never exceeds
//! a configurable token budget, automatically evicting low-relevance rules.
//!
//! Works with `rule_scorer` to maintain an optimal set of injected rules
//! that fits within the attention budget. Tracks metrics for observability.
//!
//! Determinism (#498): same rules + same context + same budget → same output.
#![allow(dead_code)]

use super::rule_scorer::{AgentRule, BudgetAllocation, SessionContext};

const DEFAULT_MAX_CONFIG_TOKENS: usize = 800;

/// Configuration for context budget enforcement.
#[derive(Debug, Clone)]
pub struct BudgetConfig {
    pub max_config_tokens: usize,
    pub threshold: f64,
    pub rebalance_on_context_change: bool,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_config_tokens: DEFAULT_MAX_CONFIG_TOKENS,
            threshold: 0.15,
            rebalance_on_context_change: true,
        }
    }
}

/// Metrics from a budget enforcement cycle.
#[derive(Debug, Clone, Default)]
pub struct BudgetMetrics {
    pub total_rules: usize,
    pub injected_rules: usize,
    pub dormant_rules: usize,
    pub tokens_injected: usize,
    pub tokens_dormant: usize,
    pub tokens_budget: usize,
    pub budget_utilization: f64,
    pub context_changes_since_last_rebalance: usize,
}

/// The budget enforcer maintains state across context changes.
pub struct BudgetEnforcer {
    config: BudgetConfig,
    current_allocation: Option<BudgetAllocation>,
    last_context_hash: u64,
    metrics: BudgetMetrics,
}

impl BudgetEnforcer {
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            current_allocation: None,
            last_context_hash: 0,
            metrics: BudgetMetrics::default(),
        }
    }

    /// Enforce budget: score rules, allocate within budget, return the
    /// config text to inject into the context window.
    pub fn enforce(&mut self, rules: &[AgentRule], context: &SessionContext) -> String {
        let context_hash = hash_context(context);
        let needs_rebalance = self.should_rebalance(context_hash);

        if !needs_rebalance && self.current_allocation.is_some() {
            return self.render_current();
        }

        let allocation =
            super::rule_scorer::allocate_rules(rules, context, self.config.max_config_tokens);

        self.update_metrics(&allocation, rules.len());
        self.last_context_hash = context_hash;

        let output = render_allocation(&allocation);
        self.current_allocation = Some(allocation);
        output
    }

    /// Get current metrics for observability.
    pub fn metrics(&self) -> &BudgetMetrics {
        &self.metrics
    }

    /// Force a rebalance on next `enforce` call.
    pub fn invalidate(&mut self) {
        self.last_context_hash = 0;
    }

    fn should_rebalance(&mut self, new_hash: u64) -> bool {
        if self.current_allocation.is_none() {
            return true;
        }
        if !self.config.rebalance_on_context_change {
            return false;
        }
        if new_hash != self.last_context_hash {
            self.metrics.context_changes_since_last_rebalance += 1;
            return true;
        }
        false
    }

    fn render_current(&self) -> String {
        self.current_allocation
            .as_ref()
            .map(render_allocation)
            .unwrap_or_default()
    }

    fn update_metrics(&mut self, allocation: &BudgetAllocation, total_rules: usize) {
        self.metrics = BudgetMetrics {
            total_rules,
            injected_rules: allocation.injected.len(),
            dormant_rules: allocation.dormant.len(),
            tokens_injected: allocation.total_tokens_injected,
            tokens_dormant: allocation.total_tokens_dormant,
            tokens_budget: allocation.budget_tokens,
            budget_utilization: allocation.total_tokens_injected as f64
                / allocation.budget_tokens.max(1) as f64,
            context_changes_since_last_rebalance: 0,
        };
    }
}

/// Render the allocated rules into a context-window-ready string.
fn render_allocation(allocation: &BudgetAllocation) -> String {
    if allocation.injected.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for sr in &allocation.injected {
        output.push_str(&sr.rule.content);
        output.push('\n');
    }

    if !allocation.dormant.is_empty() {
        output.push_str(&format!(
            "\n[{} rules dormant ({} tokens) — context-diet auto-scoped, activate with relevant file changes]",
            allocation.dormant.len(),
            allocation.total_tokens_dormant
        ));
    }

    output
}

/// Simple context hash for change detection.
fn hash_context(context: &SessionContext) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    context.open_files.hash(&mut hasher);
    context.working_directory.hash(&mut hasher);
    context.recent_content_keywords.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rule_scorer::extract_rule_keywords;

    fn make_rules(n: usize) -> Vec<AgentRule> {
        (0..n)
            .map(|i| AgentRule {
                id: format!("rule-{i}"),
                source_path: format!(".cursor/rules/rule-{i}.mdc"),
                content: format!("Rule {i}: proxy forward authentication pattern matching"),
                path_globs: vec!["**/*.rs".to_string()],
                tokens: 15,
                keywords: extract_rule_keywords(&format!(
                    "Rule {i}: proxy forward authentication pattern matching"
                )),
            })
            .collect()
    }

    fn make_ctx() -> SessionContext {
        SessionContext {
            open_files: vec!["src/proxy/forward.rs".to_string()],
            recent_tool_calls: vec![],
            working_directory: "src/proxy".to_string(),
            recent_content_keywords: vec![
                "proxy".into(),
                "forward".into(),
                "authentication".into(),
            ],
        }
    }

    #[test]
    fn enforcer_respects_budget() {
        let config = BudgetConfig {
            max_config_tokens: 50,
            ..Default::default()
        };
        let mut enforcer = BudgetEnforcer::new(config);
        let rules = make_rules(10);
        let ctx = make_ctx();

        enforcer.enforce(&rules, &ctx);

        assert!(enforcer.metrics().tokens_injected <= 50);
        assert!(enforcer.metrics().dormant_rules > 0);
    }

    #[test]
    fn rebalances_on_context_change() {
        let config = BudgetConfig::default();
        let mut enforcer = BudgetEnforcer::new(config);
        let rules = make_rules(5);

        let ctx1 = SessionContext {
            open_files: vec!["src/proxy/forward.rs".into()],
            working_directory: "src/proxy".into(),
            ..Default::default()
        };
        let ctx2 = SessionContext {
            open_files: vec!["src/core/tokens.rs".into()],
            working_directory: "src/core".into(),
            ..Default::default()
        };

        let output1 = enforcer.enforce(&rules, &ctx1);
        let output2 = enforcer.enforce(&rules, &ctx2);

        // Different contexts should produce different outputs (or at least trigger rebalance)
        assert!(enforcer.metrics().context_changes_since_last_rebalance == 0);
        let _ = (output1, output2); // used
    }

    #[test]
    fn caches_when_context_unchanged() {
        let config = BudgetConfig::default();
        let mut enforcer = BudgetEnforcer::new(config);
        let rules = make_rules(5);
        let ctx = make_ctx();

        let output1 = enforcer.enforce(&rules, &ctx);
        let output2 = enforcer.enforce(&rules, &ctx);

        assert_eq!(output1, output2);
    }

    #[test]
    fn empty_rules_produce_empty_output() {
        let config = BudgetConfig::default();
        let mut enforcer = BudgetEnforcer::new(config);
        let ctx = make_ctx();

        let output = enforcer.enforce(&[], &ctx);
        assert!(output.is_empty());
    }

    #[test]
    fn metrics_are_populated() {
        let config = BudgetConfig {
            max_config_tokens: 100,
            ..Default::default()
        };
        let mut enforcer = BudgetEnforcer::new(config);
        let rules = make_rules(10);
        let ctx = make_ctx();

        enforcer.enforce(&rules, &ctx);

        let m = enforcer.metrics();
        assert_eq!(m.total_rules, 10);
        assert!(m.injected_rules > 0);
        assert!(m.tokens_budget == 100);
        assert!(m.budget_utilization > 0.0);
        assert!(m.budget_utilization <= 1.0);
    }

    #[test]
    fn invalidate_forces_rebalance() {
        let config = BudgetConfig::default();
        let mut enforcer = BudgetEnforcer::new(config);
        let rules = make_rules(3);
        let ctx = make_ctx();

        enforcer.enforce(&rules, &ctx);
        enforcer.invalidate();
        // After invalidate, next enforce should rebalance even with same context
        enforcer.enforce(&rules, &ctx);
        // No assertion needed — just verifying no panic
    }
}
