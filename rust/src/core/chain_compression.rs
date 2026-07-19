//! Chain Compression — context deduplication between agent hops (P11 / DIM 4).
//!
//! When context flows through a chain of agents, each hop typically carries
//! overlapping references. This module identifies shared context and produces
//! minimal deltas, ensuring only novel information crosses agent boundaries.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const CHAIN_COMPRESSION_SCHEMA_VERSION: u16 = 1;
const MAX_CHAIN_HISTORY: usize = 64;

/// A content-addressed context item tracked across hops.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChainContextItem {
    pub content_ref: String,
    pub freshness_ref: String,
    pub hop_introduced: u16,
    pub last_referenced_hop: u16,
}

/// Delta produced when forwarding context to the next hop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChainDelta {
    pub schema_version: u16,
    pub chain_id: String,
    pub from_hop: u16,
    pub to_hop: u16,
    pub added_refs: Vec<String>,
    pub removed_refs: Vec<String>,
    pub unchanged_count: usize,
    pub total_refs_at_target: usize,
    pub compression_ratio: f64,
}

/// Tracks context across hops in a single chain, computes minimal deltas.
pub struct ChainCompressionTracker {
    chain_id: String,
    items_by_hop: BTreeMap<u16, BTreeSet<String>>,
    all_items: BTreeMap<String, ChainContextItem>,
    current_hop: u16,
}

impl ChainCompressionTracker {
    #[must_use]
    pub fn new(chain_id: String) -> Self {
        Self {
            chain_id,
            items_by_hop: BTreeMap::new(),
            all_items: BTreeMap::new(),
            current_hop: 0,
        }
    }

    /// Register context refs at a given hop.
    pub fn register_hop(
        &mut self,
        hop: u16,
        content_refs: Vec<String>,
        freshness_refs: Vec<String>,
    ) -> Result<(), ChainCompressionError> {
        if content_refs.len() != freshness_refs.len() {
            return Err(ChainCompressionError::MismatchedLengths);
        }
        if self.items_by_hop.len() >= MAX_CHAIN_HISTORY {
            let oldest = *self.items_by_hop.keys().next().unwrap_or(&0);
            self.items_by_hop.remove(&oldest);
        }
        let mut hop_refs = BTreeSet::new();
        for (content_ref, freshness_ref) in content_refs.into_iter().zip(freshness_refs) {
            hop_refs.insert(content_ref.clone());
            self.all_items
                .entry(content_ref.clone())
                .and_modify(|item| {
                    item.last_referenced_hop = hop;
                    item.freshness_ref = freshness_ref.clone();
                })
                .or_insert(ChainContextItem {
                    content_ref,
                    freshness_ref,
                    hop_introduced: hop,
                    last_referenced_hop: hop,
                });
        }
        self.items_by_hop.insert(hop, hop_refs);
        self.current_hop = self.current_hop.max(hop);
        Ok(())
    }

    /// Compute the delta between two hops.
    pub fn compute_delta(
        &self,
        from_hop: u16,
        to_hop: u16,
    ) -> Result<ChainDelta, ChainCompressionError> {
        let from_refs = self
            .items_by_hop
            .get(&from_hop)
            .ok_or(ChainCompressionError::HopNotFound(from_hop))?;
        let to_refs = self
            .items_by_hop
            .get(&to_hop)
            .ok_or(ChainCompressionError::HopNotFound(to_hop))?;

        let added: Vec<String> = to_refs.difference(from_refs).cloned().collect();
        let removed: Vec<String> = from_refs.difference(to_refs).cloned().collect();
        let unchanged_count = to_refs.intersection(from_refs).count();
        let total_refs_at_target = to_refs.len();

        let full_transfer_size = total_refs_at_target.max(1);
        let delta_size = (added.len() + removed.len()).max(1);
        let compression_ratio = 1.0 - (delta_size as f64 / full_transfer_size as f64);

        Ok(ChainDelta {
            schema_version: CHAIN_COMPRESSION_SCHEMA_VERSION,
            chain_id: self.chain_id.clone(),
            from_hop,
            to_hop,
            added_refs: added,
            removed_refs: removed,
            unchanged_count,
            total_refs_at_target,
            compression_ratio: compression_ratio.max(0.0),
        })
    }

