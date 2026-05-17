use chrono::Utc;

#[cfg(feature = "embeddings")]
use crate::core::embeddings::EmbeddingEngine;

use crate::core::knowledge::ProjectKnowledge;
use crate::core::memory_policy::MemoryPolicy;
use crate::core::session::SessionState;

fn load_policy_or_error() -> Result<MemoryPolicy, String> {
    super::knowledge_shared::load_policy_or_error()
}

/// Dispatches knowledge base actions (remember, recall, pattern, timeline, etc.).
#[allow(clippy::too_many_arguments)]
pub fn handle(
    project_root: &str,
    action: &str,
    category: Option<&str>,
    key: Option<&str>,
    value: Option<&str>,
    query: Option<&str>,
    session_id: &str,
    pattern_type: Option<&str>,
    examples: Option<Vec<String>>,
    confidence: Option<f32>,
    mode: Option<&str>,
) -> String {
    match action {
        "policy" => handle_policy(value),
        "remember" => handle_remember(project_root, category, key, value, session_id, confidence),
        "recall" => handle_recall(project_root, category, query, session_id, mode),
        "pattern" => handle_pattern(project_root, pattern_type, value, examples, session_id),
        "feedback" => handle_feedback(project_root, category, key, value, session_id),
        "relate" => crate::tools::ctx_knowledge_relations::handle_relate(
            project_root,
            category,
            key,
            value,
            query,
            session_id,
        ),
        "unrelate" => crate::tools::ctx_knowledge_relations::handle_unrelate(
            project_root,
            category,
            key,
            value,
            query,
        ),
        "relations" => crate::tools::ctx_knowledge_relations::handle_relations(
            project_root,
            category,
            key,
            value,
            query,
        ),
        "relations_diagram" => crate::tools::ctx_knowledge_relations::handle_relations_diagram(
            project_root,
            category,
            key,
            value,
            query,
        ),
        "status" => handle_status(project_root),
        "health" => handle_health(project_root),
        "remove" => handle_remove(project_root, category, key),
        "export" => handle_export(project_root),
        "consolidate" => handle_consolidate(project_root),
        "timeline" => handle_timeline(project_root, category),
        "rooms" => handle_rooms(project_root),
        "search" => handle_search(query),
        "wakeup" => handle_wakeup(project_root),
        "embeddings_status" => handle_embeddings_status(project_root),
        "embeddings_reset" => handle_embeddings_reset(project_root),
        "embeddings_reindex" => handle_embeddings_reindex(project_root),
        "cognition_loop" => handle_cognition_loop(project_root),
        "bridge_publish" => handle_bridge_publish(project_root, session_id),
        "bridge_pull" => handle_bridge_pull(project_root, session_id),
        "bridge_status" => handle_bridge_status(project_root),
        _ => format!(
            "Unknown action: {action}. Use: policy, remember, recall, pattern, feedback, relate, unrelate, relations, relations_diagram, status, health, remove, export, consolidate, timeline, rooms, search, wakeup, embeddings_status, embeddings_reset, embeddings_reindex, cognition_loop, bridge_publish, bridge_pull, bridge_status"
        ),
    }
}

fn handle_policy(value: Option<&str>) -> String {
    let sub = value.unwrap_or("show").trim().to_lowercase();
    let profile = crate::core::profiles::active_profile_name();

    match sub.as_str() {
        "show" => {
            let policy = match load_policy_or_error() {
                Ok(p) => p,
                Err(e) => return e,
            };

            let cfg_path = crate::core::config::Config::path().map_or_else(
                || "~/.lean-ctx/config.toml".to_string(),
                |p| p.display().to_string(),
            );

            format!(
                "Knowledge policy (effective, profile={profile}):\n\
                 - memory.knowledge.max_facts={}\n\
                 - memory.knowledge.contradiction_threshold={}\n\
                 - memory.knowledge.recall_facts_limit={}\n\
                 - memory.knowledge.rooms_limit={}\n\
                 - memory.knowledge.timeline_limit={}\n\
                 - memory.knowledge.relations_limit={}\n\
                 - memory.lifecycle.decay_rate={}\n\
                 - memory.lifecycle.stale_days={}\n\
                 \nConfig: {cfg_path}",
                policy.knowledge.max_facts,
                policy.knowledge.contradiction_threshold,
                policy.knowledge.recall_facts_limit,
                policy.knowledge.rooms_limit,
                policy.knowledge.timeline_limit,
                policy.knowledge.relations_limit,
                policy.lifecycle.decay_rate,
                policy.lifecycle.stale_days
            )
        }
        "validate" => match load_policy_or_error() {
            Ok(_) => format!("OK: memory policy valid (profile={profile})"),
            Err(e) => e,
        },
        _ => "Error: policy value must be show|validate".to_string(),
    }
}

