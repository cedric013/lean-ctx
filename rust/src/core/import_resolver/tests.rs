use super::*;
use crate::core::deep_queries::{ImportInfo, ImportKind};

fn make_ctx(files: &[&str]) -> ResolverContext {
    let file_paths: Vec<String> = files.iter().map(std::string::ToString::to_string).collect();
    ResolverContext {
        project_root: PathBuf::from("/project"),
        file_paths: file_paths.clone(),
        tsconfig_paths: HashMap::new(),
        go_module: None,
        dart_package: None,
        file_set: file_paths.iter().cloned().collect(),
        csharp_ns_index: build_csharp_namespace_index(
            &PathBuf::from("/project"),
            &file_paths,
            &HashMap::new(),
        ),
    }
}

fn make_import(source: &str) -> ImportInfo {
    ImportInfo {
        source: source.to_string(),
        names: Vec::new(),
        kind: ImportKind::Named,
        line: 1,
        is_type_only: false,
    }
}

// --- TypeScript ---

#[test]
fn ts_relative_import() {
    let ctx = make_ctx(&["src/components/Button.tsx", "src/utils/helpers.ts"]);
    let imp = make_import("./helpers");
    let results = resolve_imports(&[imp], "src/utils/index.ts", "ts", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/utils/helpers.ts")
    );
    assert!(!results[0].is_external);
}

