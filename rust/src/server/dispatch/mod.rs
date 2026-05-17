use rmcp::ErrorData;
use serde_json::Value;

use crate::server::helpers::get_str;
use crate::tools::LeanCtxServer;

impl LeanCtxServer {
    pub(super) async fn dispatch_tool(
        &self,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
        minimal: bool,
    ) -> Result<String, ErrorData> {
        fn format_rate_limited(
            tool: &str,
            agent_id: &str,
            retry_after_ms: u64,
            args: Option<&serde_json::Map<String, Value>>,
        ) -> String {
            let as_json = get_str(args, "format").as_deref() == Some("json");
            if as_json {
                serde_json::json!({
                    "error": "rate_limited",
                    "tool": tool,
                    "agent_id": agent_id,
                    "retry_after_ms": retry_after_ms,
                })
                .to_string()
            } else {
                format!("[RATE LIMITED] tool={tool} retry_after_ms={retry_after_ms}")
            }
        }

        let agent_id = self
            .agent_id
            .read()
            .await
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if name != "ctx_call" {
            if let crate::core::a2a::rate_limiter::RateLimitResult::Limited { retry_after_ms } =
                crate::core::a2a::rate_limiter::check_rate_limit(&agent_id, name)
            {
                return Ok(format_rate_limited(name, &agent_id, retry_after_ms, args));
            }
        }

        match name {
            "ctx_call" => {
                let inner = get_str(args, "name")
                    .ok_or_else(|| ErrorData::invalid_params("name is required", None))?;
                if inner == "ctx_call" {
                    return Err(ErrorData::invalid_params(
                        "ctx_call cannot invoke itself",
                        None,
                    ));
                }

                let arg_map = match args.and_then(|m| m.get("arguments")) {
                    None | Some(Value::Null) => None,
                    Some(Value::Object(map)) => Some(map.clone()),
                    Some(_) => {
                        return Err(ErrorData::invalid_params(
                            "arguments must be an object",
                            None,
                        ))
                    }
                };

                if let crate::core::a2a::rate_limiter::RateLimitResult::Limited { retry_after_ms } =
                    crate::core::a2a::rate_limiter::check_rate_limit(&agent_id, &inner)
                {
                    return Ok(format_rate_limited(
                        &inner,
                        &agent_id,
                        retry_after_ms,
                        arg_map.as_ref(),
                    ));
                }

                if inner != "ctx_workflow" {
                    let active = self.workflow.read().await.clone();
                    if let Some(run) = active {
                        if let Some(state) = run.spec.state(&run.current) {
                            if let Some(allowed) = &state.allowed_tools {
                                let ok = allowed.iter().any(|t| t == &inner) || inner == "ctx";
                                if !ok {
                                    let mut shown = allowed.clone();
                                    shown.sort();
                                    shown.truncate(30);
                                    return Ok(format!(
                                        "Tool '{inner}' blocked by workflow '{}' (state: {}). Allowed ({} shown): {}",
                                        run.spec.name,
                                        run.current,
                                        shown.len(),
                                        shown.join(", ")
                                    ));
                                }
                            }
                        }
                    }
                }

                let result = self
                    .dispatch_inner(&inner, arg_map.as_ref(), minimal)
                    .await?;
                self.record_call("ctx_call", 0, 0, Some(inner)).await;
                Ok(result)
            }
            _ => self.dispatch_inner(name, args, minimal).await,
        }
    }