fn handle_feedback(
    project_root: &str,
    category: Option<&str>,
    key: Option<&str>,
    value: Option<&str>,
    session_id: &str,
) -> String {
    let Some(cat) = category else {
        return "Error: category is required for feedback".to_string();
    };
    let Some(k) = key else {
        return "Error: key is required for feedback".to_string();
    };
    let dir = value.unwrap_or("up").trim().to_lowercase();
    let is_up = matches!(dir.as_str(), "up" | "+1" | "+" | "true" | "1");
    let is_down = matches!(dir.as_str(), "down" | "-1" | "-" | "false" | "0");
    if !is_up && !is_down {
        return "Error: feedback value must be up|down (+1|-1)".to_string();
    }

    let mut knowledge = ProjectKnowledge::load_or_create(project_root);
    let Some(f) = knowledge
        .facts
        .iter_mut()
        .find(|f| f.is_current() && f.category == cat && f.key == k)
    else {
        return format!("No current fact found: [{cat}] {k}");
    };

    if is_up {
        f.feedback_up = f.feedback_up.saturating_add(1);
    } else {
        f.feedback_down = f.feedback_down.saturating_add(1);
    }
    f.last_feedback = Some(Utc::now());

    crate::core::events::emit(crate::core::events::EventKind::KnowledgeUpdate {
        category: cat.to_string(),
        key: k.to_string(),
        action: if is_up {
            "feedback_up"
        } else {
            "feedback_down"
        }
        .to_string(),
    });

    let quality = f.quality_score();
    let up = f.feedback_up;
    let down = f.feedback_down;
    let conf = f.confidence;

    match knowledge.save() {
        Ok(()) => format!(
            "Feedback recorded ({dir}) for [{cat}] {k} (up={up}, down={down}, quality={quality:.2}, confidence={conf:.2}, session={session_id})"
        ),
        Err(e) => format!(
            "Feedback recorded ({dir}) but save failed: {e} (up={up}, down={down}, quality={quality:.2})"
        ),
    }
}

#[cfg(feature = "embeddings")]
fn embeddings_auto_download_allowed() -> bool {
    std::env::var("LEAN_CTX_EMBEDDINGS_AUTO_DOWNLOAD")
        .ok()
        .is_some_and(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

#[cfg(feature = "embeddings")]
fn embedding_engine() -> Option<&'static EmbeddingEngine> {
    let cfg = crate::core::config::Config::load();
    let profile = crate::core::config::MemoryProfile::effective(&cfg);
    if !profile.embeddings_enabled() {
        return None;
    }
    if !EmbeddingEngine::is_available() && !embeddings_auto_download_allowed() {
        return None;
    }
    crate::core::embeddings::shared_engine()
}

fn handle_embeddings_status(project_root: &str) -> String {
    #[cfg(feature = "embeddings")]
    {
        let knowledge = ProjectKnowledge::load_or_create(project_root);
        let model_available = EmbeddingEngine::is_available();
        let auto = embeddings_auto_download_allowed();

        let entries = crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::load(
            &knowledge.project_hash,
        )
        .map_or(0, |i| i.entries.len());

        let path = crate::core::data_dir::lean_ctx_data_dir()
            .ok()
            .map(|d| {
                d.join("knowledge")
                    .join(&knowledge.project_hash)
                    .join("embeddings.json")
            })
            .map_or_else(|| "<unknown>".to_string(), |p| p.display().to_string());

        format!(
            "Knowledge embeddings: model={}, auto_download={}, index_entries={}, path={path}",
            if model_available {
                "present"
            } else {
                "missing"
            },
            if auto { "on" } else { "off" },
            entries
        )
    }
    #[cfg(not(feature = "embeddings"))]
    {
        let _ = project_root;
        "ERR: embeddings feature not enabled".to_string()
    }
}

fn handle_embeddings_reset(project_root: &str) -> String {
    #[cfg(feature = "embeddings")]
    {
        let knowledge = ProjectKnowledge::load_or_create(project_root);
        match crate::core::knowledge_embedding::reset(&knowledge.project_hash) {
            Ok(()) => "Embeddings index reset.".to_string(),
            Err(e) => format!("Embeddings reset failed: {e}"),
        }
    }
    #[cfg(not(feature = "embeddings"))]
    {
        let _ = project_root;
        "ERR: embeddings feature not enabled".to_string()
    }
}

fn handle_embeddings_reindex(project_root: &str) -> String {
    #[cfg(feature = "embeddings")]
    {
        let Some(knowledge) = ProjectKnowledge::load(project_root) else {
            return "No knowledge stored for this project yet.".to_string();
        };
        let policy = match load_policy_or_error() {
            Ok(p) => p,
            Err(e) => return e,
        };

        let Some(engine) = embedding_engine() else {
            return "Embeddings model not available. Set LEAN_CTX_EMBEDDINGS_AUTO_DOWNLOAD=1 to allow auto-download, then re-run."
                    .to_string();
        };

        let mut idx =
            crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::new(&knowledge.project_hash);

        let mut facts: Vec<&crate::core::knowledge::KnowledgeFact> =
            knowledge.facts.iter().filter(|f| f.is_current()).collect();
        facts.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.last_confirmed.cmp(&a.last_confirmed))
                .then_with(|| a.category.cmp(&b.category))
                .then_with(|| a.key.cmp(&b.key))
        });

        let max = policy.embeddings.max_facts;
        let mut embedded = 0usize;
        for f in facts.into_iter().take(max) {
            if crate::core::knowledge_embedding::embed_and_store(
                &mut idx,
                engine,
                &f.category,
                &f.key,
                &f.value,
            )
            .is_ok()
            {
                embedded += 1;
            }
        }

        crate::core::knowledge_embedding::compact_against_knowledge(&mut idx, &knowledge, &policy);
        match idx.save() {
            Ok(()) => format!("Embeddings reindex ok (embedded {embedded} facts)."),
            Err(e) => format!("Embeddings reindex failed: {e}"),
        }
    }
    #[cfg(not(feature = "embeddings"))]
    {
        let _ = project_root;
        "ERR: embeddings feature not enabled".to_string()
    }
}

