//! Standalone fixes for avoidable agent-to-agent context overhead.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_AGENT_BUDGET: usize = 1_000_000;
const BUDGET_WARNING_PERCENT: f64 = 80.0;

/// Lightweight representation of an agent scratchpad message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEntry {
    pub category: String,
    pub body: String,
    pub from_agent: String,
    pub timestamp_epoch: u64,
}

/// Compatibility name for a lightweight scratchpad message.
pub type ScratchpadEntry = MessageEntry;

/// Compute the stable content ID used to track whether a message was read.
pub fn message_id(message: &MessageEntry) -> String {
    let mut hasher = blake3::Hasher::new();
    for field in [&message.category, &message.body, &message.from_agent] {
        hasher.update(&(field.len() as u64).to_le_bytes());
        hasher.update(field.as_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

/// Properly filter messages to only return genuinely unread ones.
pub fn filter_truly_unread<'a>(
    messages: &'a [ScratchpadEntry],
    read_ids: &HashSet<String>,
) -> Vec<&'a ScratchpadEntry> {
    messages
        .iter()
        .filter(|message| !read_ids.contains(&message_id(message)))
        .collect()
}

/// Compute a real agent budget from configuration instead of `usize::MAX`.
pub fn real_agent_budget(config_limit: Option<usize>) -> usize {
    config_limit.unwrap_or(DEFAULT_AGENT_BUDGET)
}

/// Result of checking a requested token consumption against a real budget.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetCheckResult {
    Allowed {
        remaining: usize,
    },
    Denied {
        over_by: usize,
    },
    Warning {
        remaining: usize,
        threshold_pct: f64,
    },
}

/// Validate that a token consumption will not exceed the budget.
pub fn check_budget_with_real_limit(
    current_used: usize,
    budget_limit: usize,
    tokens_to_consume: usize,
) -> BudgetCheckResult {
    let projected = current_used.saturating_add(tokens_to_consume);
    if projected > budget_limit {
        return BudgetCheckResult::Denied {
            over_by: projected - budget_limit,
        };
    }

    let remaining = budget_limit - projected;
    let threshold_pct = if budget_limit == 0 {
        100.0
    } else {
        projected as f64 * 100.0 / budget_limit as f64
    };
    if threshold_pct >= BUDGET_WARNING_PERCENT {
        BudgetCheckResult::Warning {
            remaining,
            threshold_pct,
        }
    } else {
        BudgetCheckResult::Allowed { remaining }
    }
}

/// Convert JSON to compact format after stripping null and empty values.
pub fn compact_json(pretty: &Value) -> String {
    match serde_json::to_string(&strip_nulls(pretty)) {
        Ok(compact) => compact,
        Err(_) => "null".to_owned(),
    }
}

/// Estimate a JSON string's token count using four characters per token.
pub fn json_token_estimate(json: &str) -> usize {
    json.chars().count().div_ceil(4)
}

/// Strip null and empty values from a JSON value recursively.
pub fn strip_nulls(value: &Value) -> Value {
    prune_value(value).unwrap_or(Value::Null)
}

fn prune_value(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::Array(values) => {
            let values: Vec<_> = values.iter().filter_map(prune_value).collect();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        Value::Object(fields) => {
            let fields: serde_json::Map<String, Value> = fields
                .iter()
                .filter_map(|(key, value)| prune_value(value).map(|value| (key.clone(), value)))
                .collect();
            (!fields.is_empty()).then_some(Value::Object(fields))
        }
        _ => Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::{
        BudgetCheckResult, MessageEntry, check_budget_with_real_limit, compact_json,
        filter_truly_unread, json_token_estimate, message_id, real_agent_budget, strip_nulls,
    };

    fn message(body: &str) -> MessageEntry {
        MessageEntry {
            category: "status".to_owned(),
            body: body.to_owned(),
            from_agent: "agent-a".to_owned(),
            timestamp_epoch: 42,
        }
    }

    #[test]
    fn filter_removes_already_read() {
        let messages: Vec<_> = (0..5).map(|index| message(&index.to_string())).collect();
        let read_ids = [message_id(&messages[1]), message_id(&messages[3])]
            .into_iter()
            .collect::<HashSet<_>>();

        let unread = filter_truly_unread(&messages, &read_ids);

        assert_eq!(unread.len(), 3);
        assert_eq!(unread[0].body, "0");
        assert_eq!(unread[1].body, "2");
        assert_eq!(unread[2].body, "4");
    }

    #[test]
    fn message_id_is_deterministic() {
        let first = message("same content");
        let mut second = first.clone();
        second.timestamp_epoch = 99;
        assert_eq!(message_id(&first), message_id(&second));
        assert_eq!(message_id(&first).len(), 16);
    }

    #[test]
    fn real_budget_not_max() {
        assert_eq!(real_agent_budget(None), 1_000_000);
        assert_ne!(real_agent_budget(None), usize::MAX);
        assert_eq!(real_agent_budget(Some(250_000)), 250_000);
    }

    #[test]
    fn budget_denied_when_exceeded() {
        assert_eq!(
            check_budget_with_real_limit(900_000, 1_000_000, 200_000),
            BudgetCheckResult::Denied { over_by: 100_000 }
        );
    }

    #[test]
    fn budget_warning_at_80_percent() {
        assert_eq!(
            check_budget_with_real_limit(800_000, 1_000_000, 10_000),
            BudgetCheckResult::Warning {
                remaining: 190_000,
                threshold_pct: 81.0,
            }
        );
    }

    #[test]
    fn compact_json_smaller() {
        let value = json!({
            "messages": [
                {"body": "short", "metadata": null, "tags": []},
                {"body": "reply", "metadata": null, "tags": []}
            ],
            "unused": null
        });
        let pretty = serde_json::to_string_pretty(&value).expect("test JSON must serialize");
        let compact = compact_json(&value);
        assert!(compact.len() * 2 < pretty.len());
    }

    #[test]
    fn strip_nulls_removes_null_fields() {
        assert_eq!(strip_nulls(&json!({"a": 1, "b": null})), json!({"a": 1}));
    }

    #[test]
    fn token_estimate_reasonable() {
        let estimate = json_token_estimate(&"x".repeat(1_000));
        assert!((200..=300).contains(&estimate));
    }
}
