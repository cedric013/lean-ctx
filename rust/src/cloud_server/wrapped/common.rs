//! Shared types, constants, and helpers for the Wrapped permalink API.

use axum::http::{HeaderMap, StatusCode};

use crate::cloud_server::auth::{constant_time_eq, sha256_hex};
use crate::cloud_server::helpers::internal_error;

pub(in crate::cloud_server) const MAX_PUBLISH_PER_HOUR: i64 = 20;
/// Documented publish body cap (contract: `413 payload_too_large` over this size).
pub(in crate::cloud_server) const MAX_BODY_BYTES: usize = 8 * 1024;
pub(in crate::cloud_server) const MAX_TOP_COMMANDS: usize = 12;
pub(in crate::cloud_server) const MAX_NAME_LEN: usize = 40;
pub(in crate::cloud_server) const MAX_LABEL_LEN: usize = 60;

pub(in crate::cloud_server) type ApiResult<T> = Result<T, (StatusCode, String)>;

/// JSON error envelope matching the cloud server convention (`helpers::internal_error`).
pub(in crate::cloud_server) fn err(status: StatusCode, code: &str) -> (StatusCode, String) {
    (status, format!(r#"{{"error":"{code}"}}"#))
}

pub(in crate::cloud_server) fn bad_payload() -> (StatusCode, String) {
    err(StatusCode::BAD_REQUEST, "invalid_payload")
}

pub(in crate::cloud_server) fn internal_error_str() -> (StatusCode, String) {
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

// ─── Helpers ────────────────────────────────────────────────────────────────

pub(in crate::cloud_server) async fn fetch_token_hash(
    client: &tokio_postgres::Client,
    id: &str,
) -> ApiResult<String> {
    let row = client
        .query_opt(
            "SELECT edit_token_hash FROM wrapped_cards WHERE id = $1",
            &[&id],
        )
        .await
        .map_err(internal_error)?;
    match row {
        Some(r) => Ok(r.get(0)),
        None => Err(err(StatusCode::NOT_FOUND, "not_found")),
    }
}

pub(in crate::cloud_server) fn require_token(presented: &str, stored_hash: &str) -> ApiResult<()> {
    if constant_time_eq(sha256_hex(presented).as_bytes(), stored_hash.as_bytes()) {
        Ok(())
    } else {
        Err(err(StatusCode::FORBIDDEN, "forbidden"))
    }
}

pub(in crate::cloud_server) fn edit_token_header(headers: &HeaderMap) -> Option<String> {
    let v = headers.get("x-edit-token")?.to_str().ok()?.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// 128-bit unguessable, hex-encoded id (the public `/w/<id>` slug).
pub(in crate::cloud_server) fn generate_card_id() -> String {
    let bytes: [u8; 16] = rand::random();
    hex::encode(bytes)
}

/// Salted hash of the client IP (from the front proxy's `X-Forwarded-For`/`X-Real-IP`),
/// for abuse rate-limiting only — the raw IP is never stored.
pub(in crate::cloud_server) fn client_ip_hash(headers: &HeaderMap, salt: &str) -> Option<String> {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })?;
    Some(sha256_hex(&format!("{salt}:{ip}")))
}
