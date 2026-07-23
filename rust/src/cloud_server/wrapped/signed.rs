use axum::http::StatusCode;
use serde::Deserialize;

use super::common::{ApiResult, bad_payload, err, internal_error_str};
use super::payload::PublishPayload;
use crate::cloud_server::auth::{generate_token, sha256_hex};
use crate::cloud_server::helpers::internal_error;

// ─── Login-less publisher identity (signed publish, VL-3c) ────────────────────

/// Length (hex chars) of the publisher id derived from the public key. 16 bytes of SHA-256 is
/// collision-safe yet compact, and reveals nothing about the key beyond a stable pseudonym.
pub(in crate::cloud_server) const PUBLISHER_ID_HEX_LEN: usize = 32;
pub(in crate::cloud_server) const EDIT_TOKEN_CHALLENGE_TTL_MINUTES: i32 = 5;

/// Wraps the whitelisted payload with the publisher's Ed25519 public key and a signature over
/// the exact `payload_json` bytes. The server derives a stable `publisher_id` from the key — no
/// login, no account — and upserts the card, so re-publishing from the same machine refreshes
/// one card instead of piling up duplicates. Old clients still POST the bare payload object.
#[derive(Deserialize)]
pub(in crate::cloud_server) struct SignedEnvelope {
    /// The serialized `PublishPayload`, byte-identical to what the client signed.
    pub(in crate::cloud_server) payload_json: String,
    /// Hex-encoded Ed25519 public key (32 bytes → 64 hex chars).
    pub(in crate::cloud_server) public_key: Option<String>,
    /// Hex-encoded Ed25519 signature over `payload_json.as_bytes()` (64 bytes → 128 hex chars).
    pub(in crate::cloud_server) signature: Option<String>,
}

pub(in crate::cloud_server) fn publisher_id_from_public_key_hex(
    public_key: &str,
) -> ApiResult<String> {
    sha256_hex(public_key)
        .get(..PUBLISHER_ID_HEX_LEN)
        .ok_or_else(internal_error_str)
        .map(str::to_string)
}

/// Verifies the envelope signature against its public key and returns the parsed payload plus
/// the derived `publisher_id`. A missing key/signature or a bad signature is rejected — there is
/// no way to publish under another machine's identity without holding its private key.
pub(in crate::cloud_server) fn verify_signed_envelope(
    env: &SignedEnvelope,
) -> ApiResult<(PublishPayload, String)> {
    use crate::core::agent_identity::{hex_decode, verify_signature};
    let (Some(pk_hex), Some(sig_hex)) = (&env.public_key, &env.signature) else {
        return Err(bad_payload());
    };
    let pk_bytes = hex_decode(pk_hex).map_err(|_| bad_payload())?;
    let sig_bytes = hex_decode(sig_hex).map_err(|_| bad_payload())?;
    if !verify_signature(&pk_bytes, env.payload_json.as_bytes(), &sig_bytes) {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid_signature"));
    }
    let payload: PublishPayload =
        serde_json::from_str(&env.payload_json).map_err(|_| bad_payload())?;
    // Stable, non-reversible pseudonym derived from the public key (its hex form). The same key
    // always maps to the same publisher_id, which is the upsert key — no account, no login.
    let publisher_id = publisher_id_from_public_key_hex(pk_hex)?;
    Ok((payload, publisher_id))
}
pub(in crate::cloud_server) async fn mint_edit_token_challenge(
    client: &tokio_postgres::Client,
    card_id: &str,
) -> ApiResult<String> {
    let nonce = generate_token();
    client
        .execute(
            "INSERT INTO wrapped_edit_token_challenges (nonce_hash, card_id, expires_at) \
             VALUES ($1, $2, now() + make_interval(mins => $3)) \
             ON CONFLICT (card_id) DO UPDATE \
             SET nonce_hash = EXCLUDED.nonce_hash, \
                 created_at = now(), \
                 expires_at = EXCLUDED.expires_at",
            &[
                &sha256_hex(&nonce),
                &card_id,
                &EDIT_TOKEN_CHALLENGE_TTL_MINUTES,
            ],
        )
        .await
        .map_err(internal_error)?;
    Ok(nonce)
}
