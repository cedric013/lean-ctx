use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use super::common::{
    ApiResult, bad_payload, edit_token_header, err, fetch_token_hash, internal_error_str,
    require_token,
};
use super::payload::PublishPayload;
use super::render::{fetch_card_svg, render_permalink_html, svg_to_png};
use super::signed::publisher_id_from_public_key_hex;
use crate::cloud_server::auth::{AppState, auth_user, generate_token, sha256_hex};
use crate::cloud_server::helpers::internal_error;

pub(in crate::cloud_server) async fn get_card(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let client = state.pool.get().await.map_err(internal_error)?;
    let row = client
        .query_opt(
            "UPDATE wrapped_cards SET view_count = view_count + 1 \
             WHERE id = $1 RETURNING payload_json, created_at, view_count",
            &[&id],
        )
        .await
        .map_err(internal_error)?;
    let Some(row) = row else {
        return Err(err(StatusCode::NOT_FOUND, "not_found"));
    };

    let payload_json: String = row.get(0);
    let created_at: chrono::DateTime<chrono::Utc> = row.get(1);
    let view_count: i64 = row.get(2);
    let card: serde_json::Value = serde_json::from_str(&payload_json).map_err(internal_error)?;

    Ok(Json(serde_json::json!({
        "id": id,
        "created_at": created_at.to_rfc3339(),
        "view_count": view_count,
        "card": card,
    })))
}

/// `GET /api/wrapped/:id/card.svg` — server-rendered share card (reuses `WrappedReport::to_svg`).
/// Does not count as a view. Cacheable; the card never changes after publish.
pub(in crate::cloud_server) async fn get_card_svg(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<axum::response::Response> {
    let svg = fetch_card_svg(&state, &id).await?;

    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use axum::response::IntoResponse;
    Ok((
        [
            (CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=86400"),
        ],
        svg,
    )
        .into_response())
}

/// `GET /api/wrapped/:id/card.png` — rasterized OG image (PNG) for social unfurls, which do
/// not render SVG. Text needs fonts: the server loads system fonts and falls back to a present
/// family, so the container image must ship a sans font (e.g. `fonts-dejavu-core`).
pub(in crate::cloud_server) async fn get_card_png(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<axum::response::Response> {
    let svg = fetch_card_svg(&state, &id).await?;
    let png = svg_to_png(&svg).map_err(internal_error)?;

    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use axum::response::IntoResponse;
    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=86400"),
        ],
        png,
    )
        .into_response())
}

