//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Persist the shrunk transition sequence of a failing state-machine test and
//! replay it on later runs.
//!
//! The failing case of a state-machine test is a concrete value —
//! `(initial_state, Vec<Transition>)` — so it can be stored as itself rather
//! than as the RNG seed proptest persists. Replaying it needs no regeneration
//! and no re-shrinking.
//!
//! [`crate::prop_state_machine_persisted`] expands to a test that replays every
//! stored case before generating new ones, and persists any new shrunk failure.
//! Cases accumulate as JSON Lines under [`store::PERSIST_DIR_ENV`] (default
//! `proptest-regressions/state-machine`); delete a line to stop replaying it.
//! `PROPTEST_CASES=0` replays the stored cases and generates nothing.
//!
//! # Large or not-directly-serializable states
//!
//! `State` and `Transition` must implement [`serde::Serialize`] +
//! [`serde::de::DeserializeOwned`]. A `State` that does not serialize as-is can
//! store a reconstructible payload instead, which is all that reaches the file:
//!
//! ```rust,ignore
//! #[derive(Clone, Serialize, Deserialize)]
//! #[serde(into = "InitSeed", from = "InitSeed")]
//! struct State { /* … */ }
//!
//! #[derive(Serialize, Deserialize)]
//! struct InitSeed { /* enough to rebuild the initial state */ }
//!
//! impl From<State> for InitSeed { /* capture */ }
//! impl From<InitSeed> for State { /* rebuild */ }
//! ```
//!
//! [`store`] holds the same machinery generic over any
//! `Serialize + DeserializeOwned` case type, for harnesses with a richer
//! fixture than [`PersistedCase`].

pub mod store;

use std::path::{Path, PathBuf};

use proptest::test_runner::Config;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use store::{reset_run_marker, PERSIST_DIR_ENV};

/// A failing state-machine case, captured as the values that reproduce it.
///
/// This is the on-disk case shape (one JSON object per line in the regression
/// file). `initial_state` and `transitions` are exactly what
/// [`StateMachineTest::test_sequential`] consumes, so replay is a direct call
/// with no strategy involvement.
///
/// [`StateMachineTest::test_sequential`]:
///     crate::test_runner::StateMachineTest::test_sequential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCase<S, T> {
    /// The reference model's initial state for the failing run.
    pub initial_state: S,
    /// The minimal (shrunk) transition sequence that still fails.
    pub transitions: Vec<T>,
}

/// Resolve the regression file for a test, from the crate's manifest directory,
/// the source file it is written in, and its name. Use
/// [`persist_path!`](crate::persist_path) rather than calling this directly.
pub fn persist_path(
    manifest_dir: &str,
    source_file: &str,
    test_name: &str,
) -> PathBuf {
    store::path_for(manifest_dir, source_file, test_name)
}

/// Load the persisted regression set at `path` as typed cases. Returns an empty
/// vec if the file does not exist. Used by replay.
pub fn load_set<S, T>(path: &Path) -> std::io::Result<Vec<PersistedCase<S, T>>>
where
    S: Serialize + DeserializeOwned,
    T: Serialize + DeserializeOwned,
{
    store::load(path)
}

/// A run's shrink chain collapses to one entry through a process-local marker,
/// so every case of a run must execute in the same process.
pub fn assert_same_process(config: &Config) {
    assert!(
        !config.fork(),
        "state-machine persistence supports neither `fork` nor `timeout`: each \
         case would run in its own process, so every failing shrink candidate \
         would be stored as a separate regression instead of collapsing to the \
         minimal one"
    );
}

/// Check that `transitions` are still legal under the current reference model,
/// walking them from `initial_state` exactly as the test will.
///
/// A persisted case outlives the model it was recorded against. One that still
/// deserializes but violates a precondition would otherwise be applied anyway
/// and report a failure the system under test never had.
pub fn assert_still_valid<R: crate::strategy::ReferenceStateMachine>(
    initial_state: &R::State,
    transitions: &[R::Transition],
    path: &Path,
) {
    let mut state = initial_state.clone();
    for (ix, transition) in transitions.iter().enumerate() {
        assert!(
            R::preconditions(&state, transition),
            "persisted regression in {} is no longer valid under the current \
             reference model: transition {} of {} ({:?}) fails its \
             precondition. Delete it to stop replaying it.",
            path.display(),
            ix + 1,
            transitions.len(),
            transition
        );
        state = R::apply(state, transition);
    }
}
