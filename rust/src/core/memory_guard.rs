//! Process-level RAM guardian with adaptive eviction and hard OOM protection.
//!
//! Monitors RSS via platform-specific APIs and triggers tiered cache eviction
//! when memory usage exceeds configurable thresholds (default: 5% of system RAM).
//! At critical levels (>3x limit), performs emergency shutdown to prevent OS OOM kill.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

static PEAK_RSS: AtomicU64 = AtomicU64::new(0);
static GUARD_RUNNING: AtomicBool = AtomicBool::new(false);
static ABORT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Current process RSS in bytes, or `None` if unavailable.
pub fn get_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        linux_rss()
    }
    #[cfg(target_os = "macos")]
    {
        macos_rss()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Total physical RAM in bytes, or `None` if unavailable.
pub fn get_system_ram_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        linux_memtotal()
    }
    #[cfg(target_os = "macos")]
    {
        macos_memsize()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Returns the RSS limit in bytes based on `max_ram_percent` config.
pub fn rss_limit_bytes() -> Option<u64> {
    let sys_ram = get_system_ram_bytes()?;
    let cfg = super::config::Config::load();
    let pct = super::config::MemoryGuardConfig::effective(&cfg).max_ram_percent;
    Some(sys_ram / 100 * u64::from(pct))
}

/// Recorded peak RSS since process start.
pub fn peak_rss_bytes() -> u64 {
    PEAK_RSS.load(Ordering::Relaxed)
}

/// Snapshot of current memory state for diagnostics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MemorySnapshot {
    pub rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub system_ram_bytes: u64,
    pub rss_limit_bytes: u64,
    pub rss_percent: f64,
    pub pressure_level: PressureLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PressureLevel {
    Normal,
    Soft,
    Medium,
    Hard,
    Critical,
}

impl MemorySnapshot {
    pub fn capture() -> Option<Self> {
        let rss = get_rss_bytes()?;
        let sys = get_system_ram_bytes()?;
        let limit = rss_limit_bytes()?;
        let pct = if sys > 0 {
            (rss as f64 / sys as f64) * 100.0
        } else {
            0.0
        };

        PEAK_RSS.fetch_max(rss, Ordering::Relaxed);

        let cfg = super::config::Config::load();
        let guard_cfg = super::config::MemoryGuardConfig::effective(&cfg);
        let base = f64::from(guard_cfg.max_ram_percent);

        let level = if pct > base * 3.0 {
            PressureLevel::Critical
        } else if pct > base * 2.0 {
            PressureLevel::Hard
        } else if pct > base * 1.4 {
            PressureLevel::Medium
        } else if pct > base {
            PressureLevel::Soft
        } else {
            PressureLevel::Normal
        };

        Some(Self {
            rss_bytes: rss,
            peak_rss_bytes: PEAK_RSS.load(Ordering::Relaxed),
            system_ram_bytes: sys,
            rss_limit_bytes: limit,
            rss_percent: pct,
            pressure_level: level,
        })
    }
}

/// Force-purge all jemalloc arenas to return memory to the OS.
pub fn jemalloc_purge() {
    #[cfg(all(feature = "jemalloc", not(windows)))]
    {
        use tikv_jemalloc_ctl::raw;
        let purge_mib = b"arena.4096.purge\0";
        unsafe {
            let _ = raw::write(purge_mib, 0u64);
        }
    }
}

/// Returns `true` if the guardian has requested background tasks to abort.
pub fn abort_requested() -> bool {
    ABORT_REQUESTED.load(Ordering::Relaxed)
}

/// Quick, non-allocating memory pressure check for hot loops (scanners, indexers).
/// Returns `true` if memory is at or above Soft pressure and work should be paused/stopped.
pub fn is_under_pressure() -> bool {
    let Some(snap) = MemorySnapshot::capture() else {
        return false;
    };
    snap.pressure_level >= PressureLevel::Soft
}

/// Start the background memory guardian task (idempotent).
/// Polls every 3s (normal) or 1s (under pressure). At Critical level, performs
/// emergency shutdown to prevent OS OOM kill.
pub fn start_guard(eviction_callback: Arc<dyn Fn(PressureLevel) + Send + Sync>) {
    if GUARD_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("memory-guard".into())
        .spawn(move || {
            let mut poll_secs = 3u64;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(poll_secs));
                let Some(snap) = MemorySnapshot::capture() else {
                    continue;
                };

                if snap.pressure_level == PressureLevel::Critical {
                    tracing::error!(
                        "[memory_guard] CRITICAL: RSS={:.0}MB ({:.1}% of {:.0}GB) — \
                         emergency shutdown to prevent OS OOM kill",
                        snap.rss_bytes as f64 / 1_048_576.0,
                        snap.rss_percent,
                        snap.system_ram_bytes as f64 / 1_073_741_824.0,
                    );
                    ABORT_REQUESTED.store(true, Ordering::SeqCst);
                    (eviction_callback)(PressureLevel::Critical);
                    jemalloc_purge();

                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if let Some(recheck) = MemorySnapshot::capture() {
                        if recheck.pressure_level >= PressureLevel::Hard {
                            tracing::error!(
                                "[memory_guard] still at {:?} after emergency eviction — \
                                 exiting process (RSS={:.0}MB)",
                                recheck.pressure_level,
                                recheck.rss_bytes as f64 / 1_048_576.0,
                            );
                            std::process::exit(137);
                        }
                    }
                }

                if snap.pressure_level >= PressureLevel::Soft {
                    poll_secs = 1;
                    ABORT_REQUESTED
                        .store(snap.pressure_level >= PressureLevel::Hard, Ordering::SeqCst);
                    tracing::warn!(
                        "[memory_guard] pressure={:?} RSS={:.0}MB limit={:.0}MB ({:.1}% of {:.0}GB)",
                        snap.pressure_level,
                        snap.rss_bytes as f64 / 1_048_576.0,
                        snap.rss_limit_bytes as f64 / 1_048_576.0,
                        snap.rss_percent,
                        snap.system_ram_bytes as f64 / 1_073_741_824.0,
                    );
                    (eviction_callback)(snap.pressure_level);

                    if snap.pressure_level >= PressureLevel::Hard {
                        jemalloc_purge();
                    }
                } else {
                    poll_secs = 3;
                    if ABORT_REQUESTED.load(Ordering::Relaxed) {
                        ABORT_REQUESTED.store(false, Ordering::SeqCst);
                        tracing::info!("[memory_guard] pressure normalized, clearing abort flag");
                    }
                }
            }
        })
        .ok();
}