    /// Dispatches a single tool via the trait-based registry.
    async fn dispatch_inner(
        &self,
        name: &str,
        args: Option<&serde_json::Map<String, Value>>,
        minimal: bool,
    ) -> Result<String, ErrorData> {
        if let Some(tool) = self.registry.as_ref().and_then(|r| r.get(name)) {
            let empty = serde_json::Map::new();
            let args_map = args.unwrap_or(&empty);
            let project_root = {
                let session = self.session.read().await;
                session.project_root.clone().unwrap_or_default()
            };

            let mut resolved_paths = std::collections::HashMap::new();
            for key in PATH_LIKE_KEYS {
                if let Some(raw) = args_map.get(*key).and_then(|v| v.as_str()) {
                    if let Ok(resolved) = self.resolve_path(raw).await {
                        if !["path", "project_root", "root"].contains(key) {
                            tracing::trace!("[pathjail] resolved non-standard path key '{key}': {raw} -> {resolved}");
                        }
                        resolved_paths.insert(key.to_string(), resolved);
                    }
                }
            }

            let crp_mode = crate::tools::CrpMode::effective();
            let pressure_snapshot = {
                let ledger = self.ledger.read().await;
                Some(ledger.pressure())
            };
            let ctx = crate::server::tool_trait::ToolContext {
                project_root,
                minimal,
                resolved_paths,
                crp_mode,
                cache: Some(self.cache.clone()),
                session: Some(self.session.clone()),
                tool_calls: Some(self.tool_calls.clone()),
                agent_id: Some(self.agent_id.clone()),
                workflow: Some(self.workflow.clone()),
                ledger: Some(self.ledger.clone()),
                client_name: Some(self.client_name.clone()),
                pipeline_stats: Some(self.pipeline_stats.clone()),
                call_count: Some(self.call_count.clone()),
                autonomy: Some(self.autonomy.clone()),
                pressure_snapshot,
            };
            let output = tokio::task::block_in_place(|| tool.handle(args_map, &ctx))?;

            if output.changed {
                if let Some(peer) = self.peer.read().await.as_ref() {
                    super::notifications::send_tools_list_changed(peer).await;
                }
            }

            let headers_only =
                crate::core::config::ResponseVerbosity::effective().is_headers_only();
            let header_line = if headers_only {
                Some(output.to_header_line(name))
            } else {
                None
            };

            let output_token_estimate = crate::core::tokens::count_tokens(&output.text) as u32;

            if let Some(ref path) = output.path {
                {
                    let sent_tokens = if output.original_tokens > 0 {
                        output.original_tokens.saturating_sub(output.saved_tokens)
                    } else {
                        crate::core::tokens::count_tokens(&output.text)
                    };
                    let orig = if output.original_tokens > 0 {
                        output.original_tokens
                    } else {
                        sent_tokens
                    };
                    let mode_str = output.mode.as_deref().unwrap_or("full");
                    let mut ledger = self.ledger.write().await;
                    ledger.record(path, mode_str, orig, sent_tokens);
                    ledger.save();
                }
                self.record_call_with_path(
                    name,
                    output.original_tokens,
                    output.saved_tokens,
                    output.mode,
                    Some(path),
                )
                .await;
            } else {
                self.record_call(
                    name,
                    output.original_tokens,
                    output.saved_tokens,
                    output.mode,
                )
                .await;
            }

            {
                let agent_id = self
                    .agent_id
                    .read()
                    .await
                    .clone()
                    .unwrap_or_else(|| "unknown".into());
                let input_hash = crate::core::audit_trail::hash_input(args_map);
                let role = crate::core::roles::active_role_name();
                crate::core::audit_trail::record(crate::core::audit_trail::AuditEntryData {
                    agent_id,
                    tool: name.to_string(),
                    action: None,
                    input_hash,
                    output_tokens: output_token_estimate,
                    role,
                    event_type: crate::core::audit_trail::AuditEventType::ToolCall,
                });
            }

            let final_text = header_line.unwrap_or(output.text);

            let reference_enabled = std::env::var("LEAN_CTX_REFERENCE_RESULTS").map_or_else(
                |_| crate::core::config::Config::load().reference_results,
                |v| v == "1" || v == "true",
            );

            if reference_enabled && final_text.len() > REFERENCE_THRESHOLD {
                let ref_id = super::reference_store::store(final_text.clone());
                let preview_end = final_text.len().min(200);
                let summary = format!(
                    "[Reference: {ref_id}] Output stored ({} chars, ~{} tokens). Resolve: /v1/references/{ref_id}\nPreview: {}...",
                    final_text.len(),
                    final_text.len() / 4,
                    &final_text[..preview_end]
                );
                return Ok(summary);
            }

            return Ok(final_text);
        }

        Err(ErrorData::invalid_params(
            format!("Unknown tool: {name}"),
            None,
        ))
    }
}

const REFERENCE_THRESHOLD: usize = 4000;

const PATH_LIKE_KEYS: &[&str] = &[
    "path",
    "project_root",
    "root",
    "file",
    "directory",
    "dir",
    "target",
    "source",
    "destination",
    "old_path",
    "new_path",
    "from",
    "to",
    "base_path",
    "config_path",
    "output",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_like_keys_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for key in PATH_LIKE_KEYS {
            assert!(seen.insert(*key), "duplicate PATH_LIKE_KEYS entry: {key}");
        }
    }

    #[test]
    fn path_like_keys_includes_primary_keys() {
        for primary in &["path", "project_root", "root"] {
            assert!(
                PATH_LIKE_KEYS.contains(primary),
                "primary key '{primary}' missing from PATH_LIKE_KEYS"
            );
        }
    }

    #[test]
    fn path_like_keys_all_non_empty() {
        for key in PATH_LIKE_KEYS {
            assert!(!key.is_empty(), "PATH_LIKE_KEYS contains empty string");
        }
        assert!(
            PATH_LIKE_KEYS.len() >= 3,
            "PATH_LIKE_KEYS must have at least the 3 primary keys"
        );
    }
}
