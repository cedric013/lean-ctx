mod analysis;
mod analysis_cache;
mod architecture;
mod callgraph;
mod deps;
mod tree;

pub(super) fn handle(
    path: &str,
    query_str: &str,
    _method: &str,
    _body: &str,
) -> Option<(&'static str, &'static str, String)> {
    deps::get_route(path, query_str)
        .or_else(|| callgraph::get_route(path, query_str))
        .or_else(|| analysis::get_route(path, query_str))
        .or_else(|| architecture::get_route(path, query_str))
        .or_else(|| tree::get_route(path, query_str))
}

fn project_basename(abs_root: &str) -> String {
    std::path::Path::new(abs_root).file_name().map_or_else(
        || "project".to_string(),
        |n| n.to_string_lossy().to_string(),
    )
}
