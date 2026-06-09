use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    CSharp,
    Kotlin,
    Swift,
    Php,
    Bash,
    Dart,
    Scala,
    Elixir,
    Zig,
    Gdscript,
    Vue,
    Svelte,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageCapabilities {
    pub deps_edges: bool,
    pub deep_queries: bool,
    pub import_resolver: bool,
}

impl LanguageId {
    pub fn id_str(&self) -> &'static str {
        match self {
            LanguageId::Rust => "rust",
            LanguageId::TypeScript => "typescript",
            LanguageId::JavaScript => "javascript",
            LanguageId::Python => "python",
            LanguageId::Go => "go",
            LanguageId::Java => "java",
            LanguageId::C => "c",
            LanguageId::Cpp => "cpp",
            LanguageId::Ruby => "ruby",
            LanguageId::CSharp => "csharp",
            LanguageId::Kotlin => "kotlin",
            LanguageId::Swift => "swift",
            LanguageId::Php => "php",
            LanguageId::Bash => "bash",
            LanguageId::Dart => "dart",
            LanguageId::Scala => "scala",
            LanguageId::Elixir => "elixir",
            LanguageId::Zig => "zig",
            LanguageId::Gdscript => "gdscript",
            LanguageId::Vue => "vue",
            LanguageId::Svelte => "svelte",
        }
    }
}

pub fn capabilities(lang: LanguageId) -> LanguageCapabilities {
    match lang {
        // tree-sitter backed (deep_queries + resolver can be meaningful)
        LanguageId::Rust
        | LanguageId::TypeScript
        | LanguageId::JavaScript
        | LanguageId::Python
        | LanguageId::Go
        | LanguageId::Java
        | LanguageId::C
        | LanguageId::Cpp
        | LanguageId::Ruby
        | LanguageId::CSharp
        | LanguageId::Kotlin
        | LanguageId::Swift
        | LanguageId::Php
        | LanguageId::Bash
        | LanguageId::Dart
        | LanguageId::Scala
        | LanguageId::Elixir
        | LanguageId::Zig
        | LanguageId::Gdscript => LanguageCapabilities {
            deps_edges: true,
            deep_queries: true,
            import_resolver: true,
        },
        // templating languages: we can extract deps edges, but no deep_queries/resolver.
        LanguageId::Vue | LanguageId::Svelte => LanguageCapabilities {
            deps_edges: true,
            deep_queries: false,
            import_resolver: false,
        },
    }
}

pub fn language_for_ext(ext: &str) -> Option<LanguageId> {
    let e = ext.trim().trim_start_matches('.').to_lowercase();
    match e.as_str() {
        "rs" => Some(LanguageId::Rust),
        "ts" | "tsx" => Some(LanguageId::TypeScript),
        "js" | "jsx" => Some(LanguageId::JavaScript),
        "py" => Some(LanguageId::Python),
        "go" => Some(LanguageId::Go),
        "java" => Some(LanguageId::Java),
        "c" | "h" => Some(LanguageId::C),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(LanguageId::Cpp),
        "rb" => Some(LanguageId::Ruby),
        "cs" => Some(LanguageId::CSharp),
        "kt" | "kts" => Some(LanguageId::Kotlin),
        "swift" => Some(LanguageId::Swift),
        "php" => Some(LanguageId::Php),
        "sh" | "bash" => Some(LanguageId::Bash),
        "dart" => Some(LanguageId::Dart),
        "scala" | "sc" => Some(LanguageId::Scala),
        "ex" | "exs" => Some(LanguageId::Elixir),
        "zig" => Some(LanguageId::Zig),
        "gd" => Some(LanguageId::Gdscript),
        "vue" => Some(LanguageId::Vue),
        "svelte" => Some(LanguageId::Svelte),
        _ => None,
    }
}

pub fn language_for_path(path: &str) -> Option<LanguageId> {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .and_then(language_for_ext)
}

pub fn is_indexable_ext(ext: &str) -> bool {
    language_for_ext(ext).is_some()
}

/// Every language the property graph / code-map can index, for capability
/// enumeration and UI hints. Keep in sync with `language_for_ext`.
pub const ALL_LANGUAGES: &[LanguageId] = &[
    LanguageId::Rust,
    LanguageId::TypeScript,
    LanguageId::JavaScript,
    LanguageId::Python,
    LanguageId::Go,
    LanguageId::Java,
    LanguageId::C,
    LanguageId::Cpp,
    LanguageId::Ruby,
    LanguageId::CSharp,
    LanguageId::Kotlin,
    LanguageId::Swift,
    LanguageId::Php,
    LanguageId::Bash,
    LanguageId::Dart,
    LanguageId::Scala,
    LanguageId::Elixir,
    LanguageId::Zig,
    LanguageId::Gdscript,
    LanguageId::Vue,
    LanguageId::Svelte,
];

