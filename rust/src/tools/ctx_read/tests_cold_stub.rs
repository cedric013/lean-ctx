//! Persistent cold stub tests extracted from tests.rs (#660 LOC gate, frozen limit).
use super::tests::primed_full_cache;
use super::*;

// ---------------------------------------------------------------------------
// Persistent cold stub (#955): after a daemon restart / idle clear the live
// cache is empty, so an unchanged re-read must be served from the persisted
// index — but only for the SAME known conversation and an unchanged file. The
// record is forged directly (modelling one that outlived the restart) and the
// current conversation is injected, so the assertions are host-independent.
// ---------------------------------------------------------------------------

/// Primes a real full delivery to capture authentic (hash, mtime, line_count,
/// file_ref), then forges a persisted record under `conv`. Clears the global
/// index before priming (so the prime isn't short-circuited by a stale record)
/// and after (to drop the prime's own write-through) — leaving exactly the one
/// forged record.
fn seed_cold_record(p: &str, conv: &str) {
    crate::core::read_stub_index::clear_for_test();
    let primed = primed_full_cache(p);
    let entry = primed.get(p).unwrap();
    let rec = crate::core::read_stub_index::StubRecord::new(
        crate::core::pathutil::normalize_tool_path(p),
        entry.hash.clone(),
        entry.stored_mtime,
        entry.line_count,
        primed.get_file_ref_readonly(p).unwrap_or_default(),
        Some(conv.to_string()),
    );
    crate::core::read_stub_index::clear_for_test();
    crate::core::read_stub_index::record(rec);
}

#[test]
#[serial_test::serial(stub_index)]
fn cold_fallback_serves_stub_for_same_conversation_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warm.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() { let x = 1; }\n").unwrap();

    seed_cold_record(&p, "conv-a");
    // Empty cache models a fresh daemon: the warm path misses, cold fallback fires.
    let cold = SessionCache::new();
    let out = try_stub_hit_readonly_scoped(&cold, &p, Some("conv-a"));
    crate::core::read_stub_index::clear_for_test();
    assert!(
        out.is_some_and(|o| o.content.contains("[unchanged")),
        "same-conversation re-read after restart must serve the persisted stub"
    );
}

#[test]
#[serial_test::serial(stub_index)]
fn cold_fallback_withheld_for_other_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warm.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() { let x = 1; }\n").unwrap();

    seed_cold_record(&p, "conv-a");
    let cold = SessionCache::new();
    let out = try_stub_hit_readonly_scoped(&cold, &p, Some("conv-b"));
    crate::core::read_stub_index::clear_for_test();
    assert!(
        out.is_none(),
        "a different conversation must get a cold full read, never a persisted stub"
    );
}

#[test]
#[serial_test::serial(stub_index)]
fn cold_fallback_withheld_without_conversation_context() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warm.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() { let x = 1; }\n").unwrap();

    seed_cold_record(&p, "conv-a");
    let cold = SessionCache::new();
    // Unlike the WARM path, an absent conversation cannot prove the content is in
    // the new process's context → no cold stub (the stricter gate keeps #954's
    // cross-chat hazard closed across restarts).
    let out = try_stub_hit_readonly_scoped(&cold, &p, None);
    crate::core::read_stub_index::clear_for_test();
    assert!(
        out.is_none(),
        "absent conversation context must NOT serve a cold persisted stub"
    );
}

#[test]
#[serial_test::serial(stub_index)]
fn cold_fallback_withheld_when_file_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warm.rs");
    let p = path.to_string_lossy().to_string();
    std::fs::write(&path, "fn main() { let x = 1; }\n").unwrap();

    seed_cold_record(&p, "conv-a");
    // Content changed during downtime → mtime/md5 mismatch → no stub.
    std::fs::write(&path, "fn main() { let x = 2; let y = 3; }\n").unwrap();
    let cold = SessionCache::new();
    let out = try_stub_hit_readonly_scoped(&cold, &p, Some("conv-a"));
    crate::core::read_stub_index::clear_for_test();
    assert!(
        out.is_none(),
        "a file changed on disk must get a cold full read, never a stale stub"
    );
}
