//! Thin content-deduplication hook for the ctx_read hot path.

use crate::core::context_kernel::ctx_read_dedup::{self, ReadDedupSummary};

/// Returns an unchanged-content stub when deduplication applies and finds a match.
#[must_use]
pub fn maybe_dedup(path: &str, content: &str, mode: &str) -> Option<String> {
    if !ctx_read_dedup::should_dedup(mode) {
        return None;
    }
    ctx_read_dedup::try_dedup(path, content)
}

/// Invalidates cached content for `path` after a file write.
pub fn on_write(path: &str) {
    ctx_read_dedup::on_file_write(path);
}

/// Returns current ctx_read deduplication statistics.
#[must_use]
pub fn summary() -> ReadDedupSummary {
    ctx_read_dedup::dedup_summary()
}

#[cfg(test)]
mod tests {
    use std::sync::MutexGuard;

    use super::{maybe_dedup, on_write};

    fn isolated() -> MutexGuard<'static, ()> {
        let guard = crate::core::context_kernel::kernel_config::KERNEL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::core::context_kernel::kernel_config::reset_features();
        crate::core::context_kernel::dedup_wiring::reset_dedup();
        guard
    }

    #[test]
    fn dedup_returns_none_for_new_content() {
        let _guard = isolated();
        assert_eq!(maybe_dedup("new.rs", "content", "full"), None);
    }

    #[test]
    fn dedup_returns_stub_for_repeated() {
        let _guard = isolated();
        assert_eq!(maybe_dedup("repeat.rs", "content", "full"), None);
        assert!(maybe_dedup("repeat.rs", "content", "full").is_some());
    }

    #[test]
    fn raw_mode_skips_dedup() {
        let _guard = isolated();
        assert_eq!(maybe_dedup("raw.rs", "content", "full"), None);
        assert_eq!(maybe_dedup("raw.rs", "content", "raw"), None);
    }

    #[test]
    fn write_invalidates_cache() {
        let _guard = isolated();
        assert_eq!(maybe_dedup("written.rs", "content", "full"), None);
        assert!(maybe_dedup("written.rs", "content", "full").is_some());
        on_write("written.rs");
        assert_eq!(maybe_dedup("written.rs", "content", "full"), None);
    }
}
