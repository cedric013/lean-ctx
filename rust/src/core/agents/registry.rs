use chrono::Utc;
use std::collections::HashMap;

#[cfg(test)]
use super::diary::{AgentDiary, DiaryEntryType, truncate};
use super::persistence::{
    FileLock, agents_dir, generate_short_id, is_process_alive, mutate_persistent,
};
use super::{AgentEntry, AgentRegistry, AgentStatus, LogicalSessionPresence, ScratchpadEntry};
use crate::core::a2a::message::{MessagePriority, PrivacyLevel};

const MAX_SCRATCHPAD_ENTRIES: usize = 200;
pub(crate) const LOGICAL_SESSION_TTL_SECONDS: u64 = 180;
const LOGICAL_SESSION_SOURCE_MAX_BYTES: usize = 64;
const LOGICAL_SESSION_WORKSPACE_MAX_BYTES: usize = 4096;
const LOGICAL_SESSION_ID_MAX_BYTES: usize = 256;

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            scratchpad: Vec::new(),
            logical_sessions: Vec::new(),
            logical_session_telemetry_seen: false,
            updated_at: Utc::now(),
        }
    }

    pub fn register(&mut self, agent_type: &str, role: Option<&str>, project_root: &str) -> String {
        self.register_process(agent_type, role, project_root, std::process::id())
    }

    fn register_process(
        &mut self,
        agent_type: &str,
        role: Option<&str>,
        project_root: &str,
        pid: u32,
    ) -> String {
        let agent_id = format!("{}-{}-{}", agent_type, pid, generate_short_id());

        if let Some(existing) = self.agents.iter_mut().find(|a| a.pid == pid) {
            existing.last_active = Utc::now();
            existing.status = AgentStatus::Active;
            existing.agent_type = agent_type.to_string();
            existing.project_root = project_root.to_string();
            if let Some(r) = role {
                existing.role = Some(r.to_string());
            }
            return existing.agent_id.clone();
        }

        self.agents.push(AgentEntry {
            agent_id: agent_id.clone(),
            agent_type: agent_type.to_string(),
            role: role.map(std::string::ToString::to_string),
            project_root: project_root.to_string(),
            started_at: Utc::now(),
            last_active: Utc::now(),
            pid,
            status: AgentStatus::Active,
            status_message: None,
        });

        self.updated_at = Utc::now();
        crate::core::events::emit_agent_action(&agent_id, "register", None);
        agent_id
    }

    /// Atomically registers this MCP process in the shared on-disk registry.
    pub fn register_mcp_process(project_root: &str) -> Result<String, String> {
        mutate_persistent(|registry| {
            registry.cleanup_stale(24);
            registry.register("mcp", Some("context-engine"), project_root)
        })
    }

    /// Atomically refreshes a registered MCP process heartbeat.
    pub fn heartbeat_persistent(agent_id: &str) -> Result<(), String> {
        mutate_persistent(|registry| registry.update_heartbeat(agent_id))
    }

    /// Atomically marks a registered MCP process as finished.
    pub fn finish_persistent(agent_id: &str) -> Result<(), String> {
        mutate_persistent(|registry| {
            registry.set_status(agent_id, AgentStatus::Finished, Some("connection closed"));
        })
    }

    pub fn update_heartbeat(&mut self, agent_id: &str) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.agent_id == agent_id) {
            agent.last_active = Utc::now();
        }
    }

    pub fn set_status(&mut self, agent_id: &str, status: AgentStatus, message: Option<&str>) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.agent_id == agent_id) {
            agent.status = status;
            agent.status_message = message.map(std::string::ToString::to_string);
            agent.last_active = Utc::now();
        }
        self.updated_at = Utc::now();
    }
    /// Records explicit logical-session presence supplied by an owning editor
    /// integration. Tool activity is deliberately never treated as a session.
    pub fn open_or_heartbeat_logical_session(
        &mut self,
        source: &str,
        workspace: &str,
        session_id: &str,
    ) {
        let now = Utc::now();
        self.logical_session_telemetry_seen = true;
        if let Some(session) = self.logical_sessions.iter_mut().find(|session| {
            session.source == source
                && session.workspace == workspace
                && session.session_id == session_id
        }) {
            session.last_heartbeat = now;
        } else {
            self.logical_sessions.push(LogicalSessionPresence {
                source: source.to_string(),
                workspace: workspace.to_string(),
                session_id: session_id.to_string(),
                opened_at: now,
                last_heartbeat: now,
            });
        }
        self.updated_at = now;
    }

    pub fn close_logical_session(
        &mut self,
        source: &str,
        workspace: &str,
        session_id: &str,
    ) -> bool {
        self.logical_session_telemetry_seen = true;
        let previous_len = self.logical_sessions.len();
        self.logical_sessions.retain(|session| {
            session.source != source
                || session.workspace != workspace
                || session.session_id != session_id
        });
        let removed = self.logical_sessions.len() != previous_len;
        self.updated_at = Utc::now();
        removed
    }

    pub fn cleanup_stale_logical_sessions(&mut self, max_age_seconds: u64) {
        let seconds = i64::try_from(max_age_seconds).unwrap_or(i64::MAX);
        let cutoff = Utc::now() - chrono::Duration::seconds(seconds);
        self.logical_sessions
            .retain(|session| session.last_heartbeat >= cutoff);
        self.updated_at = Utc::now();
    }

    pub fn record_logical_session_presence(
        event: &str,
        source: &str,
        workspace: &str,
        session_id: &str,
    ) -> Result<(), String> {
        let valid_field = |value: &str, max_bytes: usize| {
            !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
        };
        if !valid_field(source, LOGICAL_SESSION_SOURCE_MAX_BYTES)
            || !valid_field(workspace, LOGICAL_SESSION_WORKSPACE_MAX_BYTES)
            || !valid_field(session_id, LOGICAL_SESSION_ID_MAX_BYTES)
        {
            return Err(
                "presence fields are empty, too long, or contain control characters".to_string(),
            );
        }
        if !matches!(event, "open" | "heartbeat" | "close") {
            return Err("event must be open, heartbeat, or close".to_string());
        }

        mutate_persistent(|registry| {
            registry.cleanup_stale_logical_sessions(LOGICAL_SESSION_TTL_SECONDS);
            match event {
                "open" | "heartbeat" => {
                    registry.open_or_heartbeat_logical_session(source, workspace, session_id);
                }
                "close" => {
                    registry.close_logical_session(source, workspace, session_id);
                }
                _ => unreachable!("event validated above"),
            }
        })
    }

    pub fn list_active(&self, project_root: Option<&str>) -> Vec<&AgentEntry> {
        self.agents
            .iter()
            .filter(|a| {
                if let Some(root) = project_root {
                    a.project_root == root && a.status != AgentStatus::Finished
                } else {
                    a.status != AgentStatus::Finished
                }
            })
            .collect()
    }

    pub fn list_all(&self) -> &[AgentEntry] {
        &self.agents
    }

    pub fn post_message(
        &mut self,
        from_agent: &str,
        to_agent: Option<&str>,
        category: &str,
        message: &str,
    ) -> String {
        self.post_message_full(
            from_agent,
            to_agent,
            category,
            message,
            PrivacyLevel::default(),
            MessagePriority::default(),
            None,
        )
    }

    pub fn post_message_full(
        &mut self,
        from_agent: &str,
        to_agent: Option<&str>,
        category: &str,
        message: &str,
        privacy: PrivacyLevel,
        priority: MessagePriority,
        ttl_hours: Option<u64>,
    ) -> String {
        let id = generate_short_id();
        let expires_at = ttl_hours.map(|h| Utc::now() + chrono::Duration::hours(h as i64));
        self.scratchpad.push(ScratchpadEntry {
            id: id.clone(),
            from_agent: from_agent.to_string(),
            to_agent: to_agent.map(std::string::ToString::to_string),
            task_id: None,
            category: category.to_string(),
            priority,
            privacy,
            message: message.to_string(),
            metadata: HashMap::new(),
            project_root: None,
            timestamp: Utc::now(),
            read_by: vec![from_agent.to_string()],
            expires_at,
        });

        if self.scratchpad.len() > MAX_SCRATCHPAD_ENTRIES {
            self.scratchpad
                .drain(0..self.scratchpad.len() - MAX_SCRATCHPAD_ENTRIES);
        }

        self.updated_at = Utc::now();
        id
    }

    pub fn read_messages(&mut self, agent_id: &str) -> Vec<&ScratchpadEntry> {
        let unread: Vec<usize> = self
            .scratchpad
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                !e.read_by.contains(&agent_id.to_string())
                    && (e.to_agent.is_none() || e.to_agent.as_deref() == Some(agent_id))
            })
            .map(|(i, _)| i)
            .collect();

        for i in &unread {
            self.scratchpad[*i].read_by.push(agent_id.to_string());
        }

        self.scratchpad
            .iter()
            .filter(|e| e.to_agent.is_none() || e.to_agent.as_deref() == Some(agent_id))
            .filter(|e| e.from_agent != agent_id)
            .collect()
    }

    pub fn read_unread(&mut self, agent_id: &str) -> Vec<&ScratchpadEntry> {
        let unread_indices: Vec<usize> = self
            .scratchpad
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                !e.read_by.contains(&agent_id.to_string())
                    && e.from_agent != agent_id
                    && (e.to_agent.is_none() || e.to_agent.as_deref() == Some(agent_id))
            })
            .map(|(i, _)| i)
            .collect();

        for i in &unread_indices {
            self.scratchpad[*i].read_by.push(agent_id.to_string());
        }

        self.updated_at = Utc::now();

        self.scratchpad
            .iter()
            .filter(|e| {
                e.from_agent != agent_id
                    && (e.to_agent.is_none() || e.to_agent.as_deref() == Some(agent_id))
                    && e.read_by.contains(&agent_id.to_string())
                    && e.read_by.iter().filter(|r| *r == agent_id).count() == 1
            })
            .collect()
    }

    pub fn cleanup_stale(&mut self, max_age_hours: u64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours as i64);

        for agent in &mut self.agents {
            if agent.status == AgentStatus::Finished {
                continue;
            }
            if !is_process_alive(agent.pid) {
                agent.status = AgentStatus::Finished;
            }
        }

        // Remove finished agents older than the cutoff to keep recent history visible.
        // Drop each retired agent's budget entry too — a finished/dead agent can't read
        // again, so removing its budget loses no live enforcement and bounds BUDGETS.
        self.agents.retain(|a| {
            let retire = a.status == AgentStatus::Finished && a.last_active < cutoff;
            if retire {
                crate::core::agent_budget::remove(&a.agent_id);
            }
            !retire
        });

        self.updated_at = Utc::now();
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = agents_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let lock_path = dir.join("registry.lock");
        let _lock = FileLock::acquire(&lock_path)?;

        self.save_locked(&dir)
    }

    fn save_locked(&self, dir: &std::path::Path) -> Result<(), String> {
        let path = dir.join("registry.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())
    }

    pub fn load() -> Option<Self> {
        let dir = agents_dir().ok()?;
        let path = dir.join("registry.json");
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn load_or_create() -> Self {
        Self::load().unwrap_or_default()
    }

    /// Atomically load, mutate, and persist the registry under a single file
    /// lock. `load_or_create()` + mutate + `save()` is a read-modify-write
    /// race: `save()` only locks the final write, so two concurrent callers
    /// (two MCP sessions registering, or the dashboard's own poll-triggered
    /// `cleanup_stale` + save) can each load a stale snapshot and the last
    /// writer silently drops the other's changes — e.g. a second session's
    /// registration vanishing from the dashboard. Holding the lock across
    /// the re-read closes that window: the read inside always sees the
    /// latest on-disk state.
    pub fn mutate_locked<T>(f: impl FnOnce(&mut Self) -> T) -> Result<(Self, T), String> {
        let dir = agents_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let lock_path = dir.join("registry.lock");
        let _lock = FileLock::acquire(&lock_path)?;

        let mut registry = Self::load().unwrap_or_default();
        let out = f(&mut registry);
        registry.save_locked(&dir)?;
        Ok((registry, out))
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_list() {
        let mut reg = AgentRegistry::new();
        let id = reg.register("cursor", Some("dev"), "/tmp/project");
        assert!(!id.is_empty());
        assert_eq!(reg.list_active(None).len(), 1);
        assert_eq!(reg.list_active(None)[0].agent_type, "cursor");
    }

    #[test]
    fn reregister_same_pid() {
        let mut reg = AgentRegistry::new();
        let id1 = reg.register("cursor", Some("dev"), "/tmp/project");
        let id2 = reg.register("cursor", Some("review"), "/tmp/project");
        assert_eq!(id1, id2);
        assert_eq!(reg.agents.len(), 1);
        assert_eq!(reg.agents[0].role, Some("review".to_string()));
    }

    #[test]
    fn post_and_read_messages() {
        let mut reg = AgentRegistry::new();
        reg.post_message("agent-a", None, "finding", "Found a bug in auth.rs");
        reg.post_message("agent-b", Some("agent-a"), "request", "Please review");

        let msgs = reg.read_unread("agent-a");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].category, "request");
    }

    #[test]
    fn set_status() {
        let mut reg = AgentRegistry::new();
        let id = reg.register("claude", None, "/tmp/project");
        reg.set_status(&id, AgentStatus::Idle, Some("waiting for review"));
        assert_eq!(reg.agents[0].status, AgentStatus::Idle);
        assert_eq!(
            reg.agents[0].status_message,
            Some("waiting for review".to_string())
        );
    }

    #[test]
    fn broadcast_message() {
        let mut reg = AgentRegistry::new();
        reg.post_message("agent-a", None, "status", "Starting refactor");

        let msgs_b = reg.read_unread("agent-b");
        assert_eq!(msgs_b.len(), 1);
        assert_eq!(msgs_b[0].message, "Starting refactor");

        let msgs_a = reg.read_unread("agent-a");
        assert!(msgs_a.is_empty());
    }

    #[test]
    fn diary_add_and_format() {
        let mut diary = AgentDiary::new("test-agent-001", "cursor", "/tmp/project");
        diary.add_entry(
            DiaryEntryType::Discovery,
            "Found auth module at src/auth.rs",
            Some("auth"),
        );
        diary.add_entry(
            DiaryEntryType::Decision,
            "Use JWT RS256 for token signing",
            None,
        );
        diary.add_entry(
            DiaryEntryType::Progress,
            "Implemented login endpoint",
            Some("auth"),
        );

        assert_eq!(diary.entries.len(), 3);

        let summary = diary.format_summary();
        assert!(summary.contains("test-agent-001"));
        assert!(summary.contains("FOUND"));
        assert!(summary.contains("DECIDED"));
        assert!(summary.contains("DONE"));
    }

    #[test]
    fn diary_compact_format() {
        let mut diary = AgentDiary::new("test-agent-002", "claude", "/tmp/project");
        diary.add_entry(DiaryEntryType::Insight, "DB queries are N+1", None);
        diary.add_entry(
            DiaryEntryType::Blocker,
            "Missing API credentials",
            Some("deploy"),
        );

        let compact = diary.format_compact();
        assert!(compact.contains("diary:test-agent-002"));
        assert!(compact.contains("B:Missing API credentials"));
        assert!(compact.contains("I:DB queries are N+1"));
    }

    #[test]
    fn diary_entry_types() {
        let types = vec![
            DiaryEntryType::Discovery,
            DiaryEntryType::Decision,
            DiaryEntryType::Blocker,
            DiaryEntryType::Progress,
            DiaryEntryType::Insight,
        ];
        for t in types {
            assert!(!format!("{t}").is_empty());
        }
    }

    #[test]
    fn diary_truncation() {
        let mut diary = AgentDiary::new("test-agent", "cursor", "/tmp");
        for i in 0..150 {
            diary.add_entry(DiaryEntryType::Progress, &format!("Step {i}"), None);
        }
        assert!(diary.entries.len() <= 100);
    }

    #[test]
    fn truncate_utf8_emoji_no_panic() {
        let result = truncate("Agent 🤖 Name ist lang genug", 15);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_utf8_cyrillic_no_panic() {
        let result = truncate("агент выполняет длинную задачу", 15);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_short_utf8_unchanged() {
        assert_eq!(truncate("短い", 20), "短い");
    }

    fn test_entry(agent_id: &str, project_root: &str, pid: u32) -> AgentEntry {
        let now = Utc::now();
        AgentEntry {
            agent_id: agent_id.to_string(),
            agent_type: "cursor".to_string(),
            role: Some("dev".to_string()),
            project_root: project_root.to_string(),
            started_at: now,
            last_active: now,
            pid,
            status: AgentStatus::Active,
            status_message: None,
        }
    }

    /// #419: the wake-up briefing scopes agents to the current project via
    /// `list_active(Some(root))`. Peers working on *other* projects must never
    /// leak into the briefing.
    #[test]
    fn list_active_scopes_to_project_root() {
        let mut reg = AgentRegistry::new();
        reg.agents
            .push(test_entry("a-1", "/proj/a", std::process::id()));
        reg.agents
            .push(test_entry("b-1", "/proj/b", std::process::id()));

        let active_a = reg.list_active(Some("/proj/a"));
        assert_eq!(active_a.len(), 1);
        assert_eq!(active_a[0].agent_id, "a-1");

        // Unscoped still sees both.
        assert_eq!(reg.list_active(None).len(), 2);
    }

    /// #419: a crashed/exited MCP process leaves an `Active` entry behind.
    /// `cleanup_stale` must flip it to `Finished` (regardless of age) so
    /// `list_active` no longer surfaces it as a live peer — the ghost the
    /// briefing used to show. Previously `#[cfg(unix)]`-only, which is why
    /// the non-unix `is_process_alive` hardcoded-`true` regression (see its
    /// doc comment) shipped unnoticed: this exact test never ran on Windows.
    #[test]
    fn cleanup_stale_prunes_dead_pid_from_active_list() {
        // Reap a child so its PID is guaranteed dead at assertion time.
        let reaped = {
            let mut cmd = if cfg!(windows) {
                let mut c = std::process::Command::new("cmd");
                c.args(["/C", "exit"]);
                c
            } else {
                std::process::Command::new("true")
            };
            let mut child = cmd.spawn().expect("spawn short-lived helper process");
            let pid = child.id();
            child.wait().expect("reap helper process");
            pid
        };

        let mut reg = AgentRegistry::new();
        reg.agents.push(test_entry("ghost", "/proj/a", reaped));
        reg.agents
            .push(test_entry("live", "/proj/a", std::process::id()));

        reg.cleanup_stale(24);

        let ids: Vec<&str> = reg
            .list_active(Some("/proj/a"))
            .iter()
            .map(|a| a.agent_id.as_str())
            .collect();
        assert!(ids.contains(&"live"), "live same-project agent must remain");
        assert!(
            !ids.contains(&"ghost"),
            "dead-pid agent must be pruned from the active list (#419)"
        );
    }

    /// Regression: concurrent load-mutate-save cycles must not silently drop
    /// each other's changes. Before `mutate_locked`, `save()` only locked the
    /// final write — the preceding `load()` was unlocked, so a second writer
    /// could load a stale snapshot and overwrite the first writer's addition
    /// (e.g. a second Claude Code session's agent registration vanishing
    /// from the dashboard).
    #[test]
    fn mutate_locked_survives_concurrent_writers() {
        let _iso = crate::core::data_dir::isolated_data_dir();

        let handles: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    AgentRegistry::mutate_locked(|registry| {
                        registry.agents.push(AgentEntry {
                            agent_id: format!("agent-{i}"),
                            agent_type: "test".to_string(),
                            role: None,
                            project_root: "/tmp/project".to_string(),
                            started_at: Utc::now(),
                            last_active: Utc::now(),
                            pid: 10_000 + i,
                            status: AgentStatus::Active,
                            status_message: None,
                        });
                    })
                })
            })
            .collect();

        for h in handles {
            h.join()
                .expect("writer thread must not panic")
                .expect("mutate_locked must succeed");
        }

        let registry = AgentRegistry::load_or_create();
        assert_eq!(
            registry.agents.len(),
            8,
            "all 8 concurrent registrations must survive, got {}",
            registry.agents.len()
        );
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    #[test]
    fn persistent_presence_preserves_multiple_processes_and_lifecycle() {
        let isolated = crate::core::data_dir::isolated_data_dir();
        let mut registry = AgentRegistry::new();
        let first = registry.register_process("mcp", Some("context-engine"), "/project", 101);
        let second = registry.register_process("mcp", Some("context-engine"), "/project", 202);
        registry.save().expect("save registry");

        assert_ne!(first, second);
        assert_eq!(AgentRegistry::load().expect("registry").agents.len(), 2);

        AgentRegistry::heartbeat_persistent(&first).expect("heartbeat");
        AgentRegistry::finish_persistent(&second).expect("finish");
        let loaded = AgentRegistry::load().expect("registry");
        assert_eq!(
            loaded
                .agents
                .iter()
                .find(|agent| agent.agent_id == second)
                .expect("second agent")
                .status,
            AgentStatus::Finished
        );
        assert!(isolated.path().join("agents/registry.json").exists());
    }

    #[test]
    fn reregistering_process_refreshes_metadata_without_duplication() {
        let mut registry = AgentRegistry::new();
        let first = registry.register_process("unknown", None, "/old", 303);
        let second = registry.register_process("mcp", Some("context-engine"), "/new", 303);

        assert_eq!(first, second);
        assert_eq!(registry.agents.len(), 1);
        assert_eq!(registry.agents[0].agent_type, "mcp");
        assert_eq!(registry.agents[0].project_root, "/new");
        assert_eq!(registry.agents[0].role.as_deref(), Some("context-engine"));
    }

    #[test]
    fn logical_sessions_are_keyed_independently_of_transport_processes() {
        let mut registry = AgentRegistry::new();
        registry.register_process("mcp", Some("context-engine"), "/project", 303);
        registry.open_or_heartbeat_logical_session("vscode", "/project", "chat-a");
        registry.open_or_heartbeat_logical_session("vscode", "/project", "chat-b");
        let opened_at = registry.logical_sessions[0].opened_at;

        registry.open_or_heartbeat_logical_session("vscode", "/project", "chat-a");

        assert_eq!(registry.agents.len(), 1);
        assert_eq!(registry.logical_sessions.len(), 2);
        assert_eq!(registry.logical_sessions[0].opened_at, opened_at);
        assert!(registry.logical_session_telemetry_seen);
        assert!(registry.close_logical_session("vscode", "/project", "chat-b"));
        assert_eq!(registry.logical_sessions.len(), 1);
    }

    #[test]
    fn persistent_logical_session_presence_validates_and_roundtrips() {
        let _isolated = crate::core::data_dir::isolated_data_dir();

        AgentRegistry::record_logical_session_presence(
            "open",
            "vscode",
            "/project",
            "editor-session-a",
        )
        .expect("open presence");

        let registry = AgentRegistry::load().expect("persisted registry");
        assert_eq!(registry.logical_sessions.len(), 1);
        assert_eq!(registry.logical_sessions[0].session_id, "editor-session-a");
        assert!(registry.logical_session_telemetry_seen);

        assert!(
            AgentRegistry::record_logical_session_presence(
                "invalid",
                "vscode",
                "/project",
                "editor-session-a",
            )
            .is_err()
        );
        assert!(
            AgentRegistry::record_logical_session_presence(
                "heartbeat",
                "",
                "/project",
                "editor-session-a",
            )
            .is_err()
        );

        AgentRegistry::record_logical_session_presence(
            "close",
            "vscode",
            "/project",
            "editor-session-a",
        )
        .expect("close presence");
        assert!(
            AgentRegistry::load()
                .expect("persisted registry")
                .logical_sessions
                .is_empty()
        );
    }

    #[test]
    fn logical_session_expiry_is_bounded_by_heartbeat_not_tool_activity() {
        let mut registry = AgentRegistry::new();
        registry.open_or_heartbeat_logical_session("vscode", "/project", "chat-a");
        registry.logical_sessions[0].last_heartbeat = Utc::now() - chrono::Duration::seconds(181);

        registry.cleanup_stale_logical_sessions(180);

        assert!(registry.logical_sessions.is_empty());
        assert!(registry.logical_session_telemetry_seen);
    }

    #[test]
    fn legacy_registry_deserializes_without_claiming_session_telemetry() {
        let registry: AgentRegistry = serde_json::from_str(
            r#"{"agents":[],"scratchpad":[],"updated_at":"2026-01-01T00:00:00Z"}"#,
        )
        .expect("legacy registry");

        assert!(registry.logical_sessions.is_empty());
        assert!(!registry.logical_session_telemetry_seen);
    }
}
