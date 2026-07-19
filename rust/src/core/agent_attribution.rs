//! ETPAO Attribution for multi-agent chains (P11 / DIM 4).
//!
//! Attribution rule: agent costs count only when the overall chain outcome is
//! accepted. This prevents double-counting and ensures that failed/redundant
//! branches don't inflate savings or costs.
//!
//! ETPAO = Effective Token Per Accepted Outcome

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ATTRIBUTION_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeVerdict {
    Pending,
    Accepted,
    Rejected,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentCostRecord {
    pub agent_id: String,
    pub node_id: String,
    pub tokens_consumed: u64,
    pub cost_micros: u64,
    pub is_on_accepted_path: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainAttribution {
    pub schema_version: u16,
    pub chain_id: String,
    pub outcome_verdict: OutcomeVerdict,
    pub total_tokens_all_agents: u64,
    pub total_cost_all_agents: u64,
    pub effective_tokens_accepted: u64,
    pub effective_cost_accepted: u64,
    pub waste_tokens: u64,
    pub waste_cost: u64,
    pub agent_contributions: Vec<AgentContribution>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent_id: String,
    pub tokens: u64,
    pub cost_micros: u64,
    pub fraction_of_accepted: f64,
    pub on_accepted_path: bool,
}

/// Tracks costs per agent/node and computes ETPAO attribution once the chain
/// outcome is known.
pub struct AttributionTracker {
    chain_id: String,
    records: BTreeMap<String, AgentCostRecord>,
    outcome: OutcomeVerdict,
}

impl AttributionTracker {
    #[must_use]
    pub fn new(chain_id: String) -> Self {
        Self {
            chain_id,
            records: BTreeMap::new(),
            outcome: OutcomeVerdict::Pending,
        }
    }

    /// Record cost for an agent at a specific work node.
    pub fn record_cost(
        &mut self,
        agent_id: String,
        node_id: String,
        tokens: u64,
        cost_micros: u64,
    ) {
        self.records
            .entry(node_id.clone())
            .and_modify(|r| {
                r.tokens_consumed = r.tokens_consumed.saturating_add(tokens);
                r.cost_micros = r.cost_micros.saturating_add(cost_micros);
            })
            .or_insert(AgentCostRecord {
                agent_id,
                node_id,
                tokens_consumed: tokens,
                cost_micros,
                is_on_accepted_path: false,
            });
    }

    /// Mark which nodes are on the accepted outcome path.
    pub fn mark_accepted_path(&mut self, node_ids: &[String]) {
        for id in node_ids {
            if let Some(record) = self.records.get_mut(id) {
                record.is_on_accepted_path = true;
            }
        }
    }

    /// Set the final chain outcome verdict.
    pub fn set_outcome(&mut self, verdict: OutcomeVerdict) {
        self.outcome = verdict;
    }

    /// Compute the final ETPAO attribution. Costs on rejected/redundant paths
    /// are classified as waste; only accepted-path costs count toward the
    /// effective metric.
    pub fn compute_attribution(&self) -> ChainAttribution {
        let total_tokens: u64 = self.records.values().map(|r| r.tokens_consumed).sum();
        let total_cost: u64 = self.records.values().map(|r| r.cost_micros).sum();

        let (effective_tokens, effective_cost) = match self.outcome {
            OutcomeVerdict::Accepted | OutcomeVerdict::Partial => {
                let t: u64 = self
                    .records
                    .values()
                    .filter(|r| r.is_on_accepted_path)
                    .map(|r| r.tokens_consumed)
                    .sum();
                let c: u64 = self
                    .records
                    .values()
                    .filter(|r| r.is_on_accepted_path)
                    .map(|r| r.cost_micros)
                    .sum();
                (t, c)
            }
            OutcomeVerdict::Rejected | OutcomeVerdict::Pending => (0, 0),
        };

        let waste_tokens = total_tokens.saturating_sub(effective_tokens);
        let waste_cost = total_cost.saturating_sub(effective_cost);

        let mut contributions: BTreeMap<String, (u64, u64, bool)> = BTreeMap::new();
        for record in self.records.values() {
            let entry = contributions.entry(record.agent_id.clone()).or_default();
            entry.0 = entry.0.saturating_add(record.tokens_consumed);
            entry.1 = entry.1.saturating_add(record.cost_micros);
            if record.is_on_accepted_path {
                entry.2 = true;
            }
        }

        let agent_contributions: Vec<AgentContribution> = contributions
            .into_iter()
            .map(|(agent_id, (tokens, cost, on_path))| {
                let fraction = if effective_tokens > 0 && on_path {
                    tokens as f64 / effective_tokens as f64
                } else {
                    0.0
                };
                AgentContribution {
                    agent_id,
                    tokens,
                    cost_micros: cost,
                    fraction_of_accepted: fraction,
                    on_accepted_path: on_path,
                }
            })
            .collect();

        ChainAttribution {
            schema_version: ATTRIBUTION_SCHEMA_VERSION,
            chain_id: self.chain_id.clone(),
            outcome_verdict: self.outcome,
            total_tokens_all_agents: total_tokens,
            total_cost_all_agents: total_cost,
            effective_tokens_accepted: effective_tokens,
            effective_cost_accepted: effective_cost,
            waste_tokens,
            waste_cost,
            agent_contributions,
        }
    }

    pub fn outcome(&self) -> OutcomeVerdict {
        self.outcome
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_outcome_attributes_only_accepted_path() {
        let mut tracker = AttributionTracker::new("chain:1".into());
        tracker.record_cost("agent-a".into(), "node-root".into(), 500, 100);
        tracker.record_cost("agent-b".into(), "node-child-1".into(), 300, 60);
        tracker.record_cost("agent-c".into(), "node-child-2".into(), 200, 40);

        tracker.mark_accepted_path(&["node-root".into(), "node-child-1".into()]);
        tracker.set_outcome(OutcomeVerdict::Accepted);

        let attr = tracker.compute_attribution();
        assert_eq!(attr.effective_tokens_accepted, 800);
        assert_eq!(attr.effective_cost_accepted, 160);
        assert_eq!(attr.waste_tokens, 200);
        assert_eq!(attr.waste_cost, 40);
        assert_eq!(attr.total_tokens_all_agents, 1000);
    }

    #[test]
    fn rejected_outcome_counts_all_as_waste() {
        let mut tracker = AttributionTracker::new("chain:2".into());
        tracker.record_cost("agent-a".into(), "n1".into(), 500, 100);
        tracker.record_cost("agent-b".into(), "n2".into(), 300, 60);
        tracker.mark_accepted_path(&["n1".into()]);
        tracker.set_outcome(OutcomeVerdict::Rejected);

        let attr = tracker.compute_attribution();
        assert_eq!(attr.effective_tokens_accepted, 0);
        assert_eq!(attr.waste_tokens, 800);
    }

    #[test]
    fn pending_outcome_attributes_nothing() {
        let mut tracker = AttributionTracker::new("chain:3".into());
        tracker.record_cost("agent-a".into(), "n1".into(), 100, 20);
        let attr = tracker.compute_attribution();
        assert_eq!(attr.outcome_verdict, OutcomeVerdict::Pending);
        assert_eq!(attr.effective_tokens_accepted, 0);
    }

    #[test]
    fn agent_contribution_fractions_sum_to_one() {
        let mut tracker = AttributionTracker::new("chain:4".into());
        tracker.record_cost("agent-a".into(), "n1".into(), 600, 120);
        tracker.record_cost("agent-b".into(), "n2".into(), 400, 80);
        tracker.mark_accepted_path(&["n1".into(), "n2".into()]);
        tracker.set_outcome(OutcomeVerdict::Accepted);

        let attr = tracker.compute_attribution();
        let sum: f64 = attr
            .agent_contributions
            .iter()
            .filter(|c| c.on_accepted_path)
            .map(|c| c.fraction_of_accepted)
            .sum();
        assert!((sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn incremental_cost_recording() {
        let mut tracker = AttributionTracker::new("chain:5".into());
        tracker.record_cost("agent-a".into(), "n1".into(), 100, 20);
        tracker.record_cost("agent-a".into(), "n1".into(), 50, 10);
        tracker.mark_accepted_path(&["n1".into()]);
        tracker.set_outcome(OutcomeVerdict::Accepted);

        let attr = tracker.compute_attribution();
        assert_eq!(attr.effective_tokens_accepted, 150);
        assert_eq!(attr.effective_cost_accepted, 30);
    }
}
