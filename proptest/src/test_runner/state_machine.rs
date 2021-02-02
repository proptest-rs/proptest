//-
// Copyright 2021 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Test declaration helpers and runners for abstract state machine testing.

use crate::std_facade::Vec;
use crate::strategy::state_machine::AbstractStateMachine;

/// TODO
pub trait StateMachineTest {
    /// TODO
    type ConcreteState: Clone;
    /// TODO
    type Abstract: AbstractStateMachine;

    /// TODO
    fn init_test() -> Self::ConcreteState;
    /// TODO
    fn invariants(state: &Self::ConcreteState);
    /// TODO
    fn apply_transition(
        state: Self::ConcreteState,
        transition: &<Self::Abstract as AbstractStateMachine>::Transition,
    ) -> Self::ConcreteState;

    /// TODO
    fn test_sequential(
        transitions: Vec<<Self::Abstract as AbstractStateMachine>::Transition>,
    ) {
        let mut state = Self::init_test();
        for transition in transitions.iter() {
            Self::invariants(&state);
            state = Self::apply_transition(state, transition);
        }
        // also check the invariants after all the transitions are applied
        Self::invariants(&state);
    }
}

/// TODO
#[macro_export]
macro_rules! prop_state_machine {
    (#![proptest_config($config:expr)]
    $(
        $(#[$meta:meta])*
        fn $test_name:ident(sequential $test:ident $size:expr)
    )*) => {
        $(
            proptest! {
                #![proptest_config($config)]
                $(#[$meta])*
                fn $test_name(
                    transitions in <$test as StateMachineTest>::Abstract::sequential_strategy($size)
                ) {
                    $test::test_sequential(transitions)
                }
            }
        )*
    };
}
