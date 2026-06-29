//! Regenerate the committed deterministic testbench recording (#611).
//!
//! Run:   `cargo run --example gen_testbench_recording --features dev-tools`
//! Check: `cargo run --example gen_testbench_recording --features dev-tools -- --check`
//!
//! The committed local-fixture subset (`rust/eval/testbench/`) is replayed in CI to
//! gate the off-vs-on pipeline. This writer captures the canned model answers + judge
//! verdicts for that subset so the replay covers every request key. Editing a fixture
//! suite, corpus, prompt, or the assembly/judge framing changes the request keys and
//! makes the recording stale; this brings it back in sync. `--check` fails (exit 1)
//! when the on-disk recording no longer matches, mirroring the in-tree gate test.

use std::path::{Path, PathBuf};

use lean_ctx::core::eval_ab::AbRunConfig;
use lean_ctx::core::eval_ab::conditions::Condition;
use lean_ctx::core::eval_ab::model::{ModelFingerprint, ModelParams, PROVIDER_RECORDED, Recording};
use lean_ctx::core::eval_ab::suite::{Domain, Task};
use lean_ctx::core::eval_ab::testbench::lockfile::TestbenchLock;
use lean_ctx::core::eval_ab::testbench::recording::{CannedModel, build_recording};

/// Correct, test-passing reference solution for the `code-fizz` fixture — handed to
/// BOTH arms so the committed subset is an honest tie (a deterministic *mechanism*
/// gate), never a rigged win. Real quality deltas come from a live `--record` run.
const FIZZBUZZ: &str = "fizzbuzz() {\n  n=$1\n  if [ $((n % 15)) -eq 0 ]; then echo FizzBuzz\n  elif [ $((n % 3)) -eq 0 ]; then echo Fizz\n  elif [ $((n % 5)) -eq 0 ]; then echo Buzz\n  else echo \"$n\"\n  fi\n}\n";

/// Correct free-form answer for the `qa-api` fixture (contains the gold facts).
const BACKOFF_ANSWER: &str = "The PaymentGateway retries failed charges with exponential backoff: a base delay of 200ms that doubles each attempt, up to a maximum of 5 attempts before the charge is marked permanently failed.";

/// Canned policy for the committed subset: correct answers for both arms, a PASS judge.
struct FixtureModel;

impl CannedModel for FixtureModel {
    fn answer(&self, _repo: &str, task: &Task, _condition: Condition) -> String {
        match task.domain {
            Domain::Qa => BACKOFF_ANSWER.to_string(),
            Domain::Code => FIZZBUZZ.to_string(),
        }
    }

    fn judge(&self, _repo: &str, _task: &Task, _candidate: &str) -> String {
        // The candidate carries the gold facts, so the grader passes it.
        "PASS\n1.0".to_string()
    }
}

fn testbench_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/testbench")
}

fn fingerprint() -> ModelFingerprint {
    ModelFingerprint {
        provider: PROVIDER_RECORDED.to_string(),
        endpoint: "testbench-fixture".to_string(),
        params: ModelParams {
            model: "fixture".to_string(),
            ..ModelParams::default()
        },
    }
}

fn main() {
    let check_only = std::env::args().any(|a| a == "--check");
    let dir = testbench_dir();
    let lock = TestbenchLock::load(&dir.join("testbench.lock.json")).unwrap_or_else(|e| {
        eprintln!("ERROR: load lock: {e:#}");
        std::process::exit(1);
    });

    let cache = std::env::temp_dir().join("lc-testbench-gen-cache");
    let budget = AbRunConfig::default().budget_tokens;
    let built = build_recording(&lock, &cache, fingerprint(), budget, &FixtureModel)
        .unwrap_or_else(|e| {
            eprintln!("ERROR: build recording: {e:#}");
            std::process::exit(1);
        });

    let out = dir.join("recording.json");

    if check_only {
        let on_disk = Recording::load(&out).unwrap_or_else(|e| {
            eprintln!(
                "ERROR: committed recording missing or invalid ({e:#}).\nRun: cargo run --example gen_testbench_recording --features dev-tools"
            );
            std::process::exit(1);
        });
        if on_disk != built {
            eprintln!(
                "Committed testbench recording is out of date: {}\n\nRun: cargo run --example gen_testbench_recording --features dev-tools",
                out.display()
            );
            std::process::exit(1);
        }
        println!("up to date: {}", out.display());
        return;
    }

    if let Err(e) = built.save(&out) {
        eprintln!("ERROR: write recording: {e:#}");
        std::process::exit(1);
    }
    println!("wrote {} ({} entries)", out.display(), built.entries.len());
}
