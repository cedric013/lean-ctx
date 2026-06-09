//! Fingerprint-keyed memoization for expensive dashboard graph analyses.
//!
//! The architecture report recomputes communities, betweenness centrality,
//! import cycles, god-nodes and "surprising connections" on every request —
//! all pure functions of the current graph. We memoize the rendered JSON keyed
//! by a cheap, change-sensitive fingerprint (file count + edge count + last
//! scan). Any rescan bumps `last_scan` and edits change the counts, so the cache
//! invalidates automatically and never serves stale analysis.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type CacheMap = HashMap<String, (String, String)>;

fn store() -> &'static Mutex<CacheMap> {
    static CACHE: OnceLock<Mutex<CacheMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A cheap, change-sensitive fingerprint of the current graph.
pub(super) fn fingerprint(gp: &crate::core::graph_provider::GraphProvider) -> String {
    format!(
        "{}:{}:{}",
        gp.file_count(),
        gp.edge_count().unwrap_or(0),
        gp.last_scan()
    )
}

/// Return the cached JSON for `key` when its stored fingerprint still matches,
/// otherwise run `compute`, store the result under `(key, fingerprint)`, and
/// return it.
pub(super) fn cached_or_compute(
    key: &str,
    fingerprint: &str,
    compute: impl FnOnce() -> String,
) -> String {
    if let Ok(map) = store().lock() {
        if let Some((fp, json)) = map.get(key) {
            if fp == fingerprint {
                return json.clone();
            }
        }
    }
    let json = compute();
    if let Ok(mut map) = store().lock() {
        map.insert(key.to_string(), (fingerprint.to_string(), json.clone()));
    }
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_skips_recompute_until_fingerprint_changes() {
        let key = "test:route";
        let mut calls = 0;
        let mut run = |fp: &str| {
            cached_or_compute(key, fp, || {
                calls += 1;
                format!("payload-{calls}")
            })
        };

        assert_eq!(run("fp-1"), "payload-1"); // miss → compute
        assert_eq!(run("fp-1"), "payload-1"); // hit → cached, no recompute
        assert_eq!(run("fp-2"), "payload-2"); // fingerprint changed → recompute
        assert_eq!(run("fp-2"), "payload-2"); // hit again
    }
}
