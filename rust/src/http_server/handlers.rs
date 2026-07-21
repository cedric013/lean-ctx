use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value;

use super::AppState;

const MAX_HANDOFF_PAYLOAD_BYTES: usize = 1_000_000;
const MAX_HANDOFF_FILES: usize = 50;

pub(super) async fn v1_a2a_handoff(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let envelope = match crate::core::a2a_transport::parse_envelope(
        &serde_json::to_string(&body).unwrap_or_default(),
    ) {
        Ok(env) => env,
        Err(e) => {
            tracing::warn!("a2a handoff parse error: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_envelope"})),
            );
        }
    };

    if envelope.payload_json.len() > MAX_HANDOFF_PAYLOAD_BYTES {
        tracing::warn!(
            "a2a handoff payload too large: {} bytes (limit {MAX_HANDOFF_PAYLOAD_BYTES})",
            envelope.payload_json.len()
        );
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({"error": "payload_too_large"})),
        );
    }

    let rt = crate::core::context_os::runtime();
    rt.bus.append(
        &state.project_root,
        "a2a",
        &crate::core::context_os::ContextEventKindV1::SessionMutated,
        Some(&envelope.sender.agent_id),
        serde_json::json!({
            "type": "handoff_received",
            "content_type": format!("{:?}", envelope.content_type),
            "sender": envelope.sender.agent_id,
            "payload_size": envelope.payload_json.len(),
        }),
    );

    match envelope.content_type {
        crate::core::a2a_transport::TransportContentType::ContextPackage => {
            let dir = std::path::Path::new(&state.project_root)
                .join(".lean-ctx")
                .join("handoffs")
                .join("packages");
            let _ = std::fs::create_dir_all(&dir);
            evict_oldest_files(&dir, MAX_HANDOFF_FILES);
            let out = dir.join(format!(
                "ctx-{}.{}",
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                crate::core::contracts::PACKAGE_EXTENSION
            ));
            if let Err(e) = std::fs::write(&out, &envelope.payload_json) {
                tracing::error!("a2a handoff write failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "write_failed"})),
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "received",
                    "content_type": "context_package",
                })),
            )
        }
        crate::core::a2a_transport::TransportContentType::HandoffBundle => {
            // Signature enforcement at the network boundary (GL #465): a
            // payload that is not a parseable bundle, or whose signature
            // material does not verify, is rejected fail-closed before it
            // ever touches disk. Legacy unsigned bundles are stored with the
            // status surfaced so the importer can warn.
            let bundle =
                match crate::core::handoff_transfer_bundle::parse_bundle_v1(&envelope.payload_json)
                {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("a2a handoff rejected: not a valid bundle: {e}");
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": "invalid_bundle"})),
                        );
                    }
                };
            let signature =
                match crate::core::handoff_transfer_bundle::check_bundle_signature(&bundle) {
                    crate::core::handoff_transfer_bundle::BundleSignatureStatus::Invalid(
                        reason,
                    ) => {
                        tracing::warn!("a2a handoff rejected: signature invalid: {reason}");
                        crate::core::audit_trail::record(
                            crate::core::audit_trail::AuditEntryData {
                                agent_id: envelope.sender.agent_id.clone(),
                                tool: "http:/v1/a2a/handoff".to_string(),
                                action: Some("import_signature_invalid".to_string()),
                                input_hash: String::new(),
                                output_tokens: 0,
                                role: crate::core::roles::active_role_name(),
                                event_type:
                                    crate::core::audit_trail::AuditEventType::SecurityViolation,
                            },
                        );
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": "invalid_signature"})),
                        );
                    }
                    crate::core::handoff_transfer_bundle::BundleSignatureStatus::Verified(
                        signer,
                    ) => {
                        serde_json::json!({"status": "verified", "signer": signer})
                    }
                    crate::core::handoff_transfer_bundle::BundleSignatureStatus::Unsigned => {
                        serde_json::json!({"status": "unsigned"})
                    }
                };

            let dir = std::path::Path::new(&state.project_root)
                .join(".lean-ctx")
                .join("handoffs");
            let _ = std::fs::create_dir_all(&dir);
            evict_oldest_files(&dir, MAX_HANDOFF_FILES);
            let out = dir.join(format!(
                "received-{}.json",
                chrono::Utc::now().format("%Y%m%d_%H%M%S")
            ));
            if let Err(e) = std::fs::write(&out, &envelope.payload_json) {
                tracing::error!("a2a handoff write failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "write_failed"})),
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "received",
                    "content_type": "handoff_bundle",
                    "signature": signature,
                })),
            )
        }
        _ => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "received",
                "content_type": format!("{:?}", envelope.content_type),
            })),
        ),
    }
}

pub(super) fn evict_oldest_files(dir: &std::path::Path, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let meta = e.metadata().ok()?;
            if meta.is_file() {
                Some((meta.modified().unwrap_or(std::time::UNIX_EPOCH), e.path()))
            } else {
                None
            }
        })
        .collect();

    if files.len() < max_files {
        return;
    }
    files.sort_by_key(|(mtime, _)| *mtime);
    let to_remove = files.len().saturating_sub(max_files.saturating_sub(1));
    for (_, path) in files.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(path);
    }
}