#[test]
fn ts_relative_parent() {
    let ctx = make_ctx(&["src/utils.ts", "src/components/Button.tsx"]);
    let imp = make_import("../utils");
    let results = resolve_imports(&[imp], "src/components/Button.tsx", "ts", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/utils.ts"));
}

#[test]
fn ts_index_file() {
    let ctx = make_ctx(&["src/components/index.ts", "src/app.ts"]);
    let imp = make_import("./components");
    let results = resolve_imports(&[imp], "src/app.ts", "ts", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/components/index.ts")
    );
}

#[test]
fn ts_relative_js_specifier_resolves_to_ts_source() {
    let ctx = make_ctx(&["src/b.ts", "src/a.ts"]);
    let imp = make_import("./b.js");
    let results = resolve_imports(&[imp], "src/a.ts", "ts", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/b.ts"));
    assert!(!results[0].is_external);
}

#[test]
fn ts_relative_jsx_specifier_resolves_to_tsx_source() {
    let ctx = make_ctx(&["src/Button.tsx", "src/App.tsx"]);
    let imp = make_import("./Button.jsx");
    let results = resolve_imports(&[imp], "src/App.tsx", "tsx", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/Button.tsx"));
}

#[test]
fn ts_relative_mjs_specifier_resolves_to_mts_source() {
    let ctx = make_ctx(&["src/utils.mts", "src/main.mts"]);
    let imp = make_import("./utils.mjs");
    let results = resolve_imports(&[imp], "src/main.mts", "ts", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/utils.mts"));
}

#[test]
fn ts_relative_js_specifier_falls_back_to_js_file() {
    let ctx = make_ctx(&["src/legacy.js", "src/app.ts"]);
    let imp = make_import("./legacy.js");
    let results = resolve_imports(&[imp], "src/app.ts", "ts", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/legacy.js"));
}

#[test]
fn ts_external_package() {
    let ctx = make_ctx(&["src/app.ts"]);
    let imp = make_import("react");
    let results = resolve_imports(&[imp], "src/app.ts", "ts", &ctx);
    assert!(results[0].is_external);
    assert!(results[0].resolved_path.is_none());
}

#[test]
fn ts_tsconfig_paths() {
    let mut ctx = make_ctx(&["src/lib/utils/format.ts"]);
    ctx.tsconfig_paths
        .insert("@utils/*".to_string(), "src/lib/utils/*".to_string());
    let imp = make_import("@utils/format");
    let results = resolve_imports(&[imp], "src/app.ts", "ts", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/lib/utils/format.ts")
    );
    assert!(!results[0].is_external);
}

// --- Rust ---

#[test]
fn rust_crate_import() {
    let ctx = make_ctx(&["src/core/session.rs", "src/main.rs"]);
    let imp = make_import("crate::core::session");
    let results = resolve_imports(&[imp], "src/server.rs", "rs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/core/session.rs")
    );
    assert!(!results[0].is_external);
}

#[test]
fn rust_mod_rs() {
    let ctx = make_ctx(&["src/core/mod.rs", "src/main.rs"]);
    let imp = make_import("crate::core");
    let results = resolve_imports(&[imp], "src/main.rs", "rs", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("src/core/mod.rs"));
}

#[test]
fn rust_external_crate() {
    let ctx = make_ctx(&["src/main.rs"]);
    let imp = make_import("anyhow::Result");
    let results = resolve_imports(&[imp], "src/main.rs", "rs", &ctx);
    assert!(results[0].is_external);
}

#[test]
fn rust_symbol_in_module() {
    let ctx = make_ctx(&["src/core/session.rs"]);
    let imp = make_import("crate::core::session::SessionState");
    let results = resolve_imports(&[imp], "src/server.rs", "rs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/core/session.rs")
    );
}

// --- Python ---

#[test]
fn python_absolute_import() {
    let ctx = make_ctx(&["models/user.py", "app.py"]);
    let imp = make_import("models.user");
    let results = resolve_imports(&[imp], "app.py", "py", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("models/user.py"));
}

#[test]
fn python_package_init() {
    let ctx = make_ctx(&["utils/__init__.py", "app.py"]);
    let imp = make_import("utils");
    let results = resolve_imports(&[imp], "app.py", "py", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("utils/__init__.py")
    );
}

#[test]
fn python_relative_import() {
    let ctx = make_ctx(&["pkg/utils.py", "pkg/main.py"]);
    let imp = make_import(".utils");
    let results = resolve_imports(&[imp], "pkg/main.py", "py", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("pkg/utils.py"));
}

#[test]
fn python_stdlib() {
    let ctx = make_ctx(&["app.py"]);
    let imp = make_import("os");
    let results = resolve_imports(&[imp], "app.py", "py", &ctx);
    assert!(results[0].is_external);
}

// --- Go ---

#[test]
fn go_internal_package() {
    let mut ctx = make_ctx(&["cmd/server/main.go", "internal/auth/auth.go"]);
    ctx.go_module = Some("github.com/org/project".to_string());
    let imp = make_import("github.com/org/project/internal/auth");
    let results = resolve_imports(&[imp], "cmd/server/main.go", "go", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("internal/auth/auth.go")
    );
    assert!(!results[0].is_external);
}

#[test]
fn go_external_package() {
    let ctx = make_ctx(&["main.go"]);
    let imp = make_import("fmt");
    let results = resolve_imports(&[imp], "main.go", "go", &ctx);
    assert!(results[0].is_external);
}

// --- Java ---

#[test]
fn java_internal_class() {
    let ctx = make_ctx(&[
        "src/main/java/com/example/service/UserService.java",
        "src/main/java/com/example/model/User.java",
    ]);
    let imp = make_import("com.example.model.User");
    let results = resolve_imports(
        &[imp],
        "src/main/java/com/example/service/UserService.java",
        "java",
        &ctx,
    );
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/main/java/com/example/model/User.java")
    );
    assert!(!results[0].is_external);
}

#[test]
fn java_stdlib() {
    let ctx = make_ctx(&["Main.java"]);
    let imp = make_import("java.util.List");
    let results = resolve_imports(&[imp], "Main.java", "java", &ctx);
    assert!(results[0].is_external);
}

// --- Edge cases ---

#[test]
fn empty_imports() {
    let ctx = make_ctx(&["src/main.rs"]);
    let results = resolve_imports(&[], "src/main.rs", "rs", &ctx);
    assert!(results.is_empty());
}

#[test]
fn unsupported_language() {
    let ctx = make_ctx(&["main.rb"]);
    let imp = make_import("some_module");
    let results = resolve_imports(&[imp], "main.rb", "rb", &ctx);
    assert!(results[0].is_external);
}

#[test]
fn c_include_resolves_from_include_dir() {
    let ctx = make_ctx(&["include/foo/bar.h", "src/main.c"]);
    let imp = make_import("foo/bar.h");
    let results = resolve_imports(&[imp], "src/main.c", "c", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("include/foo/bar.h")
    );
    assert!(!results[0].is_external);
}

#[test]
fn ruby_require_relative_resolves() {
    let ctx = make_ctx(&["lib/utils.rb", "app.rb"]);
    let imp = make_import("./lib/utils");
    let results = resolve_imports(&[imp], "app.rb", "rb", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("lib/utils.rb"));
    assert!(!results[0].is_external);
}

#[test]
fn php_require_resolves() {
    let ctx = make_ctx(&["vendor/autoload.php", "index.php"]);
    let imp = make_import("./vendor/autoload.php");
    let results = resolve_imports(&[imp], "index.php", "php", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("vendor/autoload.php")
    );
    assert!(!results[0].is_external);
}

#[test]
fn bash_source_resolves() {
    let ctx = make_ctx(&["scripts/env.sh", "main.sh"]);
    let imp = make_import("./scripts/env.sh");
    let results = resolve_imports(&[imp], "main.sh", "sh", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("scripts/env.sh"));
    assert!(!results[0].is_external);
}

#[test]
fn dart_package_import_resolves_to_lib() {
    let mut ctx = make_ctx(&["lib/src/util.dart", "lib/app.dart"]);
    ctx.dart_package = Some("myapp".to_string());
    let imp = make_import("package:myapp/src/util.dart");
    let results = resolve_imports(&[imp], "lib/app.dart", "dart", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("lib/src/util.dart")
    );
    assert!(!results[0].is_external);
}

#[test]
fn kotlin_import_resolves_to_src_main_kotlin() {
    let ctx = make_ctx(&[
        "src/main/kotlin/com/example/service/UserService.kt",
        "src/main/kotlin/com/example/App.kt",
    ]);
    let imp = make_import("com.example.service.UserService");
    let results = resolve_imports(&[imp], "src/main/kotlin/com/example/App.kt", "kt", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/main/kotlin/com/example/service/UserService.kt")
    );
    assert!(!results[0].is_external);
}

#[test]
fn kotlin_stdlib_import_is_external() {
    let ctx = make_ctx(&["src/main/kotlin/App.kt"]);
    let imp = make_import("kotlin.collections.List");
    let results = resolve_imports(&[imp], "src/main/kotlin/App.kt", "kt", &ctx);
    assert!(results[0].is_external);
}

#[test]
fn kotlin_import_resolves_java_file() {
    let ctx = make_ctx(&[
        "src/main/java/com/example/LegacyUtil.java",
        "src/main/kotlin/com/example/App.kt",
    ]);
    let imp = make_import("com.example.LegacyUtil");
    let results = resolve_imports(&[imp], "src/main/kotlin/com/example/App.kt", "kt", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/main/java/com/example/LegacyUtil.java")
    );
    assert!(!results[0].is_external);
}

// --- C# ---

#[test]
fn csharp_using_resolves_namespace_folder_with_root_prefix() {
    // `using App.Services` must resolve even though files live under `src/`
    // (root-prefix tolerant suffix match), and even though the namespace maps to
    // a folder containing several files (a representative is returned).
    let ctx = make_ctx(&[
        "src/App/Services/UserService.cs",
        "src/App/Services/OrderService.cs",
        "src/App/Program.cs",
    ]);
    let imp = make_import("App.Services");
    let results = resolve_imports(&[imp], "src/App/Program.cs", "cs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("src/App/Services/OrderService.cs"),
        "smallest path is the deterministic representative"
    );
    assert!(!results[0].is_external);
}

#[test]
fn csharp_using_type_falls_back_to_parent_namespace() {
    // `using App.Services.UserService` references a *type*; the folder is the
    // parent namespace `App/Services`.
    let ctx = make_ctx(&["App/Services/UserService.cs", "App/Program.cs"]);
    let imp = make_import("App.Services.UserService");
    let results = resolve_imports(&[imp], "App/Program.cs", "cs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("App/Services/UserService.cs")
    );
    assert!(!results[0].is_external);
}

#[test]
fn csharp_bcl_and_nuget_namespaces_are_external() {
    let ctx = make_ctx(&["App/Program.cs"]);
    for ns in [
        "System.Text",
        "System.Collections.Generic",
        "Microsoft.Extensions.DependencyInjection",
        "Newtonsoft.Json",
    ] {
        let results = resolve_imports(&[make_import(ns)], "App/Program.cs", "cs", &ctx);
        assert!(results[0].is_external, "{ns} should be external");
        assert!(results[0].resolved_path.is_none());
    }
}

#[test]
fn csharp_unknown_internal_namespace_is_external_without_phantom_edge() {
    let ctx = make_ctx(&["App/Program.cs"]);
    let imp = make_import("Some.Other.Project");
    let results = resolve_imports(&[imp], "App/Program.cs", "cs", &ctx);
    assert!(results[0].is_external);
    assert!(results[0].resolved_path.is_none());
}

#[test]
fn csharp_using_drops_root_namespace_not_mirrored_as_folder() {
    // The RootNamespace (`MyApp`) is the assembly default namespace, NOT a folder:
    // sources live directly in `Models/` and `Services/`. `using MyApp.Models`
    // must still resolve by dropping the leading (non-folder) root segment.
    let ctx = make_ctx(&["Services/Greeter.cs", "Models/User.cs", "Models/Order.cs"]);
    let imp = make_import("MyApp.Models");
    let results = resolve_imports(&[imp], "Services/Greeter.cs", "cs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("Models/Order.cs"),
        "drops non-folder root `MyApp`, matches `Models/` (smallest file)"
    );
    assert!(!results[0].is_external);
}

#[test]
fn csharp_using_drops_multi_segment_root_namespace() {
    // A multi-segment default namespace (`Acme.MyApp`) is likewise not a folder.
    let ctx = make_ctx(&["Models/User.cs", "Program.cs"]);
    let imp = make_import("Acme.MyApp.Models");
    let results = resolve_imports(&[imp], "Program.cs", "cs", &ctx);
    assert_eq!(results[0].resolved_path.as_deref(), Some("Models/User.cs"));
    assert!(!results[0].is_external);
}

#[test]
fn csharp_bcl_tail_colliding_with_local_folder_stays_external() {
    // Hardening: a local `Text/` folder must NOT capture `using System.Text`.
    // The external-root check runs before any suffix probing.
    let ctx = make_ctx(&["Text/Formatter.cs", "Program.cs"]);
    let imp = make_import("System.Text");
    let results = resolve_imports(&[imp], "Program.cs", "cs", &ctx);
    assert!(
        results[0].is_external,
        "System.Text must not match a local Text/ folder"
    );
    assert!(results[0].resolved_path.is_none());
}

#[test]
fn csharp_longest_namespace_suffix_wins() {
    // When both a nested and a shallow folder match, the most specific
    // (longest) suffix is chosen.
    let ctx = make_ctx(&["Api/Models/Dto.cs", "Models/User.cs", "Program.cs"]);
    let imp = make_import("MyApp.Api.Models");
    let results = resolve_imports(&[imp], "Program.cs", "cs", &ctx);
    assert_eq!(
        results[0].resolved_path.as_deref(),
        Some("Api/Models/Dto.cs"),
        "`Api/Models` (len 2) beats the shallow `Models` (len 1)"
    );
    assert!(!results[0].is_external);
}