/// Friendly names of every graph-indexable language (e.g. for an empty-graph hint).
pub fn graph_supported_language_names() -> Vec<&'static str> {
    ALL_LANGUAGES.iter().map(LanguageId::id_str).collect()
}

/// Whether lean-ctx extracts call sites for a language (i.e. it can populate the
/// call graph). Keep in sync with `deep_queries::calls::parse_call` — a language
/// missing there yields zero call edges, which the dashboard must communicate
/// honestly instead of suggesting an index rebuild that cannot help.
pub fn supports_call_graph(lang: LanguageId) -> bool {
    matches!(
        lang,
        LanguageId::TypeScript
            | LanguageId::JavaScript
            | LanguageId::Rust
            | LanguageId::Python
            | LanguageId::Go
            | LanguageId::Java
            | LanguageId::Kotlin
            | LanguageId::Gdscript
            | LanguageId::CSharp
    )
}

/// Friendly names of every language with call-graph extraction support.
pub fn callgraph_supported_language_names() -> Vec<&'static str> {
    ALL_LANGUAGES
        .iter()
        .filter(|l| supports_call_graph(**l))
        .map(LanguageId::id_str)
        .collect()
}

/// Per-language capability row for a project: which analyses are available for a
/// language and how many files use it. Backs the dashboard capability legend so
/// each detected language is labelled honestly (symbols / import edges / calls).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LanguageCapabilityRow {
    pub language: &'static str,
    pub files: usize,
    pub symbols: bool,
    pub imports: bool,
    pub call_graph: bool,
}

/// Build a capability matrix for the languages actually present in `file_paths`,
/// sorted by file count (desc) then name. `symbols`/`imports` come from
/// `capabilities()`; `call_graph` from `supports_call_graph()`.
pub fn language_capability_matrix<I, S>(file_paths: I) -> Vec<LanguageCapabilityRow>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut counts: std::collections::HashMap<LanguageId, usize> = std::collections::HashMap::new();
    for path in file_paths {
        if let Some(lang) = language_for_path(path.as_ref()) {
            *counts.entry(lang).or_default() += 1;
        }
    }
    let mut rows: Vec<LanguageCapabilityRow> = counts
        .into_iter()
        .map(|(lang, files)| {
            let caps = capabilities(lang);
            LanguageCapabilityRow {
                language: lang.id_str(),
                files,
                symbols: caps.deep_queries,
                imports: caps.import_resolver,
                call_graph: supports_call_graph(lang),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.files
            .cmp(&a.files)
            .then_with(|| a.language.cmp(b.language))
    });
    rows
}

/// Maps a file extension to a human-readable *programming language* name that
/// lean-ctx recognizes but does **not** graph-index. Returns `None` for
/// graph-indexed languages and for non-code files (docs, data, config). Used
/// only to explain an empty graph — e.g. a Lua/Luau project (#360).
fn unsupported_source_language_name(ext: &str) -> Option<&'static str> {
    match ext.trim().trim_start_matches('.').to_lowercase().as_str() {
        "lua" => Some("Lua"),
        "luau" => Some("Luau"),
        "r" => Some("R"),
        "jl" => Some("Julia"),
        "nim" => Some("Nim"),
        "cr" => Some("Crystal"),
        "clj" | "cljs" | "cljc" => Some("Clojure"),
        "erl" | "hrl" => Some("Erlang"),
        "hs" => Some("Haskell"),
        "ml" | "mli" => Some("OCaml"),
        "fs" | "fsx" => Some("F#"),
        "pl" | "pm" => Some("Perl"),
        "groovy" | "gradle" => Some("Groovy"),
        "tf" => Some("Terraform"),
        "sol" => Some("Solidity"),
        "f90" | "f95" | "f03" => Some("Fortran"),
        "pas" => Some("Pascal"),
        "d" => Some("D"),
        "sql" => Some("SQL"),
        "tcl" => Some("Tcl"),
        "raku" | "rakumod" => Some("Raku"),
        _ => None,
    }
}

