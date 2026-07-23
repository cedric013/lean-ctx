use std::path::PathBuf;

pub fn claude_config_json_path(home: &std::path::Path) -> PathBuf {
    crate::core::editor_registry::claude_mcp_json_path(home)
}

pub fn claude_config_dir(home: &std::path::Path) -> PathBuf {
    crate::core::editor_registry::claude_state_dir(home)
}
