//! Response-token evidence recorded at tool output boundaries.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::kernel_config;

static TOTAL_RESPONSES: AtomicUsize = AtomicUsize::new(0);
static TOTAL_OUTPUT_TOKENS: AtomicUsize = AtomicUsize::new(0);
static CACHED_RESPONSES: AtomicUsize = AtomicUsize::new(0);

/// Cumulative response evidence observed by the Context Kernel.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ResponseSummary {
    /// Number of responses recorded.
    pub total_responses: usize,
    /// Output tokens across all recorded responses.
    pub total_output_tokens: usize,
    /// Number of responses served from cache.
    pub cached_responses: usize,
    /// Fraction of recorded responses served from cache.
    pub cache_hit_rate: f64,
}

/// Records output-token and cache evidence for one tool response.
pub fn record_response(tool_name: &str, output_tokens: usize, was_cached: bool) {
    if !kernel_config::is_enabled() {
        return;
    }
    let _ = tool_name;
    TOTAL_RESPONSES.fetch_add(1, Ordering::Relaxed);
    TOTAL_OUTPUT_TOKENS.fetch_add(output_tokens, Ordering::Relaxed);
    if was_cached {
        CACHED_RESPONSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Returns cumulative response evidence.
#[must_use]
pub fn response_summary() -> ResponseSummary {
    let total_responses = TOTAL_RESPONSES.load(Ordering::Relaxed);
    let cached_responses = CACHED_RESPONSES.load(Ordering::Relaxed);
    ResponseSummary {
        total_responses,
        total_output_tokens: TOTAL_OUTPUT_TOKENS.load(Ordering::Relaxed),
        cached_responses,
        cache_hit_rate: cached_responses as f64 / total_responses.max(1) as f64,
    }
}

/// Clears all response evidence counters.
pub fn reset() {
    TOTAL_RESPONSES.store(0, Ordering::Relaxed);
    TOTAL_OUTPUT_TOKENS.store(0, Ordering::Relaxed);
    CACHED_RESPONSES.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::{record_response, reset, response_summary};
    use crate::core::context_kernel::kernel_config;

    fn isolated() -> std::sync::MutexGuard<'static, ()> {
        let guard = kernel_config::KERNEL_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kernel_config::reset_features();
        reset();
        guard
    }

    #[test]
    fn records_response() {
        let _guard = isolated();
        for _ in 0..3 {
            record_response("ctx_read", 1, false);
        }
        assert_eq!(response_summary().total_responses, 3);
    }

    #[test]
    fn tracks_output_tokens() {
        let _guard = isolated();
        record_response("ctx_read", 13, false);
        record_response("ctx_search", 21, false);
        assert_eq!(response_summary().total_output_tokens, 34);
    }

    #[test]
    fn cache_hit_rate() {
        let _guard = isolated();
        for was_cached in [true, false, true, false] {
            record_response("ctx_read", 1, was_cached);
        }
        assert_eq!(response_summary().cache_hit_rate, 0.5);
    }

    #[test]
    fn disabled_kernel_noop() {
        let _guard = isolated();
        let mut features = kernel_config::features();
        features.enabled = false;
        kernel_config::update_features(features);
        record_response("ctx_read", 10, true);
        let summary = response_summary();
        assert_eq!(summary.total_responses, 0);
        assert_eq!(summary.total_output_tokens, 0);
        assert_eq!(summary.cached_responses, 0);
    }
}
