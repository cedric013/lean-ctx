//! End-to-end conformance tests for the proxy-to-kernel bridge.

#[cfg(test)]
mod tests {
    use super::super::accounting_fix;
    use super::super::coverage_class::CoverageClass;
    use super::super::hotpath_wiring;
    use super::super::identity::CallerRole;
    use super::super::proxy_bridge::{self, ProxyRequestData};
    use super::super::tool_surface;

    fn request_for(user_id: &str) -> ProxyRequestData {
        ProxyRequestData {
            headers: vec![("x-user-id".to_owned(), user_id.to_owned())],
            input_tokens: 1_000,
            output_tokens: 200,
            tokens_saved: 300,
            request_count: 1,
            ..ProxyRequestData::default()
        }
    }

    #[test]
    fn full_proxy_lifecycle() {
        proxy_bridge::reset_state();
        let request = ProxyRequestData {
            headers: vec![
                ("x-user-id".to_owned(), "alice".to_owned()),
                ("x-team-id".to_owned(), "backend".to_owned()),
            ],
            input_tokens: 1_000,
            output_tokens: 200,
            tokens_saved: 300,
            request_count: 1,
            ..ProxyRequestData::default()
        };

        let result = proxy_bridge::process_proxy_request(&request);

        assert_eq!(result.identity.user_id.as_deref(), Some("alice"));
        assert_eq!(result.identity.role, CallerRole::Developer);
        assert_eq!(result.coverage, CoverageClass::FullInline);
        assert!(result.is_addressable);
    }

    #[test]
    fn proxy_records_etpao() {
        proxy_bridge::reset_state();
        for _ in 0..5 {
            proxy_bridge::process_proxy_request(&request_for("alice"));
        }

        assert!(proxy_bridge::current_etpao() > 0.0);
    }

    #[test]
    fn proxy_records_identity_ledger() {
        proxy_bridge::reset_state();
        for _ in 0..3 {
            proxy_bridge::process_proxy_request(&request_for("bob"));
        }

        assert!(proxy_bridge::identity_summary().total_users >= 1);
    }

    #[test]
    fn tool_optimization_saves_tokens() {
        proxy_bridge::reset_state();
        let schemas = (0..15)
            .map(|index| tool_surface::ToolSchema {
                name: format!("tool-{index}"),
                description: "Conformance test tool".to_owned(),
                parameters_json: r#"{"type":"object"}"#.to_owned(),
                token_count: 2_000,
                priority: 1,
                category: tool_surface::ToolCategory::Core,
            })
            .collect::<Vec<_>>();

        let reduction = tool_surface::optimize_for_request(&[], &schemas);

        assert!(reduction.tokens_saved > 0);
    }

    #[test]
    fn honest_accounting_detects_phantom() {
        proxy_bridge::reset_state();
        let accounting = accounting_fix::account_proxy_request(1_000, 300, 200, 50);

        assert!(accounting.phantom_savings_pct > 0.0);
        assert!(accounting_fix::format_proxy_accounting(&accounting).contains("phantom"));
    }

    #[test]
    fn mcp_integration_respects_coverage() {
        proxy_bridge::reset_state();
        let managed = hotpath_wiring::integrate_for_mcp("q", "/tmp", &[], 1_000, 300);
        let unmanaged_headers = vec![("x-coverage-class".to_owned(), "unmanaged".to_owned())];
        let unmanaged =
            hotpath_wiring::integrate_for_mcp("q", "/tmp", &unmanaged_headers, 1_000, 300);

        assert_eq!(managed.accounting.original_tokens, 1_000);
        assert_eq!(unmanaged.budget_used, 0);
    }

    #[test]
    fn end_to_end_identity_to_etpao() {
        proxy_bridge::reset_state();
        for index in 0..10 {
            let user = format!("user-{}", index % 3);
            proxy_bridge::process_proxy_request(&request_for(&user));
        }

        assert!(proxy_bridge::identity_summary().total_users >= 3);
        let summary = proxy_bridge::etpao_summary();
        assert!(summary.total_tokens > 0);
        assert!(summary.accepted_outcomes > 0);
    }

    #[test]
    fn outcome_signal_integrated() {
        proxy_bridge::reset_state();
        let request = ProxyRequestData {
            is_retry: true,
            request_count: 3,
            ..request_for("alice")
        };

        let result = proxy_bridge::process_proxy_request(&request);

        assert_eq!(
            result.outcome_signal.outcome,
            super::super::types::ReceiptOutcome::Rejected
        );
    }
}
