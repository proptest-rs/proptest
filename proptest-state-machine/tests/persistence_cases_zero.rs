//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! `PROPTEST_CASES=0` replays the stored regressions and generates nothing.
//!
//! Its own binary: the persistence directory is selected by a process-global
//! environment variable, and this test owns it for the whole run.

#![cfg(feature = "persistence")]

use proptest::prelude::*;
use proptest::strategy::Just;
use proptest::test_runner::Config;
use proptest_state_machine::persistence::{default_persist_path, PERSIST_DIR_ENV};
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum Op {
    Inc,
}

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

struct BuggyCounter;

impl StateMachineTest for BuggyCounter {
    type SystemUnderTest = u32;
    type Reference = RefCounter;

    fn init_test(_ref_state: &RefCounter) -> u32 {
        0
    }

    fn apply(state: u32, _ref_state: &RefCounter, _transition: Op) -> u32 {
        (state + 1).min(3)
    }

    fn check_invariants(state: &u32, ref_state: &RefCounter) {
        assert_eq!(*state, ref_state.value, "SUT counter diverged from reference");
    }
}

/// Evaluated before the macro replays anything, so the regression it writes is
/// on disk by the time replay reads it.
fn seeded_config() -> Config {
    let dir = std::env::temp_dir().join(format!("psm-cases-zero-{}", std::process::id()));
    std::env::set_var(PERSIST_DIR_ENV, &dir);
    let path = default_persist_path::<BuggyCounter>();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "{\"initial_state\":{\"value\":0},\"transitions\":[\"Inc\",\"Inc\",\"Inc\",\"Inc\"]}\n",
    )
    .unwrap();
    Config {
        cases: 0,
        ..Config::default()
    }
}

proptest_state_machine::prop_state_machine_persisted! {
    #![proptest_config(seeded_config())]
    #[test]
    #[should_panic(expected = "SUT counter diverged from reference")]
    fn cases_zero_replays_the_regression(sequential 1..10 => BuggyCounter);
}