    /// Compute what a new hop needs vs the parent hop (forward delta).
    pub fn forward_delta(
        &self,
        parent_hop: u16,
        child_refs: &[String],
    ) -> Result<ChainDelta, ChainCompressionError> {
        let parent_refs = self
            .items_by_hop
            .get(&parent_hop)
            .ok_or(ChainCompressionError::HopNotFound(parent_hop))?;
        let child_set: BTreeSet<&String> = child_refs.iter().collect();
        let parent_set: BTreeSet<&String> = parent_refs.iter().collect();

        let added: Vec<String> = child_set
            .difference(&parent_set)
            .map(|s| (*s).clone())
            .collect();
        let removed: Vec<String> = parent_set
            .difference(&child_set)
            .map(|s| (*s).clone())
            .collect();
        let unchanged_count = child_set.intersection(&parent_set).count();
        let total = child_refs.len();

        let full_size = total.max(1);
        let delta_size = (added.len() + removed.len()).max(1);
        let ratio = 1.0 - (delta_size as f64 / full_size as f64);

        Ok(ChainDelta {
            schema_version: CHAIN_COMPRESSION_SCHEMA_VERSION,
            chain_id: self.chain_id.clone(),
            from_hop: parent_hop,
            to_hop: self.current_hop + 1,
            added_refs: added,
            removed_refs: removed,
            unchanged_count,
            total_refs_at_target: total,
            compression_ratio: ratio.max(0.0),
        })
    }

    /// Items that haven't been referenced since `stale_threshold_hop`.
    pub fn stale_items(&self, stale_threshold_hop: u16) -> Vec<&ChainContextItem> {
        self.all_items
            .values()
            .filter(|item| item.last_referenced_hop < stale_threshold_hop)
            .collect()
    }

    pub fn current_hop(&self) -> u16 {
        self.current_hop
    }

    pub fn total_tracked_items(&self) -> usize {
        self.all_items.len()
    }
}

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ChainCompressionError {
    #[error("content_refs and freshness_refs have different lengths")]
    MismatchedLengths,
    #[error("hop {0} not found in chain history")]
    HopNotFound(u16),
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_shows_only_differences() {
        let mut tracker = ChainCompressionTracker::new("chain:1".into());
        tracker
            .register_hop(
                0,
                vec!["blake3:a".into(), "blake3:b".into(), "blake3:c".into()],
                vec!["fresh:1".into(), "fresh:1".into(), "fresh:1".into()],
            )
            .unwrap();
        tracker
            .register_hop(
                1,
                vec!["blake3:b".into(), "blake3:c".into(), "blake3:d".into()],
                vec!["fresh:1".into(), "fresh:1".into(), "fresh:1".into()],
            )
            .unwrap();

        let delta = tracker.compute_delta(0, 1).unwrap();
        assert_eq!(delta.added_refs, vec!["blake3:d"]);
        assert_eq!(delta.removed_refs, vec!["blake3:a"]);
        assert_eq!(delta.unchanged_count, 2);
        assert!(delta.compression_ratio > 0.0);
    }

    #[test]
    fn forward_delta_for_child() {
        let mut tracker = ChainCompressionTracker::new("chain:2".into());
        tracker
            .register_hop(
                0,
                vec!["blake3:x".into(), "blake3:y".into(), "blake3:z".into()],
                vec!["fresh:1".into(), "fresh:1".into(), "fresh:1".into()],
            )
            .unwrap();

        let child_refs = vec!["blake3:y".into(), "blake3:z".into(), "blake3:new".into()];
        let delta = tracker.forward_delta(0, &child_refs).unwrap();
        assert_eq!(delta.added_refs, vec!["blake3:new"]);
        assert_eq!(delta.removed_refs, vec!["blake3:x"]);
        assert_eq!(delta.unchanged_count, 2);
    }

    #[test]
    fn identical_hops_yield_perfect_compression() {
        let mut tracker = ChainCompressionTracker::new("chain:3".into());
        let refs = vec!["blake3:a".into(), "blake3:b".into()];
        let fresh = vec!["fresh:1".into(), "fresh:1".into()];
        tracker
            .register_hop(0, refs.clone(), fresh.clone())
            .unwrap();
        tracker.register_hop(1, refs, fresh).unwrap();

        let delta = tracker.compute_delta(0, 1).unwrap();
        assert!(delta.added_refs.is_empty());
        assert!(delta.removed_refs.is_empty());
        assert_eq!(delta.unchanged_count, 2);
    }

    #[test]
    fn stale_detection() {
        let mut tracker = ChainCompressionTracker::new("chain:4".into());
        tracker
            .register_hop(
                0,
                vec!["blake3:old".into(), "blake3:active".into()],
                vec!["fresh:1".into(), "fresh:1".into()],
            )
            .unwrap();
        tracker
            .register_hop(
                3,
                vec!["blake3:active".into(), "blake3:new".into()],
                vec!["fresh:2".into(), "fresh:2".into()],
            )
            .unwrap();

        let stale = tracker.stale_items(2);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].content_ref, "blake3:old");
    }

    #[test]
    fn mismatched_lengths_rejected() {
        let mut tracker = ChainCompressionTracker::new("chain:5".into());
        assert!(
            tracker
                .register_hop(0, vec!["blake3:a".into()], vec![])
                .is_err()
        );
    }
}
