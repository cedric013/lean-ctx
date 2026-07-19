//! AgentGateway OCLA trait (SHARED / DIM 4).
//!
//! Defines the validated relay boundary for agent-to-agent communication.
//! The trait establishes admission control but does NOT perform network
//! transport, resolve capsule content, or grant commercial authority.

use crate::core::context_capsule::AgentEnvelopeV1;

/// Object-safe relay boundary for multi-agent capsule delivery.
pub trait AgentGateway: Send + Sync {
    /// Validate and admit a relay envelope. Returns the envelope unchanged
    /// if admission succeeds. Implementations may emit bus events and
    /// journal the admission, but must not resolve capsule content.
    fn relay_agent(&self, envelope: AgentEnvelopeV1) -> Result<AgentEnvelopeV1, AgentGatewayError>;

    /// Check if a capsule ref is eligible for relay (pre-admission check).
    fn can_relay(&self, capsule_ref: &str, to_agent_id: &str) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum AgentGatewayError {
    #[error("relay rejected: {0}")]
    Rejected(String),
    #[error("invalid envelope: {0}")]
    InvalidEnvelope(String),
    #[error("bus emission failed: {0}")]
    BusError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _assert_object_safe(_: &dyn AgentGateway) {}

    struct MockGateway;

    impl AgentGateway for MockGateway {
        fn relay_agent(
            &self,
            envelope: AgentEnvelopeV1,
        ) -> Result<AgentEnvelopeV1, AgentGatewayError> {
            Ok(envelope)
        }
        fn can_relay(&self, _capsule_ref: &str, _to_agent_id: &str) -> bool {
            true
        }
    }

    #[test]
    fn trait_is_object_safe() {
        let gw = MockGateway;
        let _dyn: &dyn AgentGateway = &gw;
    }
}
