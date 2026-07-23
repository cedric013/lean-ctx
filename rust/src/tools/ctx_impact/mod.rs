//! `ctx_impact` — Graph-based impact analysis tool.
//!
//! Uses the SQLite-backed Property Graph to answer: "What breaks when file X changes?"
//! Performs BFS traversal of reverse import edges to find all transitively affected files.

use crate::core::property_graph::CodeGraph;

/// Extensions whose files become Property Graph source nodes. Must stay a subset
/// of `language_capabilities::is_indexable_ext` and align with the deep-query
/// extractors (`deep_queries::{type_defs, calls}`) so each language contributes
/// real symbol/import/call structure rather than bare file nodes.
const GRAPH_SOURCE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "gd", "cs", "kt", "kts",
];

fn open_graph(root: &str) -> Result<CodeGraph, String> {
    CodeGraph::open(root).map_err(|e| format!("Failed to open graph: {e}"))
}

mod analyze;
mod build;

pub use analyze::*;
#[allow(unused_imports)]
pub(crate) use build::*;
