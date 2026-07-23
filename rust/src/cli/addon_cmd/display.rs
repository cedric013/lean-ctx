use super::{AddonManifest, Path};

pub(super) fn print_install_preview(manifest: &AddonManifest) {
    let mcp = &manifest.mcp;
    println!(
        "  trust:     {}",
        crate::core::addons::TrustTier::of(manifest).label()
    );
    println!("  transport: {}", mcp.transport.as_str());
    match mcp.transport {
        crate::core::mcp_catalog::TransportKind::Stdio => {
            println!("  command:   {}", mcp.command);
            if !mcp.args.is_empty() {
                println!("  args:      {}", mcp.args.join(" "));
            }
            if !mcp.env.is_empty() {
                let keys: Vec<&str> = mcp.env.keys().map(String::as_str).collect();
                println!("  env:       {}", keys.join(", "));
            }
            if !mcp.sha256.trim().is_empty() {
                println!("  binary:    sha256-pinned");
            }
            if let Some(asset) = manifest.artifact_for_current_platform() {
                println!(
                    "  artifact:  {} → managed bin dir (sha256-pinned, never PATH)",
                    asset.filename
                );
            }
        }
        crate::core::mcp_catalog::TransportKind::Http => {
            println!("  url:       {}", mcp.url);
            if !mcp.headers.is_empty() {
                let keys: Vec<&str> = mcp.headers.keys().map(String::as_str).collect();
                println!("  headers:   {}", keys.join(", "));
            }
        }
    }
    print_bootstrap(manifest);
    print_capabilities(manifest);
    print_security_review(manifest);
}

/// Disclose the bootstrap install a `[install]` block performs on `add` (#1105):
/// the exact, shell-free package-manager commands the user is consenting to.
pub(super) fn print_bootstrap(manifest: &AddonManifest) {
    let install = &manifest.install;
    if !install.is_declared() {
        return;
    }
    // A managed artifact for this platform supersedes the bootstrap (GH #725) —
    // say so instead of describing an install that will not run.
    if manifest.artifact_for_current_platform().is_some() {
        println!(
            "\n  Install on add: skipped — the prebuilt artifact above is used \
             instead of `{}`.",
            install.manager.trim()
        );
        return;
    }
    let prog = install
        .manager()
        .map_or_else(|| install.manager.trim().to_string(), |m| m.as_str().into());
    println!("\n  Install on add — runs a pinned package manager before first use:");
    println!("    manager:   {}", install.manager.trim());
    println!(
        "    package:   {} (pinned {})",
        install.package.trim(),
        install.version.trim()
    );
    println!("    install:   {prog} {}", install.install_argv().join(" "));
    println!(
        "    uninstall: {prog} {}   (run on `addon remove`)",
        install.uninstall_argv().join(" ")
    );
    // Pre-flight: tell the user up front whether the manager is even present, so
    // a missing toolchain is visible before they consent rather than mid-install.
    if let Some(m) = install.manager() {
        if m.is_available() {
            println!("    requires:  `{prog}` on PATH — ✓ found");
        } else {
            println!(
                "    requires:  `{prog}` on PATH — ✗ NOT found ({})",
                m.install_hint()
            );
        }
    }
}

/// Show the declared capabilities the user is about to grant (P1). A declared
/// `[capabilities]` block means the addon runs under a per-addon OS sandbox +
/// scrubbed environment derived from exactly these permissions; an addon with
/// no block runs under the legacy `addons.sandbox` mode.
pub(super) fn print_capabilities(manifest: &AddonManifest) {
    match &manifest.capabilities {
        Some(caps) => {
            println!(
                "\n  Capabilities — network/filesystem/env enforced (sandbox + scrub, \
                 inherited by children); exec declared + audited:"
            );
            for line in caps.summary() {
                println!("    • {line}");
            }
        }
        None => {
            if manifest.mcp.transport == crate::core::mcp_catalog::TransportKind::Stdio {
                println!(
                    "\n  Capabilities: none declared — governed by `addons.sandbox` \
                     (set a [capabilities] block for a per-addon sandbox)."
                );
            }
        }
    }
}

/// Static risk review shown before install — disclosure, not a verdict (the
/// install policy gate enforces; see [`crate::core::addons::policy`]). Sourced
/// from the full audit (#403) so wiring risk, capability-coherence and malware
/// heuristics all surface before the user consents.
pub(super) fn print_security_review(manifest: &AddonManifest) {
    let findings = crate::core::addons::audit::audit(manifest).findings;
    if findings.is_empty() {
        return;
    }
    println!("\n  Security review:");
    for f in &findings {
        println!(
            "    {} [{}] {}",
            f.level.glyph(),
            f.level.as_str(),
            f.message
        );
    }
}