fn handle_remember(
    project_root: &str,
    category: Option<&str>,
    key: Option<&str>,
    value: Option<&str>,
    session_id: &str,
    confidence: Option<f32>,
) -> String {
    let Some(cat) = category else {
        return "Error: category is required for remember".to_string();
    };
    let Some(k) = key else {
        return "Error: key is required for remember".to_string();
    };
    let Some(v) = value else {
        return "Error: value is required for remember".to_string();
    };
    let conf = confidence.unwrap_or(0.8);
    let policy = match load_policy_or_error() {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut knowledge = ProjectKnowledge::load_or_create(project_root);
    let contradiction = knowledge.remember(cat, k, v, session_id, conf, &policy);
    let _ = knowledge.run_memory_lifecycle(&policy);

    let mut result = format!(
        "Remembered [{cat}] {k}: {v} (confidence: {:.0}%)",
        conf * 100.0
    );

    if let Some(c) = contradiction {
        result.push_str(&format!("\n⚠ CONTRADICTION DETECTED: {}", c.resolution));
    }

    #[cfg(feature = "embeddings")]
    {
        if let Some(engine) = embedding_engine() {
            let mut idx = crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::load(
                &knowledge.project_hash,
            )
            .unwrap_or_else(|| {
                crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::new(
                    &knowledge.project_hash,
                )
            });

            match crate::core::knowledge_embedding::embed_and_store(&mut idx, engine, cat, k, v) {
                Ok(()) => {
                    crate::core::knowledge_embedding::compact_against_knowledge(
                        &mut idx, &knowledge, &policy,
                    );
                    if let Err(e) = idx.save() {
                        result.push_str(&format!("\n(warn: embeddings save failed: {e})"));
                    }
                }
                Err(e) => {
                    result.push_str(&format!("\n(warn: embeddings update failed: {e})"));
                }
            }
        }
    }

    match knowledge.save() {
        Ok(()) => result,
        Err(e) => format!("{result}\n(save failed: {e})"),
    }
}

fn handle_recall(
    project_root: &str,
    category: Option<&str>,
    query: Option<&str>,
    session_id: &str,
    mode: Option<&str>,
) -> String {
    let Some(mut knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge stored for this project yet.".to_string();
    };
    let policy = match load_policy_or_error() {
        Ok(p) => p,
        Err(e) => return e,
    };

    if let Some(cat) = category {
        let limit = policy.knowledge.recall_facts_limit;
        let (facts, total) = knowledge.recall_by_category_for_output(cat, limit);
        if facts.is_empty() || total == 0 {
            // System 2: archive rehydrate (category-only)
            let rehydrated =
                rehydrate_from_archives(&mut knowledge, Some(cat), None, session_id, &policy);
            if rehydrated {
                let (facts2, total2) = knowledge.recall_by_category_for_output(cat, limit);
                if !facts2.is_empty() && total2 > 0 {
                    let mut out2 = format_facts(&facts2, total2, Some(cat));
                    if let Err(e) = knowledge.save() {
                        out2.push_str(&format!(
                            "\n(warn: failed to persist retrieval signals: {e})"
                        ));
                    }
                    return out2;
                }
            }
            return format!("No facts in category '{cat}'.");
        }
        let mut out = format_facts(&facts, total, Some(cat));
        if let Err(e) = knowledge.save() {
            out.push_str(&format!(
                "\n(warn: failed to persist retrieval signals: {e})"
            ));
        }
        return out;
    }

    if let Some(q) = query {
        let mode = mode.unwrap_or("auto").trim().to_lowercase();
        #[cfg(feature = "embeddings")]
        {
            if let Some(engine) = embedding_engine() {
                if let Some(idx) = crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::load(
                    &knowledge.project_hash,
                ) {
                    let limit = policy.knowledge.recall_facts_limit;
                    if mode == "semantic" {
                        let scored =
                            crate::core::knowledge_embedding::semantic_recall_semantic_only(
                                &knowledge, &idx, engine, q, limit,
                            );
                        if scored.is_empty() {
                            return format!("No semantic facts matching '{q}'.");
                        }
                        let hits: Vec<SemanticHit> = scored
                            .iter()
                            .map(|s| SemanticHit {
                                category: s.fact.category.clone(),
                                key: s.fact.key.clone(),
                                value: s.fact.value.clone(),
                                score: s.score,
                                semantic_score: s.semantic_score,
                                confidence_score: s.confidence_score,
                            })
                            .collect();
                        apply_retrieval_signals_from_hits(&mut knowledge, &hits);
                        let mut out = format_semantic_facts(&format!("{q} (mode=semantic)"), &hits);
                        if let Err(e) = knowledge.save() {
                            out.push_str(&format!(
                                "\n(warn: failed to persist retrieval signals: {e})"
                            ));
                        }
                        return out;
                    }

                    if mode == "hybrid" || mode == "auto" {
                        let scored = crate::core::knowledge_embedding::semantic_recall(
                            &knowledge, &idx, engine, q, limit,
                        );
                        if !scored.is_empty() {
                            let hits: Vec<SemanticHit> = scored
                                .iter()
                                .map(|s| SemanticHit {
                                    category: s.fact.category.clone(),
                                    key: s.fact.key.clone(),
                                    value: s.fact.value.clone(),
                                    score: s.score,
                                    semantic_score: s.semantic_score,
                                    confidence_score: s.confidence_score,
                                })
                                .collect();
                            apply_retrieval_signals_from_hits(&mut knowledge, &hits);
                            let mut out =
                                format_semantic_facts(&format!("{q} (mode=hybrid)"), &hits);
                            if let Err(e) = knowledge.save() {
                                out.push_str(&format!(
                                    "\n(warn: failed to persist retrieval signals: {e})"
                                ));
                            }
                            return out;
                        }
                    }
                }
            }
        }

        if mode == "semantic" {
            return "Semantic recall requires embeddings. Run ctx_knowledge(action=\"embeddings_reindex\") and ensure embeddings are enabled.".to_string();
        }

        let limit = policy.knowledge.recall_facts_limit;
        let (facts, total) = knowledge.recall_for_output(q, limit);
        if facts.is_empty() || total == 0 {
            // System 2: archive rehydrate (query)
            let rehydrated =
                rehydrate_from_archives(&mut knowledge, None, Some(q), session_id, &policy);
            if rehydrated {
                let (facts2, total2) = knowledge.recall_for_output(q, limit);
                if !facts2.is_empty() && total2 > 0 {
                    let mut out2 = format_facts(&facts2, total2, None);
                    if let Err(e) = knowledge.save() {
                        out2.push_str(&format!(
                            "\n(warn: failed to persist retrieval signals: {e})"
                        ));
                    }
                    return out2;
                }
            }
            return format!("No facts matching '{q}'.");
        }
        let mut out = format_facts(&facts, total, None);
        if let Err(e) = knowledge.save() {
            out.push_str(&format!(
                "\n(warn: failed to persist retrieval signals: {e})"
            ));
        }
        return out;
    }

    "Error: provide query or category for recall".to_string()
}

fn rehydrate_from_archives(
    knowledge: &mut ProjectKnowledge,
    category: Option<&str>,
    query: Option<&str>,
    session_id: &str,
    policy: &MemoryPolicy,
) -> bool {
    let mut archives = crate::core::memory_lifecycle::list_archives();
    if archives.is_empty() {
        return false;
    }
    archives.sort();
    let max_archives = crate::core::budgets::KNOWLEDGE_REHYDRATE_MAX_ARCHIVES;
    if archives.len() > max_archives {
        archives = archives[archives.len() - max_archives..].to_vec();
    }

    let terms: Vec<String> = query
        .unwrap_or("")
        .to_lowercase()
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    #[derive(Clone)]
    struct Cand {
        category: String,
        key: String,
        value: String,
        confidence: f32,
        score: f32,
    }

    let mut cands: Vec<Cand> = Vec::new();

    for p in &archives {
        let p_str = p.to_string_lossy().to_string();
        let Ok(facts) = crate::core::memory_lifecycle::restore_archive(&p_str) else {
            continue;
        };
        for f in facts {
            if let Some(cat) = category {
                if f.category != cat {
                    continue;
                }
            }
            if terms.is_empty() {
                cands.push(Cand {
                    category: f.category,
                    key: f.key,
                    value: f.value,
                    confidence: f.confidence,
                    score: f.confidence,
                });
            } else {
                let searchable = format!(
                    "{} {} {} {}",
                    f.category.to_lowercase(),
                    f.key.to_lowercase(),
                    f.value.to_lowercase(),
                    f.source_session.to_lowercase()
                );
                let match_count = terms.iter().filter(|t| searchable.contains(*t)).count();
                if match_count == 0 {
                    continue;
                }
                let rel = match_count as f32 / terms.len() as f32;
                let score = rel * f.confidence;
                cands.push(Cand {
                    category: f.category,
                    key: f.key,
                    value: f.value,
                    confidence: f.confidence,
                    score,
                });
            }
        }
    }

    if cands.is_empty() {
        return false;
    }

    cands.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.value.cmp(&b.value))
    });
    cands.truncate(crate::core::budgets::KNOWLEDGE_REHYDRATE_LIMIT);

    let mut any = false;
    for c in &cands {
        knowledge.remember(
            &c.category,
            &c.key,
            &c.value,
            session_id,
            c.confidence.max(0.6),
            policy,
        );
        any = true;
    }
    if any {
        let _ = knowledge.run_memory_lifecycle(policy);
    }
    any
}

