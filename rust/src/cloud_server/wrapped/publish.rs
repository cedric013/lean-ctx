use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use super::common::{
    ApiResult, MAX_BODY_BYTES, MAX_PUBLISH_PER_HOUR, bad_payload, client_ip_hash, err,
    generate_card_id,
};
use super::payload::PublishPayload;
use super::signed::{
    EDIT_TOKEN_CHALLENGE_TTL_MINUTES, SignedEnvelope, mint_edit_token_challenge,
    verify_signed_envelope,
};
use crate::cloud_server::auth::{AppState, generate_token, sha256_hex};
use crate::cloud_server::helpers::internal_error;

/// `POST /api/wrapped` — publish (or refresh) a Wrapped card. Body parsed from raw bytes so
/// unknown/oversized payloads return our own `invalid_payload` / `payload_too_large` instead of
/// axum defaults. Two body shapes are accepted:
///   • a signed envelope `{payload_json, public_key, signature}` — the client proves a login-less
///     identity and the card is UPSERTed by `(publisher_id, period)` (one stable card/URL); or
///   • a bare payload object — legacy anonymous insert (may create duplicates) for old clients.
pub(in crate::cloud_server) async fn publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if body.len() > MAX_BODY_BYTES {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"));
    }

    // Signed envelope (login-less identity → upsert) vs. legacy bare payload (anonymous insert).
    // The signed `payload_json` is stored verbatim so the stored card stays signature-verifiable.
    let (payload, payload_json, publisher_id) = if let Ok(env) =
        serde_json::from_slice::<SignedEnvelope>(&body)
    {
        let (payload, pid) = verify_signed_envelope(&env)?;
        (payload, env.payload_json, Some(pid))
    } else {
        let payload: PublishPayload = serde_json::from_slice(&body).map_err(|_| bad_payload())?;
        let json = serde_json::to_string(&payload).map_err(internal_error)?;
        (payload, json, None)
    };
    payload.validate()?;

    let client = state.pool.get().await.map_err(internal_error)?;
    let ip_hash = client_ip_hash(&headers, &state.cfg.ip_hash_salt);

    if let Some(h) = &ip_hash {
        let row = client
            .query_one(
                "SELECT count(*) FROM wrapped_cards \
                 WHERE ip_hash = $1 AND created_at > now() - interval '1 hour'",
                &[h],
            )
            .await
            .map_err(internal_error)?;
        let recent: i64 = row.get(0);
        if recent >= MAX_PUBLISH_PER_HOUR {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
        }
    }

    let id = generate_card_id();
    let edit_token = generate_token();
    let edit_token_hash = sha256_hex(&edit_token);
    let base = state.cfg.public_base_url.trim_end_matches('/');

    // Signed → UPSERT by (publisher_id, period): the same machine refreshes one stable card.
    // Legacy → plain INSERT (anonymous, may duplicate); `period` is still recorded.
    if let Some(pid) = &publisher_id {
        let row = client
            .query_one(
                "INSERT INTO wrapped_cards \
                 (id, edit_token_hash, payload_json, ip_hash, leaderboard_opt_in, tokens_saved, publisher_id, period) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 ON CONFLICT (publisher_id, period) WHERE publisher_id IS NOT NULL \
                 DO UPDATE SET payload_json = EXCLUDED.payload_json, \
                               leaderboard_opt_in = EXCLUDED.leaderboard_opt_in, \
                               tokens_saved = EXCLUDED.tokens_saved \
                 RETURNING id, (xmax = 0) AS inserted",
                &[
                    &id,
                    &edit_token_hash,
                    &payload_json,
                    &ip_hash,
                    &payload.leaderboard_opt_in,
                    &payload.tokens_saved,
                    pid,
                    &payload.period,
                ],
            )
            .await
            .map_err(internal_error)?;
        let final_id: String = row.get(0);
        let inserted: bool = row.get(1);
        let url = format!("{base}/w/{final_id}");
        let recovery_nonce = if inserted {
            None
        } else {
            Some(mint_edit_token_challenge(&client, &final_id).await?)
        };
        let mut out = serde_json::json!({ "id": final_id, "url": url });
        if inserted {
            out["edit_token"] = serde_json::Value::String(edit_token);
            Ok((StatusCode::CREATED, Json(out)))
        } else {
            out["edit_token_challenge"] =
                serde_json::Value::String(recovery_nonce.expect("update challenge exists"));
            out["challenge_expires_in_secs"] =
                serde_json::Value::from(i64::from(EDIT_TOKEN_CHALLENGE_TTL_MINUTES) * 60);
            Ok((StatusCode::OK, Json(out)))
        }
    } else {
        client
            .execute(
                "INSERT INTO wrapped_cards \
                 (id, edit_token_hash, payload_json, ip_hash, leaderboard_opt_in, tokens_saved, period) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &id,
                    &edit_token_hash,
                    &payload_json,
                    &ip_hash,
                    &payload.leaderboard_opt_in,
                    &payload.tokens_saved,
                    &payload.period,
                ],
            )
            .await
            .map_err(internal_error)?;
        let url = format!("{base}/w/{id}");
        Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({ "id": id, "edit_token": edit_token, "url": url })),
        ))
    }
}
