use std::sync::atomic::{AtomicU64, Ordering};

static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static TOKENS_SAVED_TOTAL: AtomicU64 = AtomicU64::new(0);
static BYTES_COMPRESSED: AtomicU64 = AtomicU64::new(0);

pub fn record_request(tokens_saved: u64, bytes_compressed: u64) {
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    TOKENS_SAVED_TOTAL.fetch_add(tokens_saved, Ordering::Relaxed);
    BYTES_COMPRESSED.fetch_add(bytes_compressed, Ordering::Relaxed);
}

pub struct ProxyMetrics {
    pub requests_total: u64,
    pub tokens_saved_total: u64,
    pub bytes_compressed: u64,
}

pub fn snapshot() -> ProxyMetrics {
    ProxyMetrics {
        requests_total: REQUESTS_TOTAL.load(Ordering::Relaxed),
        tokens_saved_total: TOKENS_SAVED_TOTAL.load(Ordering::Relaxed),
        bytes_compressed: BYTES_COMPRESSED.load(Ordering::Relaxed),
    }
}
