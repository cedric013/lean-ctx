#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::core::context_field::{ContextItemId, TokenBudget};
    use crate::core::context_kernel::attribution::{AttributionReport, compute_attribution};
    use crate::core::context_kernel::learning::{OutcomeLearner, WeightUpdate};
    use crate::core::context_kernel::orchestrator::ContextKernel;
    use crate::core::context_kernel::policy::{ContextPolicy, PolicyFilter};
    use crate::core::context_kernel::types::{
        ContextObjectKind, ContextObjectV1, ContextPlanV1, ContextReceiptV1, PlanBudget, PlanEntry,
        QualitySignal, ReceiptOutcome, RetrievalContext, SensitivityLevel,
    };

    fn test_candidate(
        source: &str,
        sensitivity: SensitivityLevel,
        tokens: usize,
    ) -> ContextObjectV1 {
        ContextObjectV1 {
            id: ContextItemId(format!("test:{source}")),
            kind: ContextObjectKind::Fact,
            source: source.to_owned(),
            sensitivity,
            token_estimate: tokens,
            ..ContextObjectV1::default()
        }
    }

    #[test]
    fn plan_receipt_roundtrip() {
        let project_root = std::env::temp_dir().join("lean-ctx-kernel-conformance");
        let project_root_text = project_root.to_string_lossy();
        let kernel = ContextKernel::for_project(project_root_text.as_ref());
        let context = RetrievalContext {
            query: "context kernel conformance".to_owned(),
            task: Some("verify plan receipt roundtrip".to_owned()),
            project_root: project_root_text.into_owned(),
            budget: TokenBudget {
                total: 1_000,
                used: 0,
            },
            max_candidates: 10,
        };

        let plan = kernel.plan(&context);
        let receipt = kernel.record_receipt(&plan, 64, ReceiptOutcome::Accepted);

        assert_eq!(receipt.plan_id, plan.plan_id);
        assert!(receipt.delivered_tokens > 0);
    }

    #[test]
    fn policy_filters_sensitive_candidates() {
        let candidates: Vec<ContextObjectV1> = vec![
            test_candidate("public", SensitivityLevel::Public, 20),
            test_candidate("restricted", SensitivityLevel::Restricted, 20),
        ];
        let policy = ContextPolicy {
            max_sensitivity: SensitivityLevel::Internal,
            allowed_sources: None,
            blocked_sources: Vec::new(),
            budget_cap_tokens: None,
            retention_days: None,
        };
        let filter = PolicyFilter::new(policy);

        let filtered = filter.apply(candidates);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source, "public");
        assert_eq!(filtered[0].sensitivity, SensitivityLevel::Public);
    }

    #[test]
    fn attribution_no_double_counting() {
        let plan = ContextPlanV1 {
            plan_id: "plan:conformance".to_owned(),
            intent: "verify provider attribution".to_owned(),
            budget: PlanBudget {
                total_tokens: 1_000,
                used_tokens: 700,
                remaining_tokens: 300,
            },
            selected: vec![
                plan_entry("file:a", "files", 400),
                plan_entry("file:b", "files", 100),
                plan_entry("fact:c", "knowledge", 200),
            ],
            excluded: Vec::new(),
            deferred: Vec::new(),
            provider_stats: HashMap::new(),
        };
        let receipt = ContextReceiptV1 {
            receipt_id: "receipt:conformance".to_owned(),
            plan_id: plan.plan_id.clone(),
            delivered_tokens: 500,
            cache_hits: 1,
            cache_misses: 2,
            outcome: ReceiptOutcome::Accepted,
            quality_signals: vec![QualitySignal {
                signal_type: "outcome".to_owned(),
                value: 1.0,
            }],
            feedback_attribution: HashMap::new(),
        };

        let report: AttributionReport = compute_attribution(&plan, &receipt);
        let summed_savings: usize = report.entries.iter().map(|entry| entry.tokens_saved).sum();
        let mut provider_occurrences: HashMap<&str, usize> = HashMap::new();
        for entry in &report.entries {
            *provider_occurrences
                .entry(entry.provider.as_str())
                .or_insert(0) += 1;
        }

        assert!(summed_savings <= report.total_tokens_saved);
        assert_eq!(provider_occurrences.get("files"), Some(&1));
        assert_eq!(provider_occurrences.get("knowledge"), Some(&1));
        assert!(provider_occurrences.values().all(|count| *count == 1));
    }

    #[test]
    fn learning_updates_provider_weights() {
        let initial_weights: HashMap<String, f64> =
            HashMap::from([("files".to_owned(), 1.0), ("knowledge".to_owned(), 1.0)]);
        let receipt = ContextReceiptV1 {
            receipt_id: "receipt:learning".to_owned(),
            plan_id: "plan:learning".to_owned(),
            delivered_tokens: 300,
            cache_hits: 0,
            cache_misses: 0,
            outcome: ReceiptOutcome::Accepted,
            quality_signals: Vec::new(),
            feedback_attribution: HashMap::from([
                ("files".to_owned(), 0.6),
                ("knowledge".to_owned(), 0.4),
            ]),
        };
        let learner = OutcomeLearner::default_learner();

        let updates: Vec<WeightUpdate> = learner.learn_from_receipt(&receipt, &initial_weights);

        assert_eq!(updates.len(), initial_weights.len());
        for update in updates {
            let old_weight = initial_weights
                .get(&update.provider)
                .copied()
                .expect("attributed provider has an initial weight");
            assert_eq!(update.old_weight, old_weight);
            assert_ne!(update.new_weight, old_weight);
            assert!(update.new_weight >= old_weight);
        }
    }

    fn plan_entry(object_id: &str, provider: &str, tokens: usize) -> PlanEntry {
        PlanEntry {
            object_id: object_id.to_owned(),
            provider: provider.to_owned(),
            view: "summary".to_owned(),
            tokens,
            phi: 0.8,
            reason: "selected for conformance".to_owned(),
        }
    }
}