fn handle_pattern(
    project_root: &str,
    pattern_type: Option<&str>,
    value: Option<&str>,
    examples: Option<Vec<String>>,
    session_id: &str,
) -> String {
    let Some(pt) = pattern_type else {
        return "Error: pattern_type is required".to_string();
    };
    let Some(desc) = value else {
        return "Error: value (description) is required for pattern".to_string();
    };
    let exs = examples.unwrap_or_default();
    let policy = match crate::core::config::Config::load().memory_policy_effective() {
        Ok(p) => p,
        Err(e) => {
            let path = crate::core::config::Config::path().map_or_else(
                || "~/.lean-ctx/config.toml".to_string(),
                |p| p.display().to_string(),
            );
            return format!("Error: invalid memory policy: {e}\nFix: edit {path}");
        }
    };
    let mut knowledge = ProjectKnowledge::load_or_create(project_root);
    knowledge.add_pattern(pt, desc, exs, session_id, &policy);
    match knowledge.save() {
        Ok(()) => format!("Pattern [{pt}] added: {desc}"),
        Err(e) => format!("Pattern added but save failed: {e}"),
    }
}

fn handle_status(project_root: &str) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge stored for this project yet. Use ctx_knowledge(action=\"remember\") to start.".to_string();
    };

    let current_facts = knowledge.facts.iter().filter(|f| f.is_current()).count();
    let archived_facts = knowledge.facts.len() - current_facts;

    let mut out = format!(
        "Project Knowledge: {} active facts ({} archived), {} patterns, {} history entries\n",
        current_facts,
        archived_facts,
        knowledge.patterns.len(),
        knowledge.history.len()
    );
    out.push_str(&format!(
        "Last updated: {}\n",
        knowledge.updated_at.format("%Y-%m-%d %H:%M UTC")
    ));

    let rooms = knowledge.list_rooms();
    if !rooms.is_empty() {
        out.push_str("Rooms: ");
        let room_strs: Vec<String> = rooms.iter().map(|(c, n)| format!("{c}({n})")).collect();
        out.push_str(&room_strs.join(", "));
        out.push('\n');
    }

    out.push_str(&knowledge.format_summary());
    out
}

