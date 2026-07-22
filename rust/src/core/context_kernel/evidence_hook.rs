//! Thin evidence-recording hooks for tool and proxy hot paths.

/// Unified evidence totals across tool and proxy calls.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct EvidenceReport {
    /// Number of recorded MCP tool calls.
    pub tool_calls: usize,
    /// Number of recorded proxy calls.
    pub proxy_calls: usize,
    /// Input tokens across all recorded calls.
    pub total_input_tokens: usize,
    /// Output tokens across all recorded calls.
    pub total_output_tokens: usize,
    /// Tokens saved across all recorded calls.
    pub total_tokens_saved: usize,
    /// Number of entries in the evidence receipt chain.
    pub evidence_chain_entries: usize,
}

/// Records evidence for one MCP tool call when the kernel is enabled.
pub fn record_tool_call(tool_name: &str, input_tokens: usize, output_tokens: usize) {
    if !super::kernel_config::is_enabled() {
        return;
    }
    let data = super::mcp_bridge::McpCallData {
        tool_name: tool_name.to_owned(),
        input_tokens,
        output_tokens,
        is_retry: false,
        call_number: 1,
    };
    super::mcp_bridge::record_mcp_call(&data);
    super::envelope_wiring::process_mcp_evidence(&data);
}

/// Records evidence for one proxy-forwarded request when the kernel is enabled.
pub fn record_proxy_call(
    input_tokens: usize,
    output_tokens: usize,
    tokens_saved: usize,
    model: Option<&str>,
    provider: Option<&str>,
) {
    if !super::kernel_config::is_enabled() {
        return;
    }
    let data = super::proxy_bridge::ProxyRequestData {
        input_tokens,
        output_tokens,
        tokens_saved,
        model: model.map(str::to_owned),
        provider: provider.map(str::to_owned),
        request_count: 1,
        ..Default::default()
    };
    let result = super::proxy_bridge::process_proxy_request(&data);
    super::envelope_wiring::process_proxy_evidence(&data, &result);
}

/// Returns unified evidence totals for the current process.
#[must_use]
pub fn evidence_report() -> EvidenceReport {
    let evidence = super::envelope_wiring::evidence_summary();
    let mcp = super::mcp_bridge::mcp_summary();
    let proxy = super::proxy_bridge::identity_summary();
    let usage = super::usage_normalizer::session_usage();
    let (total_input_tokens, total_output_tokens) =
        usage
            .per_model
            .values()
            .fold((0_usize, 0_usize), |totals, entry| {
                (
                    totals.0.saturating_add(entry.input_tokens),
                    totals.1.saturating_add(entry.output_tokens),
                )
            });

    EvidenceReport {
        tool_calls: mcp.total_calls,
        proxy_calls: evidence.proxy_requests,
        total_input_tokens,
        total_output_tokens,
        total_tokens_saved: proxy.total_savings,
        evidence_chain_entries: evidence.chain_entries,
    }
}

/// Clears all tool, proxy, and evidence-pipeline state.
pub fn reset() {
    super::envelope_wiring::reset_evidence();
    super::proxy_bridge::reset_state();
    super::mcp_bridge::reset_mcp_state();
}

#[cfg(test)]
mod tests {
    use super::{EvidenceReport, evidence_report, record_proxy_call, record_tool_call, reset};

    fn isolated() -> std::sync::MutexGuard<'static, ()> {
        let guard = super::super::kernel_config::KERNEL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::super::kernel_config::reset_features();
        super::super::envelope_wiring::reset_evidence();
        super::super::proxy_bridge::reset_state();
        super::super::mcp_bridge::reset_mcp_state();
        guard
    }

    #[test]
    fn tool_call_records_evidence() {
        let _guard = isolated();
        for _ in 0..3 {
            record_tool_call("ctx_read", 10, 4);
        }
        let report = evidence_report();
        assert!(report.tool_calls >= 3);
    }

    #[test]
    fn proxy_call_records_evidence() {
        let _guard = isolated();
        for _ in 0..2 {
            record_proxy_call(20, 5, 8, Some("gpt-test"), Some("openai"));
        }
        let report = evidence_report();
        assert!(report.proxy_calls >= 2);
    }

    #[test]
    fn disabled_kernel_no_evidence() {
        let _guard = isolated();
        let mut features = super::super::kernel_config::features();
        features.enabled = false;
        super::super::kernel_config::update_features(features);
        record_tool_call("ctx_read", 10, 4);
        record_proxy_call(20, 5, 8, None, None);
        assert_eq!(evidence_report(), EvidenceReport::default());
    }

    #[test]
    fn reset_clears_state() {
        let _guard = isolated();
        record_tool_call("ctx_read", 10, 4);
        record_proxy_call(20, 5, 8, None, None);
        reset();
        assert_eq!(evidence_report(), EvidenceReport::default());
    }
}
