//! Deterministic recording builder for the committed testbench subset (#611).
//!
//! The `gen_testbench_recording` example and the in-tree CI gate must agree on the
//! exact replay keys, so the walk "lock → materialize → assemble both arms → enumerate
//! the answer + judge requests" lives here once, parameterised only by the canned text
//! a [`CannedModel`] supplies. The example provides the canned policy; the test simply
//! replays the file this produces.

use std::path::Path;

use anyhow::{Context, Result};

use super::super::build_request;
use super::super::conditions::{Condition, assemble};
use super::super::judge::LlmJudge;
use super::super::model::{ModelFingerprint, ModelResponse, Recording};
use super::super::suite::{Domain, EvalSuite, Task};
use super::clone;
use super::lockfile::TestbenchLock;

/// The two arms every task is recorded under — identical framing, different context.
const ARMS: [Condition; 2] = [Condition::Baseline, Condition::LeanCtx];

/// Supplies the canned model text the recording captures. Implemented by the example
/// with fixture-specific strings; kept as a trait so the builder carries no fixtures.
pub trait CannedModel {
    /// The answer the model returns for `task` under `condition` in repo `repo`.
    fn answer(&self, repo: &str, task: &Task, condition: Condition) -> String;
    /// The judge's reply for a QA `candidate` (only consulted for [`Domain::Qa`]).
    fn judge(&self, repo: &str, task: &Task, candidate: &str) -> String;
}

/// Builds a [`Recording`] covering every replay key a [`super::run_testbench`] over
/// `lock` will request (one answer per arm, plus one judge reply per distinct QA
/// candidate), drawing all text from `model`. Pure given the fixtures + policy.
pub fn build_recording(
    lock: &TestbenchLock,
    cache_dir: &Path,
    fingerprint: ModelFingerprint,
    budget_tokens: usize,
    model: &dyn CannedModel,
) -> Result<Recording> {
    let mut rec = Recording::new(fingerprint);
    for entry in &lock.repos {
        let repo_dir = clone::materialize(entry, lock.dir(), cache_dir)?;
        let suite_path = lock.dir().join(&entry.suite);
        let raw = std::fs::read_to_string(&suite_path)
            .with_context(|| format!("reading suite {}", suite_path.display()))?;
        let suite = EvalSuite::parse(&raw, repo_dir)
            .with_context(|| format!("parsing suite for repo {}", entry.name))?;

        for task in &suite.tasks {
            let ws = task.workspace_path(&suite.dir);
            for condition in ARMS {
                let ctx = assemble(condition, &ws, task.query(), budget_tokens)?;
                let answer = model.answer(&entry.name, task, condition);
                let req = build_request(&ctx.text, &task.prompt);
                rec.entries
                    .insert(req.key(), ModelResponse::new(answer.clone()));

                // QA answers are graded by the LLM judge, so the judge call for this
                // exact candidate must also be recorded (same runner, distinct key).
                if task.domain == Domain::Qa {
                    let jreq = LlmJudge::request(task, &answer);
                    let verdict = model.judge(&entry.name, task, &answer);
                    rec.entries.insert(jreq.key(), ModelResponse::new(verdict));
                }
            }
        }
    }
    Ok(rec)
}
