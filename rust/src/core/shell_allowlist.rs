/// Checks if a command is allowed by the shell allowlist.
/// Returns Ok(()) if allowed, Err(message) if blocked.
///
/// When the allowlist is empty, all commands pass (blocklist-only mode).
/// When non-empty, only commands whose base binary matches are allowed.
pub fn check_shell_allowlist(command: &str) -> Result<(), String> {
    check_against_allowlist(command, &effective_allowlist())
}

fn check_against_allowlist(command: &str, allowlist: &[String]) -> Result<(), String> {
    if allowlist.is_empty() {
        return Ok(());
    }
    let base = extract_base_command(command);
    if allowlist.iter().any(|a| a == &base) {
        Ok(())
    } else {
        Err(format!(
            "[SHELL ALLOWLIST] Command '{}' (base: '{}') is not in the allowed commands list. Allowed: {}",
            command, base, allowlist.join(", ")
        ))
    }
}

fn effective_allowlist() -> Vec<String> {
    if let Ok(env_val) = std::env::var("LEAN_CTX_SHELL_ALLOWLIST") {
        return env_val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    crate::core::config::Config::load().shell_allowlist
}

fn extract_base_command(command: &str) -> String {
    let trimmed = command.trim();
    // Split on && | || ; and take the first command
    let first = trimmed
        .split(&['&', '|', ';'][..])
        .next()
        .unwrap_or(trimmed)
        .trim();
    // Skip env var assignments (KEY=VALUE patterns)
    let parts: Vec<&str> = first.split_whitespace().collect();
    let cmd_part = parts
        .iter()
        .find(|p| !p.contains('='))
        .copied()
        .unwrap_or(parts.first().copied().unwrap_or(""));
    // Strip path: /usr/bin/git -> git
    cmd_part.rsplit('/').next().unwrap_or(cmd_part).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_simple_command() {
        assert_eq!(extract_base_command("git status"), "git");
    }

    #[test]
    fn extract_with_path() {
        assert_eq!(extract_base_command("/usr/bin/git log"), "git");
    }

    #[test]
    fn extract_with_env_assignment() {
        assert_eq!(extract_base_command("LANG=en_US git log"), "git");
    }

    #[test]
    fn extract_chained_commands() {
        assert_eq!(extract_base_command("cd /tmp && ls -la"), "cd");
    }

    #[test]
    fn extract_piped_command() {
        assert_eq!(extract_base_command("grep foo | wc -l"), "grep");
    }

    #[test]
    fn extract_semicolon_chain() {
        assert_eq!(extract_base_command("echo hello; rm -rf /"), "echo");
    }

    #[test]
    fn extract_empty_command() {
        assert_eq!(extract_base_command(""), "");
    }

    #[test]
    fn extract_whitespace_only() {
        assert_eq!(extract_base_command("   "), "");
    }

    #[test]
    fn extract_multiple_env_vars() {
        assert_eq!(extract_base_command("FOO=bar BAZ=qux cargo test"), "cargo");
    }

    fn allow(cmds: &[&str]) -> Vec<String> {
        cmds.iter().map(std::string::ToString::to_string).collect()
    }

    #[test]
    fn allowlist_empty_always_passes() {
        assert!(check_against_allowlist("anything", &[]).is_ok());
    }

    #[test]
    fn allowlist_blocks_unlisted() {
        let list = allow(&["git", "cargo"]);
        let result = check_against_allowlist("npm install", &list);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("npm"));
        assert!(msg.contains("SHELL ALLOWLIST"));
    }

    #[test]
    fn allowlist_allows_listed() {
        let list = allow(&["git", "cargo", "npm"]);
        assert!(check_against_allowlist("git status", &list).is_ok());
        assert!(check_against_allowlist("cargo test --release", &list).is_ok());
        assert!(check_against_allowlist("npm run build", &list).is_ok());
    }

    #[test]
    fn allowlist_allows_full_path() {
        let list = allow(&["git"]);
        assert!(check_against_allowlist("/usr/bin/git status", &list).is_ok());
    }

    #[test]
    fn allowlist_allows_with_env_prefix() {
        let list = allow(&["git"]);
        assert!(check_against_allowlist("LANG=C git log", &list).is_ok());
    }

    #[test]
    fn allowlist_blocks_similar_names() {
        let list = allow(&["git"]);
        assert!(check_against_allowlist("gitk --all", &list).is_err());
    }
}
