use serde::{Deserialize, Serialize};

use super::common::{ApiResult, MAX_LABEL_LEN, MAX_NAME_LEN, MAX_TOP_COMMANDS, bad_payload};

// ─── Whitelisted payload (the ONLY fields that may be published) ──────────────

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cloud_server) struct TopCommand {
    pub name: String,
    pub pct: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cloud_server) struct PublishPayload {
    pub period: String,
    pub tokens_saved: i64,
    pub cost_avoided_usd: f64,
    pub pricing_estimated: bool,
    pub compression_rate_pct: f64,
    // The fields below were removed from the publish whitelist (privacy minimalism): current
    // clients no longer send them. They remain declared as optional/defaulted ONLY so that
    // cards published by older clients still deserialize under `deny_unknown_fields`. Nothing
    // public renders them anymore — the hosted card omits any that are zero/empty.
    #[serde(default)]
    pub total_commands: i64,
    #[serde(default)]
    pub sessions_count: i64,
    #[serde(default)]
    pub files_touched: i64,
    #[serde(default)]
    pub top_commands: Vec<TopCommand>,
    #[serde(default)]
    pub model_key: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Opt-in (default off): show this card on the public leaderboard.
    #[serde(default)]
    pub leaderboard_opt_in: bool,
}

impl PublishPayload {
    /// Rejects anything outside the documented bounds. Pure (no I/O) so it is unit-tested.
    pub(in crate::cloud_server) fn validate(&self) -> ApiResult<()> {
        if !matches!(self.period.as_str(), "day" | "week" | "month" | "all") {
            return Err(bad_payload());
        }
        if self.tokens_saved < 0 || self.total_commands < 0 {
            return Err(bad_payload());
        }
        if self.sessions_count < 0 || self.files_touched < 0 {
            return Err(bad_payload());
        }
        if !finite_nonneg(self.cost_avoided_usd) {
            return Err(bad_payload());
        }
        if !in_pct(self.compression_rate_pct) {
            return Err(bad_payload());
        }
        if self.top_commands.len() > MAX_TOP_COMMANDS {
            return Err(bad_payload());
        }
        for c in &self.top_commands {
            let len = c.name.chars().count();
            if len == 0 || len > MAX_NAME_LEN || has_markup(&c.name) || !in_pct(c.pct) {
                return Err(bad_payload());
            }
        }
        if let Some(m) = &self.model_key
            && (m.chars().count() > MAX_LABEL_LEN || has_markup(m))
        {
            return Err(bad_payload());
        }
        if let Some(name) = &self.display_name {
            let len = name.chars().count();
            if len == 0 || len > MAX_LABEL_LEN || has_markup(name) {
                return Err(bad_payload());
            }
        }
        Ok(())
    }

    /// Rebuilds a `WrappedReport` for server-side card rendering. Fields outside the privacy
    /// whitelist (sparkline history, bounce, input tokens) take neutral defaults.
    pub(in crate::cloud_server) fn to_report(&self) -> crate::core::wrapped::WrappedReport {
        crate::core::wrapped::WrappedReport {
            period: self.period.clone(),
            tokens_saved: u64::try_from(self.tokens_saved).unwrap_or(0),
            tokens_input: 0,
            cost_avoided_usd: self.cost_avoided_usd,
            total_commands: u64::try_from(self.total_commands).unwrap_or(0),
            sessions_count: usize::try_from(self.sessions_count).unwrap_or(0),
            top_commands: self
                .top_commands
                .iter()
                .map(|c| (c.name.clone(), 0u64, c.pct))
                .collect(),
            compression_rate_pct: self.compression_rate_pct,
            files_touched: u64::try_from(self.files_touched).unwrap_or(0),
            daily_savings: Vec::new(),
            bounce_tokens: 0,
            model_key: self.model_key.clone().unwrap_or_default(),
            pricing_estimated: self.pricing_estimated,
            percentile: None,
        }
    }
}

fn finite_nonneg(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

fn in_pct(v: f64) -> bool {
    v.is_finite() && (0.0..=100.0).contains(&v)
}

/// Rejects markup and control characters — defence against stored XSS in user-chosen text.
fn has_markup(s: &str) -> bool {
    s.chars()
        .any(|c| c == '<' || c == '>' || (c.is_control() && c != '\t'))
}
