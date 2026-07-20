use axum::http::StatusCode;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::io::{Read, Write};

pub(super) fn decode_zstd_bounded(data: &[u8], max_bytes: usize) -> Result<Vec<u8>, StatusCode> {
    let decoder = zstd::Decoder::new(data).map_err(|e| {
        tracing::warn!("lean-ctx proxy: invalid zstd request body: {e}");
        StatusCode::BAD_REQUEST
    })?;
    read_bounded(decoder, max_bytes).inspect_err(|e| {
        tracing::warn!("lean-ctx proxy: zstd request decode failed: {e}");
    })
}

pub(super) fn encode_zstd(data: &[u8]) -> Result<Vec<u8>, StatusCode> {
    zstd::encode_all(data, 3).map_err(|e| {
        tracing::error!("lean-ctx proxy: zstd request encode failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

pub(super) fn decode_gzip_bounded(data: &[u8], max_bytes: usize) -> Result<Vec<u8>, StatusCode> {
    read_bounded(GzDecoder::new(data), max_bytes).inspect_err(|e| {
        tracing::warn!("lean-ctx proxy: gzip request decode failed: {e}");
    })
}

pub(super) fn encode_gzip(data: &[u8]) -> Result<Vec<u8>, StatusCode> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(|e| {
        tracing::error!("lean-ctx proxy: gzip request encode failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    encoder.finish().map_err(|e| {
        tracing::error!("lean-ctx proxy: gzip request encode failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

pub(super) fn read_bounded<R: Read>(reader: R, max_bytes: usize) -> Result<Vec<u8>, StatusCode> {
    let mut limited = reader.take(max_bytes as u64 + 1);
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if out.len() > max_bytes {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(out)
}
