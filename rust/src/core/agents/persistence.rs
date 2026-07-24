use super::AgentRegistry;
use std::path::PathBuf;

pub(super) fn agents_dir() -> Result<PathBuf, String> {
    let dir = crate::core::data_dir::lean_ctx_data_dir()?;
    Ok(dir.join("agents"))
}

pub(super) fn mutate_persistent<T>(
    mutate: impl FnOnce(&mut AgentRegistry) -> T,
) -> Result<T, String> {
    let dir = agents_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _lock = FileLock::acquire(&dir.join("registry.lock"))?;
    let path = dir.join("registry.json");
    let mut registry = std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    let result = mutate(&mut registry);
    let json = serde_json::to_string_pretty(&registry).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(result)
}

pub(super) fn generate_short_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

/// #576 already fixed this exact hardcoded-`true` anti-pattern for
/// `daemon::is_daemon_running` by delegating to `ipc::process::is_alive`
/// (which has a real Windows `OpenProcess` check); this duplicate copy was
/// missed, so on non-unix targets `cleanup_stale` could never flip a dead
/// MCP session's entry to `Finished`, leaving `registry.json` accumulating
/// stale `Active` entries forever — the root cause of the "N active agents"
/// dashboard bug on Windows.
pub fn is_process_alive(pid: u32) -> bool {
    crate::ipc::process::is_alive(pid)
}

pub(crate) struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub(crate) fn acquire(path: &std::path::Path) -> Result<Self, String> {
        for _ in 0..50 {
            if std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .is_ok()
            {
                return Ok(Self {
                    path: path.to_path_buf(),
                });
            }
            if let Ok(metadata) = std::fs::metadata(path)
                && let Ok(modified) = metadata.modified()
                && modified.elapsed().unwrap_or_default().as_secs() > 5
            {
                let _ = std::fs::remove_file(path);
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err("Could not acquire lock after 5 seconds".to_string())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
