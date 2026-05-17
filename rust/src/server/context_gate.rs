use crate::core::context_field::{ContextItemId, ContextState};
use crate::core::context_ledger::{ContextLedger, PressureAction};
use crate::core::context_overlay::{OverlayOp, OverlayStore};

#[derive(Debug, Clone)]
pub struct PreDispatchResult {
    pub overridden_mode: Option<String>,
    pub reason: Option<&'static str>,
    pub pressure_downgraded: bool,
    pub budget_blocked: bool,
    pub budget_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PostDispatchResult {
    pub eviction_hint: Option<String>,
    pub elicitation_hint: Option<String>,
    pub resource_changed: bool,
}

pub fn pre_dispatch_read(
    path: &str,
    requested_mode: &str,
    task: Option<&str>,
    project_root: Option<&str>,
    pressure: Option<&PressureAction>,
) -> PreDispatchResult {
    pre_dispatch_read_for_agent(path, requested_mode, task, project_root, pressure, None)
}

pub fn pre_dispatch_read_for_agent(
    path: &str,
    requested_mode: &str,
    task: Option<&str>,
    project_root: Option<&str>,
    pressure: Option<&PressureAction>,
    agent_id: Option<&str>,
) -> PreDispatchResult {
    let no_change = PreDispatchResult {
        overridden_mode: None,
        reason: None,
        pressure_downgraded: false,
        budget_blocked: false,
        budget_warning: None,
    };

    if let Some(aid) = agent_id {
        let estimated_tokens = estimate_read_tokens(path, requested_mode);
        match crate::core::agent_budget::check_budget(aid, estimated_tokens) {
            crate::core::agent_budget::BudgetCheckResult::Exceeded { limit, consumed } => {
                return PreDispatchResult {
                    overridden_mode: None,
                    reason: Some("agent-budget-exceeded"),
                    pressure_downgraded: false,
                    budget_blocked: true,
                    budget_warning: Some(format!(
                        "Agent budget exceeded: {consumed}/{limit} tokens consumed. Reset via ctx_session or set a higher limit."
                    )),
                };
            }
            crate::core::agent_budget::BudgetCheckResult::Warning {
                remaining,
                percent_used,
            } => {
                let warning = format!(
                    "[BUDGET WARNING] Agent '{aid}' at {:.0}% budget ({remaining} tokens remaining)",
                    percent_used * 100.0
                );
                let mut result = no_change.clone();
                result.budget_warning = Some(warning);
                if requested_mode == "diff" || requested_mode.starts_with("lines") {
                    return result;
                }
                let rest = pre_dispatch_inner(path, requested_mode, task, project_root, pressure);
                return PreDispatchResult {
                    budget_warning: result.budget_warning,
                    ..rest
                };
            }
            crate::core::agent_budget::BudgetCheckResult::Allowed { .. } => {}
        }
    }

    pre_dispatch_inner(path, requested_mode, task, project_root, pressure)
}

fn pre_dispatch_inner(
    path: &str,
    requested_mode: &str,
    task: Option<&str>,
    project_root: Option<&str>,
    pressure: Option<&PressureAction>,
) -> PreDispatchResult {
    let no_change = PreDispatchResult {
        overridden_mode: None,
        reason: None,
        pressure_downgraded: false,
        budget_blocked: false,
        budget_warning: None,
    };

    if requested_mode == "diff" || requested_mode.starts_with("lines") {
        return no_change;
    }

    if let Some(root) = project_root {
        let overlay = OverlayStore::load_project(&std::path::PathBuf::from(root));
        if let Some(result) = check_overlay_mode_override(path, requested_mode, &overlay) {
            return result;
        }
    }

    if let Some(action) = pressure {
        if let Some(downgraded) = pressure_downgrade(requested_mode, action) {
            return PreDispatchResult {
                overridden_mode: Some(downgraded),
                reason: Some("pressure-auto-downgrade"),
                pressure_downgraded: true,
                budget_blocked: false,
                budget_warning: None,
            };
        }
    }

    if requested_mode == "full" {
        return no_change;
    }

    if let Ok(bt) = crate::core::bounce_tracker::global().lock() {
        if bt.should_force_full(path) {
            return PreDispatchResult {
                overridden_mode: Some("full".to_string()),
                reason: Some("bounce-prevention"),
                pressure_downgraded: false,
                budget_blocked: false,
                budget_warning: None,
            };
        }
    }

    if let Some(task_str) = task {
        let intent = crate::core::intent_engine::StructuredIntent::from_query(task_str);
        let norm = crate::core::pathutil::normalize_tool_path(path);
        let is_target = intent
            .targets
            .iter()
            .any(|t| norm.ends_with(t) || norm.contains(t));
        if is_target {
            return PreDispatchResult {
                overridden_mode: Some("full".to_string()),
                reason: Some("intent-target"),
                pressure_downgraded: false,
                budget_blocked: false,
                budget_warning: None,
            };
        }
    }

    if let Some(root) = project_root {
        if let Some(index) = try_load_graph(root) {
            let related = index.get_related(path, 1);
            if let Some(task_str) = task {
                let intent = crate::core::intent_engine::StructuredIntent::from_query(task_str);
                for target in &intent.targets {
                    let target_related = index.get_related(target, 1);
                    let norm = crate::core::pathutil::normalize_tool_path(path);
                    if target_related
                        .iter()
                        .any(|r| r.contains(&norm) || norm.contains(r))
                    {
                        return PreDispatchResult {
                            overridden_mode: Some("map".to_string()),
                            reason: Some("graph-direct-import"),
                            pressure_downgraded: false,
                            budget_blocked: false,
                            budget_warning: None,
                        };
                    }
                }
            }
            if !related.is_empty() && requested_mode == "auto" {
                let reverse_deps = index.get_reverse_deps(path, 1);
                if reverse_deps.len() > 3 {
                    return PreDispatchResult {
                        overridden_mode: Some("map".to_string()),
                        reason: Some("graph-hub-file"),
                        pressure_downgraded: false,
                        budget_blocked: false,
                        budget_warning: None,
                    };
                }
            }
        }
    }

    if let Some(root) = project_root {
        if let Some(knowledge) = crate::core::knowledge::ProjectKnowledge::load(root) {
            let norm = crate::core::pathutil::normalize_tool_path(path);
            let mentions = knowledge
                .facts
                .iter()
                .filter(|f| f.value.contains(&norm) || f.key.contains(&norm))
                .count();
            if mentions >= 3 {
                return PreDispatchResult {
                    overridden_mode: Some("map".to_string()),
                    reason: Some("knowledge-high-relevance"),
                    pressure_downgraded: false,
                    budget_blocked: false,
                    budget_warning: None,
                };
            }
        }
    }

    no_change
}

fn estimate_read_tokens(path: &str, mode: &str) -> usize {
    let file_size = std::fs::metadata(path).map_or(4000, |m| m.len() as usize);
    let char_estimate = file_size;
    let full_tokens = char_estimate / 4;
    match mode {
        "signatures" => full_tokens / 5,
        "map" => full_tokens / 3,
        "aggressive" | "entropy" => full_tokens / 4,
        "diff" => full_tokens / 10,
        _ if mode.starts_with("lines:") => {
            if let Some(range) = mode.strip_prefix("lines:") {
                let parts: Vec<&str> = range.split('-').collect();
                if parts.len() == 2 {
                    let start = parts[0].parse::<usize>().unwrap_or(1);
                    let end = parts[1].parse::<usize>().unwrap_or(start + 100);
                    (end.saturating_sub(start) + 1) * 10
                } else {
                    full_tokens / 10
                }
            } else {
                full_tokens / 10
            }
        }
        _ => full_tokens,
    }
}

fn pressure_downgrade(requested_mode: &str, action: &PressureAction) -> Option<String> {
    match action {
        PressureAction::ForceCompression => match requested_mode {
            "full" => Some("map".to_string()),
            "map" => Some("signatures".to_string()),
            _ => None,
        },
        PressureAction::EvictLeastRelevant => match requested_mode {
            "full" => Some("map".to_string()),
            "map" | "auto" => Some("signatures".to_string()),
            _ => None,
        },
        PressureAction::NoAction | PressureAction::SuggestCompression => None,
    }
}

fn check_overlay_mode_override(
    path: &str,
    requested_mode: &str,
    overlay: &OverlayStore,
) -> Option<PreDispatchResult> {
    let item_id = ContextItemId::from_file(path);
    let overlays = overlay.for_item(&item_id);

    for ov in overlays.iter().rev() {
        match &ov.operation {
            OverlayOp::SetView(view) => {
                let mode_str = view.as_str();
                if mode_str != requested_mode {
                    return Some(PreDispatchResult {
                        overridden_mode: Some(mode_str.to_string()),
                        reason: Some("overlay-set-view"),
                        pressure_downgraded: false,
                        budget_blocked: false,
                        budget_warning: None,
                    });
                }
            }
            OverlayOp::Pin { .. } if requested_mode != "full" => {
                return Some(PreDispatchResult {
                    overridden_mode: Some("full".to_string()),
                    reason: Some("pinned"),
                    pressure_downgraded: false,
                    budget_blocked: false,
                    budget_warning: None,
                });
            }
            OverlayOp::Exclude { .. } if requested_mode != "signatures" => {
                return Some(PreDispatchResult {
                    overridden_mode: Some("signatures".to_string()),
                    reason: Some("excluded"),
                    pressure_downgraded: false,
                    budget_blocked: false,
                    budget_warning: None,
                });
            }
            _ => {}
        }
    }
    None
}

pub fn post_dispatch_record(
    path: &str,
    mode: &str,
    original_tokens: usize,
    sent_tokens: usize,
    ledger: &mut ContextLedger,
    overlay: &OverlayStore,
) -> PostDispatchResult {
    post_dispatch_record_with_task(
        path,
        mode,
        original_tokens,
        sent_tokens,
        ledger,
        overlay,
        None,
    )
}

pub fn post_dispatch_record_with_task(
    path: &str,
    mode: &str,
    original_tokens: usize,
    sent_tokens: usize,
    ledger: &mut ContextLedger,
    overlay: &OverlayStore,
    task: Option<&str>,
) -> PostDispatchResult {
    let prev_count = ledger.entries.len();
    let prev_pressure = ledger.pressure().recommendation;

    ledger.record_with_task(path, mode, original_tokens, sent_tokens, task);

    let item_id = ContextItemId::from_file(path);
    let state = overlay.apply_to_state(&item_id, ContextState::Included);

    if state == ContextState::Excluded {
        return PostDispatchResult {
            eviction_hint: Some(format!("File '{path}' is excluded by overlay.")),
            elicitation_hint: None,
            resource_changed: true,
        };
    }

    let elicitation =
        super::elicitation::check_elicitation_needed(ledger, Some(path), Some(sent_tokens))
            .map(|s| s.format_fallback_hint());

    let pressure = ledger.pressure();

    apply_reinjection_plan(ledger, &pressure.recommendation);

    let new_entry = ledger.entries.len() != prev_count;
    let pressure_shifted = pressure.recommendation != prev_pressure;
    let resource_changed = new_entry || pressure_shifted;

    if pressure.utilization > 0.9 {
        let candidates = ledger.eviction_candidates_by_phi(3);
        if !candidates.is_empty() {
            let names: Vec<_> = candidates
                .iter()
                .take(3)
                .map(|p| crate::core::protocol::shorten_path(p))
                .collect();
            return PostDispatchResult {
                eviction_hint: Some(format!(
                    "Context pressure {:.0}%. Consider evicting: {}",
                    pressure.utilization * 100.0,
                    names.join(", ")
                )),
                elicitation_hint: elicitation,
                resource_changed,
            };
        }
    }

    PostDispatchResult {
        eviction_hint: None,
        elicitation_hint: elicitation,
        resource_changed,
    }
}

fn apply_reinjection_plan(ledger: &mut ContextLedger, action: &PressureAction) {
    if *action != PressureAction::ForceCompression && *action != PressureAction::EvictLeastRelevant
    {
        return;
    }
    for entry in &mut ledger.entries {
        if entry.mode == "full" {
            entry.mode = "map".to_string();
        }
    }
}

fn try_load_graph(project_root: &str) -> Option<crate::core::graph_index::ProjectIndex> {
    crate::core::graph_index::ProjectIndex::load(project_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_dispatch_passthrough_for_full() {
        let result = pre_dispatch_read("src/main.rs", "full", None, None, None);
        assert!(result.overridden_mode.is_none());
    }

    #[test]
    fn pre_dispatch_passthrough_for_diff() {
        let result = pre_dispatch_read("src/main.rs", "diff", None, None, None);
        assert!(result.overridden_mode.is_none());
    }

    #[test]
    fn pre_dispatch_no_override_without_signals() {
        let result = pre_dispatch_read("src/unknown.rs", "auto", None, None, None);
        assert!(result.overridden_mode.is_none());
    }

    #[test]
    fn pre_dispatch_bounce_prevention_forces_full() {
        {
            let mut bt = crate::core::bounce_tracker::global().lock().unwrap();
            bt.set_seq(1);
            bt.record_read("src/bouncy.yml", "map", 30, 400);
            bt.set_seq(2);
            bt.record_read("src/bouncy.yml", "full", 400, 400);
            bt.set_seq(3);
            bt.record_read("a2.yml", "map", 30, 400);
            bt.set_seq(4);
            bt.record_read("a2.yml", "full", 400, 400);
            bt.set_seq(5);
            bt.record_read("a3.yml", "map", 30, 400);
            bt.set_seq(6);
            bt.record_read("a3.yml", "full", 400, 400);
        }
        let result = pre_dispatch_read("new.yml", "auto", None, None, None);
        assert_eq!(result.overridden_mode, Some("full".to_string()));
        assert_eq!(result.reason, Some("bounce-prevention"));
    }

    #[test]
    fn pressure_downgrade_full_to_map() {
        let result = pre_dispatch_read(
            "c.rs",
            "full",
            None,
            None,
            Some(&PressureAction::ForceCompression),
        );
        assert_eq!(result.overridden_mode, Some("map".to_string()));
        assert_eq!(result.reason, Some("pressure-auto-downgrade"));
        assert!(result.pressure_downgraded);
    }

    #[test]
    fn pressure_downgrade_map_to_signatures_on_evict() {
        let result = pre_dispatch_read(
            "c.rs",
            "map",
            None,
            None,
            Some(&PressureAction::EvictLeastRelevant),
        );
        assert_eq!(result.overridden_mode, Some("signatures".to_string()));
        assert!(result.pressure_downgraded);
    }

    #[test]
    fn no_pressure_downgrade_when_low() {
        let result = pre_dispatch_read("c.rs", "full", None, None, Some(&PressureAction::NoAction));
        assert!(result.overridden_mode.is_none());
        assert!(!result.pressure_downgraded);
    }

    #[test]
    fn post_dispatch_reinjection_downgrades_entries() {
        let mut ledger = ContextLedger::with_window_size(1000);
        ledger.record("a.rs", "full", 400, 400);
        ledger.record("b.rs", "full", 400, 400);
        let overlay = OverlayStore::new();
        let result = post_dispatch_record("c.rs", "full", 300, 300, &mut ledger, &overlay);
        assert!(result.resource_changed);
        let a_entry = ledger.entries.iter().find(|e| e.path == "a.rs").unwrap();
        assert_eq!(a_entry.mode, "map");
    }

    #[test]
    fn overlay_pin_forces_full_mode() {
        let dir = tempfile::tempdir().expect("tmp dir");
        let root = dir.path();
        let mut store = OverlayStore::new();
        let target = ContextItemId::from_file("src/important.rs");
        store.add(crate::core::context_overlay::ContextOverlay::new(
            target,
            OverlayOp::Pin { verbatim: false },
            crate::core::context_overlay::OverlayScope::Project,
            String::new(),
            crate::core::context_overlay::OverlayAuthor::User,
        ));
        store.save_project(root).unwrap();

        let result = pre_dispatch_read(
            "src/important.rs",
            "auto",
            None,
            Some(root.to_str().unwrap()),
            None,
        );
        assert_eq!(result.overridden_mode, Some("full".to_string()));
        assert_eq!(result.reason, Some("pinned"));
    }

    #[test]
    fn overlay_exclude_forces_signatures_mode() {
        let dir = tempfile::tempdir().expect("tmp dir");
        let root = dir.path();
        let mut store = OverlayStore::new();
        let target = ContextItemId::from_file("src/noisy.rs");
        store.add(crate::core::context_overlay::ContextOverlay::new(
            target,
            OverlayOp::Exclude {
                reason: "noise".to_string(),
            },
            crate::core::context_overlay::OverlayScope::Project,
            String::new(),
            crate::core::context_overlay::OverlayAuthor::User,
        ));
        store.save_project(root).unwrap();

        let result = pre_dispatch_read(
            "src/noisy.rs",
            "auto",
            None,
            Some(root.to_str().unwrap()),
            None,
        );
        assert_eq!(result.overridden_mode, Some("signatures".to_string()));
        assert_eq!(result.reason, Some("excluded"));
    }

    #[test]
    fn overlay_set_view_forces_specified_mode() {
        let dir = tempfile::tempdir().expect("tmp dir");
        let root = dir.path();
        let mut store = OverlayStore::new();
        let target = ContextItemId::from_file("src/big.rs");
        store.add(crate::core::context_overlay::ContextOverlay::new(
            target,
            OverlayOp::SetView(crate::core::context_field::ViewKind::Map),
            crate::core::context_overlay::OverlayScope::Project,
            String::new(),
            crate::core::context_overlay::OverlayAuthor::User,
        ));
        store.save_project(root).unwrap();

        let result = pre_dispatch_read(
            "src/big.rs",
            "auto",
            None,
            Some(root.to_str().unwrap()),
            None,
        );
        assert_eq!(result.overridden_mode, Some("map".to_string()));
        assert_eq!(result.reason, Some("overlay-set-view"));
    }
}
