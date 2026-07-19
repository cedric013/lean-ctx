//! Local reference transport for authenticated OCLA A2A capsule delivery (P11).
//!
//! In-process delivery queue that accepts signed, payload-free capsule manifests.
//! Bytes are measured exactly after serialization; the token count is a local
//! tokenizer proxy. This is delivery evidence only — never convertible into
//! compression savings or provider billing.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::core::context_capsule::SignedContextCapsuleV1;

const MAX_INBOX_SIZE: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveredTokenCountKindV1 {
    LocalTokenizerProxy,
}

/// Delivery receipt measured after a concrete local queue write.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentRelayDeliveryV1 {
    pub relay_id: String,
    pub capsule_ref: String,
    pub recipient_agent_id: String,
    pub delivered_bytes: u64,
    pub delivered_tokens: u64,
    pub token_count_kind: DeliveredTokenCountKindV1,
}

#[derive(Clone)]
struct QueuedCapsule {
    signed_json: String,
    delivery: AgentRelayDeliveryV1,
}

/// Bounded local transport. Deployments must place a durable, authenticated
/// transport behind the same receipt semantics before remote use.
pub struct LocalSignedCapsuleTransport {
    inboxes: Mutex<BTreeMap<String, VecDeque<QueuedCapsule>>>,
}

impl Default for LocalSignedCapsuleTransport {
    fn default() -> Self {
        Self {
            inboxes: Mutex::new(BTreeMap::new()),
        }
    }
}

impl LocalSignedCapsuleTransport {
    /// Deliver a signed capsule to a recipient's inbox.
    pub fn deliver(
        &self,
        signed: &SignedContextCapsuleV1,
        recipient_agent_id: &str,
    ) -> Result<AgentRelayDeliveryV1, TransportError> {
        signed
            .validate_structure()
            .map_err(|e| TransportError::Validation(e.to_string()))?;

        let envelope = signed
            .capsule
            .agent_envelope(recipient_agent_id)
            .map_err(|e| TransportError::Validation(e.to_string()))?;

        let signed_json = serde_json::to_string(signed)
            .map_err(|e| TransportError::Serialization(e.to_string()))?;

        let delivered_bytes = u64::try_from(signed_json.len()).unwrap_or(u64::MAX);
        let delivered_tokens = estimate_tokens(&signed_json);

        let delivery = AgentRelayDeliveryV1 {
            relay_id: envelope.relay_id.clone(),
            capsule_ref: envelope.capsule_ref,
            recipient_agent_id: recipient_agent_id.to_string(),
            delivered_bytes,
            delivered_tokens,
            token_count_kind: DeliveredTokenCountKindV1::LocalTokenizerProxy,
        };

        let mut inboxes = self
            .inboxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let inbox = inboxes.entry(recipient_agent_id.to_string()).or_default();

        if inbox.len() >= MAX_INBOX_SIZE {
            inbox.pop_front();
        }

        inbox.push_back(QueuedCapsule {
            signed_json,
            delivery: delivery.clone(),
        });

        Ok(delivery)
    }

    /// Receive the next capsule from an agent's inbox.
    pub fn receive(
        &self,
        agent_id: &str,
    ) -> Option<(SignedContextCapsuleV1, AgentRelayDeliveryV1)> {
        let mut inboxes = self
            .inboxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let inbox = inboxes.get_mut(agent_id)?;
        let queued = inbox.pop_front()?;
        let signed: SignedContextCapsuleV1 = serde_json::from_str(&queued.signed_json).ok()?;
        Some((signed, queued.delivery))
    }

    /// Peek at inbox depth for a given agent.
    pub fn inbox_depth(&self, agent_id: &str) -> usize {
        let inboxes = self
            .inboxes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inboxes.get(agent_id).map_or(0, VecDeque::len)
    }
}

/// Simple token estimation: ~4 chars per token (conservative for JSON).
fn estimate_tokens(text: &str) -> u64 {
    u64::try_from(text.len() / 4).unwrap_or(u64::MAX).max(1)
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("serialization failed: {0}")]
    Serialization(String),
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context_capsule::*;

    fn test_signed_capsule() -> SignedContextCapsuleV1 {
        let mut capsule = ContextCapsuleV1 {
            schema_version: CONTEXT_CAPSULE_SCHEMA_VERSION,
            capsule_id: "capsule:pending".to_string(),
            request_id: "request:1".to_string(),
            session_id: "session:1".to_string(),
            agent_id: "sender-agent".to_string(),
            intent_ref: "intent:test".to_string(),
            task_ref: "task:1".to_string(),
            expected_outcome_ref: "outcome:pass".to_string(),
            acceptance_criteria_refs: vec!["criteria:ci-green".to_string()],
            references: vec![ContextCapsuleReferenceV1 {
                kind: CapsuleReferenceKindV1::File,
                content_ref: "blake3:file1".to_string(),
                freshness_ref: "freshness:1".to_string(),
                recovery_ref: None,
            }],
            finding_refs: vec![],
            decision_refs: vec![],
            uncertainty_refs: vec![],
            negative_result_refs: vec![],
            source_ref: "source:test".to_string(),
            policy_ref: "policy:default".to_string(),
            contract_ref: "contract:v1".to_string(),
            freshness_ref: "freshness:now".to_string(),
            sensitivity: CapsuleSensitivityV1::Internal,
            allowed_agent_ids: vec!["recipient-agent".to_string()],
            budget: ContextCapsuleBudgetV1 {
                tokens_used: 50,
                tokens_remaining: 950,
                cost_micros_used: 5,
                cost_micros_remaining: 95,
                latency_ms_used: 10,
                latency_ms_remaining: 90,
            },
            chain: ContextCapsuleChainV1 {
                chain_id: "chain:test".to_string(),
                parent_capsule_ref: None,
                owner_agent_id: "sender-agent".to_string(),
                attribution_ref: "attribution:1".to_string(),
                hop: 0,
            },
            quality_signal_refs: vec![],
            recovery_refs: vec![],
            delta_from: None,
        };
        capsule.assign_capsule_id().unwrap();
        let keypair = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        SignedContextCapsuleV1::sign(&capsule, &keypair).unwrap()
    }

    #[test]
    fn deliver_and_receive_roundtrip() {
        let transport = LocalSignedCapsuleTransport::default();
        let signed = test_signed_capsule();

        let delivery = transport.deliver(&signed, "recipient-agent").unwrap();
        assert!(delivery.delivered_bytes > 0);
        assert!(delivery.delivered_tokens > 0);
        assert_eq!(delivery.recipient_agent_id, "recipient-agent");
        assert_eq!(transport.inbox_depth("recipient-agent"), 1);

        let (received, receipt) = transport.receive("recipient-agent").unwrap();
        assert_eq!(received.capsule.capsule_id, signed.capsule.capsule_id);
        assert_eq!(receipt.relay_id, delivery.relay_id);
        assert_eq!(transport.inbox_depth("recipient-agent"), 0);
    }

    #[test]
    fn receive_from_empty_inbox_returns_none() {
        let transport = LocalSignedCapsuleTransport::default();
        assert!(transport.receive("nobody").is_none());
    }

    #[test]
    fn rejects_unauthorized_recipient() {
        let transport = LocalSignedCapsuleTransport::default();
        let signed = test_signed_capsule();
        assert!(transport.deliver(&signed, "unauthorized-agent").is_err());
    }
}
