//! Delta-explicit (#498) tests extracted from tests.rs (#660 LOC gate, frozen limit).
use super::tests::primed_full_cache;
use super::*;
use std::time::Duration;

#[test]
fn delta_explicit_changed_file_diverts_full_reread_to_diff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changed.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let mut cache = primed_full_cache(&p);

    // File changes on disk after the first full read.
    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn main() { changed(); }\n").unwrap();

    let decision = resolve_explicit_delta_mode(
        &cache, &p, "full", /*explicit*/ true, /*fresh*/ false, true,
    );
    assert_eq!(
        decision.mode, "diff",
        "changed full re-read must divert to diff"
    );
    let note = decision
        .note
        .expect("a diff diversion must carry an advisory note");
    assert!(
        note.contains("[delta-explicit]"),
        "note tag missing: {note}"
    );
    assert!(
        note.contains("fresh=true"),
        "note must mention the bypass: {note}"
    );

    // End-to-end: the engine renders the diff against the FULL cached content.
    let out = handle_with_task_resolved(&mut cache, &p, "diff", CrpMode::Off, None);
    assert_eq!(out.resolved_mode, "diff");
    assert!(
        out.content.contains("[diff]"),
        "engine must emit a diff: {}",
        out.content
    );
    assert!(
        out.content.contains("changed()"),
        "diff must reflect the new on-disk content: {}",
        out.content
    );
}

#[test]
fn delta_explicit_changed_lines_request_diverts_to_diff() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lines.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn a() {}\nfn b() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn a() { x(); }\nfn b() {}\n").unwrap();

    let decision = resolve_explicit_delta_mode(&cache, &p, "lines:1-1", true, false, true);
    assert_eq!(
        decision.mode, "diff",
        "a changed-file lines: re-read must divert to diff, not re-extract a window"
    );
    assert!(decision.note.is_some());
}

#[test]
fn delta_explicit_diff_base_is_full_cached_content_not_compressed() {
    // Fix #2 guard: the diff base must be the full source the cache stored, even
    // when the most recent read of the file was a COMPRESSED view (map). If the
    // base were the compressed view, the diff would be garbage.
    let _iso = crate::core::data_dir::isolated_data_dir();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.rs");
    let p = path.to_string_lossy().to_string();
    let mut content = String::new();
    for i in 0..60 {
        content.push_str(&format!(
            "pub fn original_fn_{i}(x: i32) -> i32 {{ x + {i} }}\n"
        ));
    }
    std::fs::write(&path, &content).unwrap();

    let mut cache = SessionCache::new();
    // Cache the full content, then read a compressed (map) view — last_mode=map,
    // but the entry still stores the full source.
    let _ = handle_with_task_resolved(&mut cache, &p, "full", CrpMode::Off, None);
    let _ = handle_with_task_resolved(&mut cache, &p, "map", CrpMode::Off, None);

    // Change exactly one line on disk.
    std::thread::sleep(Duration::from_secs(1));
    let changed = content.replace(
        "pub fn original_fn_7(x: i32) -> i32 { x + 7 }",
        "pub fn original_fn_7(x: i32) -> i32 { x + 70707 }",
    );
    std::fs::write(&path, &changed).unwrap();

    let out = handle_with_task_resolved(&mut cache, &p, "diff", CrpMode::Off, None);
    assert!(
        out.content.contains("[diff]"),
        "expected a diff: {}",
        out.content
    );
    // The marker appears only if the diff compared against the FULL original
    // source (a compressed map base would never contain this literal).
    assert!(
        out.content.contains("70707"),
        "diff must be computed against full cached source, got: {}",
        out.content
    );
    // And it must be a one-line edit, not a wholesale replacement of a
    // compressed base against the full file.
    assert!(
        out.content.contains("+1/-1") || out.content.contains("-1/+1"),
        "single-line change should diff as +1/-1: {}",
        out.content
    );
}

