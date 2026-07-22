//! Coverage classification and capability detection for client integrations.

/// Describes how completely lean-ctx can manage a client integration.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum CoverageClass {
    /// All client traffic passes through the inline proxy.
    FullInline,
    /// Context is managed through MCP, without inline traffic control.
    #[default]
    ContextControlled,
    /// Hooks expose client activity for observation only.
    ObserveOnly,
    /// The client has no supported integration surface.
    Unmanaged,
}

/// Operations available for a coverage class.
#[derive(Debug, Clone, Copy)]
pub struct CoverageCapabilities {
    /// Whether input context can be compressed.
    pub can_compress: bool,
    /// Whether requests can be routed.
    pub can_route: bool,
    /// Whether context results can be cached.
    pub can_cache: bool,
    /// Whether client output can be measured.
    pub can_measure_output: bool,
}

/// Detects the strongest coverage class supported by the available integrations.
pub fn detect_coverage(has_proxy: bool, has_mcp: bool, has_hooks: bool) -> CoverageClass {
    if has_proxy {
        CoverageClass::FullInline
    } else if has_mcp {
        CoverageClass::ContextControlled
    } else if has_hooks {
        CoverageClass::ObserveOnly
    } else {
        CoverageClass::Unmanaged
    }
}

/// Returns the stable machine-readable label for a coverage class.
pub fn coverage_label(class: CoverageClass) -> &'static str {
    match class {
        CoverageClass::FullInline => "full_inline",
        CoverageClass::ContextControlled => "context_controlled",
        CoverageClass::ObserveOnly => "observe_only",
        CoverageClass::Unmanaged => "unmanaged",
    }
}

/// Returns whether lean-ctx can directly modify context for the client.
pub fn is_addressable(class: CoverageClass) -> bool {
    matches!(
        class,
        CoverageClass::FullInline | CoverageClass::ContextControlled
    )
}

/// Returns the operations available for a coverage class.
pub fn capabilities(class: CoverageClass) -> CoverageCapabilities {
    match class {
        CoverageClass::FullInline => CoverageCapabilities {
            can_compress: true,
            can_route: true,
            can_cache: true,
            can_measure_output: true,
        },
        CoverageClass::ContextControlled => CoverageCapabilities {
            can_compress: true,
            can_route: false,
            can_cache: true,
            can_measure_output: false,
        },
        CoverageClass::ObserveOnly | CoverageClass::Unmanaged => CoverageCapabilities {
            can_compress: false,
            can_route: false,
            can_cache: false,
            can_measure_output: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{CoverageClass, capabilities, detect_coverage, is_addressable};

    #[test]
    fn detect_proxy_is_full_inline() {
        assert_eq!(
            detect_coverage(true, false, false),
            CoverageClass::FullInline
        );
    }

    #[test]
    fn detect_mcp_only_is_context_controlled() {
        assert_eq!(
            detect_coverage(false, true, false),
            CoverageClass::ContextControlled
        );
    }

    #[test]
    fn detect_hooks_only_is_observe_only() {
        assert_eq!(
            detect_coverage(false, false, true),
            CoverageClass::ObserveOnly
        );
    }

    #[test]
    fn detect_nothing_is_unmanaged() {
        assert_eq!(
            detect_coverage(false, false, false),
            CoverageClass::Unmanaged
        );
    }

    #[test]
    fn full_inline_is_addressable() {
        assert!(is_addressable(CoverageClass::FullInline));
    }

    #[test]
    fn unmanaged_not_addressable() {
        assert!(!is_addressable(CoverageClass::Unmanaged));
    }

    #[test]
    fn capabilities_full_inline_all_true() {
        let capabilities = capabilities(CoverageClass::FullInline);

        assert!(capabilities.can_compress);
        assert!(capabilities.can_route);
        assert!(capabilities.can_cache);
        assert!(capabilities.can_measure_output);
    }

    #[test]
    fn serde_roundtrip() {
        let class = CoverageClass::ObserveOnly;
        let serialized = serde_json::to_string(&class).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();

        assert_eq!(class, deserialized);
    }
}
