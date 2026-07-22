//! Global content-deduplication wiring for context delivery hot paths.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::context_dedup::{ContextDedup, DedupResult, format_unchanged_stub};

static DEDUP: OnceLock<Mutex<ContextDedup>> = OnceLock::new();
static SEEN_PATHS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static STATS: OnceLock<Mutex<DedupStats>> = OnceLock::new();
/// Action to take based on a content deduplication check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupAction {
    /// Content is new and should be delivered in full.
    DeliverFull,
    /// Content is unchanged and can be replaced by a compact reference.
    DeliverStub {
        /// Compact reference to the content already present in context.
        stub: String,
    },
    /// Content changed and should be delivered in full.
    DeliverModified,
}

/// Cumulative content deduplication statistics for this process.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DedupStats {
    /// Number of enabled deduplication checks.
    pub total_checks: usize,
    /// Number of checks that found unchanged content.
    pub cache_hits: usize,
    /// Number of checks that required full delivery.
    pub cache_misses: usize,
    /// Estimated tokens avoided by unchanged-content stubs.
    pub tokens_saved: usize,
    /// Fraction of enabled checks that found unchanged content.
    pub hit_rate: f64,
}

/// Checks whether `content` changed since its last delivery at `path`.
#[must_use]
pub fn check_content(path: &str, content: &str) -> DedupAction {
    check_content_enabled(
        super::kernel_config::features().content_dedup,
        path,
        content,
    )
}

/// Returns a snapshot of cumulative content deduplication statistics.
#[must_use]
pub fn dedup_stats() -> DedupStats {
    let mut snapshot = *lock(stats());
    snapshot.hit_rate = if snapshot.total_checks == 0 {
        0.0
    } else {
        snapshot.cache_hits as f64 / snapshot.total_checks as f64
    };
    snapshot
}

/// Applies content deduplication, returning either full content or a stub.
#[must_use]
pub fn apply_dedup(path: &str, content: &str) -> String {
    apply_dedup_enabled(
        super::kernel_config::features().content_dedup,
        path,
        content,
    )
}

fn apply_dedup_enabled(enabled: bool, path: &str, content: &str) -> String {
    match check_content_enabled(enabled, path, content) {
        DedupAction::DeliverStub { stub } => stub,
        DedupAction::DeliverFull | DedupAction::DeliverModified => content.to_owned(),
    }
}

/// Invalidates cached content for `path` after a write or external change.
pub fn invalidate(path: &str) {
    lock(dedup()).invalidate(path);
    lock(seen_paths()).remove(path);
}

/// Clears cached content and all cumulative deduplication statistics.
pub fn reset_dedup() {
    lock(dedup()).clear();
    lock(seen_paths()).clear();
    *lock(stats()) = DedupStats::default();
}

fn check_content_enabled(enabled: bool, path: &str, content: &str) -> DedupAction {
    if !enabled {
        return DedupAction::DeliverFull;
    }

    let result = lock(dedup()).check_and_record(path, content);
    match result {
        DedupResult::Unchanged { hash, saved_tokens } => {
            record_check(true, saved_tokens);
            DedupAction::DeliverStub {
                stub: format_unchanged_stub(path, &hash),
            }
        }
        DedupResult::Fresh => {
            let modified = !lock(seen_paths()).insert(path.to_owned());
            record_check(false, 0);
            if modified {
                DedupAction::DeliverModified
            } else {
                DedupAction::DeliverFull
            }
        }
    }
}
fn record_check(hit: bool, saved_tokens: usize) {
    let mut current = lock(stats());
    current.total_checks += 1;
    if hit {
        current.cache_hits += 1;
        current.tokens_saved += saved_tokens;
    } else {
        current.cache_misses += 1;
    }
}

fn dedup() -> &'static Mutex<ContextDedup> {
    DEDUP.get_or_init(|| {
        Mutex::new(ContextDedup::new(
            super::kernel_config::features().dedup_capacity,
        ))
    })
}

fn seen_paths() -> &'static Mutex<HashSet<String>> {
    SEEN_PATHS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn stats() -> &'static Mutex<DedupStats> {
    STATS.get_or_init(|| Mutex::new(DedupStats::default()))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::{
        DedupAction, apply_dedup_enabled, check_content_enabled, dedup_stats, invalidate,
        reset_dedup,
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn isolated() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_dedup();
        guard
    }

    #[test]
    fn new_content_delivers_full() {
        let _guard = isolated();
        assert_eq!(
            check_content_enabled(true, "src/lib.rs", "content"),
            DedupAction::DeliverFull
        );
    }

    #[test]
    fn repeated_content_delivers_stub() {
        let _guard = isolated();
        check_content_enabled(true, "src/lib.rs", "content");
        assert!(matches!(
            check_content_enabled(true, "src/lib.rs", "content"),
            DedupAction::DeliverStub { .. }
        ));
    }

    #[test]
    fn modified_content_delivers_modified() {
        let _guard = isolated();
        check_content_enabled(true, "src/lib.rs", "before");
        assert_eq!(
            check_content_enabled(true, "src/lib.rs", "after"),
            DedupAction::DeliverModified
        );
    }

    #[test]
    fn disabled_always_full() {
        let _guard = isolated();
        assert_eq!(
            check_content_enabled(false, "src/lib.rs", "content"),
            DedupAction::DeliverFull
        );
        assert_eq!(
            check_content_enabled(false, "src/lib.rs", "content"),
            DedupAction::DeliverFull
        );
        assert_eq!(dedup_stats().total_checks, 0);
    }

    #[test]
    fn apply_dedup_returns_stub() {
        let _guard = isolated();
        assert_eq!(
            apply_dedup_enabled(true, "src/lib.rs", "content"),
            "content"
        );
        let stub = apply_dedup_enabled(true, "src/lib.rs", "content");
        assert!(stub.contains("src/lib.rs unchanged"));
    }

    #[test]
    fn invalidate_forces_full() {
        let _guard = isolated();
        check_content_enabled(true, "src/lib.rs", "content");
        invalidate("src/lib.rs");
        assert_eq!(
            check_content_enabled(true, "src/lib.rs", "content"),
            DedupAction::DeliverFull
        );
    }

    #[test]
    fn stats_track_hits() {
        let _guard = isolated();
        check_content_enabled(true, "a", "one");
        check_content_enabled(true, "a", "one");
        check_content_enabled(true, "a", "one");
        check_content_enabled(true, "b", "two");
        check_content_enabled(true, "b", "two");

        let stats = dedup_stats();
        assert_eq!(stats.total_checks, 5);
        assert_eq!(stats.cache_hits, 3);
        assert_eq!(stats.cache_misses, 2);
        assert!((stats.hit_rate - 0.6).abs() < f64::EPSILON);
    }
}
