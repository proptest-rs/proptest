//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! A reusable, `StateMachineTest`-agnostic store for proptest regression cases.
//!
//! This is the persistence machinery behind
//! [`prop_state_machine_persisted`](crate::prop_state_machine_persisted),
//! factored out so it can be driven on its own — e.g. by a project with a
//! richer case type than [`PersistedCase`](super::PersistedCase). It
//! accumulates distinct serde-serializable cases as JSON Lines, collapses the
//! within-run shrink chain to a single minimal case, and captures on panic.
//! Nothing here refers to `StateMachineTest` or `ReferenceStateMachine`; the
//! case type is any `C: Serialize + DeserializeOwned`.
//!
//! Used like:
//!
//! ```rust,ignore
//! let path = store::default_path(&store::slug(std::any::type_name::<MyTest>()));
//! store::reset_run_marker(&path);                 // once per run, before generation
//! for case in store::load::<MyCase>(&path)? { /* replay */ }
//! let guard = store::CaptureGuard::arm(path, &my_case);
//! // … run the case; on panic the guard merges `my_case` into the set …
//! guard.disarm();                                 // call on success
//! ```
//!
//! This layer is `pub` so it can be reused; upstream may choose to narrow its
//! visibility — it is not part of the `prop_state_machine_persisted!` contract.

use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Environment variable naming the directory under which regression files are
/// written. Defaults to `proptest-regressions/state-machine`.
pub const PERSIST_DIR_ENV: &str = "PROPTEST_STATE_MACHINE_PERSIST_DIR";

/// Turn a Rust type path into a filesystem-safe slug
/// (`foo::Bar<baz::Qux>` -> `foo__Bar_baz__Qux_`).
pub fn slug(type_name: &str) -> String {
    type_name
        .chars()
        .map(|c| match c {
            ':' => '_',
            c if c.is_alphanumeric() || c == '_' || c == '-' => c,
            _ => '_',
        })
        .collect()
}

/// Resolve the regression file for `slug`. Uses [`PERSIST_DIR_ENV`] if set, else
/// `proptest-regressions/state-machine`, with a `.jsonl` extension.
pub fn default_path(slug: &str) -> PathBuf {
    let dir = env::var_os(PERSIST_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("proptest-regressions").join("state-machine"));
    dir.join(format!("{slug}.jsonl"))
}

/// Header written at the top of a regression file (comment lines, ignored on
/// read) so the format is self-documenting in source control.
const FILE_HEADER: &str = "\
# proptest-state-machine regressions — one JSON case per line (JSON Lines).
# These cases are replayed before any generated cases on every run. Delete a
# line (or the whole file) to stop replaying that case. It is recommended to
# commit this file so everyone running the test benefits from the saved cases.
";

/// Read the case lines of a regression file as JSON values paired with their
/// 1-based line number, skipping blank and `#`-comment lines. Missing file →
/// empty. A malformed case line is a hard error (a corrupt regression file
/// should be surfaced, not silently dropped).
fn read_case_lines(path: &Path) -> std::io::Result<Vec<(usize, Value)>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    text.lines()
        .enumerate()
        .map(|(ix, line)| (ix + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .map(|(no, line)| {
            serde_json::from_str(line)
                .map(|value| (no, value))
                .map_err(|e| corrupt(path, no, &e.to_string()))
        })
        .collect()
}

/// Error for a regression line that cannot be turned back into a case. Names
/// the line and how to get rid of it, because the file is committed and every
/// run of the test hits this until someone acts on it.
fn corrupt(path: &Path, line_no: usize, cause: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "{}:{line_no}: {cause}\n\
             This regression no longer matches the current model. Delete line \
             {line_no} to stop replaying it.\n\
             (A state holding a non-finite float is stored as `null` and reads \
             back as a type error.)",
            path.display()
        ),
    )
}

fn read_case_values(path: &Path) -> std::io::Result<Vec<Value>> {
    Ok(read_case_lines(path)?
        .into_iter()
        .map(|(_, value)| value)
        .collect())
}