pub(super) fn print_field(label: &str, value: &str) {
    if !value.trim().is_empty() {
        println!(
            "  {label}:{}{value}",
            " ".repeat(11usize.saturating_sub(label.len() + 1))
        );
    }
}

pub(super) fn looks_like_path(target: &str) -> bool {
    Path::new(target)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        || target.contains('/')
        || target.starts_with('.')
        || Path::new(target).is_file()
}

pub(super) fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > 88 {
        let cut: String = line.chars().take(87).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

pub(super) fn print_help() {
    eprintln!(
        "lean-ctx addon — community extensions (MCP servers) for lean-ctx\n\
         \n\
         USAGE:\n    \
             lean-ctx addon <action> [args]\n\
         \n\
         ACTIONS:\n    \
             list                 List installed addons + the registry\n    \
             init [name]          Scaffold a lean-ctx-addon.toml here\n                         \
                                  [--http] [--force]\n                         \
                                  [--command \"npx -y pkg@1.2.3\"]\n    \
             search [query]       Search the registry (empty = list all)\n    \
             categories           Browse the registry by category\n    \
             usage                Per-addon / per-tool call counters\n    \
             info <name|path>     Show an addon's details + MCP wiring\n    \
             add <name|path>      Install from the registry, a hosted pack\n                         \
                                  (<namespace>/<name>, ctxpkg.com) or a local\n                         \
                                  lean-ctx-addon.toml (asks for confirmation)\n    \
             update <name>        Update an addon from where it came (side-by-\n                         \
                                  side managed binary, health-gated, auto-prune)\n    \
             publish [manifest]   Build + sign the kind=addon pack and upload\n                         \
                                  it to ctxpkg.com --namespace <ns> [--check]\n    \
             remove <name>        Uninstall an addon\n    \
             revoke <name>        Block an addon from running (kill-switch)\n                         \
                                  [--reason \"…\"] [--version X]\n    \
             unrevoke <name>      Lift a revocation\n    \
             revocations          List active revocations\n    \
             verify               Re-check installed addons against their\n                         \
                                  pinned wiring (integrity lock)\n    \
             audit <name|path>    Run the publish/list gate: wiring risk +\n                         \
                                  capability coherence + malware heuristics\n    \
             registry validate [path]\n                         \
                                  Validate a registry file (or the installed\n                         \
                                  registry) against the security + quality bar\n    \
             help                 Show this help\n\
         \n\
         FLAGS:\n    \
             -y, --yes            Skip the confirmation prompt (scripts/CI)\n    \
             --no-verify          add: skip the post-install MCP health probe\n    \
             --force, -f          add: install despite an under-declared\n                         \
                                  capability warning (init: overwrite)\n\
         \n\
         BUILD YOUR OWN ADDON:\n    \
             1. Expose your tool as an MCP server (stdio binary or HTTP endpoint).\n    \
             2. Add a lean-ctx-addon.toml to your repo:\n\
         \n        \
                 [addon]\n        \
                 name = \"my-addon\"            # slug: [a-z0-9-]\n        \
                 display_name = \"My Addon\"\n        \
                 description = \"What it does, in one line.\"\n        \
                 author = \"you\"\n        \
                 homepage = \"https://github.com/you/my-addon\"\n        \
                 license = \"Apache-2.0\"\n        \
                 categories = [\"workflow\"]\n        \
                 keywords = [\"...\"]\n\
         \n        \
                 [mcp]\n        \
                 transport = \"stdio\"          # or \"http\"\n        \
                 command = \"my-addon-mcp\"     # stdio: executable to spawn\n        \
                 args = [\"serve\"]\n        \
                 # sha256 = \"<shasum -a 256>\"  # stdio: pin the binary (P3)\n        \
                 # url = \"https://...\"         # http: streamable endpoint\n\
         \n        \
                 [capabilities]               # secure-by-default; widen only what you need\n        \
                 network = \"none\"             # \"full\" to reach the internet\n        \
                 filesystem = \"read_only\"     # \"read_write\" to write outside tmp\n        \
                 exec = \"none\"                # or [\"lean-ctx\"] if you spawn subprocesses\n\
         \n    \
             3. Test it live:  lean-ctx addon add ./lean-ctx-addon.toml\n    \
             4. Publish:       lean-ctx addon publish --namespace <your-handle>\n                      \
                               — self-service via ctxpkg.com; users install with\n                      \
                               `lean-ctx addon add <your-handle>/my-addon`.\n                      \
                               (Curated default catalog: MR against\n                      \
                               rust/data/addon_registry.json, docs/guides/addons.md.)\n\
         \n    \
             Full guide: docs/guides/addons.md"
    );
}
