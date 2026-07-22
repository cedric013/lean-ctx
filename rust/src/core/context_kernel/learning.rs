//! Receipt-driven learning for Context Kernel provider weights.

use std::collections::HashMap;

use super::types::{ContextReceiptV1, ReceiptOutcome};

/// A provider weight change inferred from a receipt outcome.
#[derive(Debug, Clone)]
pub struct WeightUpdate {
    pub provider: String,
    pub old_weight: f64,
    pub new_weight: f64,
    pub delta: f64,
    pub reason: String,
}

/// Converts receipt outcomes into bounded provider weight updates.
pub struct OutcomeLearner {
    alpha: f64,
}

impl OutcomeLearner {
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(0.01, 0.5),
        }
    }

    pub fn default_learner() -> Self {
        Self::new(0.1)
    }

    pub fn learn_from_receipt(
        &self,
        receipt: &ContextReceiptV1,
        current_weights: &HashMap<String, f64>,
    ) -> Vec<WeightUpdate> {
        let (outcome_score, outcome_name) = match receipt.outcome {
            ReceiptOutcome::Accepted => (1.0, "accepted"),
            ReceiptOutcome::Partial => (0.5, "partial"),
            ReceiptOutcome::Rejected => (0.0, "rejected"),
            ReceiptOutcome::Unknown => return Vec::new(),
        };

        let mut providers: Vec<&String> = receipt.feedback_attribution.keys().collect();
        providers.sort_by_key(|provider| provider.as_str());

        providers
            .into_iter()
            .map(|provider| {
                let old_weight = current_weights.get(provider).copied().unwrap_or(1.0);
                let new_weight = old_weight * (1.0 - self.alpha) + outcome_score * self.alpha;

                WeightUpdate {
                    provider: provider.clone(),
                    old_weight,
                    new_weight,
                    delta: new_weight - old_weight,
                    reason: format!("{outcome_name} receipt outcome"),
                }
            })
            .collect()
    }

    pub fn apply_updates(weights: &mut HashMap<String, f64>, updates: &[WeightUpdate]) {
        for update in updates {
            weights.insert(update.provider.clone(), update.new_weight);
        }
    }
}

/// Learns provider changes from a receipt and persists the receipt feedback.
pub fn learn_and_update(project_root: &str, receipt: &ContextReceiptV1) -> Vec<WeightUpdate> {
    let mut collector = super::feedback::FeedbackCollector::default_for_project(project_root);
    collector.load_weights();
    let learner = OutcomeLearner::default_learner();

    let mut current: HashMap<String, f64> = HashMap::new();
    for provider in receipt.feedback_attribution.keys() {
        current.insert(provider.clone(), collector.provider_weight(provider));
    }

    let updates = learner.learn_from_receipt(receipt, &current);
    collector.record_outcome(receipt);
    updates
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::OutcomeLearner;
    use crate::core::context_kernel::types::{ContextReceiptV1, ReceiptOutcome};

    fn receipt(outcome: ReceiptOutcome) -> ContextReceiptV1 {
        ContextReceiptV1 {
            receipt_id: "receipt-1".to_owned(),
            plan_id: "plan-1".to_owned(),
            delivered_tokens: 100,
            cache_hits: 0,
            cache_misses: 0,
            outcome,
            quality_signals: Vec::new(),
            feedback_attribution: HashMap::from([("files".to_owned(), 1.0)]),
        }
    }

    #[test]
    fn accepted_increases_weight() {
        let learner = OutcomeLearner::default_learner();
        let current: HashMap<String, f64> = HashMap::from([("files".to_owned(), 0.5)]);

        let updates = learner.learn_from_receipt(&receipt(ReceiptOutcome::Accepted), &current);

        assert_eq!(updates.len(), 1);
        assert!(updates[0].new_weight > updates[0].old_weight);
        assert!(updates[0].delta > 0.0);
    }

    #[test]
    fn rejected_decreases_weight() {
        let learner = OutcomeLearner::default_learner();
        let current: HashMap<String, f64> = HashMap::from([("files".to_owned(), 1.0)]);

        let updates = learner.learn_from_receipt(&receipt(ReceiptOutcome::Rejected), &current);

        assert_eq!(updates.len(), 1);
        assert!(updates[0].new_weight < updates[0].old_weight);
        assert!(updates[0].delta < 0.0);
    }

    #[test]
    fn unknown_skips_learning() {
        let learner = OutcomeLearner::default_learner();
        let current: HashMap<String, f64> = HashMap::new();

        let updates = learner.learn_from_receipt(&receipt(ReceiptOutcome::Unknown), &current);

        assert!(updates.is_empty());
    }

    #[test]
    fn apply_updates_replaces_provider_weights() {
        let learner = OutcomeLearner::default_learner();
        let current: HashMap<String, f64> = HashMap::from([("files".to_owned(), 0.5)]);
        let updates = learner.learn_from_receipt(&receipt(ReceiptOutcome::Accepted), &current);
        let mut weights: HashMap<String, f64> = HashMap::new();

        OutcomeLearner::apply_updates(&mut weights, &updates);

        assert_eq!(weights.get("files"), Some(&updates[0].new_weight));
    }
}
