//! Pinned-repo lockfile for the public off-vs-on testbench (#611).
//!
//! A lockfile names the external repositories the testbench runs against and pins
//! each to an exact commit, so a public run is reproducible by anyone. Each entry is
//! either a **remote** repo (`url` + `commit`, cloned + checked out by
//! [`super::clone`]) or a **local** fixture (`path`, used by the committed
//! deterministic CI subset which must run offline). Every entry points at an NDJSON
//! [`super::super::suite`] file whose task `workspace`s resolve *inside* the repo.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::core::eval_ab::sha256_hex;

/// Lockfile schema discriminator.
pub const TESTBENCH_LOCK_KIND: &str = "lean-ctx.testbench-lock";

/// One pinned repository under test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEntry {
    /// Stable, unique label used in reports + the cache directory name.
    pub name: String,
    /// Git remote to clone (mutually exclusive with `path`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Exact commit to check out (required with `url`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Local fixture directory (relative to the lockfile), used instead of cloning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// NDJSON suite (relative to the lockfile) whose task workspaces resolve inside the repo.
    pub suite: String,
}

impl RepoEntry {
    /// True for a committed local fixture (no network), false for a remote clone.
    pub fn is_local(&self) -> bool {
        self.path.is_some()
    }

    fn validate(&self) -> std::result::Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("repo entry has an empty name".into());
        }
        if self.suite.trim().is_empty() {
            return Err(format!("repo {}: suite is empty", self.name));
        }
        match (&self.url, &self.commit, &self.path) {
            (Some(u), Some(c), None) => {
                if u.trim().is_empty() || c.trim().is_empty() {
                    return Err(format!(
                        "repo {}: url and commit must be non-empty",
                        self.name
                    ));
                }
                Ok(())
            }
            (None, None, Some(p)) => {
                if p.trim().is_empty() {
                    return Err(format!("repo {}: path is empty", self.name));
                }
                Ok(())
            }
            _ => Err(format!(
                "repo {}: set EITHER url+commit (remote) OR path (local fixture)",
                self.name
            )),
        }
    }
}

/// A parsed, validated lockfile plus the directory it was loaded from (the resolution
/// root for relative `path` / `suite` entries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestbenchLock {
    pub kind: String,
    pub repos: Vec<RepoEntry>,
    #[serde(skip)]
    dir: PathBuf,
}

impl TestbenchLock {
    /// Loads + validates a lockfile, recording its parent dir for path resolution.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading testbench lock {}", path.display()))?;
        let dir = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        Self::parse(&raw, dir)
    }

    /// Pure parser (testable without a file on disk).
    pub fn parse(raw: &str, dir: PathBuf) -> Result<Self> {
        let mut lock: TestbenchLock =
            serde_json::from_str(raw).context("parsing testbench lock JSON")?;
        if lock.kind != TESTBENCH_LOCK_KIND {
            bail!("not a {TESTBENCH_LOCK_KIND} file (kind = {:?})", lock.kind);
        }
        if lock.repos.is_empty() {
            bail!("testbench lock contains no repos");
        }
        let mut seen = HashSet::new();
        for repo in &lock.repos {
            if let Err(reason) = repo.validate() {
                bail!("invalid lock entry: {reason}");
            }
            if !seen.insert(repo.name.as_str()) {
                bail!("duplicate repo name: {}", repo.name);
            }
        }
        lock.dir = dir;
        Ok(lock)
    }

    /// Resolution root for relative `path` / `suite` entries.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Machine-independent digest of the pinned set (names, sources, commits, suites),
    /// embedded in the report so a third party can confirm *what* was run.
    pub fn digest(&self) -> String {
        // Serialize only the repos (not the local `dir`, which varies per machine).
        let bytes = serde_json::to_vec(&self.repos).unwrap_or_default();
        sha256_hex(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_lock() -> &'static str {
        r#"{"kind":"lean-ctx.testbench-lock","repos":[
          {"name":"qa","path":"repos/qa","suite":"qa.ndjson"},
          {"name":"code","path":"repos/code","suite":"code.ndjson"}
        ]}"#
    }

    #[test]
    fn parses_local_fixture_lock() {
        let lock = TestbenchLock::parse(local_lock(), PathBuf::from("/lock")).unwrap();
        assert_eq!(lock.repos.len(), 2);
        assert!(lock.repos[0].is_local());
        assert_eq!(lock.dir(), Path::new("/lock"));
    }

    #[test]
    fn parses_remote_entry() {
        let raw = r#"{"kind":"lean-ctx.testbench-lock","repos":[
          {"name":"r","url":"https://example.com/r.git","commit":"abc123","suite":"r.ndjson"}
        ]}"#;
        let lock = TestbenchLock::parse(raw, PathBuf::from(".")).unwrap();
        assert!(!lock.repos[0].is_local());
    }

    #[test]
    fn rejects_mixed_source() {
        let raw = r#"{"kind":"lean-ctx.testbench-lock","repos":[
          {"name":"r","url":"u","commit":"c","path":"p","suite":"s"}
        ]}"#;
        assert!(TestbenchLock::parse(raw, PathBuf::from(".")).is_err());
    }

    #[test]
    fn rejects_remote_without_commit() {
        let raw = r#"{"kind":"lean-ctx.testbench-lock","repos":[
          {"name":"r","url":"u","suite":"s"}
        ]}"#;
        assert!(TestbenchLock::parse(raw, PathBuf::from(".")).is_err());
    }

    #[test]
    fn rejects_duplicate_names() {
        let raw = r#"{"kind":"lean-ctx.testbench-lock","repos":[
          {"name":"r","path":"a","suite":"s"},
          {"name":"r","path":"b","suite":"s"}
        ]}"#;
        assert!(TestbenchLock::parse(raw, PathBuf::from(".")).is_err());
    }

    #[test]
    fn rejects_foreign_kind() {
        let raw = r#"{"kind":"nope","repos":[{"name":"r","path":"p","suite":"s"}]}"#;
        assert!(TestbenchLock::parse(raw, PathBuf::from(".")).is_err());
    }

    #[test]
    fn digest_is_stable_and_ignores_dir() {
        let a = TestbenchLock::parse(local_lock(), PathBuf::from("/one")).unwrap();
        let b = TestbenchLock::parse(local_lock(), PathBuf::from("/two")).unwrap();
        assert_eq!(a.digest(), b.digest());
    }
}