/// `GET /w/:id` — the public, crawler-friendly permalink page. Server-rendered so Open Graph /
/// Twitter meta carry per-card data (static hosts can proxy `/w/` here). Counts as a view.
pub(in crate::cloud_server) async fn get_permalink_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<axum::response::Response> {
    let client = state.pool.get().await.map_err(internal_error)?;
    let row = client
        .query_opt(
            "UPDATE wrapped_cards SET view_count = view_count + 1 \
             WHERE id = $1 RETURNING payload_json",
            &[&id],
        )
        .await
        .map_err(internal_error)?;
    let Some(row) = row else {
        return Err(err(StatusCode::NOT_FOUND, "not_found"));
    };
    let payload_json: String = row.get(0);
    let payload: PublishPayload = serde_json::from_str(&payload_json).map_err(internal_error)?;

    let html = render_permalink_html(
        &id,
        &payload,
        &state.cfg.public_base_url,
        &state.cfg.api_base_url,
    );

    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    Ok(([(CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}
/// `DELETE /api/wrapped/:id` — requires the matching `X-Edit-Token`.
pub(in crate::cloud_server) async fn delete_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let token =
        edit_token_header(&headers).ok_or_else(|| err(StatusCode::FORBIDDEN, "forbidden"))?;
    let client = state.pool.get().await.map_err(internal_error)?;

    let stored = fetch_token_hash(&client, &id).await?;
    require_token(&token, &stored)?;

    client
        .execute("DELETE FROM wrapped_cards WHERE id = $1", &[&id])
        .await
        .map_err(internal_error)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// `POST /api/wrapped/:id/claim` — binds an anonymous card to the authenticated account.
pub(in crate::cloud_server) async fn claim_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let (user_id, _) = auth_user(&state, &headers).await?;
    let token =
        edit_token_header(&headers).ok_or_else(|| err(StatusCode::FORBIDDEN, "forbidden"))?;
    let client = state.pool.get().await.map_err(internal_error)?;

    let stored = fetch_token_hash(&client, &id).await?;
    require_token(&token, &stored)?;

    client
        .execute(
            "UPDATE wrapped_cards SET user_id = $1 WHERE id = $2",
            &[&user_id, &id],
        )
        .await
        .map_err(internal_error)?;
    Ok(Json(serde_json::json!({ "claimed": true })))
}

#[derive(Deserialize)]
pub(in crate::cloud_server) struct RecoverEditTokenBody {
    pub(in crate::cloud_server) nonce: String,
    pub(in crate::cloud_server) public_key: String,
    pub(in crate::cloud_server) signature: String,
}

pub(in crate::cloud_server) fn verify_edit_token_recovery_proof(
    card_id: &str,
    body: &RecoverEditTokenBody,
) -> ApiResult<String> {
    use crate::core::agent_identity::{hex_decode, verify_signature};

    if body.nonce.is_empty()
        || body.nonce.len() > 256
        || body.public_key.len() != 64
        || body.signature.len() != 128
    {
        return Err(bad_payload());
    }
    let public_key = hex_decode(&body.public_key).map_err(|_| bad_payload())?;
    let signature = hex_decode(&body.signature).map_err(|_| bad_payload())?;
    let proof = crate::core::wrapped::edit_token_recovery_message(card_id, &body.nonce);
    if !verify_signature(&public_key, proof.as_bytes(), &signature) {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid_signature"));
    }
    publisher_id_from_public_key_hex(&body.public_key)
}

/// `POST /api/wrapped/:id/edit-token/recover` — rotate a lost local edit
/// token after proving possession of the card's persistent publisher key.
pub(in crate::cloud_server) async fn recover_edit_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RecoverEditTokenBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let publisher_id = verify_edit_token_recovery_proof(&id, &body)?;

    let mut client = state.pool.get().await.map_err(internal_error)?;
    let row = client
        .query_opt(
            "SELECT publisher_id FROM wrapped_cards WHERE id = $1",
            &[&id],
        )
        .await
        .map_err(internal_error)?;
    let Some(stored_publisher_id) = row.map(|row| row.get::<_, Option<String>>(0)) else {
        return Err(err(StatusCode::NOT_FOUND, "not_found"));
    };
    if stored_publisher_id.as_deref() != Some(publisher_id.as_str()) {
        return Err(err(StatusCode::FORBIDDEN, "forbidden"));
    }

    let edit_token = generate_token();
    let edit_token_hash = sha256_hex(&edit_token);
    let transaction = client.transaction().await.map_err(internal_error)?;
    let consumed = transaction
        .execute(
            "DELETE FROM wrapped_edit_token_challenges              WHERE nonce_hash = $1 AND card_id = $2 AND expires_at >= now()",
            &[&sha256_hex(&body.nonce), &id],
        )
        .await
        .map_err(internal_error)?;
    if consumed != 1 {
        return Err(err(StatusCode::NOT_FOUND, "challenge_invalid_or_expired"));
    }
    let updated = transaction
        .execute(
            "UPDATE wrapped_cards SET edit_token_hash = $1              WHERE id = $2 AND publisher_id = $3",
            &[&edit_token_hash, &id, &publisher_id],
        )
        .await
        .map_err(internal_error)?;
    if updated != 1 {
        return Err(internal_error_str());
    }
    transaction.commit().await.map_err(internal_error)?;

    Ok(Json(serde_json::json!({ "edit_token": edit_token })))
}
