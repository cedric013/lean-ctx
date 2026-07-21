use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::TeamScope;

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    hex_lower(&digest)
}

pub(super) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
    String::from_utf8(out).unwrap_or_default()
}

pub(super) fn parse_sha256_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.len() != 64 {
        return Err(anyhow!("sha256 hex must be 64 chars"));
    }
    let mut out = Vec::with_capacity(32);
    let bytes = s.as_bytes();
    let to_nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    for i in (0..64).step_by(2) {
        let hi = to_nibble(bytes[i]).ok_or_else(|| anyhow!("invalid hex"))?;
        let lo = to_nibble(bytes[i + 1]).ok_or_else(|| anyhow!("invalid hex"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub(crate) fn required_scopes(
    tool_name: &str,
    args: Option<&Value>,
) -> Option<BTreeSet<TeamScope>> {
    if matches!(tool_name, "ctx_shell" | "ctx_execute" | "ctx_edit") {
        return None;
    }

    if tool_name == "ctx" {
        let Value::Object(m) = args? else {
            return None;
        };
        let sub = m.get("tool")?.as_str()?.trim();
        if sub.is_empty() {
            return None;
        }
        let canonical = if sub.starts_with("ctx_") {
            sub.to_string()
        } else {
            format!("ctx_{sub}")
        };
        let mut m2 = m.clone();
        m2.remove("tool");
        return required_scopes(&canonical, Some(&Value::Object(m2)));
    }

    let mut s = BTreeSet::new();
    match tool_name {
        // Search scope (read/discovery/analysis)
        "ctx_read" | "ctx_multi_read" | "ctx_smart_read" | "ctx_search" | "ctx_tree"
        | "ctx_outline" | "ctx_expand" | "ctx_delta" | "ctx_dedup" | "ctx_prefetch"
        | "ctx_preload" | "ctx_review" | "ctx_response" | "ctx_task" | "ctx_overview"
        | "ctx_architecture" | "ctx_benchmark" | "ctx_cost" | "ctx_intent" | "ctx_heatmap"
        | "ctx_gain" | "ctx_analyze" | "ctx_discover_tools" | "ctx_discover" | "ctx_symbol"
        | "ctx_index" | "ctx_metrics" | "ctx_cache" | "ctx_agent" => {
            s.insert(TeamScope::Search);
            Some(s)
        }
        // Pack needs search + graph (it includes impact/graph-derived context)
        "ctx_pack" => {
            s.insert(TeamScope::Search);
            s.insert(TeamScope::Graph);
            Some(s)
        }
        // Graph scope
        "ctx_graph" | "ctx_impact" | "ctx_callgraph" | "ctx_routes" => {
            s.insert(TeamScope::Graph);

            if tool_name == "ctx_graph" {
                let action = args
                    .and_then(|v| v.get("action"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if matches!(
                    action,
                    "index-build"
                        | "index-build-full"
                        | "index-build-background"
                        | "index-build-full-background"
                ) {
                    s.insert(TeamScope::Index);
                }
            }

            Some(s)
        }
        "ctx_semantic_search" => {
            s.insert(TeamScope::Search);
            if args
                .and_then(|v| v.get("artifacts"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                s.insert(TeamScope::Artifacts);
            }
            if args
                .and_then(|v| v.get("action"))
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("reindex"))
            {
                s.insert(TeamScope::Index);
            }
            Some(s)
        }
        // Session-mutating tools
        "ctx_session" | "ctx_handoff" | "ctx_workflow" | "ctx_compress" | "ctx_share" => {
            s.insert(TeamScope::SessionMutations);
            Some(s)
        }
        // Knowledge tools
        "ctx_knowledge" | "ctx_knowledge_relations" => {
            s.insert(TeamScope::Knowledge);
            Some(s)
        }
        // Artifact + proof tools
        "ctx_artifacts" | "ctx_proof" | "ctx_verify" => {
            s.insert(TeamScope::Artifacts);
            Some(s)
        }
        _ => None,
    }
}
