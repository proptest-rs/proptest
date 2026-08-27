//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! End-to-end test for the `persistence` feature: a deliberately buggy state
//! machine must (1) persist its shrunk failing transition sequence on failure,
//! and (2) reproduce the failure when that file is replayed — without
//! regeneration or re-shrinking.

#![cfg(feature = "persistence")]

use std::panic;
use std::path::PathBuf;
use std::sync::Mutex;

/// The persistence env vars are process-global; serialize the tests that set
/// them so they don't race each other.
static ENV_LOCK: Mutex<()> = Mutex::new(());

use proptest::prelude::*;
use proptest::strategy::{Just, ValueTree};
use proptest::test_runner::{Config, TestError, TestRunner};
use proptest_state_machine::persistence::{
    default_persist_path, load_set, PersistedCase, PERSIST_DIR_ENV,
};
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};

use serde::{Deserialize, Serialize};

/// The only transition: increment the counter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Op {
    Inc,
}

/// Reference model: an honest counter.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RefCounter {
    value: u32,
}

impl ReferenceStateMachine for RefCounter {
    type State = RefCounter;
    type Transition = Op;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(RefCounter { value: 0 }).boxed()
    }

    fn transitions(_state: &Self::State) -> BoxedStrategy<Self::Transition> {
        Just(Op::Inc).boxed()
    }

    fn apply(mut state: Self::State, _transition: &Self::Transition) -> Self::State {
        state.value += 1;
        state
    }
}

/// SUT with a bug: the concrete counter saturates at 3, so once the reference
/// reaches 4 the post-condition `sut == ref` fails. The minimal failing case is
/// therefore exactly four `Inc`s.
struct BuggyCounter;

impl StateMachineTest for BuggyCounter {
    type SystemUnderTest = u32;
    type Reference = RefCounter;

    fn init_test(_ref_state: &RefCounter) -> u32 {
        0
    }

    fn apply(state: u32, _ref_state: &RefCounter, _transition: Op) -> u32 {
        // BUG: caps at 3 instead of counting forever.
        (state + 1).min(3)
    }

    fn check_invariants(state: &u32, ref_state: &RefCounter) {
        assert_eq!(
            *state, ref_state.value,
            "SUT counter diverged from reference"
        );
    }
}

fn quiet_panic<R>(f: impl FnOnce() -> R) -> R {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let r = f();
    panic::set_hook(prev);
    r
}

#[test]
fn persists_shrunk_case_and_replays_it() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Isolate persistence to a unique temp dir and keep proptest's own seed
    // regression file out of the picture.
    let tmp = std::env::temp_dir().join(format!(
        "psm-persistence-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // SAFETY: single test, no other test touches these vars; std edition 2021.
    std::env::set_var(PERSIST_DIR_ENV, &tmp);

    let config = Config {
        failure_persistence: None,
        cases: 256,
        ..Config::default()
    };
    let path: PathBuf = default_persist_path::<BuggyCounter>();

    // --- Phase 1: capture ---------------------------------------------------
    let result = quiet_panic(|| {
        let mut runner = TestRunner::new(config.clone());
        runner.run(
            &RefCounter::sequential_strategy(1..20usize),
            |(init, transitions, counter)| {
                BuggyCounter::test_sequential_persisted(
                    config.clone(),
                    init,
                    transitions,
                    counter,
                );
                Ok(())
            },
        )
    });
    assert!(
        matches!(result, Err(TestError::Fail(..))),
        "the buggy machine must fail, got {result:?}"
    );

    let set: Vec<PersistedCase<RefCounter, Op>> = load_set(&path)
        .unwrap_or_else(|e| panic!("expected persisted set at {path:?}: {e}"));
    assert_eq!(set.len(), 1, "one distinct failure → one persisted case");
    assert_eq!(
        set[0].transitions,
        vec![Op::Inc, Op::Inc, Op::Inc, Op::Inc],
        "shrunk case must be the minimal four increments"
    );
    assert_eq!(set[0].initial_state.value, 0);

    // On-disk format is JSON Lines: a `#` header plus exactly one case line.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.starts_with('#'), "file should begin with a comment header");
    let case_lines: Vec<&str> = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert_eq!(case_lines.len(), 1, "exactly one case line");
    serde_json::from_str::<serde_json::Value>(case_lines[0])
        .expect("each case line is standalone JSON");

    // --- Phase 2: replay ----------------------------------------------------
    // `replay_persisted_regressions` loads the file written above and re-runs
    // it; since the bug is still present it must reproduce the failure. This is
    // the call `prop_state_machine_persisted!` makes once per run.
    let replayed = quiet_panic(|| {
        panic::catch_unwind(panic::AssertUnwindSafe(|| {
            BuggyCounter::replay_persisted_regressions(config.clone())
        }))
    });
    std::env::remove_var(PERSIST_DIR_ENV);
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        replayed.is_err(),
        "replaying the stored failing case must reproduce the failure"
    );
}

