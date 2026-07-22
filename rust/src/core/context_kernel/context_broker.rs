//! Client-adaptive selection of tools, context, and output representation.

use std::cmp::Reverse;

use serde::{Deserialize, Serialize};

use super::client_profile::ClientEfficiencyProfile;
use super::coverage_class::CoverageClass;

/// Detail level used when supplying source context to a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContextMode {
    /// Supply only a manifest of available context.
    ManifestOnly,
    /// Supply symbol signatures and a structural map.
    SignaturesMap,
    /// Supply only lines relevant to the request.
    #[default]
    RelevantLines,
    /// Supply complete source text.
    FullText,
}

/// Representation used for broker output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    /// Return a compact typed result.
    TypedResult,
    /// Return a natural-language summary.
    #[default]
    Summary,
    /// Return the complete output.
    Full,
}

/// Tool metadata used for budget-aware selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// Tool name exposed to the client.
    pub name: String,
    /// Tokens consumed by the tool schema.
    pub schema_tokens: usize,
    /// Selection priority, where larger values rank first.
    pub priority: u8,
}

/// Token allocation computed for a client context window.
#[derive(Debug, Clone, Copy)]
pub struct BrokerBudget {
    /// Tokens allocated to request context.
    pub context_tokens: usize,
    /// Tokens allocated to kernel instructions.
    pub kernel_tokens: usize,
    /// Tokens allocated to tool schemas.
    pub schema_tokens: usize,
}

/// Selects context resources according to client efficiency constraints.
pub struct ContextBroker {
    profile: ClientEfficiencyProfile,
}

impl ContextBroker {
    /// Creates a broker for a client efficiency profile.
    pub fn new(profile: ClientEfficiencyProfile) -> Self {
        Self { profile }
    }

    /// Selects highest-priority tools within count and schema-token limits.
    pub fn select_tools(&self, available: &[ToolDescriptor]) -> Vec<ToolDescriptor> {
        let mut ranked = available.to_vec();
        ranked.sort_unstable_by_key(|tool| Reverse(tool.priority));

        let max_tools = self.profile.tool_budget.max_tools;
        let mut selected = Vec::with_capacity(max_tools.min(ranked.len()));
        let mut remaining_tokens = self.profile.tool_budget.max_schema_tokens;
        for tool in ranked {
            if selected.len() == max_tools {
                break;
            }
            if tool.schema_tokens <= remaining_tokens {
                remaining_tokens -= tool.schema_tokens;
                selected.push(tool);
            }
        }
        selected
    }

    /// Selects context detail from the client's context-window size.
    pub fn select_context_mode(&self) -> ContextMode {
        match self.profile.context_window {
            128_000.. => ContextMode::FullText,
            64_000.. => ContextMode::RelevantLines,
            32_000.. => ContextMode::SignaturesMap,
            _ => ContextMode::ManifestOnly,
        }
    }

    /// Splits the context window 70/10/20 between context, kernel, and schemas.
    pub fn compute_budget(&self) -> BrokerBudget {
        let window = self.profile.context_window;
        BrokerBudget {
            context_tokens: window.saturating_mul(70) / 100,
            kernel_tokens: window.saturating_mul(10) / 100,
            schema_tokens: window.saturating_mul(20) / 100,
        }
    }

    /// Returns whether a small context window should use indirect handles.
    pub fn should_use_handles(&self) -> bool {
        self.profile.context_window < 32_000
    }

    /// Selects the output representation supported by the coverage class.
    pub fn select_output_format(&self) -> OutputFormat {
        match self.profile.coverage {
            CoverageClass::FullInline => OutputFormat::TypedResult,
            CoverageClass::ContextControlled => OutputFormat::Summary,
            CoverageClass::ObserveOnly | CoverageClass::Unmanaged => OutputFormat::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextBroker, ContextMode, OutputFormat, ToolDescriptor};
    use crate::core::context_kernel::client_profile::{ProfileBuilder, ToolBudget};
    use crate::core::context_kernel::coverage_class::CoverageClass;

    fn broker(
        context_window: usize,
        coverage: CoverageClass,
        max_tools: usize,
        max_schema_tokens: usize,
    ) -> ContextBroker {
        let profile = ProfileBuilder::new("broker-test")
            .context_window(context_window)
            .coverage(coverage)
            .tool_budget(ToolBudget {
                max_tools,
                max_schema_tokens,
            })
            .build();
        ContextBroker::new(profile)
    }

    fn tool(index: usize, priority: u8, schema_tokens: usize) -> ToolDescriptor {
        ToolDescriptor {
            name: format!("tool-{index}"),
            schema_tokens,
            priority,
        }
    }

    #[test]
    fn select_tools_respects_budget() {
        let available = (0..20)
            .map(|index| tool(index, index as u8, 10))
            .collect::<Vec<_>>();
        let selected = broker(128_000, CoverageClass::default(), 5, 50).select_tools(&available);
        assert!(selected.len() <= 5);
        assert!(
            selected
                .iter()
                .map(|tool| tool.schema_tokens)
                .sum::<usize>()
                <= 50
        );
    }

    #[test]
    fn select_tools_by_priority() {
        let available = vec![tool(0, 1, 10), tool(1, 9, 10), tool(2, 5, 10)];
        let selected = broker(128_000, CoverageClass::default(), 2, 20).select_tools(&available);
        assert_eq!(selected[0].priority, 9);
        assert_eq!(selected[1].priority, 5);
    }

    #[test]
    fn context_mode_large_window() {
        let broker = broker(200_000, CoverageClass::default(), 1, 1);
        assert_eq!(broker.select_context_mode(), ContextMode::FullText);
    }

    #[test]
    fn context_mode_small_window() {
        let broker = broker(16_000, CoverageClass::default(), 1, 1);
        assert_eq!(broker.select_context_mode(), ContextMode::ManifestOnly);
    }

    #[test]
    fn budget_split_proportional() {
        let budget = broker(100_000, CoverageClass::default(), 1, 1).compute_budget();
        assert_eq!(budget.context_tokens, 70_000);
        assert_eq!(budget.kernel_tokens, 10_000);
        assert_eq!(budget.schema_tokens, 20_000);
    }

    #[test]
    fn handles_for_small_window() {
        let broker = broker(16_000, CoverageClass::default(), 1, 1);
        assert!(broker.should_use_handles());
    }

    #[test]
    fn output_format_full_inline() {
        let broker = broker(128_000, CoverageClass::FullInline, 1, 1);
        assert_eq!(broker.select_output_format(), OutputFormat::TypedResult);
    }
}