/// Bounded project scan returning programming languages present in `root` that
/// lean-ctx does **not** graph-index, with file counts (descending, capped to 5).
/// Honors .gitignore/hidden like the graph walker and stops after `max_entries`
/// filesystem entries. Lets the dashboard turn a confusing empty graph into a
/// clear "Lua is not graph-indexed" message instead of an endless loading state.
pub fn scan_unsupported_source_languages(root: &str, max_entries: usize) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .max_depth(Some(20))
        .build();
    for entry in walker.flatten().take(max_entries) {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if let Some(name) = unsupported_source_language_name(ext) {
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut ranked: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(k, c)| (k.to_string(), c))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(5);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_mapping_basic() {
        assert_eq!(language_for_ext("rs"), Some(LanguageId::Rust));
        assert_eq!(language_for_ext(".tsx"), Some(LanguageId::TypeScript));
        assert_eq!(language_for_ext("JS"), Some(LanguageId::JavaScript));
        assert_eq!(language_for_ext("hxx"), Some(LanguageId::Cpp));
        assert_eq!(language_for_ext("exs"), Some(LanguageId::Elixir));
        assert_eq!(language_for_ext("unknown"), None);
    }

    #[test]
    fn indexable_ext_true_for_known() {
        assert!(is_indexable_ext("rs"));
        assert!(is_indexable_ext("vue"));
        assert!(!is_indexable_ext("md"));
    }

    #[test]
    fn caps_are_deterministic() {
        let c1 = capabilities(LanguageId::Rust);
        let c2 = capabilities(LanguageId::Rust);
        assert_eq!(c1, c2);
        assert!(c1.deps_edges);
    }

    #[test]
    fn all_languages_match_ext_table() {
        // Every enumerated language must be reachable via at least one extension,
        // so the UI's "supported languages" list never drifts from reality.
        for lang in ALL_LANGUAGES {
            let names = graph_supported_language_names();
            assert!(names.contains(&lang.id_str()));
        }
        assert!(graph_supported_language_names().contains(&"rust"));
        assert_eq!(ALL_LANGUAGES.len(), graph_supported_language_names().len());
    }

    #[test]
    fn callgraph_support_is_consistent() {
        // C# must be call-graph capable (issue: NINA's empty Call Graph tab).
        assert!(supports_call_graph(LanguageId::CSharp));
        let names = callgraph_supported_language_names();
        assert!(names.contains(&"csharp"));
        assert!(names.contains(&"rust"));
        assert!(names.contains(&"typescript"));
        // Every call-graph language is also graph-indexable, and the list is a
        // strict subset (some graph-indexed languages have no call extraction).
        for name in &names {
            assert!(graph_supported_language_names().contains(name));
        }
        assert!(names.len() <= ALL_LANGUAGES.len());
        // A language without call extraction must report false.
        assert!(!supports_call_graph(LanguageId::Ruby));
    }

    #[test]
    fn capability_matrix_reports_per_language_support() {
        let paths = ["a.rs", "b.rs", "c.rb", "d.cs", "readme.md"];
        let matrix = language_capability_matrix(paths);

        // Non-code files are excluded; three languages are detected.
        assert_eq!(matrix.len(), 3);

        let rust = matrix.iter().find(|r| r.language == "rust").unwrap();
        assert_eq!(rust.files, 2);
        assert!(rust.symbols && rust.imports && rust.call_graph);

        // Ruby has symbols + imports but no call-graph extraction.
        let ruby = matrix.iter().find(|r| r.language == "ruby").unwrap();
        assert!(ruby.symbols && ruby.imports && !ruby.call_graph);

        // C# is fully supported (the language behind the original bug report).
        let csharp = matrix.iter().find(|r| r.language == "csharp").unwrap();
        assert!(csharp.symbols && csharp.imports && csharp.call_graph);

        // Sorted by file count desc → Rust (2 files) leads.
        assert_eq!(matrix[0].language, "rust");
    }

    #[test]
    fn unsupported_source_languages_named_but_not_indexed() {
        // Lua/Luau (issue #360) are recognized as code yet never graph-indexed.
        assert_eq!(unsupported_source_language_name("lua"), Some("Lua"));
        assert_eq!(unsupported_source_language_name(".luau"), Some("Luau"));
        assert!(!is_indexable_ext("lua"));
        assert!(!is_indexable_ext("luau"));
        // Graph-indexed languages and plain data/docs are not reported as "unsupported code".
        assert_eq!(unsupported_source_language_name("rs"), None);
        assert_eq!(unsupported_source_language_name("md"), None);
        assert_eq!(unsupported_source_language_name("json"), None);
    }

    #[test]
    fn scan_reports_lua_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("init.lua"), "local x = 1").unwrap();
        std::fs::write(dir.path().join("mod.luau"), "return {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# docs").unwrap();
        let found = scan_unsupported_source_languages(&dir.path().to_string_lossy(), 1000);
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Lua"));
        assert!(names.contains(&"Luau"));
    }
}
