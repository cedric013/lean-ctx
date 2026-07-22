//! Server bridge for Context Kernel list-tools schema optimization.

use std::borrow::Cow;

use crate::core::context_kernel::list_tools_opt;

/// Optimizes tool descriptions when schema optimization supports the client.
#[must_use]
pub fn optimize_tools(tools: Vec<rmcp::model::Tool>, client_name: &str) -> Vec<rmcp::model::Tool> {
    if !list_tools_opt::should_optimize_for_client(client_name) {
        return tools;
    }

    let tuples = tools
        .iter()
        .map(|tool| {
            let parameter_count = tool
                .input_schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .map_or(0, serde_json::Map::len);
            (
                tool.name.to_string(),
                tool.description.as_deref().unwrap_or("").to_owned(),
                parameter_count,
            )
        })
        .collect();
    let optimized = list_tools_opt::optimize_descriptions(tuples, client_name);

    tools
        .into_iter()
        .zip(optimized)
        .map(|(mut tool, (_, description, _))| {
            tool.description = Some(Cow::Owned(description));
            tool
        })
        .collect()
}

/// Returns cumulative list-tools schema optimization statistics.
#[must_use]
pub fn summary() -> list_tools_opt::SchemaOptSummary {
    list_tools_opt::schema_opt_summary()
}

#[cfg(test)]
mod tests {
    use std::sync::MutexGuard;

    use super::list_tools_opt;
    use crate::core::context_kernel::kernel_config::{self, KERNEL_TEST_LOCK, KernelFeatures};

    fn setup() -> MutexGuard<'static, ()> {
        let guard = KERNEL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kernel_config::reset_features();
        list_tools_opt::reset();
        guard
    }

    #[test]
    fn disabled_returns_same_count() {
        let _guard = setup();
        let mut features = KernelFeatures::default();
        features.enabled = false;
        kernel_config::update_features(features);
        let tools = vec![("tool".to_owned(), "description".to_owned(), 1)];

        assert_eq!(
            list_tools_opt::optimize_descriptions(tools, "cursor").len(),
            1
        );
    }

    #[test]
    fn summary_works() {
        let _guard = setup();
        let summary = list_tools_opt::schema_opt_summary();

        assert_eq!(summary.optimizations_applied, 0);
        assert_eq!(summary.total_tokens_saved, 0);
        assert_eq!(summary.avg_reduction_percent, 0.0);
    }

    #[test]
    fn optimization_flag_check() {
        let _guard = setup();

        assert!(list_tools_opt::should_optimize_for_client("cursor"));
        assert!(!list_tools_opt::should_optimize_for_client(
            "unknown-client"
        ));
    }
}
