//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Persist the *shrunk transition sequence* of a failing state-machine test,
//! and replay it on every run alongside freshly generated cases.
//!
//! Proptest's built-in failure persistence is seed-based: it stores the RNG
//! seed of the failing run. Replaying a seed regenerates the case from the
//! strategy and re-runs the shrinking process to reach the minimal counter-
//! example, and it only reproduces the original failure while the reference
//! model and transition strategy are unchanged — change either and the seed
//! maps to a different case.
//!
//! For a state-machine test the failing case is itself a concrete, human-
//! readable value — `(initial_state, Vec<Transition>)` — that, unlike a generic
//! proptest `Value`, can usually implement [`serde::Serialize`]. This module
//! persists that value directly. Replaying it needs no regeneration and no
//! re-shrinking, and it keeps reproducing the same scenario across strategy
//! changes (as long as the transition type can still be deserialized).
//!
//! # Model
//!
//! This mirrors how proptest treats seed regressions: the persisted case is
//! **replayed on every run**, and freshly generated cases run on top to keep
//! discovering new failures. Concretely, [`crate::prop_state_machine_persisted`]
//! expands to a test that first calls
//! [`StateMachineTest::replay_persisted_regressions`] and then runs the normal
//! generation loop, capturing and persisting any new shrunk failure.
//!
//! On failure the shrunk case is written under [`store::PERSIST_DIR_ENV`]
//! (default `proptest-regressions/state-machine`). Set `PROPTEST_CASES=0` to
//! replay the persisted regressions without generating anything new.
//!
//! # Accumulating distinct regressions
//!
//! The persisted file holds a *set* of cases as JSON Lines (one case per line,
//! under a `#` comment header), like proptest's line-oriented seed-regression
//! file — so it diffs and merges cleanly in source control and distinct failures
//! accumulate instead of overwriting each other:
//!
//! - Within one failing run, proptest re-runs the body for every shrink
//!   candidate. Those are shrunk versions of the *same* failure, so they collapse
//!   to the single minimal case (each write replaces this run's previous one).
//! - Across runs, a newly discovered minimal case is appended. Because every run
//!   replays the whole set *before* generating, a new case can only appear once
//!   all known regressions pass — i.e. it is a genuinely distinct failure, not a
//!   re-shrink of one already stored. Exact duplicates are de-duplicated.
//!
//! Delete a line (or the whole file) to stop replaying that case.
//!
//! # Large or not-directly-serializable states
//!
//! Persistence requires the reference `State` (and `Transition`) to implement
//! [`serde::Serialize`] + [`serde::de::DeserializeOwned`]. If your `State` is
//! large, derived, or holds values that don't serialize as-is, you don't have
//! to serialize it verbatim: implement serde for it in terms of a small,
//! reconstructible payload with `#[serde(into = "…", from = "…")]`. Only that
//! payload is stored, and `From<Payload>` rebuilds the state on replay:
//!
//! ```rust,ignore
//! #[derive(Clone, Serialize, Deserialize)]
//! #[serde(into = "InitSeed", from = "InitSeed")]
//! struct State { /* … large / not directly serializable … */ }
//!
//! #[derive(Serialize, Deserialize)]
//! struct InitSeed { /* just what's needed to reconstruct the initial state */ }
//!
//! impl From<State> for InitSeed { /* capture */ }
//! impl From<InitSeed> for State { /* rebuild */ }
//! ```
//!
//! `Transition` is handled the same way. This keeps persistence a single,
//! uniform `serde` requirement rather than a bespoke per-type mechanism.
//!
//! # Reusing the store
//!
//! The on-disk machinery — JSON Lines I/O, the accumulate/de-dup merge, the
//! per-run marker, and the panic [`CaptureGuard`](store::CaptureGuard) — lives
//! in the [`store`] submodule and is generic over any
//! `Serialize + DeserializeOwned` case type. It does not depend on
//! `StateMachineTest`, so a project with a richer fixture type can drive it
//! directly while keeping its own harness. The items here ([`PersistedCase`],
//! [`load_set`], [`default_persist_path`]) are the thin
//! `StateMachineTest`-specific layer on top.
//!
//! [`StateMachineTest::replay_persisted_regressions`]:
//!     crate::test_runner::StateMachineTest::replay_persisted_regressions

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

/// Resolve the default persistence file for a test type `T` (typically the
/// `StateMachineTest` impl), with a filename derived from its fully-qualified
/// name. See [`store::default_path`] / [`store::PERSIST_DIR_ENV`].
pub fn default_persist_path<T: ?Sized>() -> PathBuf {
    store::default_path(&store::slug(std::any::type_name::<T>()))
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
