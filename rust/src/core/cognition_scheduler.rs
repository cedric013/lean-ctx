//! Opportunistic, time-gated trigger for the background Cognition Loop.
//!
//! The MCP server is request-driven, so instead of holding a wall-clock timer
//! thread (which would tick with no project context and complicate shutdown), we
//! piggyback on tool activity: after dispatch, [`maybe_run`] fires the loop at
//! most once per `autonomy.cognition_loop_interval_secs`, in a single-flight
//! background thread. When the agent is idle no maintenance is needed anyway.
//!
//! This is what turns the eight-step [`crate::core::cognition_loop`] (seed
//! promote → repair → synthesis → contradiction → hebbian → decay → compact)
//! from a manually-invoked tool into genuinely self-managing memory.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Unix seconds of the last loop start (`0` = never run this process).
static LAST_RUN_SECS: AtomicU64 = AtomicU64::new(0);
/// Single-flight guard: never spawn a second loop while one is in flight.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Floor for the configured interval — guards against a pathological `0`/tiny
/// value turning every dispatch into a consolidation run.
const MIN_INTERVAL_SECS: u64 = 60;

/// Resets [`RUNNING`] on drop so a panicking loop can never wedge the guard.
struct RunningGuard;
impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Pure due-check, factored out for deterministic testing: the loop is due when
/// it has never run (`last == 0`) or `interval` seconds have elapsed since then.
fn is_due(now: u64, last: u64, interval: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= interval
}

/// Fire the cognition loop in the background when enabled and the configured
/// interval has elapsed. Non-blocking, single-flight, and cheap on the hot path
/// (one config read + two atomic loads when not due).
pub fn maybe_run(project_root: &str) {
    let cfg = crate::core::config::Config::load();
    if !cfg.autonomy.cognition_loop_enabled {
        return;
    }
    let interval = cfg
        .autonomy
        .cognition_loop_interval_secs
        .max(MIN_INTERVAL_SECS);
    let now = now_secs();
    if !is_due(now, LAST_RUN_SECS.load(Ordering::Relaxed), interval) {
        return;
    }
    // Claim the slot before spawning so concurrent dispatches never double-fire.
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    LAST_RUN_SECS.store(now, Ordering::Relaxed);

    let root = project_root.to_string();
    std::thread::spawn(move || {
        let _guard = RunningGuard;
        let report = crate::core::cognition_loop::run_cognition_loop(&root, 8);
        tracing::debug!(target: "cognition", "background cognition loop: {report}");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_run_is_always_due() {
        assert!(is_due(1_000, 0, 3600));
        assert!(is_due(0, 0, 3600));
    }

    #[test]
    fn due_only_after_interval_elapses() {
        let last = 10_000;
        assert!(!is_due(last + 59, last, 60));
        assert!(is_due(last + 60, last, 60));
        assert!(is_due(last + 7_200, last, 3600));
    }

    #[test]
    fn clock_skew_backwards_is_not_due() {
        // A backwards clock jump must not retrigger (saturating_sub → 0).
        assert!(!is_due(9_000, 10_000, 3600));
    }
}