/// A passing machine must not write a persistence file.
#[test]
fn passing_case_writes_nothing() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    struct GoodCounter;
    impl StateMachineTest for GoodCounter {
        type SystemUnderTest = u32;
        type Reference = RefCounter;
        fn init_test(_r: &RefCounter) -> u32 {
            0
        }
        fn apply(state: u32, _r: &RefCounter, _t: Op) -> u32 {
            state + 1
        }
        fn check_invariants(state: &u32, r: &RefCounter) {
            assert_eq!(*state, r.value);
        }
    }

    let tmp = std::env::temp_dir().join(format!(
        "psm-persistence-good-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var(PERSIST_DIR_ENV, &tmp);
    let path = default_persist_path::<GoodCounter>();

    // Drive a single case directly through the value tree (no proptest! macro).
    let mut runner = TestRunner::new(Config {
        failure_persistence: None,
        ..Config::default()
    });
    let tree = RefCounter::sequential_strategy(1..10usize)
        .new_tree(&mut runner)
        .unwrap();
    let (init, transitions, counter) = tree.current();
    GoodCounter::test_sequential_persisted(
        Config::default(),
        init,
        transitions,
        counter,
    );

    assert!(!path.exists(), "passing case must not persist a file");
    std::env::remove_var(PERSIST_DIR_ENV);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A correct machine driven through the `prop_state_machine_persisted!` macro:
/// it expands, the once-per-run regression replay no-ops (no persisted file),
/// generation runs, and the test passes. Exercises the macro end to end.
struct GoodMachine;
impl StateMachineTest for GoodMachine {
    type SystemUnderTest = u32;
    type Reference = RefCounter;
    fn init_test(_r: &RefCounter) -> u32 {
        0
    }
    fn apply(state: u32, _r: &RefCounter, _t: Op) -> u32 {
        state + 1
    }
    fn check_invariants(state: &u32, r: &RefCounter) {
        assert_eq!(*state, r.value);
    }
}

proptest_state_machine::prop_state_machine_persisted! {
    #[test]
    fn macro_drives_good_machine(sequential 1..10 => GoodMachine);
}

// --- Accumulation of distinct regressions -----------------------------------

use std::cell::Cell;

thread_local! {
    /// Selects which invariant `TwoBugs` violates, so a single test type can
    /// exhibit two *distinct* minimal failures across runs.
    static BUG_MODE: Cell<u8> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
enum Ab {
    A,
    B,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AbRef {
    a: u32,
    b: u32,
}

impl ReferenceStateMachine for AbRef {
    type State = AbRef;
    type Transition = Ab;
    fn init_state() -> BoxedStrategy<Self::State> {
        Just(AbRef { a: 0, b: 0 }).boxed()
    }
    fn transitions(_s: &Self::State) -> BoxedStrategy<Self::Transition> {
        prop_oneof![Just(Ab::A), Just(Ab::B)].boxed()
    }
    fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State {
        match transition {
            Ab::A => state.a += 1,
            Ab::B => state.b += 1,
        }
        state
    }
}

/// Bug 1 (mode 1): breaks after two `A`s → minimal `[A, A]`.
/// Bug 2 (mode 2): breaks after one `B` → minimal `[B]`.
struct TwoBugs;
impl StateMachineTest for TwoBugs {
    type SystemUnderTest = ();
    type Reference = AbRef;
    fn init_test(_r: &AbRef) {}
    fn apply(_s: (), _r: &AbRef, _t: Ab) {}
    fn check_invariants(_s: &(), r: &AbRef) {
        match BUG_MODE.with(Cell::get) {
            1 => assert!(r.a < 2, "mode 1: two A transitions"),
            2 => assert!(r.b < 1, "mode 2: a B transition"),
            _ => {}
        }
    }
}

/// Distinct failures (not shrunk versions of each other) accumulate into the
/// regression set across runs, while re-discovering one already stored does not
/// duplicate it.
#[test]
fn accumulates_distinct_regressions() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "psm-persistence-accum-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var(PERSIST_DIR_ENV, &tmp);
    let path = default_persist_path::<TwoBugs>();
    let config = Config {
        failure_persistence: None,
        cases: 512,
        ..Config::default()
    };

    // Each `run_once` is a fresh logical run: reset the accumulation marker,
    // then let proptest find and shrink one failure.
    let run_once = |mode: u8| {
        proptest_state_machine::persistence::reset_run_marker(&path);
        BUG_MODE.with(|m| m.set(mode));
        let _ = quiet_panic(|| {
            let mut runner = TestRunner::new(config.clone());
            runner.run(
                &AbRef::sequential_strategy(1..20usize),
                |(init, transitions, counter)| {
                    TwoBugs::test_sequential_persisted(
                        config.clone(),
                        init,
                        transitions,
                        counter,
                    );
                    Ok(())
                },
            )
        });
    };

    run_once(1);
    let set1: Vec<PersistedCase<AbRef, Ab>> = load_set(&path).unwrap();
    assert_eq!(set1.len(), 1, "first failure stored");
    assert_eq!(set1[0].transitions, vec![Ab::A, Ab::A]);

    run_once(2);
    let set2: Vec<PersistedCase<AbRef, Ab>> = load_set(&path).unwrap();
    let seqs: Vec<_> = set2.iter().map(|c| c.transitions.clone()).collect();
    assert_eq!(set2.len(), 2, "distinct second failure accumulated");
    assert!(seqs.contains(&vec![Ab::A, Ab::A]));
    assert!(seqs.contains(&vec![Ab::B]));

    // Re-discover bug 1: must not duplicate the existing [A, A] entry.
    run_once(1);
    let set3: Vec<PersistedCase<AbRef, Ab>> = load_set(&path).unwrap();
    assert_eq!(set3.len(), 2, "re-discovered failure must not duplicate");

    std::env::remove_var(PERSIST_DIR_ENV);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn corrupt_regression_line_names_itself() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!(
        "psm-persistence-corrupt-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::env::set_var(PERSIST_DIR_ENV, &tmp);
    let path = default_persist_path::<BuggyCounter>();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "# header\n{\"initial_state\":{\"value\":0},\"transitions\":[\"Inc\"]}\n\
         {\"initial_state\":{\"value\":0},\"transitions\":[\"Nope\"]}\n",
    )
    .unwrap();

    let err = load_set::<RefCounter, Op>(&path).unwrap_err().to_string();
    std::env::remove_var(PERSIST_DIR_ENV);
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(err.contains(":3:"), "error must name the offending line: {err}");
    assert!(err.contains("Delete line 3"), "error must say how to clear it: {err}");
}
