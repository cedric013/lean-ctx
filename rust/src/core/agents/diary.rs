use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_DIARY_ENTRIES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDiary {
    pub agent_id: String,
    pub agent_type: String,
    pub project_root: String,
    pub entries: Vec<DiaryEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub entry_type: DiaryEntryType,
    pub content: String,
    pub context: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiaryEntryType {
    Discovery,
    Decision,
    Blocker,
    Progress,
    Insight,
}

impl AgentDiary {
    pub fn new(agent_id: &str, agent_type: &str, project_root: &str) -> Self {
        let now = Utc::now();
        Self {
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            project_root: project_root.to_string(),
            entries: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_entry(&mut self, entry_type: DiaryEntryType, content: &str, context: Option<&str>) {
        self.entries.push(DiaryEntry {
            entry_type,
            content: content.to_string(),
            context: context.map(std::string::ToString::to_string),
            timestamp: Utc::now(),
        });
        if self.entries.len() > MAX_DIARY_ENTRIES {
            self.entries
                .drain(0..self.entries.len() - MAX_DIARY_ENTRIES);
        }
        self.updated_at = Utc::now();
    }

    pub fn format_summary(&self) -> String {
        if self.entries.is_empty() {
            return format!("Diary [{}]: empty", self.agent_id);
        }
        let mut out = format!(
            "Diary [{}] ({} entries):\n",
            self.agent_id,
            self.entries.len()
        );
        let now = Utc::now();
        for e in self.entries.iter().rev().take(10) {
            let age = (now - e.timestamp).num_minutes();
            let prefix = match e.entry_type {
                DiaryEntryType::Discovery => "FOUND",
                DiaryEntryType::Decision => "DECIDED",
                DiaryEntryType::Blocker => "BLOCKED",
                DiaryEntryType::Progress => "DONE",
                DiaryEntryType::Insight => "INSIGHT",
            };
            let ctx = e
                .context
                .as_deref()
                .map(|c| format!(" [{c}]"))
                .unwrap_or_default();
            out.push_str(&format!("  [{prefix}] {}{ctx} ({age}m ago)\n", e.content));
        }
        out
    }

    pub fn format_compact(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let items: Vec<String> = self
            .entries
            .iter()
            .rev()
            .take(5)
            .map(|e| {
                let prefix = match e.entry_type {
                    DiaryEntryType::Discovery => "F",
                    DiaryEntryType::Decision => "D",
                    DiaryEntryType::Blocker => "B",
                    DiaryEntryType::Progress => "P",
                    DiaryEntryType::Insight => "I",
                };
                format!("{prefix}:{}", truncate(&e.content, 50))
            })
            .collect();
        format!("diary:{}|{}", self.agent_id, items.join("|"))
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = diary_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.json", sanitize_filename(&self.agent_id)));
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())
    }

    pub fn load(agent_id: &str) -> Option<Self> {
        let dir = diary_dir().ok()?;
        let path = dir.join(format!("{}.json", sanitize_filename(agent_id)));
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn load_or_create(agent_id: &str, agent_type: &str, project_root: &str) -> Self {
        Self::load(agent_id).unwrap_or_else(|| Self::new(agent_id, agent_type, project_root))
    }

    pub fn list_all() -> Vec<(String, usize, DateTime<Utc>)> {
        let Ok(dir) = diary_dir() else {
            return Vec::new();
        };
        if !dir.exists() {
            return Vec::new();
        }
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("json")
                    && let Ok(content) = std::fs::read_to_string(entry.path())
                    && let Ok(diary) = serde_json::from_str::<AgentDiary>(&content)
                {
                    results.push((diary.agent_id, diary.entries.len(), diary.updated_at));
                }
            }
        }
        results.sort_by_key(|x| std::cmp::Reverse(x.2));
        results
    }

    /// Load every diary whose `project_root` matches `project_root`, most
    /// recently updated first. Used by skillify to mine a project's decisions
    /// and insights across all its agents (#290).
    pub fn load_all_for_project(project_root: &str) -> Vec<AgentDiary> {
        let Ok(dir) = diary_dir() else {
            return Vec::new();
        };
        if !dir.exists() {
            return Vec::new();
        }
        let want = project_root.trim_end_matches('/');
        let mut diaries: Vec<AgentDiary> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(entry.path())
                    && let Ok(diary) = serde_json::from_str::<AgentDiary>(&content)
                    && diary.project_root.trim_end_matches('/') == want
                {
                    diaries.push(diary);
                }
            }
        }
        diaries.sort_by_key(|d| std::cmp::Reverse(d.updated_at));
        diaries
    }
}

impl std::fmt::Display for DiaryEntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiaryEntryType::Discovery => write!(f, "discovery"),
            DiaryEntryType::Decision => write!(f, "decision"),
            DiaryEntryType::Blocker => write!(f, "blocker"),
            DiaryEntryType::Progress => write!(f, "progress"),
            DiaryEntryType::Insight => write!(f, "insight"),
        }
    }
}

fn diary_dir() -> Result<PathBuf, String> {
    let dir = crate::core::data_dir::lean_ctx_data_dir()?;
    Ok(dir.join("agents").join("diaries"))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..s.floor_char_boundary(max.saturating_sub(3))])
    }
}
