use std::path::PathBuf;

use super::super::{mcp_server_quiet_mode, resolve_binary_path, write_file};
use super::shared::prepare_project_rules_path;

pub(crate) fn install_cline_rules(global: bool) {
    if global {
        let vscode_mcp = crate::core::editor_registry::vscode_mcp_path();
        if vscode_mcp.as_os_str() != "/nonexistent" {
            install_vscode_mcp_for_cline(&vscode_mcp);
        }
    } else {
        let vscode_dir = PathBuf::from(".vscode");
        let _ = std::fs::create_dir_all(&vscode_dir);
        install_vscode_mcp_for_cline(&vscode_dir.join("mcp.json"));
    }

    let Some(rules_path) = prepare_project_rules_path(global, ".clinerules") else {
        return;
    };

    let binary = resolve_binary_path();
    let rules = format!(
        r"# lean-ctx Shell Optimization
# Prefix all shell commands with `{binary} -c` for 60-90% token savings.

When running shell commands, always prefix them with `{binary} -c` to compress output. For example:
- `{binary} -c git status` instead of `git status`
- `{binary} -c cargo test` instead of `cargo test`
- `{binary} -c ls src/` instead of `ls src/`

Supported commands: git, cargo, npm, pnpm, docker, kubectl, pip, ruff, go, curl, grep, find, ls, aws, helm, and 95+ more.
"
    );

    write_file(&rules_path, &rules);
    if !mcp_server_quiet_mode() {
        eprintln!("Installed .clinerules in current project.");
    }
}

fn install_vscode_mcp_for_cline(mcp_path: &std::path::Path) {
    let binary = resolve_binary_path();
    let entry = serde_json::json!({
        "type": "stdio",
        "command": binary,
        "args": [],
        "env": super::super::mcp_server_env_json()
    });

    crate::hooks::install_named_json_server(
        "Cline/Roo",
        &mcp_path.display().to_string(),
        mcp_path,
        "servers",
        entry,
    );
}