/// Force immediate purge of all caches and jemalloc arenas.
pub fn force_purge() {
    jemalloc_purge();
    tracing::info!("[memory_guard] force_purge completed");
}

// --- Platform-specific implementations ---

#[cfg(target_os = "linux")]
fn linux_rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(val) = line.strip_prefix("VmRSS:") {
            let kb: u64 = val.trim().trim_end_matches(" kB").trim().parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_memtotal() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in info.lines() {
        if let Some(val) = line.strip_prefix("MemTotal:") {
            let kb: u64 = val.trim().trim_end_matches(" kB").trim().parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(target_os = "macos")]
#[allow(deprecated, clippy::borrow_as_ptr, clippy::ptr_as_ptr)]
fn macos_rss() -> Option<u64> {
    use std::mem;
    let mut info: libc::mach_task_basic_info_data_t = unsafe { mem::zeroed() };
    let mut count = (mem::size_of::<libc::mach_task_basic_info_data_t>()
        / mem::size_of::<libc::natural_t>()) as libc::mach_msg_type_number_t;
    let kr = unsafe {
        libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            std::ptr::from_mut(&mut info).cast::<i32>(),
            std::ptr::from_mut(&mut count),
        )
    };
    if kr == libc::KERN_SUCCESS {
        Some(info.resident_size)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
#[allow(clippy::borrow_as_ptr, clippy::ptr_as_ptr)]
fn macos_memsize() -> Option<u64> {
    use std::mem;
    let mut memsize: u64 = 0;
    let mut len = mem::size_of::<u64>();
    let name = b"hw.memsize\0";
    let ret = unsafe {
        libc::sysctlbyname(
            name.as_ptr().cast(),
            std::ptr::from_mut(&mut memsize).cast::<libc::c_void>(),
            std::ptr::from_mut(&mut len),
            std::ptr::null_mut(),
            0,
        )
    };
    if ret == 0 {
        Some(memsize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_returns_some_on_supported_os() {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            let rss = get_rss_bytes();
            assert!(rss.is_some(), "RSS should be readable");
            assert!(rss.unwrap() > 0, "RSS should be > 0");
        }
    }

    #[test]
    fn system_ram_returns_some_on_supported_os() {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            let ram = get_system_ram_bytes();
            assert!(ram.is_some(), "System RAM should be readable");
            assert!(ram.unwrap() > 1_000_000, "System RAM should be > 1MB");
        }
    }

    #[test]
    fn snapshot_captures_correctly() {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            let snap = MemorySnapshot::capture();
            assert!(snap.is_some());
            let s = snap.unwrap();
            assert!(s.rss_bytes > 0);
            assert!(s.system_ram_bytes > s.rss_bytes);
            assert!(s.rss_percent > 0.0 && s.rss_percent < 100.0);
        }
    }

    #[test]
    fn peak_rss_tracks_maximum() {
        PEAK_RSS.store(0, Ordering::Relaxed);
        PEAK_RSS.fetch_max(100, Ordering::Relaxed);
        PEAK_RSS.fetch_max(50, Ordering::Relaxed);
        assert_eq!(PEAK_RSS.load(Ordering::Relaxed), 100);
    }
}
