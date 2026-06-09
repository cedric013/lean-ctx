use rmcp::model::Tool;
use rmcp::ErrorData;
use serde_json::{json, Map, Value};

use crate::server::tool_trait::{
    get_bool, get_str, get_str_array, McpTool, ToolContext, ToolOutput,
};
use crate::tool_defs::tool_def;

pub struct CtxMultiReadTool;

impl McpTool for CtxMultiReadTool {
    fn name(&self) -> &'static str {
        "ctx_multi_read"
    }

    fn tool_def(&self) -> Tool {
        tool_def(
            "ctx_multi_read",
            "Batch read files in one call. Same modes as ctx_read.",
            json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Absolute file paths to read, in order"
                    },
                    "mode": {
                        "type": "string",
                        "description": "Compression mode (default: full). Same modes as ctx_read (auto, full, raw, map, signatures, diff, aggressive, entropy, task, reference, lines:N-M). Use 'raw' for zero-overhead output."
                    },
                    "fresh": {
                        "type": "boolean",
                        "description": "Bypass cache and force a full re-read for all paths. Use when running as a subagent that may not have the parent's context."
                    }
                },
                "required": ["paths"]
            }),
        )
    }

    fn handle(
        &self,
        args: &Map<String, Value>,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ErrorData> {
        // Panic guard (mirrors ctx_read): a panic in tree-sitter / compression must
        // never unwind through the dispatch `block_in_place` and kill the MCP server.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle_inner(args, ctx)))
        {
            Ok(result) => result,
            Err(_) => Err(ErrorData::internal_error(
                "ctx_multi_read panicked while processing the batch. This is a bug — please report it.",
                None,
            )),
        }
    }
}

impl CtxMultiReadTool {
    #[allow(clippy::unused_self)]
    fn handle_inner(
        &self,
        args: &Map<String, Value>,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ErrorData> {
        let raw_paths = get_str_array(args, "paths")
            .ok_or_else(|| ErrorData::invalid_params("paths array is required", None))?;

        let session_lock = ctx
            .session
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("session not available", None))?;
        let cache_lock = ctx
            .cache
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("cache not available", None))?;

        let cap = crate::core::limits::max_read_bytes() as u64;

        // Resolve + filter paths and capture the active task under one short read lock.
        // `bounded_lock` uses `Handle::block_on` directly — NOT a nested
        // `block_in_place` — because the dispatch layer already wraps this handler in
        // `block_in_place`. The previous nested `block_in_place` calls could exhaust the
        // 32-thread blocking pool under concurrent reads and freeze the server (#271).
        let (paths, current_task) = {
            let Some(session) =
                crate::server::bounded_lock::read(session_lock, "ctx_multi_read:session")
            else {
                return Err(ErrorData::internal_error(
                    "session read-lock timeout in ctx_multi_read — another tool may be holding it. Retry in a moment.",
                    None,
                ));
            };
            let mut paths = Vec::with_capacity(raw_paths.len());
            for p in &raw_paths {
                let resolved = super::resolve_path_sync(&session, p)
                    .map_err(|e| ErrorData::invalid_params(e, None))?;
                if crate::core::binary_detect::is_binary_file(&resolved) {
                    continue;
                }
                if let Ok(meta) = std::fs::metadata(&resolved) {
                    if meta.len() > cap {
                        continue;
                    }
                }
                paths.push(resolved);
            }
            let current_task = session.task.as_ref().map(|t| t.description.clone());
            (paths, current_task)
        };

        if paths.is_empty() {
            return Err(ErrorData::invalid_params(
                "all paths are binary or exceed the size limit",
                None,
            ));
        }

        let mode = get_str(args, "mode").unwrap_or_else(|| {
            let p = crate::core::profiles::active_profile();
            let dm = p.read.default_mode_effective();
            if dm == "auto" {
                "full".to_string()
            } else {
                dm.to_string()
            }
        });
        let fresh = get_bool(args, "fresh").unwrap_or(false);

        // Batch read under one bounded write lock. `bounded_lock` guarantees we never
        // block the runtime indefinitely and degrade gracefully on contention instead
        // of hanging; ctx_read's own fast/slow path tolerates this lock being held.
        let Some(mut cache) =
            crate::server::bounded_lock::write(cache_lock, "ctx_multi_read:cache")
        else {
            return Err(ErrorData::internal_error(
                "cache write-lock timeout in ctx_multi_read — another tool may be holding it. Retry in a moment.",
                None,
            ));
        };
        let output = crate::tools::ctx_multi_read::handle_with_task_fresh(
            &mut cache,
            &paths,
            &mode,
            fresh,
            ctx.crp_mode,
            current_task.as_deref(),
        );
        let mut total_original: usize = 0;
        for path in &paths {
            total_original =
                total_original.saturating_add(cache.get(path).map_or(0, |e| e.original_tokens));
        }
        let tokens = crate::core::tokens::count_tokens(&output);
        drop(cache);

        Ok(ToolOutput {
            text: output,
            original_tokens: total_original,
            saved_tokens: total_original.saturating_sub(tokens),
            mode: Some(mode),
            path: None,
            changed: false,
        })
    }
}