fn handle_health(project_root: &str) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge stored. Nothing to report.".to_string();
    };

    let total = knowledge.facts.len();
    let current: Vec<_> = knowledge.facts.iter().filter(|f| f.is_current()).collect();
    let archived = total - current.len();

    let mut low_quality = 0u32;
    let mut high_quality = 0u32;
    let mut stale_candidates = 0u32;
    let mut total_quality: f32 = 0.0;
    let mut never_retrieved = 0u32;
    let mut room_counts: std::collections::HashMap<String, (u32, f32)> =
        std::collections::HashMap::new();

    let now = chrono::Utc::now();
    for f in &current {
        let q = f.quality_score();
        total_quality += q;
        if q < 0.4 {
            low_quality += 1;
        } else if q >= 0.8 {
            high_quality += 1;
        }
        if f.retrieval_count == 0 {
            never_retrieved += 1;
        }
        let age_days = (now - f.created_at).num_days();
        if age_days > 30 && f.retrieval_count == 0 {
            stale_candidates += 1;
        }

        let entry = room_counts.entry(f.category.clone()).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += q;
    }

    let avg_quality = if current.is_empty() {
        0.0
    } else {
        total_quality / current.len() as f32
    };

    let mut out = String::from("=== Knowledge Health Report ===\n");
    out.push_str(&format!(
        "Total: {} facts ({} active, {} archived)\n",
        total,
        current.len(),
        archived
    ));
    out.push_str(&format!("Avg Quality: {avg_quality:.2}\n"));
    out.push_str(&format!(
        "Distribution: {high_quality} high (>=0.8) | {low_quality} low (<0.4)\n"
    ));
    out.push_str(&format!(
        "Stale (>30d, never retrieved): {stale_candidates}\n"
    ));
    out.push_str(&format!("Never retrieved: {never_retrieved}\n"));

    if !room_counts.is_empty() {
        out.push_str("\nRoom Balance:\n");
        let mut rooms: Vec<_> = room_counts.into_iter().collect();
        rooms.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
        for (cat, (count, total_q)) in &rooms {
            let avg = if *count > 0 {
                total_q / *count as f32
            } else {
                0.0
            };
            out.push_str(&format!("  {cat}: {count} facts, avg quality {avg:.2}\n"));
        }
    }

    let policy = crate::core::memory_policy::MemoryPolicy::default();
    out.push_str(&format!(
        "\nPolicy: max {} facts, max {} patterns\n",
        policy.knowledge.max_facts, policy.knowledge.max_patterns
    ));

    if current.len() > policy.knowledge.max_facts {
        out.push_str(&format!(
            "WARNING: Active facts ({}) exceed policy max ({})\n",
            current.len(),
            policy.knowledge.max_facts
        ));
    }

    out
}

fn handle_remove(project_root: &str, category: Option<&str>, key: Option<&str>) -> String {
    let Some(cat) = category else {
        return "Error: category is required for remove".to_string();
    };
    let Some(k) = key else {
        return "Error: key is required for remove".to_string();
    };
    let policy = match crate::core::config::Config::load().memory_policy_effective() {
        Ok(p) => p,
        Err(e) => {
            let path = crate::core::config::Config::path().map_or_else(
                || "~/.lean-ctx/config.toml".to_string(),
                |p| p.display().to_string(),
            );
            return format!("Error: invalid memory policy: {e}\nFix: edit {path}");
        }
    };
    let mut knowledge = ProjectKnowledge::load_or_create(project_root);
    if knowledge.remove_fact(cat, k) {
        let _ = knowledge.run_memory_lifecycle(&policy);

        #[cfg(feature = "embeddings")]
        {
            if let Some(mut idx) = crate::core::knowledge_embedding::KnowledgeEmbeddingIndex::load(
                &knowledge.project_hash,
            ) {
                idx.remove(cat, k);
                crate::core::knowledge_embedding::compact_against_knowledge(
                    &mut idx, &knowledge, &policy,
                );
                let _ = idx.save();
            }
        }

        match knowledge.save() {
            Ok(()) => format!("Removed [{cat}] {k}"),
            Err(e) => format!("Removed but save failed: {e}"),
        }
    } else {
        format!("No fact found: [{cat}] {k}")
    }
}

fn handle_export(project_root: &str) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge to export.".to_string();
    };
    let data_dir = match crate::core::data_dir::lean_ctx_data_dir() {
        Ok(d) => d,
        Err(e) => return format!("Export failed: {e}"),
    };

    let export_dir = data_dir.join("exports").join("knowledge");
    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    let filename = format!(
        "knowledge-{}-{ts}.json",
        short_hash(&knowledge.project_hash)
    );
    let path = export_dir.join(filename);

    match serde_json::to_string_pretty(&knowledge) {
        Ok(mut json) => {
            json.push('\n');
            match crate::config_io::write_atomic_with_backup(&path, &json) {
                Ok(()) => format!(
                    "Export saved: {} (active facts: {}, patterns: {}, history: {})",
                    path.display(),
                    knowledge.facts.iter().filter(|f| f.is_current()).count(),
                    knowledge.patterns.len(),
                    knowledge.history.len()
                ),
                Err(e) => format!("Export failed: {e}"),
            }
        }
        Err(e) => format!("Export failed: {e}"),
    }
}

