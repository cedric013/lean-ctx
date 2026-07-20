//! Newline-delimited streaming frames for the public OCLA wire contract.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::types::{CanonicalTokenEnvelopeV1, OclaError, OclaResult};
use super::wire::MAX_OCLA_WIRE_BYTES;

/// One newline-delimited message in an OCLA stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum StreamFrame {
    Data(CanonicalTokenEnvelopeV1),
    Heartbeat,
    Cancel,
    Done,
}

impl StreamFrame {
    fn validate(&self) -> OclaResult<()> {
        if let Self::Data(envelope) = self {
            envelope.validate()?;
        }
        Ok(())
    }
}

/// Runtime limits for one OCLA stream.
///
/// `deadline_ms` is an absolute Unix timestamp in milliseconds. A stream may
/// have at most `max_backpressure_frames` frames queued for delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamConfig {
    pub deadline_ms: u64,
    pub max_backpressure_frames: usize,
}

/// Encodes one stream frame as a newline-delimited JSON record.
pub fn encode_frame(frame: &StreamFrame) -> OclaResult<String> {
    frame.validate()?;
    let json = serde_json::to_string(frame).map_err(|error| {
        OclaError::InvalidRequest(format!("cannot encode stream frame: {error}"))
    })?;
    let line = format!("{json}\n");
    if line.len() > MAX_OCLA_WIRE_BYTES {
        return Err(OclaError::InvalidRequest(format!(
            "stream frame exceeds {MAX_OCLA_WIRE_BYTES} bytes"
        )));
    }
    Ok(line)
}

/// Decodes one newline-delimited JSON record into a stream frame.
pub fn decode_frame(line: &str) -> OclaResult<StreamFrame> {
    if line.len() > MAX_OCLA_WIRE_BYTES {
        return Err(OclaError::InvalidRequest(format!(
            "stream frame exceeds {MAX_OCLA_WIRE_BYTES} bytes"
        )));
    }
    let line = line.trim_end_matches(['\r', '\n']);
    let frame: StreamFrame = serde_json::from_str(line).map_err(|error| {
        OclaError::InvalidRequest(format!("cannot decode stream frame: {error}"))
    })?;
    frame.validate()?;
    Ok(frame)
}

/// Rejects a stream whose absolute deadline has passed.
pub fn validate_deadline(config: &StreamConfig) -> OclaResult<()> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| OclaError::InvalidRequest(format!("cannot read system clock: {error}")))?
        .as_millis();
    if now_ms >= u128::from(config.deadline_ms) {
        return Err(OclaError::InvalidRequest("stream deadline expired".into()));
    }
    Ok(())
}

impl StreamConfig {
    /// Rejects a queue that exceeds the configured backpressure limit.
    pub fn validate_backpressure(&self, queued_frames: usize) -> OclaResult<()> {
        if queued_frames > self.max_backpressure_frames {
            return Err(OclaError::InvalidRequest(format!(
                "stream backpressure limit exceeded: {queued_frames} queued, limit {}",
                self.max_backpressure_frames
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ocla::{
        CANONICAL_TOKEN_ENVELOPE_SCHEMA_VERSION, OclaRequestContext, TokenBalanceV1,
        TokenEnvelopeSurface, TokenFlowDirection,
    };

    fn envelope() -> CanonicalTokenEnvelopeV1 {
        CanonicalTokenEnvelopeV1 {
            schema_version: CANONICAL_TOKEN_ENVELOPE_SCHEMA_VERSION,
            context: OclaRequestContext {
                request_id: "request-1".into(),
                session_id: "session-1".into(),
                agent_id: "agent-1".into(),
                content_ref: "blake3:content".into(),
                tenant_id: None,
            },
            surface: TokenEnvelopeSurface::Proxy,
            direction: TokenFlowDirection::Input,
            provider: "openai".into(),
            model: "gpt-5".into(),
            token_balance: TokenBalanceV1 {
                original_tokens: 100,
                materialized_tokens: 80,
                delivered_tokens: 60,
                provider_billed_tokens: 60,
            },
            route_ref: None,
            policy_ref: None,
            idempotency_key: "request-1:input".into(),
        }
    }

    #[test]
    fn data_frame_roundtrips_with_a_trailing_newline() {
        let original = StreamFrame::Data(envelope());
        let encoded = encode_frame(&original).expect("encode data frame");

        assert!(encoded.ends_with('\n'));
        assert_eq!(decode_frame(&encoded).expect("decode data frame"), original);
    }

    #[test]
    fn cancel_frame_roundtrips_as_a_payload_free_tag() {
        let encoded = encode_frame(&StreamFrame::Cancel).expect("encode cancel frame");

        assert_eq!(encoded, "{\"type\":\"cancel\"}\n");
        assert_eq!(
            decode_frame(&encoded).expect("decode cancel frame"),
            StreamFrame::Cancel
        );
    }

    #[test]
    fn expired_deadline_is_rejected_and_future_deadline_is_allowed() {
        let expired = StreamConfig {
            deadline_ms: 0,
            max_backpressure_frames: 1,
        };
        assert!(validate_deadline(&expired).is_err());

        let future = StreamConfig {
            deadline_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_millis()
                .try_into()
                .expect("test deadline fits u64"),
            max_backpressure_frames: 1,
        };
        assert!(validate_deadline(&future).is_err());

        let future = StreamConfig {
            deadline_ms: future.deadline_ms.saturating_add(60_000),
            ..future
        };
        assert!(validate_deadline(&future).is_ok());
    }

    #[test]
    fn backpressure_limit_allows_boundary_and_rejects_overflow() {
        let config = StreamConfig {
            deadline_ms: u64::MAX,
            max_backpressure_frames: 2,
        };

        assert!(config.validate_backpressure(2).is_ok());
        assert!(config.validate_backpressure(3).is_err());
    }

    #[test]
    fn invalid_data_frame_is_rejected_before_wire_encoding() {
        let mut invalid = envelope();
        invalid.token_balance.delivered_tokens = 81;

        assert!(encode_frame(&StreamFrame::Data(invalid)).is_err());
    }
}
