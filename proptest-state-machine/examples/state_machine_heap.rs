//-
// Copyright 2023 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! In this example, we demonstrate using the state machine testing approach
//! for a heap implementation that has a bug in it. The heap `MyHeap` is in the
//! `system_under_test` module inlined at the bottom of this file.

#[macro_use]
extern crate proptest_state_machine;

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
use system_under_test::MyHeap;

// Setup the state machine test using the `prop_state_machine!` macro
prop_state_machine! {
    #![proptest_config(Config {
        // Turn failure persistence off for demonstration. This means that no
        // regression file will be captured.
        failure_persistence: None,
        // Enable verbose mode to make the state machine test print the
        // transitions for each case.
        verbose: 1,
        .. Config::default()
    })]

    // NOTE: The `#[test]` attribute is commented out in here so we can run it
    // as an example from the `fn main`.

    // #[test]
    fn run_my_heap_test(
        // This is a macro's keyword - only `sequential` is currently supported.
        sequential
        // The number of transitions to be generated for each case. This can
        // be a single numerical value or a range as in here.
        1..20
        // Macro's boilerplate to separate the following identifier.
        =>
        // The name of the type that implements `StateMachineTest`.
        MyHeap<i32>
    );
}

fn main() {
    run_my_heap_test();
}

/// An empty type used for the `ReferenceStateMachine` implementation. The
/// actual state of it represented by `Vec<i32>`, but it doesn't have to
/// contained inside this type.
pub struct HeapStateMachine;

/// The possible transitions of the state machine.
#[derive(Clone, Debug)]
pub enum Transition {
    Pop,
    Push(i32),
}

// Implementation of the reference state machine that drives the test. That is,
// it's used to generate a sequence of transitions the `StateMachineTest`.
impl ReferenceStateMachine for HeapStateMachine {
    type State = Vec<i32>;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(vec![]).boxed()
    }

    fn transitions(_state: &Self::State) -> BoxedStrategy<Self::Transition> {
        // Using the regular proptest constructs here, the transitions can be
        // given different weights.
        prop_oneof![
            1 => Just(Transition::Pop),
            2 => (any::<i32>()).prop_map(Transition::Push),
        ]
        .boxed()
    }

    fn apply(
        mut state: Self::State,
        transition: &Self::Transition,
    ) -> Self::State {
        match transition {
            Transition::Pop => {
                state.pop();
            }
            Transition::Push(value) => state.push(*value),
        }
        state
    }
}

impl StateMachineTest for MyHeap<i32> {
    type SystemUnderTest = Self;
    type Reference = HeapStateMachine;

    fn init_test(
        _ref_state: &<Self::Reference as ReferenceStateMachine>::State,
    ) -> Self::SystemUnderTest {
        MyHeap::new()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state: &<Self::Reference as ReferenceStateMachine>::State,
        transition: Transition,
    ) -> Self::SystemUnderTest {
        match transition {
            Transition::Pop => {
                // We read the state before applying the transition.
                let was_empty = state.is_empty();

                // We use the broken implementation of pop, which should be
                // discovered by the test.
                let result = state.pop_wrong();

                // NOTE: To fix the issue that gets found by the state machine,
                // you can comment out the last statement with `pop_wrong` and
                // uncomment this one to see the test pass:
                // let result = state.pop();

                // Check a post-condition.
                match result {
                    Some(value) => {
                        assert!(!was_empty);
                        // The heap must not contain any value which was
                        // greater than the "maximum" we were just given.
                        for in_heap in state.iter() {
                            assert!(
                                value >= *in_heap,
                                "Popped value {:?}, which was less \
                                    than {:?} still in the heap",
                                value,
                                in_heap
                            );
                        }
                    }
                    None => assert!(was_empty),
                }
            }
            Transition::Push(value) => state.push(value),
        }
        state
    }

    fn check_invariants(
        state: &Self::SystemUnderTest,
        _ref_state: &<Self::Reference as ReferenceStateMachine>::State,
    ) {
        // Check that the heap's API gives consistent results
        assert_eq!(0 == state.len(), state.is_empty());
    }
}

/// A hand-rolled implementation of a binary heap, like
/// <https://doc.rust-lang.org/stable/std/collections/struct.BinaryHeap.html>,
/// except slow and buggy.
mod system_under_test {
    use std::cmp;

    #[derive(Clone, Debug)]
    pub struct MyHeap<T> {
        data: Vec<T>,
    }

    impl<T: cmp::Ord> MyHeap<T> {
        pub fn new() -> Self {
            MyHeap { data: vec![] }
        }

        pub fn is_empty(&self) -> bool {
            self.data.is_empty()
        }

        pub fn len(&self) -> usize {
            self.data.len()
        }

        pub fn iter(&self) -> impl Iterator<Item = &T> {
            self.data.iter()
        }

        pub fn push(&mut self, value: T) {
            self.data.push(value);
            let mut index = self.data.len() - 1;
            while index > 0 {
                let parent = (index - 1) / 2;
                if self.data[parent] < self.data[index] {
                    self.data.swap(index, parent);
                    index = parent;
                } else {
                    break;
                }
            }
        }

        // This implementation is wrong, because it doesn't preserve ordering
        pub fn pop_wrong(&mut self) -> Option<T> {
            if self.is_empty() {
                None
            } else {
                Some(self.data.swap_remove(0))
            }
        }

        // Fixed implementation of pop()
        #[allow(dead_code)]
        pub fn pop(&mut self) -> Option<T> {
            if self.is_empty() {
                return None;
            }

            let ret = self.data.swap_remove(0);

            // Restore the heap property
            let mut index = 0;
            loop {
                let child1 = index * 2 + 1;
                let child2 = index * 2 + 2;
                if child1 >= self.data.len() {
                    break;
                }

                let child = if child2 == self.data.len()
                    || self.data[child1] > self.data[child2]
                {
                    child1
                } else {
                    child2
                };

                if self.data[index] < self.data[child] {
                    self.data.swap(child, index);
                    index = child;
                } else {
                    break;
                }
            }

            Some(ret)
        }
    }
}
