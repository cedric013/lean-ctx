use axum::{
    body::Body,
    extract::State,
    http::{request::Parts, Request, StatusCode},
    response::Response,
};

use super::ProxyState;

const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

pub type CompressFn = fn(&[u8]) -> (Vec<u8>, usize, usize);

pub async fn forward_request(
    State(state): State<ProxyState>,
    req: Request<Body>,
    upstream_base: &str,
    default_path: &str,
    compress_body: CompressFn,
    provider_label: &str,
    extra_stream_types: &[&str],
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    state.stats.record_request();

    // Introspect the request for context analysis
    if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
        let provider = match provider_label {
            "Anthropic" => super::introspect::Provider::Anthropic,
            "OpenAI" => super::introspect::Provider::OpenAi,
            _ => super::introspect::Provider::Gemini,
        };
        let breakdown = super::introspect::analyze_request(&parsed, provider);
        state.introspect.record(breakdown);
    }

    let (compressed_body, original_size, compressed_size) = compress_body(&body_bytes);

    if compressed_size < original_size {
        state
            .stats
            .record_compression(original_size, compressed_size);
    }

    let tokens_saved = original_size.saturating_sub(compressed_size) as u64 / 4;
    super::metrics::record_request(tokens_saved, compressed_size as u64);

    let upstream_url = build_upstream_url(&parts, upstream_base, default_path);
    let response = send_upstream(
        &state,
        &parts,
        &upstream_url,
        compressed_body,
        provider_label,
    )
    .await?;

    build_response(response, extra_stream_types).await
}

fn build_upstream_url(parts: &Parts, base: &str, default_path: &str) -> String {
    format!(
        "{base}{}",
        parts
            .uri
            .path_and_query()
            .map_or(default_path, axum::http::uri::PathAndQuery::as_str)
    )
}

async fn send_upstream(
    state: &ProxyState,
    parts: &Parts,
    url: &str,
    body: Vec<u8>,
    provider_label: &str,
) -> Result<reqwest::Response, StatusCode> {
    let mut req = state.client.request(parts.method.clone(), url);

    const ALLOWED_HEADERS: &[&str] = &[
        "authorization",
        "x-api-key",
        "content-type",
        "accept",
        "user-agent",
        "anthropic-version",
        "anthropic-beta",
        "anthropic-dangerous-direct-browser-access",
        "openai-organization",
        "openai-beta",
        "x-goog-api-key",
        "x-goog-api-client",
    ];
    for (key, value) in &parts.headers {
        let k = key.as_str().to_lowercase();
        if ALLOWED_HEADERS.contains(&k.as_str()) {
            req = req.header(key.clone(), value.clone());
        }
    }

    req.body(body).send().await.map_err(|e| {
        tracing::error!("lean-ctx proxy: {provider_label} upstream error: {e}");
        StatusCode::BAD_GATEWAY
    })
}

const FORWARDED_HEADERS: &[&str] = &[
    "content-type",
    "content-encoding",
    "x-request-id",
    "openai-organization",
    "openai-processing-ms",
    "openai-version",
    "anthropic-ratelimit-requests-limit",
    "anthropic-ratelimit-requests-remaining",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
    "retry-after",
    "x-ratelimit-limit-requests",
    "x-ratelimit-remaining-requests",
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-tokens",
    "cache-control",
];

async fn build_response(
    response: reqwest::Response,
    extra_stream_types: &[&str],
) -> Result<Response, StatusCode> {
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);
    let resp_headers = response.headers().clone();

    let is_stream = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            ct.contains("text/event-stream") || extra_stream_types.iter().any(|t| ct.contains(t))
        });

    if is_stream {
        let stream = response.bytes_stream();
        let body = Body::from_stream(stream);
        let mut resp = Response::builder().status(status);
        for (k, v) in &resp_headers {
            let ks = k.as_str().to_lowercase();
            if FORWARDED_HEADERS.contains(&ks.as_str()) {
                resp = resp.header(k, v);
            }
        }
        return resp
            .body(body)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    let resp_bytes = response
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut resp = Response::builder().status(status);
    for (k, v) in &resp_headers {
        let ks = k.as_str().to_lowercase();
        if FORWARDED_HEADERS.contains(&ks.as_str()) {
            resp = resp.header(k, v);
        }
    }
    resp.body(Body::from(resp_bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
