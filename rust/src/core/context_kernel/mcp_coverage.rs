//! MCP-specific client coverage detection and efficiency profiles.

use super::client_profile::{ClientEfficiencyProfile, ProfileBuilder};
use super::client_wiring::OptimizationLevel;
use super::coverage_class::CoverageClass;

/// Detects the context coverage available for an MCP client and its capabilities.
#[must_use]
pub fn detect_mcp_coverage(client: &str, has_roots: bool, has_sampling: bool) -> CoverageClass {
    let base = match client.to_ascii_lowercase().as_str() {
        "cursor" | "cursor-ide" => CoverageClass::FullInline,
        "vscode" | "visual-studio-code" | "zed" => CoverageClass::ContextControlled,
        _ => CoverageClass::ObserveOnly,
    };

    if base == CoverageClass::ObserveOnly && has_roots && has_sampling {
        CoverageClass::ContextControlled
    } else {
        base
    }
}

/// Builds the kernel efficiency profile for an MCP client.
#[must_use]
pub fn mcp_client_profile(client: &str) -> ClientEfficiencyProfile {
    let context_window = match client.to_ascii_lowercase().as_str() {
        "cursor" | "cursor-ide" => 200_000,
        "vscode" | "visual-studio-code" => 128_000,
        _ => 64_000,
    };

    ProfileBuilder::new(client)
        .coverage(detect_mcp_coverage(client, false, false))
        .context_window(context_window)
        .build()
}

/// Returns the optimization level supported by an MCP client.
#[must_use]
pub fn mcp_optimization_level(client: &str) -> OptimizationLevel {
    match detect_mcp_coverage(client, false, false) {
        CoverageClass::FullInline => OptimizationLevel::Full,
        CoverageClass::ContextControlled => OptimizationLevel::Partial,
        CoverageClass::ObserveOnly => OptimizationLevel::ObserveOnly,
        CoverageClass::Unmanaged => OptimizationLevel::None,
    }
}

/// Returns the maximum schema-token budget for an MCP client.
#[must_use]
pub fn client_schema_budget(client: &str) -> usize {
    mcp_client_profile(client).tool_budget.max_schema_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_full_inline() {
        assert_eq!(
            detect_mcp_coverage("cursor", false, false),
            CoverageClass::FullInline
        );
    }

    #[test]
    fn vscode_is_context_controlled() {
        assert_eq!(
            detect_mcp_coverage("vscode", false, false),
            CoverageClass::ContextControlled
        );
    }

    #[test]
    fn claude_is_observe_only() {
        assert_eq!(
            detect_mcp_coverage("claude-code", false, false),
            CoverageClass::ObserveOnly
        );
    }

    #[test]
    fn unknown_is_observe_only() {
        assert_eq!(
            detect_mcp_coverage("random-client", false, false),
            CoverageClass::ObserveOnly
        );
    }

    #[test]
    fn roots_sampling_upgrades() {
        assert_eq!(
            detect_mcp_coverage("random-client", true, true),
            CoverageClass::ContextControlled
        );
    }

    #[test]
    fn profile_context_window_varies() {
        assert_eq!(mcp_client_profile("cursor").context_window, 200_000);
        assert_eq!(mcp_client_profile("vscode").context_window, 128_000);
        assert_eq!(mcp_client_profile("random-client").context_window, 64_000);
    }

    #[test]
    fn optimization_matches_coverage() {
        assert_eq!(mcp_optimization_level("cursor"), OptimizationLevel::Full);
        assert_eq!(mcp_optimization_level("vscode"), OptimizationLevel::Partial);
        assert_eq!(
            mcp_optimization_level("claude"),
            OptimizationLevel::ObserveOnly
        );
    }

    #[test]
    fn schema_budget_comes_from_profile() {
        assert_eq!(
            client_schema_budget("cursor"),
            mcp_client_profile("cursor").tool_budget.max_schema_tokens
        );
    }
}