fn handle_consolidate(project_root: &str) -> String {
    let Some(session) = SessionState::load_latest() else {
        return "No active session to consolidate.".to_string();
    };
    let policy = match crate::core::config::Config::load().memory_policy_effective() {
        Ok(p) => p,
        Err(e) => {
            let path = crate::core::config::Config::path().map_or_else(
                || "~/.lean-ctx/config.toml".to_string(),
                |p| p.display().to_string(),
            );
            return format!("Error: invalid memory policy: {e}\nFix: edit {path}");
        }
    };

    let mut knowledge = ProjectKnowledge::load_or_create(project_root);
    let mut consolidated = 0u32;

    for finding in &session.findings {
        let key_text = if let Some(ref file) = finding.file {
            if let Some(line) = finding.line {
                format!("{file}:{line}")
            } else {
                file.clone()
            }
        } else {
            format!("finding-{consolidated}")
        };

        knowledge.remember(
            "finding",
            &key_text,
            &finding.summary,
            &session.id,
            0.7,
            &policy,
        );
        consolidated += 1;
    }

    for decision in &session.decisions {
        let key_text = decision
            .summary
            .chars()
            .take(50)
            .collect::<String>()
            .replace(' ', "-")
            .to_lowercase();

        knowledge.remember(
            "decision",
            &key_text,
            &decision.summary,
            &session.id,
            0.85,
            &policy,
        );
        consolidated += 1;
    }

    let task_desc = session
        .task
        .as_ref()
        .map_or_else(|| "(no task)".into(), |t| t.description.clone());

    let summary = format!(
        "Session {}: {} — {} findings, {} decisions consolidated",
        session.id,
        task_desc,
        session.findings.len(),
        session.decisions.len()
    );
    knowledge.consolidate(&summary, vec![session.id.clone()], &policy);
    let _ = knowledge.run_memory_lifecycle(&policy);

    match knowledge.save() {
        Ok(()) => format!(
            "Consolidated {consolidated} items from session {} into project knowledge.\n\
             Facts: {}, Patterns: {}, History: {}",
            session.id,
            knowledge.facts.len(),
            knowledge.patterns.len(),
            knowledge.history.len()
        ),
        Err(e) => format!("Consolidation done but save failed: {e}"),
    }
}

fn handle_timeline(project_root: &str, category: Option<&str>) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge stored yet.".to_string();
    };

    let policy = match load_policy_or_error() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let Some(cat) = category else {
        return "Error: category is required for timeline".to_string();
    };

    let facts = knowledge.timeline(cat);
    if facts.is_empty() {
        return format!("No history for category '{cat}'.");
    }

    let mut ordered: Vec<&crate::core::knowledge::KnowledgeFact> = facts;
    ordered.sort_by(|a, b| {
        let a_start = a.valid_from.unwrap_or(a.created_at);
        let b_start = b.valid_from.unwrap_or(b.created_at);
        a_start
            .cmp(&b_start)
            .then_with(|| a.last_confirmed.cmp(&b.last_confirmed))
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.value.cmp(&b.value))
    });

    let total = ordered.len();
    let limit = policy.knowledge.timeline_limit;
    if ordered.len() > limit {
        ordered = ordered[ordered.len() - limit..].to_vec();
    }

    let mut out = format!(
        "Timeline [{cat}] (showing {}/{} entries):\n",
        ordered.len(),
        total
    );
    for f in &ordered {
        let status = if f.is_current() {
            "CURRENT"
        } else {
            "archived"
        };
        let valid_range = match (f.valid_from, f.valid_until) {
            (Some(from), Some(until)) => format!(
                "{} → {}",
                from.format("%Y-%m-%d %H:%M"),
                until.format("%Y-%m-%d %H:%M")
            ),
            (Some(from), None) => format!("{} → now", from.format("%Y-%m-%d %H:%M")),
            _ => "unknown".to_string(),
        };
        out.push_str(&format!(
            "  {} = {} [{status}] ({valid_range}) conf={:.0}% x{}\n",
            f.key,
            f.value,
            f.confidence * 100.0,
            f.confirmation_count
        ));
    }
    out
}

fn handle_rooms(project_root: &str) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge stored yet.".to_string();
    };

    let policy = match load_policy_or_error() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let rooms = knowledge.list_rooms();
    if rooms.is_empty() {
        return "No knowledge rooms yet. Use ctx_knowledge(action=\"remember\", category=\"...\") to create rooms.".to_string();
    }

    let mut rooms = rooms;
    rooms.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total = rooms.len();
    rooms.truncate(policy.knowledge.rooms_limit);

    let mut out = format!(
        "Knowledge Rooms (showing {}/{} rooms, project: {}):\n",
        rooms.len(),
        total,
        short_hash(&knowledge.project_hash)
    );
    for (cat, count) in &rooms {
        out.push_str(&format!("  [{cat}] {count} fact(s)\n"));
    }
    out
}

