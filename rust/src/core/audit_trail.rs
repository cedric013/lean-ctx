use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub agent_id: String,
    pub tool: String,
    pub action: Option<String>,
    pub input_hash: String,
    pub output_tokens: u32,
    pub role: String,
    pub event_type: AuditEventType,
    pub prev_hash: String,
    pub entry_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    ToolCall,
    ToolDenied,
    PathJailViolation,
    BudgetExceeded,
    CrossProjectAccess,
    RateLimited,
    SecurityViolation,
}

pub struct AuditEntryData {
    pub agent_id: String,
    pub tool: String,
    pub action: Option<String>,
    pub input_hash: String,
    pub output_tokens: u32,
    pub role: String,
    pub event_type: AuditEventType,
}

pub struct ChainVerifyResult {
    pub total_entries: usize,
    pub valid: bool,
    pub first_invalid_at: Option<usize>,
}

static LAST_HASH: Mutex<Option<String>> = Mutex::new(None);

fn trail_path() -> Option<PathBuf> {
    let dir = crate::core::data_dir::lean_ctx_data_dir().ok()?;
    let audit_dir = dir.join("audit");
    fs::create_dir_all(&audit_dir).ok()?;
    Some(audit_dir.join("trail.jsonl"))
}

fn init_last_hash(path: &PathBuf) -> String {
    let mut guard = LAST_HASH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(ref h) = *guard {
        return h.clone();
    }
    let hash = read_last_hash_from_file(path);
    *guard = Some(hash.clone());
    hash
}

fn read_last_hash_from_file(path: &PathBuf) -> String {
    let Ok(file) = fs::File::open(path) else {
        return "genesis".to_string();
    };
    let reader = std::io::BufReader::new(file);
    let mut last_hash = "genesis".to_string();
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<AuditEntry>(&line) {
            last_hash = entry.entry_hash;
        }
    }
    last_hash
}

fn compute_entry_hash(prev_hash: &str, data_json: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(data_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn record(data: AuditEntryData) {
    let Some(path) = trail_path() else { return };
    let prev_hash = init_last_hash(&path);

    let partial = serde_json::json!({
        "agent_id": data.agent_id,
        "tool": data.tool,
        "action": data.action,
        "input_hash": data.input_hash,
        "output_tokens": data.output_tokens,
        "role": data.role,
        "event_type": data.event_type,
    });
    let data_json = serde_json::to_string(&partial).unwrap_or_default();
    let entry_hash = compute_entry_hash(&prev_hash, &data_json);

    let entry = AuditEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        agent_id: data.agent_id,
        tool: data.tool,
        action: data.action,
        input_hash: data.input_hash,
        output_tokens: data.output_tokens,
        role: data.role,
        event_type: data.event_type,
        prev_hash,
        entry_hash: entry_hash.clone(),
    };

    if let Ok(line) = serde_json::to_string(&entry) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{line}");
        }
    }

    if let Ok(mut guard) = LAST_HASH.lock() {
        *guard = Some(entry_hash);
    }
}

pub fn load_recent(limit: usize) -> Vec<AuditEntry> {
    let Some(path) = trail_path() else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(&path) else {
        return Vec::new();
    };
    let reader = std::io::BufReader::new(file);
    let entries: Vec<AuditEntry> = reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    let skip = entries.len().saturating_sub(limit);
    entries.into_iter().skip(skip).collect()
}

pub fn verify_chain() -> ChainVerifyResult {
    let Some(path) = trail_path() else {
        return ChainVerifyResult {
            total_entries: 0,
            valid: true,
            first_invalid_at: None,
        };
    };
    let Ok(file) = fs::File::open(&path) else {
        return ChainVerifyResult {
            total_entries: 0,
            valid: true,
            first_invalid_at: None,
        };
    };
    let reader = std::io::BufReader::new(file);
    let mut prev_hash = "genesis".to_string();
    let mut total = 0usize;

    for line in reader.lines().map_while(Result::ok) {
        let entry: AuditEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => {
                return ChainVerifyResult {
                    total_entries: total,
                    valid: false,
                    first_invalid_at: Some(total),
                }
            }
        };

        if entry.prev_hash != prev_hash {
            return ChainVerifyResult {
                total_entries: total,
                valid: false,
                first_invalid_at: Some(total),
            };
        }

        let partial = serde_json::json!({
            "agent_id": entry.agent_id,
            "tool": entry.tool,
            "action": entry.action,
            "input_hash": entry.input_hash,
            "output_tokens": entry.output_tokens,
            "role": entry.role,
            "event_type": entry.event_type,
        });
        let data_json = serde_json::to_string(&partial).unwrap_or_default();
        let expected = compute_entry_hash(&prev_hash, &data_json);

        if entry.entry_hash != expected {
            return ChainVerifyResult {
                total_entries: total,
                valid: false,
                first_invalid_at: Some(total),
            };
        }

        prev_hash = entry.entry_hash;
        total += 1;
    }

    ChainVerifyResult {
        total_entries: total,
        valid: true,
        first_invalid_at: None,
    }
}

pub fn hash_input(args: &serde_json::Map<String, serde_json::Value>) -> String {
    let serialized = serde_json::to_string(args).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    format!("{:x}", hasher.finalize())
}