#[test]
fn delta_explicit_unchanged_lines_collapse_to_full_stub() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn a() {}\nfn b() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    // No disk change. A lines: re-read of a fully-delivered file re-emits text
    // the model holds → collapse to the full-mode stub (no diff, no note).
    let decision = resolve_explicit_delta_mode(&cache, &p, "lines:1-1", true, false, true);
    assert_eq!(
        decision.mode, "full",
        "unchanged lines: of a full file must collapse to the stub"
    );
    assert!(
        decision.note.is_none(),
        "a silent stub collapse must not carry a note"
    );
}

#[test]
fn delta_explicit_unchanged_full_reread_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("same.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn a() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    // An unchanged full re-read already hits the downstream `[unchanged]` stub;
    // the resolver leaves it untouched.
    let decision = resolve_explicit_delta_mode(&cache, &p, "full", true, false, true);
    assert_eq!(decision.mode, "full");
    assert!(decision.note.is_none());
}

#[test]
fn delta_explicit_off_preserves_current_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changed.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn main() { changed(); }\n").unwrap();

    // enabled=false → the mode is never rewritten, no matter the disk state.
    let decision =
        resolve_explicit_delta_mode(&cache, &p, "full", true, false, /*enabled*/ false);
    assert_eq!(
        decision.mode, "full",
        "feature OFF must preserve the requested mode"
    );
    assert!(decision.note.is_none());

    let lines = resolve_explicit_delta_mode(&cache, &p, "lines:1-1", true, false, false);
    assert_eq!(
        lines.mode, "lines:1-1",
        "feature OFF must not touch lines: either"
    );
    assert!(lines.note.is_none());
}

#[test]
fn delta_explicit_fresh_bypasses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changed.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn main() { changed(); }\n").unwrap();

    // fresh=true → always bypass even with the feature on and a changed file.
    let decision = resolve_explicit_delta_mode(&cache, &p, "full", true, /*fresh*/ true, true);
    assert_eq!(
        decision.mode, "full",
        "fresh=true must bypass the diff diversion"
    );
    assert!(decision.note.is_none());
}

#[test]
fn delta_explicit_first_read_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    // Nothing cached yet — the very first read can never be a diff.
    let cache = SessionCache::new();
    let decision = resolve_explicit_delta_mode(&cache, &p, "full", true, false, true);
    assert_eq!(
        decision.mode, "full",
        "an uncached first read must be served normally"
    );
    assert!(decision.note.is_none());

    let lines = resolve_explicit_delta_mode(&cache, &p, "lines:1-1", true, false, true);
    assert_eq!(lines.mode, "lines:1-1");
    assert!(lines.note.is_none());
}

#[test]
fn delta_explicit_only_fires_for_explicit_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changed.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let cache = primed_full_cache(&p);

    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn main() { changed(); }\n").unwrap();

    // explicit_mode=false (mode was auto-resolved) → never diverted; auto-mode
    // already has its own staleness handling.
    let decision =
        resolve_explicit_delta_mode(&cache, &p, "full", /*explicit*/ false, false, true);
    assert_eq!(
        decision.mode, "full",
        "auto-resolved modes must not be diverted to diff"
    );
    assert!(decision.note.is_none());
}

#[test]
fn delta_explicit_decision_is_byte_stable() {
    // #498 determinism: the resolver's note carries no timestamp/counter, so
    // repeated calls on the same changed-file state are byte-identical.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("changed.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let cache = primed_full_cache(&p);
    std::thread::sleep(Duration::from_secs(1));
    std::fs::write(&path, "fn main() { changed(); }\n").unwrap();

    let d1 = resolve_explicit_delta_mode(&cache, &p, "full", true, false, true);
    let d2 = resolve_explicit_delta_mode(&cache, &p, "full", true, false, true);
    assert_eq!(
        d1, d2,
        "delta-explicit decision drifted between identical calls"
    );
}