fn handle_search(query: Option<&str>) -> String {
    let Some(q) = query else {
        return "Error: query is required for search".to_string();
    };

    let Ok(data_dir) = crate::core::data_dir::lean_ctx_data_dir() else {
        return "Cannot determine data directory.".to_string();
    };

    let sessions_dir = data_dir.join("sessions");

    if !sessions_dir.exists() {
        return "No sessions found.".to_string();
    }

    let knowledge_dir = data_dir.join("knowledge");

    let allow_cross_project = {
        let role = crate::core::roles::active_role();
        role.io.allow_cross_project_search
    };

    let current_project_hash = std::env::current_dir()
        .ok()
        .map(|p| crate::core::project_hash::hash_project_root(&p.to_string_lossy()));

    let q_lower = q.to_lowercase();
    let terms: Vec<&str> = q_lower.split_whitespace().collect();
    let mut results = Vec::new();

    if knowledge_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&knowledge_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name().to_string_lossy().to_string();

                if !allow_cross_project {
                    if let Some(ref current_hash) = current_project_hash {
                        if &dir_name != current_hash {
                            continue;
                        }
                    }
                }

                if let Some(ref current_hash) = current_project_hash {
                    if dir_name != *current_hash {
                        let policy = crate::core::config::Config::load().boundary_policy;
                        let allowed = crate::core::memory_boundary::check_boundary(
                            current_hash,
                            &dir_name,
                            &policy,
                            &crate::core::memory_boundary::CrossProjectEventType::Search,
                        );
                        crate::core::memory_boundary::record_audit_event(
                            &crate::core::memory_boundary::CrossProjectAuditEvent {
                                timestamp: Utc::now().to_rfc3339(),
                                event_type:
                                    crate::core::memory_boundary::CrossProjectEventType::Search,
                                source_project_hash: current_hash.clone(),
                                target_project_hash: dir_name.clone(),
                                tool: "ctx_knowledge".to_string(),
                                action: "search".to_string(),
                                facts_accessed: 0,
                                allowed,
                                policy_reason: if allowed {
                                    "boundary_policy_allowed".to_string()
                                } else {
                                    "boundary_policy_denied".to_string()
                                },
                            },
                        );
                        if !allowed {
                            continue;
                        }
                    }
                }

                let knowledge_file = entry.path().join("knowledge.json");
                if let Ok(content) = std::fs::read_to_string(&knowledge_file) {
                    if let Ok(knowledge) = serde_json::from_str::<ProjectKnowledge>(&content) {
                        let is_foreign = current_project_hash
                            .as_ref()
                            .is_some_and(|h| h != &knowledge.project_hash);

                        for fact in &knowledge.facts {
                            if is_foreign
                                && fact.privacy
                                    == crate::core::memory_boundary::FactPrivacy::ProjectOnly
                            {
                                continue;
                            }

                            let searchable = format!(
                                "{} {} {}",
                                fact.category.to_lowercase(),
                                fact.key.to_lowercase(),
                                fact.value.to_lowercase()
                            );
                            let match_count =
                                terms.iter().filter(|t| searchable.contains(**t)).count();
                            if match_count > 0 {
                                results.push((
                                    knowledge.project_root.clone(),
                                    fact.category.clone(),
                                    fact.key.clone(),
                                    fact.value.clone(),
                                    fact.confidence,
                                    match_count as f32 / terms.len() as f32,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("latest.json") {
                continue;
            }
            if let Ok(json) = std::fs::read_to_string(&path) {
                if let Ok(session) = serde_json::from_str::<SessionState>(&json) {
                    for finding in &session.findings {
                        let searchable = finding.summary.to_lowercase();
                        let match_count = terms.iter().filter(|t| searchable.contains(**t)).count();
                        if match_count > 0 {
                            let project = session
                                .project_root
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string());
                            results.push((
                                project,
                                "session-finding".to_string(),
                                session.id.clone(),
                                finding.summary.clone(),
                                0.6,
                                match_count as f32 / terms.len() as f32,
                            ));
                        }
                    }
                    for decision in &session.decisions {
                        let searchable = decision.summary.to_lowercase();
                        let match_count = terms.iter().filter(|t| searchable.contains(**t)).count();
                        if match_count > 0 {
                            let project = session
                                .project_root
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string());
                            results.push((
                                project,
                                "session-decision".to_string(),
                                session.id.clone(),
                                decision.summary.clone(),
                                0.7,
                                match_count as f32 / terms.len() as f32,
                            ));
                        }
                    }
                }
            }
        }
    }

    if results.is_empty() {
        return format!("No results found for '{q}' across all sessions and projects.");
    }

    results.sort_by(|a, b| {
        b.5.partial_cmp(&a.5)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.cmp(&b.3))
    });
    results.truncate(crate::core::budgets::KNOWLEDGE_CROSS_PROJECT_SEARCH_LIMIT);

    let mut out = format!("Cross-session search '{q}' ({} results):\n", results.len());
    for (project, cat, key, value, conf, _relevance) in &results {
        let project_short = short_path(project);
        out.push_str(&format!(
            "  [{cat}/{key}] {value} (project: {project_short}, conf: {:.0}%)\n",
            conf * 100.0
        ));
    }
    out
}

fn handle_cognition_loop(project_root: &str) -> String {
    let cfg = crate::core::config::Config::load().autonomy;
    if !cfg.cognition_loop_enabled {
        return "Cognition loop is disabled (autonomy.cognition_loop_enabled=false).".to_string();
    }
    let max_steps = cfg.cognition_loop_max_steps;
    let report = crate::core::cognition_loop::run_cognition_loop(project_root, max_steps);
    format!("{report}")
}

fn handle_bridge_publish(project_root: &str, session_id: &str) -> String {
    let knowledge = ProjectKnowledge::load_or_create(project_root);
    let mut bridge =
        crate::core::knowledge_bridge::KnowledgeBridge::load_or_create(&knowledge.project_hash);
    let count = bridge.publish(session_id, &knowledge.facts);
    match bridge.save() {
        Ok(()) => format!(
            "Published {count} fact(s) to bridge (total: {}, agent: {session_id})",
            bridge.shared_facts.len()
        ),
        Err(e) => format!("Published {count} fact(s) but save failed: {e}"),
    }
}

fn handle_bridge_pull(project_root: &str, session_id: &str) -> String {
    let knowledge = ProjectKnowledge::load_or_create(project_root);
    let bridge =
        crate::core::knowledge_bridge::KnowledgeBridge::load_or_create(&knowledge.project_hash);
    let entries = bridge.pull(session_id);
    if entries.is_empty() {
        return "No facts available from other agents.".to_string();
    }

    let policy = match load_policy_or_error() {
        Ok(p) => p,
        Err(e) => return e,
    };

    let mut target = knowledge;
    let mut imported = 0u32;
    for entry in &entries {
        let fact = crate::core::knowledge_bridge::KnowledgeBridge::entry_to_fact(entry);
        let existing = target
            .facts
            .iter()
            .any(|f| f.is_current() && f.category == fact.category && f.key == fact.key);
        if !existing {
            target.remember(
                &fact.category,
                &fact.key,
                &fact.value,
                session_id,
                fact.confidence,
                &policy,
            );
            imported += 1;
        }
    }

    if imported == 0 {
        return format!(
            "Bridge has {} fact(s) from other agents, but all already exist locally.",
            entries.len()
        );
    }

    match target.save() {
        Ok(()) => format!(
            "Pulled {imported}/{} fact(s) from bridge into local knowledge.",
            entries.len()
        ),
        Err(e) => format!("Pulled {imported} fact(s) but save failed: {e}"),
    }
}

fn handle_bridge_status(project_root: &str) -> String {
    let knowledge = ProjectKnowledge::load_or_create(project_root);
    let bridge =
        crate::core::knowledge_bridge::KnowledgeBridge::load_or_create(&knowledge.project_hash);
    bridge.summary()
}

fn handle_wakeup(project_root: &str) -> String {
    let Some(knowledge) = ProjectKnowledge::load(project_root) else {
        return "No knowledge for wake-up briefing.".to_string();
    };
    let aaak = knowledge.format_aaak();
    if aaak.is_empty() {
        return "No knowledge yet. Start using ctx_knowledge(action=\"remember\") to build project memory.".to_string();
    }
    format!("WAKE-UP BRIEFING:\n{aaak}")
}

#[cfg(feature = "embeddings")]
struct SemanticHit {
    category: String,
    key: String,
    value: String,
    score: f32,
    semantic_score: f32,
    confidence_score: f32,
}

#[cfg(feature = "embeddings")]
fn apply_retrieval_signals_from_hits(knowledge: &mut ProjectKnowledge, hits: &[SemanticHit]) {
    let now = Utc::now();
    for s in hits {
        for f in &mut knowledge.facts {
            if !f.is_current() {
                continue;
            }
            if f.category == s.category && f.key == s.key {
                f.retrieval_count = f.retrieval_count.saturating_add(1);
                f.last_retrieved = Some(now);
                break;
            }
        }
    }
}

#[cfg(feature = "embeddings")]
fn format_semantic_facts(query: &str, hits: &[SemanticHit]) -> String {
    if hits.is_empty() {
        return format!("No facts matching '{query}'.");
    }
    let mut out = format!("Semantic recall '{query}' (showing {}):\n", hits.len());
    for s in hits {
        out.push_str(&format!(
            "  [{}/{}]: {} (score: {:.0}%, sem: {:.0}%, conf: {:.0}%)\n",
            s.category,
            s.key,
            s.value,
            s.score * 100.0,
            s.semantic_score * 100.0,
            s.confidence_score * 100.0
        ));
    }
    out
}

fn format_facts(
    facts: &[crate::core::knowledge::KnowledgeFact],
    total: usize,
    category: Option<&str>,
) -> String {
    let mut facts: Vec<&crate::core::knowledge::KnowledgeFact> = facts.iter().collect();
    facts.sort_by(|a, b| sort_fact_for_output(a, b));

    let mut out = String::new();
    if let Some(cat) = category {
        out.push_str(&format!(
            "Facts [{cat}] (showing {}/{}):\n",
            facts.len(),
            total
        ));
    } else {
        out.push_str(&format!(
            "Matching facts (showing {}/{}):\n",
            facts.len(),
            total
        ));
    }
    for f in facts {
        let temporal = if f.is_current() { "" } else { " [archived]" };
        out.push_str(&format!(
            "  [{}/{}]: {} (quality: {:.0}%, confidence: {:.0}%, confirmed: {} x{}){temporal}\n",
            f.category,
            f.key,
            f.value,
            f.quality_score() * 100.0,
            f.confidence * 100.0,
            f.last_confirmed.format("%Y-%m-%d"),
            f.confirmation_count
        ));
    }
    out
}

fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    parts[parts.len() - 2..].join("/")
}

