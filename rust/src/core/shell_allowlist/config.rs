pub(super) fn effective_allowlist() -> Vec<String> {
    // LEAN_CTX_SHELL_ALLOWLIST_OVERRIDE completely replaces the config (for testing)
    if let Ok(ov) = std::env::var("LEAN_CTX_SHELL_ALLOWLIST_OVERRIDE") {
        return ov
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let cfg = crate::core::config::Config::load();
    let mut list = cfg.shell_allowlist;
    // `shell_allowlist_extra` is purely additive (written by `lean-ctx allow <cmd>`),
    // so users can permit a command without nuking the built-in defaults. It only
    // matters in restricted mode — when the base list is empty all commands pass anyway.
    if !list.is_empty() {
        for entry in cfg.shell_allowlist_extra {
            if !entry.is_empty() && !list.contains(&entry) {
                list.push(entry);
            }
        }
    }
    if let Ok(env_val) = std::env::var("LEAN_CTX_SHELL_ALLOWLIST") {
        for entry in env_val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
    }
    list
}

/// Builds the actionable, self-diagnosing message shown when a command's base binary
/// is not in the allowlist. Unlike a bare "not allowed" string, it tells the user
/// (1) the exact additive fix, (2) the real config path the MCP server reads, and
/// (3) — crucially — whether their `config.toml` silently failed to parse (in which
/// case lean-ctx is on defaults, which is the usual reason an allowlist edit "did
/// nothing"). That last signal is otherwise invisible over an MCP/stdio transport.
pub(super) fn allowlist_block_message(base: &str) -> String {
    let cfg_path = crate::core::config::Config::path().map_or_else(
        || "~/.lean-ctx/config.toml".to_string(),
        |p| p.display().to_string(),
    );

    let mut msg = format!(
        "[BLOCKED — DO NOT RETRY] '{base}' is not in the shell allowlist. \
         This is a permanent restriction, not a transient error.\n\
         Fix (additive, keeps the defaults): run  lean-ctx allow {base}\n\
         Config in effect: {cfg_path}\n\
         Or disable the allowlist entirely: set  shell_allowlist = []\n\
         Or turn off all shell gating (you own the risk): set  shell_security = \"off\"  \
         (or env LEAN_CTX_SHELL_SECURITY=off) — compression still applies.\n\
         Do NOT reroute through ctx_execute(language=\"shell\"): both tools enforce the same \
         policy. Allow the command explicitly or change shell_security deliberately."
    );

    if crate::core::config::cloud_infra_commands().contains(&base) {
        msg.push_str(
            "\nNote: cloud/infra CLIs (terraform, kubectl, aws, …) are deliberately \
             excluded from the defaults — they mutate remote infrastructure with \
             ambient credentials. Opting in is a deliberate user decision.",
        );
    }

    if let Some(parse_err) = crate::core::config::last_config_parse_error() {
        msg.push_str(&format!(
            "\n\n⚠ Your config.toml currently FAILS to parse, so lean-ctx is running on the \
             built-in defaults — this is almost certainly why editing the allowlist had no \
             effect. Fix the TOML error below, then retry:\n  {parse_err}\n  File: {cfg_path}"
        ));
    } else if let Some(missing) = crate::core::config::Config::missing_config_path() {
        // The resolved config doesn't exist → lean-ctx is on defaults. An edit
        // made to a config.toml in a different dir (XDG vs legacy ~/.lean-ctx) or
        // under a sandboxed/container HOME is never read — say so over MCP (#540).
        msg.push_str(&format!(
            "\n\n⚠ No config file exists at {} — lean-ctx is running on built-in defaults. \
             If you added the command to a config.toml in a DIFFERENT location (XDG \
             ~/.config/lean-ctx vs legacy ~/.lean-ctx, or your MCP client launches lean-ctx \
             in a sandbox/container with a different HOME), the runtime never reads it. \
             `lean-ctx doctor` prints the path actually in effect; pin it with \
             LEAN_CTX_CONFIG_DIR.",
            missing.display()
        ));
    }

    // A project-local `shell_allowlist`/`shell_allowlist_extra` is silently
    // withheld for an untrusted workspace; surface that here so the edit's
    // no-op reason isn't buried in an MCP-invisible stderr warning (#540).
    if let Some(notice) = crate::core::workspace_trust::untrusted_override_notice() {
        msg.push_str("\n\n⚠ ");
        msg.push_str(&notice);
    }

    msg
}
/// Public accessor: the fully-resolved allowlist actually enforced by the MCP tools
/// (base `shell_allowlist` + additive `shell_allowlist_extra` + env), deduplicated.
/// Empty means blocklist-only mode (all commands pass). Used by `lean-ctx allow`
/// and `lean-ctx doctor` to show users exactly what the runtime sees.
#[must_use]
pub fn effective_allowlist_pub() -> Vec<String> {
    effective_allowlist()
}
