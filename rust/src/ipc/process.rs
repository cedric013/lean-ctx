use anyhow::Result;

/// Check whether a process with the given PID is still running.
pub fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let wait = WaitForSingleObject(handle, 0);
            if wait == WAIT_TIMEOUT {
                CloseHandle(handle);
                return true;
            }
            let mut exit_code: u32 = 0;
            GetExitCodeProcess(handle, &mut exit_code);
            CloseHandle(handle);
            exit_code == STILL_ACTIVE as u32
        }
    }
}

/// Ask a process to terminate gracefully (SIGTERM on Unix, nothing on Windows
/// since we prefer HTTP shutdown; the caller should have already tried that).
pub fn terminate_gracefully(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if ret != 0 {
            anyhow::bail!(
                "Failed to send SIGTERM to PID {pid}: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        force_kill(pid)
    }
}

/// Unconditionally kill a process.
pub fn force_kill(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if ret != 0 {
            anyhow::bail!(
                "Failed to send SIGKILL to PID {pid}: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                anyhow::bail!(
                    "Failed to open PID {pid} for termination: {}",
                    std::io::Error::last_os_error()
                );
            }
            let ok = TerminateProcess(handle, 1);
            CloseHandle(handle);
            if ok == 0 {
                anyhow::bail!(
                    "Failed to terminate PID {pid}: {}",
                    std::io::Error::last_os_error()
                );
            }
            Ok(())
        }
    }
}

/// Find all PIDs of processes whose executable name matches `name`.
/// Excludes the current process.
pub fn find_pids_by_name(name: &str) -> Vec<u32> {
    let my_pid = std::process::id();
    let mut pids = Vec::new();

    #[cfg(unix)]
    {
        // Exact name match first
        if let Ok(output) = std::process::Command::new("pgrep")
            .arg("-x")
            .arg(name)
            .output()
        {
            collect_pids(&output.stdout, my_pid, &mut pids);
        }

        // Also find processes where the full command line contains the binary path
        // (catches processes launched via absolute path, e.g. /Users/x/.local/bin/lean-ctx)
        if let Ok(output) = std::process::Command::new("pgrep")
            .arg("-f")
            .arg(format!("/{name}(\\s|$)"))
            .output()
        {
            collect_pids(&output.stdout, my_pid, &mut pids);
        }

        pids.sort_unstable();
        pids.dedup();
    }

    #[cfg(windows)]
    {
        if let Ok(output) = std::process::Command::new("tasklist")
            .args([
                "/FI",
                &format!("IMAGENAME eq {name}.exe"),
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    let pid_str = parts[1].trim().trim_matches('"');
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        if pid != my_pid {
                            pids.push(pid);
                        }
                    }
                }
            }
        }
    }

    pids
}

#[cfg(unix)]
fn collect_pids(stdout: &[u8], exclude_pid: u32, out: &mut Vec<u32>) {
    let text = String::from_utf8_lossy(stdout);
    for line in text.lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            if pid != exclude_pid {
                out.push(pid);
            }
        }
    }
}

/// Returns PIDs that are NOT MCP stdio servers (safe to kill during `lean-ctx stop`).
/// MCP servers are child processes of the IDE and must not be killed — the IDE
/// will immediately respawn them, causing a kill loop that requires a reboot.
pub fn find_killable_pids(name: &str) -> Vec<u32> {
    let all = find_pids_by_name(name);
    let mcp_pids = find_mcp_server_pids(name);
    all.into_iter().filter(|p| !mcp_pids.contains(p)).collect()
}

#[cfg(unix)]
fn find_mcp_server_pids(name: &str) -> Vec<u32> {
    find_pids_by_name(name)
        .into_iter()
        .filter(|&pid| is_mcp_stdio_process(pid))
        .collect()
}

#[cfg(not(unix))]
fn find_mcp_server_pids(_name: &str) -> Vec<u32> {
    Vec::new()
}

#[cfg(unix)]
fn is_mcp_stdio_process(pid: u32) -> bool {
    if let Ok(output) = std::process::Command::new("ps")
        .args(["-o", "ppid=,command=", "-p", &pid.to_string()])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        let t = text.trim();
        if t.contains("Cursor") || t.contains("cursor") || t.contains("code") {
            return true;
        }
        let parts: Vec<&str> = t.split_whitespace().collect();
        if let Some(ppid_str) = parts.first() {
            if let Ok(ppid) = ppid_str.parse::<u32>() {
                if let Ok(pp_out) = std::process::Command::new("ps")
                    .args(["-o", "command=", "-p", &ppid.to_string()])
                    .output()
                {
                    let pp_cmd = String::from_utf8_lossy(&pp_out.stdout);
                    if pp_cmd.contains("Cursor")
                        || pp_cmd.contains("cursor")
                        || pp_cmd.contains("code")
                    {
                        return true;
                    }
                }
            }
        }
        let cmd_part = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
        // MCP stdio servers: bare `lean-ctx` with no subcommand (or just `mcp`)
        if cmd_part.ends_with("/lean-ctx") || cmd_part == "lean-ctx" {
            if !cmd_part.contains("proxy")
                && !cmd_part.contains("dashboard")
                && !cmd_part.contains("daemon")
                && !cmd_part.contains("stop")
                && !cmd_part.contains("hook")
            {
                return true;
            }
        }
        // Hook observer/rewriter processes spawned by IDE
        if cmd_part.contains("hook observe")
            || cmd_part.contains("hook rewrite")
            || cmd_part.contains("hook redirect")
        {
            return true;
        }
    }
    false
}

/// Kill non-MCP processes matching `name` (SIGTERM then SIGKILL).
/// Returns count of killed processes.
pub fn kill_all_by_name(name: &str) -> usize {
    let pids = find_killable_pids(name);
    if pids.is_empty() {
        return 0;
    }

    for &pid in &pids {
        let _ = terminate_gracefully(pid);
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    let mut killed = 0;
    for &pid in &pids {
        if is_alive(pid) {
            let _ = force_kill(pid);
        }
        killed += 1;
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        assert!(is_alive(std::process::id()));
    }

    #[test]
    fn bogus_pid_is_not_alive() {
        assert!(!is_alive(u32::MAX - 42));
    }
}