fn short_hash(hash: &str) -> &str {
    if hash.len() > 8 {
        &hash[..8]
    } else {
        hash
    }
}

fn sort_fact_for_output(
    a: &crate::core::knowledge::KnowledgeFact,
    b: &crate::core::knowledge::KnowledgeFact,
) -> std::cmp::Ordering {
    salience_score(b)
        .cmp(&salience_score(a))
        .then_with(|| {
            b.quality_score()
                .partial_cmp(&a.quality_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| b.confirmation_count.cmp(&a.confirmation_count))
        .then_with(|| b.retrieval_count.cmp(&a.retrieval_count))
        .then_with(|| b.last_retrieved.cmp(&a.last_retrieved))
        .then_with(|| b.last_confirmed.cmp(&a.last_confirmed))
        .then_with(|| a.category.cmp(&b.category))
        .then_with(|| a.key.cmp(&b.key))
        .then_with(|| a.value.cmp(&b.value))
}

fn salience_score(f: &crate::core::knowledge::KnowledgeFact) -> u32 {
    let cat = f.category.to_lowercase();
    let base: u32 = match cat.as_str() {
        "decision" => 70,
        "gotcha" => 75,
        "architecture" | "arch" => 60,
        "security" => 65,
        "testing" | "tests" | "deployment" | "deploy" => 55,
        "conventions" | "convention" => 45,
        "finding" => 40,
        _ => 30,
    };

    let quality_bonus = (f.quality_score() * 60.0) as u32;
    let recency_bonus = f.last_retrieved.map_or(0u32, |t| {
        let days = chrono::Utc::now().signed_duration_since(t).num_days();
        if days <= 7 {
            10u32
        } else if days <= 30 {
            5u32
        } else {
            0u32
        }
    });

    base + quality_bonus + recency_bonus
}
