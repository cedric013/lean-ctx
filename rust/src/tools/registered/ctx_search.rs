use rmcp::model::Tool;
use rmcp::ErrorData;
use serde_json::{json, Map, Value};

use crate::server::tool_trait::{get_bool, get_int, get_str, McpTool, ToolContext, ToolOutput};
use crate::tool_defs::tool_def;

pub struct CtxSearchTool;

impl McpTool for CtxSearchTool {
    fn name(&self) -> &'static str {
        "ctx_search"
    }

    fn tool_def(&self) -> Tool {
        tool_def(
            "ctx_search",
            "Search code by regex. Prefer over native Grep/rg/find (compact output).\n\
             Respects .gitignore; supports multi-root via `paths` array. Secret-like files skipped unless role allows.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern" },
                    "path": { "type": "string", "description": "Directory to search" },
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Multiple directories to search (alternative to path)"
                    },
                    "include": { "type": "string", "description": "File filter glob (e.g. *.ts, *.{rs,ts}, src/**/*.tsx)" },
                    "ext": { "type": "string", "description": "Deprecated alias for `include`: a bare extension like `rs` or `.rs` is treated as `*.rs`. Prefer `include`." },
                    "max_results": { "type": "integer", "description": "Max results (default: 20)" },
                    "ignore_gitignore": { "type": "boolean", "description": "Set true to scan ALL files including .gitignore'd paths (default: false). Requires role policy (e.g. admin)." }
                },
                "required": ["pattern"]
            }),
        )
    }

    fn handle(
        &self,
        args: &Map<String, Value>,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ErrorData> {
        let pattern = get_str(args, "pattern")
            .ok_or_else(|| ErrorData::invalid_params("pattern is required", None))?;
        let resolved = crate::server::multi_path::resolve_tool_paths(args, ctx);
        // `include` is the canonical glob filter; `ext` is the deprecated alias
        // (bare extension → `*.{ext}`). `include` wins when both are supplied.
        let include =
            get_str(args, "include").or_else(|| get_str(args, "ext").map(|e| ext_to_include(&e)));
        let max = (get_int(args, "max_results").unwrap_or(20) as usize).min(500);
        let no_gitignore = get_bool(args, "ignore_gitignore").unwrap_or(false);

        if no_gitignore {
            if let Err(e) = crate::core::io_boundary::ensure_ignore_gitignore_allowed("ctx_search")
            {
                return Ok(ToolOutput::simple(e));
            }
        }

        let crp = ctx.crp_mode;
        let respect = !no_gitignore;
        let allow_secret_paths = crate::core::roles::active_role().io.allow_secret_paths;

        if !resolved.is_multi {
            return search_single(
                &pattern,
                &resolved.roots[0],
                include.as_deref(),
                max,
                crp,
                respect,
                allow_secret_paths,
            );
        }

        let _mode_guard = crate::core::savings_footer::ModeGuard::new("search");
        let per_root_max = (max / resolved.roots.len()).max(5);
        let mut combined = String::new();
        let mut total_original: usize = 0;
        let mut total_sent: usize = 0;

        for root in &resolved.roots {
            let pat = pattern.clone();
            let r = root.clone();
            let inc = include.clone();

            let search_result = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::tools::ctx_search::handle(
                        &pat,
                        &r,
                        inc.as_deref(),
                        per_root_max,
                        crp,
                        respect,
                        allow_secret_paths,
                    )
                }))
                .ok()
            });

            let Some((result, original)) = search_result else {
                combined.push_str(&format!("── {root} ──\nERROR: search panicked\n\n"));
                continue;
            };

            if result.starts_with("ERROR:") || result.trim().is_empty() {
                if !result.trim().is_empty() {
                    combined.push_str(&format!("── {root} ──\n{result}\n\n"));
                }
                continue;
            }

            combined.push_str(&format!("── {root} ──\n{result}\n\n"));
            total_original += original;
            total_sent += crate::core::tokens::count_tokens(&result);
        }

        if combined.is_empty() {
            combined = "No matches found across any root.".to_string();
        }

        let final_out =
            crate::core::protocol::append_savings(&combined, total_original, total_sent);
        let saved = total_original.saturating_sub(total_sent);

        Ok(ToolOutput {
            text: final_out,
            original_tokens: total_original,
            saved_tokens: saved,
            mode: None,
            path: None,
            changed: false,
        })
    }
}

fn search_single(
    pattern: &str,
    path: &str,
    include: Option<&str>,
    max: usize,
    crp: crate::tools::CrpMode,
    respect_gitignore: bool,
    allow_secret_paths: bool,
) -> Result<ToolOutput, ErrorData> {
    let _mode_guard = crate::core::savings_footer::ModeGuard::new("search");
    let pattern_clone = pattern.to_string();
    let path_clone = path.to_string();

    let search_result = tokio::task::block_in_place(|| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::tools::ctx_search::handle(
                &pattern_clone,
                &path_clone,
                include,
                max,
                crp,
                respect_gitignore,
                allow_secret_paths,
            )
        }));
        match result {
            Ok(r) => Ok(r),
            Err(_) => Err("search task panicked"),
        }
    });

    let (result, original) = match search_result {
        Ok(r) => r,
        Err(e) => {
            return Err(ErrorData::internal_error(
                format!("search task failed: {e}"),
                None,
            ));
        }
    };

    if result.starts_with("ERROR:") {
        return Err(ErrorData::invalid_params(result, None));
    }

    let sent = crate::core::tokens::count_tokens(&result);
    let saved = original.saturating_sub(sent);
    let final_out = crate::core::protocol::append_savings(&result, original, sent);

    Ok(ToolOutput {
        text: final_out,
        original_tokens: original,
        saved_tokens: saved,
        mode: None,
        path: Some(path.to_string()),
        changed: false,
    })
}

/// Translate the deprecated `ext` parameter into an `include` glob.
///
/// The historical `ext` accepted a bare extension (`rs` or `.rs`) and matched it
/// exactly; the equivalent glob is `*.{ext}` (the `glob` crate's `*` spans path
/// separators, so it still matches at any depth, preserving the old behaviour).
/// A value that already looks like a glob/path (`*`, `{`, `?`, `/`) is passed
/// through untouched so any power user who put a pattern in `ext` keeps working.
fn ext_to_include(ext: &str) -> String {
    if ext.contains(['*', '{', '?', '/']) {
        return ext.to_string();
    }
    let bare = ext.strip_prefix('.').unwrap_or(ext);
    format!("*.{bare}")
}

#[cfg(test)]
mod tests {
    use super::ext_to_include;

    #[test]
    fn ext_alias_bare_extension_becomes_glob() {
        assert_eq!(ext_to_include("rs"), "*.rs");
        assert_eq!(ext_to_include("ts"), "*.ts");
    }

    #[test]
    fn ext_alias_strips_leading_dot() {
        assert_eq!(ext_to_include(".rs"), "*.rs");
        assert_eq!(ext_to_include(".tsx"), "*.tsx");
    }

    #[test]
    fn ext_alias_passes_through_glob_like_values() {
        // Already a glob/path → keep verbatim, don't double-wrap.
        assert_eq!(ext_to_include("*.rs"), "*.rs");
        assert_eq!(ext_to_include("*.{rs,ts}"), "*.{rs,ts}");
        assert_eq!(ext_to_include("src/**/*.tsx"), "src/**/*.tsx");
    }
}
