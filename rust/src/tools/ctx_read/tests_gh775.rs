//! GitHub Issue #775 regression tests extracted from tests.rs (#660 LOC gate, frozen limit).
//! After a full-file read is cached, a subsequent ranged read with `lines:N-M`
//! must return only the requested window — not the full file content again.
use super::*;

/// Helper: create a test file with `n` numbered lines ("line 1\nline 2\n…").
fn write_numbered_file(dir: &std::path::Path, name: &str, n: usize) -> String {
    let path = dir.join(name);
    let content: String = (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &content).unwrap();
    path.to_string_lossy().to_string()
}

#[test]
fn gh775_full_then_ranged_returns_only_window() {
    let _iso = crate::core::data_dir::isolated_data_dir();
    crate::test_env::set_var("LEAN_CTX_SHOW_SAVINGS", "0");

    let dir = tempfile::tempdir().unwrap();
    let p = write_numbered_file(dir.path(), "big.ts", 2000);

    let mut cache = SessionCache::new();

    // 1. Full read — delivers all 2000 lines, marks as fully delivered.
    let full = handle_with_task_resolved(&mut cache, &p, "full", CrpMode::Off, None);
    assert!(
        full.content.contains("line 1"),
        "full read must include first line"
    );
    assert!(
        full.content.contains("line 2000"),
        "full read must include last line"
    );

    // 2. Ranged read — must return ONLY lines 1480–1489.
    let ranged =
        handle_fresh_with_task_resolved(&mut cache, &p, "lines:1480-1489", CrpMode::Off, None);
    assert!(
        ranged.content.contains("line 1480"),
        "ranged read must contain the first requested line:\n{}",
        &ranged.content[..ranged.content.len().min(300)]
    );
    assert!(
        ranged.content.contains("line 1489"),
        "ranged read must contain the last requested line"
    );
    assert!(
        !ranged.content.contains("line 1\n") && !ranged.content.contains("line 2000"),
        "ranged read must NOT contain lines outside the window:\n{}",
        &ranged.content[..ranged.content.len().min(500)]
    );

    crate::test_env::remove_var("LEAN_CTX_SHOW_SAVINGS");
}

#[test]
fn gh775_full_then_ranged_with_fresh_returns_only_window() {
    let _iso = crate::core::data_dir::isolated_data_dir();
    crate::test_env::set_var("LEAN_CTX_SHOW_SAVINGS", "0");

    let dir = tempfile::tempdir().unwrap();
    let p = write_numbered_file(dir.path(), "big2.ts", 2000);

    let mut cache = SessionCache::new();

    // 1. Full read.
    handle_with_task_resolved(&mut cache, &p, "full", CrpMode::Off, None);

    // 2. Fresh ranged read (simulates `fresh:true` in the tool args).
    let ranged =
        handle_fresh_with_task_resolved(&mut cache, &p, "lines:1480-1489", CrpMode::Off, None);
    assert!(
        ranged.content.contains("line 1480"),
        "fresh ranged read must contain requested start line"
    );
    assert!(
        ranged.content.contains("line 1489"),
        "fresh ranged read must contain requested end line"
    );
    assert!(
        !ranged.content.contains("line 1\n") && !ranged.content.contains("line 2000"),
        "fresh ranged read must NOT contain lines outside the window"
    );

    crate::test_env::remove_var("LEAN_CTX_SHOW_SAVINGS");
}

#[test]
fn gh775_ranged_response_starts_at_requested_line() {
    let _iso = crate::core::data_dir::isolated_data_dir();
    crate::test_env::set_var("LEAN_CTX_SHOW_SAVINGS", "0");

    let dir = tempfile::tempdir().unwrap();
    let p = write_numbered_file(dir.path(), "big3.ts", 2000);

    let mut cache = SessionCache::new();

    // Full read to warm cache.
    handle_with_task_resolved(&mut cache, &p, "full", CrpMode::Off, None);

    // Ranged read.
    let ranged =
        handle_fresh_with_task_resolved(&mut cache, &p, "lines:500-504", CrpMode::Off, None);

    // The first numbered output line must be line 500.
    // extract_line_range formats as " 500| line 500".
    let body_lines: Vec<&str> = ranged
        .content
        .lines()
        .filter(|l| l.contains("| line "))
        .collect();
    assert!(
        !body_lines.is_empty(),
        "ranged read must contain numbered output lines"
    );
    assert!(
        body_lines[0].contains("500| line 500"),
        "first body line must be line 500, got: {}",
        body_lines[0]
    );
    assert_eq!(
        body_lines.len(),
        5,
        "lines:500-504 must return exactly 5 lines, got {}",
        body_lines.len()
    );

    crate::test_env::remove_var("LEAN_CTX_SHOW_SAVINGS");
}

#[test]
fn gh775_cold_ranged_read_returns_only_window() {
    let _iso = crate::core::data_dir::isolated_data_dir();
    crate::test_env::set_var("LEAN_CTX_SHOW_SAVINGS", "0");

    let dir = tempfile::tempdir().unwrap();
    let p = write_numbered_file(dir.path(), "cold.ts", 2000);

    let mut cache = SessionCache::new();

    // No prior full read — cold ranged read must still return only the window.
    let ranged = handle_with_task_resolved(&mut cache, &p, "lines:100-109", CrpMode::Off, None);
    assert!(
        ranged.content.contains("line 100"),
        "cold ranged read must contain start line"
    );
    assert!(
        ranged.content.contains("line 109"),
        "cold ranged read must contain end line"
    );
    assert!(
        !ranged.content.contains("line 2000"),
        "cold ranged read must NOT contain lines outside window"
    );

    crate::test_env::remove_var("LEAN_CTX_SHOW_SAVINGS");
}
