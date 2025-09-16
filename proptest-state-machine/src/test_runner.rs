//-
// Copyright 2023 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Test declaration helpers and runners for abstract state machine testing.

use std::sync::atomic::{self, AtomicUsize};
use std::sync::Arc;

use crate::strategy::ReferenceStateMachine;
use proptest::test_runner::Config;

/// State machine test that relies on a reference state machine model
pub trait StateMachineTest {
    /// The concrete state, that is the system under test (SUT).
    type SystemUnderTest;

    /// The abstract state machine that implements [`ReferenceStateMachine`]
    /// drives the generation of the state machine's transitions.
    type Reference: ReferenceStateMachine;

    /// Initialize the state of SUT.
    ///
    /// If the reference state machine is generated from a non-constant
    /// strategy, ensure to use it to initialize the SUT to a corresponding
    /// state.
    fn init_test(
        ref_state: &<Self::Reference as ReferenceStateMachine>::State,
    ) -> Self::SystemUnderTest;

    /// Apply a transition in the SUT state and check post-conditions.
    /// The post-conditions are properties of your state machine that you want
    /// to assert.
    ///
    /// Note that the `ref_state` is the state *after* this `transition` is
    /// applied. You can use it to compare it with your SUT after you apply
    /// the transition.
    fn apply(
        state: Self::SystemUnderTest,
        ref_state: &<Self::Reference as ReferenceStateMachine>::State,
        transition: <Self::Reference as ReferenceStateMachine>::Transition,
    ) -> Self::SystemUnderTest;

    /// Check some invariant on the SUT state after every transition.
    ///
    /// Note that just like in [`StateMachineTest::apply`] you can use
    /// the `ref_state` to compare it with your SUT.
    fn check_invariants(
        state: &Self::SystemUnderTest,
        ref_state: &<Self::Reference as ReferenceStateMachine>::State,
    ) {
        // This is to avoid `unused_variables` warning
        let _ = (state, ref_state);
    }

    /// Override this function to add some teardown logic on the SUT state
    /// at the end of each test case. The default implementation simply drops
    /// the state.
    fn teardown(
        state: Self::SystemUnderTest,
        ref_state: <Self::Reference as ReferenceStateMachine>::State,
    ) {
        // This is to avoid `unused_variables` warning
        let _ = state;
        let _ = ref_state;
    }

    /// Run the test sequentially. You typically don't need to override this
    /// method.
    fn test_sequential(
        config: Config,
        mut ref_state: <Self::Reference as ReferenceStateMachine>::State,
        transitions: Vec<
            <Self::Reference as ReferenceStateMachine>::Transition,
        >,
        mut seen_counter: Option<Arc<AtomicUsize>>,
    ) {
        #[cfg(feature = "std")]
        use proptest::test_runner::INFO_LOG;

        let trans_len = transitions.len();
        #[cfg(feature = "std")]
        if config.verbose >= INFO_LOG {
            eprintln!();
            eprintln!("Running a test case with {} transitions.", trans_len);
        }
        #[cfg(not(feature = "std"))]
        let _ = (config, trans_len);

        let mut concrete_state = Self::init_test(&ref_state);

        // Check the invariants on the initial state
        Self::check_invariants(&concrete_state, &ref_state);

        for (ix, transition) in transitions.into_iter().enumerate() {
            // The counter is `Some` only before shrinking. When it's `Some` it
            // must be incremented before every transition that's being applied
            // to inform the strategy that the transition has been applied for
            // the first step of its shrinking process which removes any unseen
            // transitions.
            if let Some(seen_counter) = seen_counter.as_mut() {
                seen_counter.fetch_add(1, atomic::Ordering::SeqCst);
            }

            #[cfg(feature = "std")]
            if config.verbose >= INFO_LOG {
                eprintln!();
                eprintln!(
                    "Applying transition {}/{}: {:?}",
                    ix + 1,
                    trans_len,
                    transition
                );
            }
            #[cfg(not(feature = "std"))]
            let _ = ix;

            // Apply the transition on the states
            ref_state = <Self::Reference as ReferenceStateMachine>::apply(
                ref_state,
                &transition,
            );
            concrete_state =
                Self::apply(concrete_state, &ref_state, transition);

            // Check the invariants after the transition is applied
            Self::check_invariants(&concrete_state, &ref_state);
        }

        Self::teardown(concrete_state, ref_state)
    }
}

