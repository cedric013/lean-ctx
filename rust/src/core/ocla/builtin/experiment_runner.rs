//! BuiltinExperimentRunner — executes holdout/A-B experiments locally.
//!
//! Wraps `proxy/holdout.rs` behind the OCLA trait. Experiments are identified
//! by deterministic refs. Results carry an outcome ref for correlation with
//! the OutcomeTracker and an optional rollback ref for reverting the cohort.

use crate::core::ocla::traits::{ExperimentRunner, OclaService};
use crate::core::ocla::types::{
    ExperimentRequest, ExperimentResult, OclaCapability, OclaCapabilityKind, OclaResult,
};

pub struct BuiltinExperimentRunner;

impl BuiltinExperimentRunner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BuiltinExperimentRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl OclaService for BuiltinExperimentRunner {
    fn capability(&self) -> OclaCapability {
        OclaCapability::available(OclaCapabilityKind::ExperimentRunner)
    }
}

impl ExperimentRunner for BuiltinExperimentRunner {
    fn run_experiment(&self, request: ExperimentRequest) -> OclaResult<ExperimentResult> {
        let config = crate::core::config::Config::load();
        let requested_model = config
            .proxy
            .baseline
            .reference_model
            .as_deref()
            .ok_or_else(|| {
                crate::core::ocla::types::OclaError::Rejected(
                    OclaCapabilityKind::ExperimentRunner,
                    "no reference model configured for routing evaluation".into(),
                )
            })?;
        let pricing = crate::core::gain::model_pricing::ModelPricing::load();

        crate::core::eval_ab::routing_eval::run_routing_experiment(
            &request,
            requested_model,
            &config.proxy.routing,
            &pricing,
        )
        .map_err(|error| {
            crate::core::ocla::types::OclaError::Rejected(
                OclaCapabilityKind::ExperimentRunner,
                error.to_string(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ocla::types::OclaRequestContext;

    fn experiment(name: &str) -> ExperimentRequest {
        ExperimentRequest {
            context: OclaRequestContext {
                request_id: "r1".into(),
                session_id: "s1".into(),
                agent_id: "agent-test".into(),
                content_ref: "ref:test".into(),
                tenant_id: None,
            },
            experiment_ref: name.into(),
            cohort_ref: "cohort:control".into(),
        }
    }

    #[test]
    fn rejects_missing_suite_instead_of_fabricating_result() {
        let runner = BuiltinExperimentRunner::new();
        let error = runner.run_experiment(experiment("/definitely/missing-suite.ndjson"));
        assert!(error.is_err());
    }

    #[test]
    fn invalid_request_never_returns_synthetic_refs() {
        let runner = BuiltinExperimentRunner::new();
        let result = runner.run_experiment(experiment("exp-b"));
        assert!(result.is_err());
    }

    #[test]
    fn registry_builtins_route_experiment_requests_to_runner() {
        let registry = crate::core::ocla::registry::OclaRegistry::with_builtins();
        let result = registry
            .experiment_runner
            .run_experiment(experiment("/definitely/missing-suite.ndjson"));
        assert!(result.is_err());
    }
}
