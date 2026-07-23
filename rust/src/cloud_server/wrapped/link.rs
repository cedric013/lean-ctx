use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use super::common::{
    ApiResult, bad_payload, edit_token_header, err, fetch_token_hash, require_token,
};
use crate::cloud_server::auth::{AppState, sha256_hex};
use crate::cloud_server::helpers::internal_error;

// ─── Login-less machine linking (GH #736) ─────────────────────────────────────
//
// Two machines prove ownership of their own cards via edit_token possession and
// get joined into one `link_group`; the leaderboard then stacks all cards of a
// group into a single entry. No account, no email, no PII — the pairing code is
// a short-lived (10 min), single-use, hashed-at-rest capability.

/// Pairing-code lifetime. Long enough to walk to the other machine, short
/// enough that a leaked code is useless soon after.
const LINK_CODE_TTL_MINUTES: i32 = 10;
/// Codes a single card may have outstanding — prevents minting unlimited codes.
const MAX_ACTIVE_LINK_CODES_PER_CARD: i64 = 3;

/// `POST /api/wrapped/:id/link/start` — mint a pairing code for this card.
/// Requires the card's edit token; returns `{code, expires_in_secs}`.
pub(in crate::cloud_server) async fn link_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let token =
        edit_token_header(&headers).ok_or_else(|| err(StatusCode::FORBIDDEN, "forbidden"))?;
    let client = state.pool.get().await.map_err(internal_error)?;

    let stored = fetch_token_hash(&client, &id).await?;
    require_token(&token, &stored)?;

    // Opportunistic GC + per-card cap: expired codes vanish, live ones are bounded.
    client
        .execute(
            "DELETE FROM wrapped_link_codes WHERE expires_at < now()",
            &[],
        )
        .await
        .map_err(internal_error)?;
    let active: i64 = client
        .query_one(
            "SELECT count(*) FROM wrapped_link_codes WHERE card_id = $1",
            &[&id],
        )
        .await
        .map_err(internal_error)?
        .get(0);
    if active >= MAX_ACTIVE_LINK_CODES_PER_CARD {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }

    let code = generate_link_code();
    client
        .execute(
            "INSERT INTO wrapped_link_codes (code_hash, card_id, expires_at) \
             VALUES ($1, $2, now() + make_interval(mins => $3))",
            &[&sha256_hex(&code), &id, &LINK_CODE_TTL_MINUTES],
        )
        .await
        .map_err(internal_error)?;

    Ok(Json(serde_json::json!({
        "code": code,
        "expires_in_secs": i64::from(LINK_CODE_TTL_MINUTES) * 60,
    })))
}

#[derive(Deserialize)]
pub(in crate::cloud_server) struct LinkCompleteBody {
    pub code: String,
}

/// `POST /api/wrapped/:id/link/complete` — join this card into the pairing
/// code's group. Requires *this* card's edit token (both sides prove ownership).
/// The group id is the initiating card's existing group, else its publisher_id,
/// else its card id — so repeated links from any machine converge on one group.
pub(in crate::cloud_server) async fn link_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<LinkCompleteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let token =
        edit_token_header(&headers).ok_or_else(|| err(StatusCode::FORBIDDEN, "forbidden"))?;
    let client = state.pool.get().await.map_err(internal_error)?;

    let stored = fetch_token_hash(&client, &id).await?;
    require_token(&token, &stored)?;

    let code = body.code.trim().to_uppercase().replace('-', "");
    if code.is_empty() || code.len() > 32 {
        return Err(bad_payload());
    }
    // Single use: consume the code atomically so it can never join two parties.
    let row = client
        .query_opt(
            "DELETE FROM wrapped_link_codes \
             WHERE code_hash = $1 AND expires_at >= now() \
             RETURNING card_id",
            &[&sha256_hex(&format_link_code(&code))],
        )
        .await
        .map_err(internal_error)?;
    let Some(row) = row else {
        return Err(err(StatusCode::NOT_FOUND, "code_invalid_or_expired"));
    };
    let initiator_id: String = row.get(0);
    if initiator_id == id {
        return Err(bad_payload());
    }

    // Canonical group: initiator's existing group > its publisher_id > its id.
    // Merging an already-grouped card drags its whole group along (transitive).
    let group: String = client
        .query_one(
            "SELECT COALESCE(link_group, publisher_id, id) FROM wrapped_cards WHERE id = $1",
            &[&initiator_id],
        )
        .await
        .map_err(internal_error)?
        .get(0);
    let joined = client
        .execute(
            "UPDATE wrapped_cards \
             SET link_group = $1 \
             WHERE id IN ($2, $3) \
                OR link_group IN (SELECT link_group FROM wrapped_cards \
                                  WHERE id IN ($2, $3) AND link_group IS NOT NULL) \
                OR (publisher_id IS NOT NULL AND publisher_id IN \
                     (SELECT publisher_id FROM wrapped_cards \
                      WHERE id IN ($2, $3) AND publisher_id IS NOT NULL))",
            &[&group, &initiator_id, &id],
        )
        .await
        .map_err(internal_error)?;

    tracing::info!(group, joined, "wrapped cards linked (login-less)");
    Ok(Json(
        serde_json::json!({ "linked": true, "group_size": joined }),
    ))
}

/// 8-char pairing code from the crypto RNG, formatted `XXXX-XXXX`. Alphabet
/// drops easily-confused characters (0/O, 1/I/L). 32^8 ≈ 1.1e12 combinations
/// against a 10-minute, 3-codes-per-card window.
pub(in crate::cloud_server) fn generate_link_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let bytes: [u8; 8] = rand::random();
    let chars: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

/// Normalizes a user-entered code back to the canonical `XXXX-XXXX` form used
/// for hashing (dashes stripped on input, re-inserted here).
pub(in crate::cloud_server) fn format_link_code(bare: &str) -> String {
    if bare.len() == 8 {
        format!("{}-{}", &bare[..4], &bare[4..])
    } else {
        bare.to_string()
    }
}
