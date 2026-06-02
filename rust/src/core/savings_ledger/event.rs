//! The auditable per-event savings record (the G1 counterfactual unit).
//!
//! One [`SavingsEvent`] is appended per value-producing read: it captures the
//! counterfactual (`baseline_tokens` = what the agent would have consumed) against the
//! `actual_tokens` actually sent, the resolved pricing model, and a SHA-256 hash chain
//! so the history is tamper-evident. See `docs/business/03-verified-savings-ledger.md`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavingsEvent {
    pub ts: String,
    /// Originating tool (e.g. "ctx_read"). Coarse for now; per-mode granularity is a
    /// later refinement (stats already tracks per-mode).
    pub tool: String,
    /// Resolved pricing model key the saving was valued against.
    pub model_id: String,
    /// Tokenizer family that produced `baseline_tokens`/`actual_tokens` (e.g.
    /// `"o200k_base"`). Recorded separately from `model_id` because lean-ctx counts with
    /// one tokenizer as a proxy; the model's own tokenizer may differ by a few percent.
    pub tokenizer: String,
    /// Counterfactual: tokens the agent would have consumed without lean-ctx.
    pub baseline_tokens: u64,
    /// Tokens actually sent.
    pub actual_tokens: u64,
    /// `baseline_tokens - actual_tokens`.
    pub saved_tokens: u64,
    /// Tokens later wasted by a compressed->full re-read (G7). Always 0 until a
    /// *persisted* bounce signal exists — we never silently inflate with a guessed 0.
    pub bounce_adjustment: u64,
    /// Model input price per 1M tokens used to value the saving.
    pub unit_price_per_m_usd: f64,
    /// `(saved_tokens - bounce_adjustment) * unit_price_per_m_usd / 1e6`. Upper bound
    /// (ignores prompt-cache discounts), consistent with the Wrapped headline.
    pub saved_usd: f64,
    /// Attribution: SHA-256 (truncated) of the recording process working directory.
    /// Privacy-preserving — never the file path or its contents.
    pub repo_hash: String,
    pub agent_id: String,
    pub prev_hash: String,
    pub entry_hash: String,
}

impl SavingsEvent {
    /// Float-stable canonical representation of the *content* fields (everything except
    /// the chain hashes). Used identically on append and on verify so the entry hash is
    /// reproducible regardless of JSON float formatting.
    pub fn canonical_content(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{:.6}|{:.6}|{}|{}",
            self.ts,
            self.tool,
            self.model_id,
            self.tokenizer,
            self.baseline_tokens,
            self.actual_tokens,
            self.saved_tokens,
            self.bounce_adjustment,
            self.unit_price_per_m_usd,
            self.saved_usd,
            self.repo_hash,
            self.agent_id,
        )
    }
}

/// `SHA-256(prev_hash || content)` as lowercase hex — the chain link primitive.
pub fn compute_hash(prev_hash: &str, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev() -> SavingsEvent {
        SavingsEvent {
            ts: "2026-06-01T00:00:00+00:00".into(),
            tool: "ctx_read".into(),
            model_id: "claude-3.5-sonnet".into(),
            tokenizer: "o200k_base".into(),
            baseline_tokens: 1000,
            actual_tokens: 300,
            saved_tokens: 700,
            bounce_adjustment: 0,
            unit_price_per_m_usd: 3.0,
            saved_usd: 0.0021,
            repo_hash: "abc123".into(),
            agent_id: "local".into(),
            prev_hash: String::new(),
            entry_hash: String::new(),
        }
    }

    #[test]
    fn hash_is_deterministic() {
        let e = ev();
        let a = compute_hash("genesis", &e.canonical_content());
        let b = compute_hash("genesis", &e.canonical_content());
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "sha-256 hex is 64 chars");
    }

    #[test]
    fn hash_changes_when_content_changes() {
        let mut e = ev();
        let a = compute_hash("genesis", &e.canonical_content());
        e.saved_tokens = 701;
        let b = compute_hash("genesis", &e.canonical_content());
        assert_ne!(a, b, "tampering with a content field must change the hash");
    }

    #[test]
    fn hash_depends_on_prev() {
        let e = ev();
        let a = compute_hash("genesis", &e.canonical_content());
        let b = compute_hash("other", &e.canonical_content());
        assert_ne!(a, b, "chain link must depend on prev_hash");
    }
}