/// This macro helps to turn a state machine test implementation into a runnable
/// test. The macro expects a function header whose arguments follow a special
/// syntax rules: First, we declare if we want to apply the state machine
/// transitions sequentially or concurrently (currently, only the `sequential`
/// is supported). Next, we give a range of how many transitions to generate,
/// followed by `=>` and finally, an identifier that must implement
/// `StateMachineTest`.
///
/// ## Example
///
/// ```rust,ignore
/// struct MyTest;
///
/// impl StateMachineTest for MyTest {}
///
/// prop_state_machine! {
///     #[test]
///     fn run_with_macro(sequential 1..20 => MyTest);
/// }
/// ```
///
/// This example will expand to:
///
/// ```rust,ignore
/// struct MyTest;
///
/// impl StateMachineTest for MyTest {}
///
/// proptest! {
///     #[test]
///     fn run_with_macro(
///         (initial_state, transitions) in MyTest::sequential_strategy(1..20)
///     ) {
///        MyTest::test_sequential(initial_state, transitions)
///     }
/// }
/// ```
#[macro_export]
macro_rules! prop_state_machine {
    // With proptest config annotation
    (#![proptest_config($config:expr)]
    $(
        $(#[$meta:meta])*
        fn $test_name:ident(sequential $size:expr => $test:ident $(< $( $ty_param:tt ),+ >)?);
    )*) => {
        $(
            ::proptest::proptest! {
                #![proptest_config($config)]
                $(#[$meta])*
                fn $test_name(
                    (initial_state, transitions, seen_counter) in <<$test $(< $( $ty_param ),+ >)? as $crate::StateMachineTest>::Reference as $crate::ReferenceStateMachine>::sequential_strategy($size)
                ) {

                    let config = $config.__sugar_to_owned();
                    <$test $(::< $( $ty_param ),+ >)? as $crate::StateMachineTest>::test_sequential(config, initial_state, transitions, seen_counter)
                }
            }
        )*
    };

    // Without proptest config annotation
    ($(
        $(#[$meta:meta])*
        fn $test_name:ident(sequential $size:expr => $test:ident $(< $( $ty_param:tt ),+ >)?);
    )*) => {
        $(
            ::proptest::proptest! {
                $(#[$meta])*
                fn $test_name(
                    (initial_state, transitions, seen_counter) in <<$test $(< $( $ty_param ),+ >)? as $crate::StateMachineTest>::Reference as $crate::ReferenceStateMachine>::sequential_strategy($size)
                ) {
                    <$test $(::< $( $ty_param ),+ >)? as $crate::StateMachineTest>::test_sequential(
                        ::proptest::test_runner::Config::default(), initial_state, transitions, seen_counter)
                }
            }
        )*
    };
}

#[cfg(test)]
mod tests {

    mod macro_test {
        //! tests to verify that invocations of all forms of the
        //! `prop_state_machine!` macro compile cleanly, and hygenically,
        //!  as intended.

        /// Note: no imports here, so as to guarantee hygienic macros

        /// A no-op test. Exists strictly as something to reference
        /// in the macro invocation.
        struct Test;
        impl crate::ReferenceStateMachine for Test {
            type State = ();
            type Transition = ();

            fn init_state() -> proptest::strategy::BoxedStrategy<Self::State> {
                use proptest::prelude::*;
                Just(()).boxed()
            }

            fn transitions(
                _: &Self::State,
            ) -> proptest::strategy::BoxedStrategy<Self::Transition>
            {
                use proptest::prelude::*;
                Just(()).boxed()
            }

            fn apply(_: Self::State, _: &Self::Transition) -> Self::State {
                ()
            }
        }

        impl crate::StateMachineTest for Test {
            type SystemUnderTest = ();

            type Reference = Self;

            fn init_test(
                _: &<Self::Reference as crate::ReferenceStateMachine>::State,
            ) -> Self::SystemUnderTest {
            }

            fn apply(
                _: Self::SystemUnderTest,
                _: &<Self::Reference as crate::ReferenceStateMachine>::State,
                _: <Self::Reference as crate::ReferenceStateMachine>::Transition,
            ) -> Self::SystemUnderTest {
            }
        }

        // Invocation of the `prop_state_machine` macro without
        // a `![proptest_config]` annotation
        prop_state_machine! {
            #[test]
            fn no_config_annotation(sequential 1..2 => Test);
        }

        // Invocation of the `prop_state_machine` macro with a
        // `![proptest_config]` annotation
        prop_state_machine! {
            #![proptest_config(::proptest::test_runner::Config::default())]

            #[test]
            fn with_config_annotation(sequential 1..2 => Test);
        }
    }
}
