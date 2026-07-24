use super::{AgentRegistry, ScratchpadEntry};
use crate::core::a2a::message::{MessagePriority, PrivacyLevel};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFact {
    pub from_agent: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub received_by: Vec<String>,
}

impl AgentRegistry {
    pub fn share_knowledge(&mut self, from: &str, category: &str, facts: &[(String, String)]) {
        for (key, value) in facts {
            self.scratchpad.push(ScratchpadEntry {
                id: format!("knowledge-{}", chrono::Utc::now().timestamp_millis()),
                from_agent: from.to_string(),
                to_agent: None,
                task_id: None,
                category: category.to_string(),
                priority: MessagePriority::default(),
                privacy: PrivacyLevel::Team,
                message: format!("[knowledge] {key}={value}"),
                metadata: HashMap::new(),
                project_root: None,
                timestamp: Utc::now(),
                read_by: Vec::new(),
                expires_at: None,
            });
        }
        let shared_path = Self::shared_knowledge_path();
        let mut existing: Vec<SharedFact> = std::fs::read_to_string(&shared_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        for (key, value) in facts {
            existing.push(SharedFact {
                from_agent: from.to_string(),
                category: category.to_string(),
                key: key.clone(),
                value: value.clone(),
                timestamp: Utc::now(),
                received_by: Vec::new(),
            });
        }

        if existing.len() > 500 {
            existing.drain(..existing.len() - 500);
        }
        if let Ok(json) = serde_json::to_string_pretty(&existing) {
            let _ = std::fs::write(&shared_path, json);
        }
    }

    pub fn receive_shared_knowledge(&mut self, agent_id: &str) -> Vec<SharedFact> {
        let shared_path = Self::shared_knowledge_path();
        let mut all: Vec<SharedFact> = std::fs::read_to_string(&shared_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let mut new_facts = Vec::new();
        for fact in &mut all {
            if fact.from_agent != agent_id && !fact.received_by.contains(&agent_id.to_string()) {
                fact.received_by.push(agent_id.to_string());
                new_facts.push(fact.clone());
            }
        }

        if !new_facts.is_empty()
            && let Ok(json) = serde_json::to_string_pretty(&all)
        {
            let _ = std::fs::write(&shared_path, json);
        }
        new_facts
    }

    fn shared_knowledge_path() -> PathBuf {
        // GH #439: route through the typed data resolver so a post-migration
        // split install writes to $XDG_DATA_HOME, not a re-created ~/.lean-ctx.
        crate::core::paths::data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("shared_knowledge.json")
    }
}
