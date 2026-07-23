#[derive(Debug, Clone, Copy, Default)]
pub struct SetupOptions {
    pub non_interactive: bool,
    pub yes: bool,
    pub fix: bool,
    pub json: bool,
    pub no_auto_approve: bool,
    pub skip_proxy: bool,
    pub skip_rules: bool,
    /// Explicitly request rules injection (overrides config).
    pub force_inject_rules: bool,
}