/// Write a regression set to `path` as JSON Lines (header + one compact case per
/// line), creating parent directories as needed. The file is replaced by a
/// rename, so a concurrent reader never observes a half-written set.
fn write_case_values(path: &Path, set: &[Value]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::from(FILE_HEADER);
    for case in set {
        out.push_str(
            &serde_json::to_string(case)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
        );
        out.push('\n');
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp.{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, out)?;
    fs::rename(&tmp, path)
}

/// Load the regression set at `path` as typed cases `C`. Empty if the file is
/// missing; a malformed line is a hard error.
pub fn load<C: DeserializeOwned>(path: &Path) -> std::io::Result<Vec<C>> {
    read_case_lines(path)?
        .into_iter()
        .map(|(no, value)| {
            serde_json::from_value(value)
                .map_err(|e| corrupt(path, no, &e.to_string()))
        })
        .collect()
}

thread_local! {
    /// Per-(test, thread) marker of the case this run last wrote, so successive
    /// shrink writes replace it rather than accumulate. Cleared at the start of
    /// each run by [`reset_run_marker`]. Keyed by the persistence file path.
    static RUN_MARKER: RefCell<HashMap<PathBuf, Value>> =
        RefCell::new(HashMap::new());
}

/// Clear this run's "last written case" marker for `path`. Call once per run,
/// before replaying/generating, so the next failure starts a fresh accumulation
/// rather than replacing a previous run's case.
pub fn reset_run_marker(path: &Path) {
    RUN_MARKER.with(|m| {
        m.borrow_mut().remove(path);
    });
}

/// Merge a newly observed minimal case into a regression set.
///
/// `prev_this_run` is the case this run last contributed (if any): it is removed
/// first, so the within-run shrink chain collapses to a single entry. `new` is
/// then appended unless an equal case is already present. Pure; the I/O wrapper
/// is [`record_minimal`].
fn merge_minimal(
    mut set: Vec<Value>,
    prev_this_run: Option<&Value>,
    new: Value,
) -> Vec<Value> {
    if let Some(prev) = prev_this_run {
        if let Some(ix) = set.iter().position(|c| c == prev) {
            set.remove(ix);
        }
    }
    if !set.iter().any(|c| *c == new) {
        set.push(new);
    }
    set
}

/// Serializes the read-merge-write cycle of [`record_minimal`]. Several test
/// functions can share one regression file, and libtest runs them on parallel
/// threads.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Load the set at `path`, merge `case` (this run's current minimal) into it,
/// and write it back. Tracks this run's previous contribution in [`RUN_MARKER`]
/// so repeated shrink writes collapse to one entry.
fn record_minimal(path: &Path, case: Value) {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let set = match read_case_values(path) {
        Ok(set) => set,
        Err(e) => {
            eprintln!(
                "[state-machine persistence] could not read existing regressions \
                 at {} ({e}); not overwriting",
                path.display()
            );
            return;
        }
    };
    let merged = RUN_MARKER.with(|m| {
        let mut markers = m.borrow_mut();
        let prev = markers.get(path).cloned();
        let merged = merge_minimal(set, prev.as_ref(), case.clone());
        markers.insert(path.to_path_buf(), case);
        merged
    });
    match write_case_values(path, &merged) {
        Ok(()) => eprintln!(
            "[state-machine persistence] persisted regression set now holds \
             {} case(s) at {}",
            merged.len(),
            path.display()
        ),
        Err(e) => eprintln!(
            "[state-machine persistence] failed to write {}: {e}",
            path.display()
        ),
    }
}

/// RAII guard that, if the current test panics while it is alive, merges the
/// case it holds into the regression set at `path`. Because proptest re-runs the
/// test body for every candidate during shrinking — ending with the minimal
/// failing case — successive writes within a run collapse to that minimal,
/// while distinct failures from earlier runs are preserved.
///
/// The case is serialized eagerly when the guard is armed (cheap relative to
/// running a transition sequence), so the unwind path carries no `Serialize`
/// bound. On a passing case [`CaptureGuard::disarm`] is called and nothing is
/// written.
pub struct CaptureGuard {
    path: PathBuf,
    case: Option<Value>,
}

impl CaptureGuard {
    /// Arm a guard that merges `case` into the regression set at `path` if the
    /// test unwinds before [`disarm`](Self::disarm) is called.
    pub fn arm<C: Serialize>(path: PathBuf, case: &C) -> Self {
        let value = serde_json::to_value(case).unwrap_or_else(|e| {
            panic!(
                "state-machine case cannot be serialized, so it could never \
                 be persisted to {}: {e}",
                path.display()
            )
        });
        Self {
            path,
            case: Some(value),
        }
    }

    /// Mark the case as passing: the guard will not write on drop.
    pub fn disarm(mut self) {
        self.case = None;
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let Some(case) = self.case.take() else {
            return;
        };
        record_minimal(&self.path, case);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Simulate a run's shrink chain: successive writes (prev -> new) collapse to
    /// the run's single minimal, while a distinct second run accumulates and an
    /// exact-duplicate third run adds nothing.
    #[test]
    fn merge_collapses_within_run_and_accumulates_across_runs() {
        let a_big = json!({ "transitions": ["A", "A", "A"] });
        let a_min = json!({ "transitions": ["A"] });
        let b_min = json!({ "transitions": ["B"] });

        // Run 1: shrink A_big -> A_min (prev removed, not accumulated).
        let mut set = merge_minimal(Vec::new(), None, a_big.clone());
        set = merge_minimal(set, Some(&a_big), a_min.clone());
        assert_eq!(set, vec![a_min.clone()]);

        // Run 2 (fresh marker): a distinct failure B is appended.
        set = merge_minimal(set, None, b_min.clone());
        assert_eq!(set, vec![a_min.clone(), b_min.clone()]);

        // Run 3 (fresh marker): re-discovering A_min must not duplicate it.
        set = merge_minimal(set, None, a_min.clone());
        assert_eq!(set, vec![a_min, b_min]);
    }
}
