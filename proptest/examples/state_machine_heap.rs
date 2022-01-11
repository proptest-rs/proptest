//-
// Copyright 2021 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[macro_use]
extern crate proptest;

use std::cmp;

/// A hand-rolled implementation of a binary heap, like
/// https://doc.rust-lang.org/stable/std/collections/struct.BinaryHeap.html,
/// except slow and buggy.
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

use proptest::prelude::*;
use proptest::state_machine::{AbstractStateMachine, StateMachineTest};
use proptest::test_runner::Config;

#[derive(Clone, Debug)]
enum Transition {
    Pop,
    Push(i32),
}

struct HeapStateMachine;
impl AbstractStateMachine for HeapStateMachine {
    type State = Vec<i32>;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(vec![]).boxed()
    }

    fn transitions(_state: &Self::State) -> BoxedStrategy<Self::Transition> {
        // The element can be given different weights.
        prop_oneof![
            1 => Just(Transition::Pop),
            2 => (any::<i32>()).prop_map(Transition::Push),
        ]
        .boxed()
    }

    fn apply_abstract(
        mut state: Self::State,
        transition: &Self::Transition,
    ) -> Self::State {
        match transition {
            Transition::Pop => {
                state.pop();
                ()
            }
            Transition::Push(value) => state.push(*value),
        }
        state
    }
}

struct MyHeapTest;
impl StateMachineTest for MyHeapTest {
    type ConcreteState = MyHeap<i32>;
    type Abstract = HeapStateMachine;

    fn init_test(
        _initial_state: <Self::Abstract as AbstractStateMachine>::State,
    ) -> Self::ConcreteState {
        MyHeap::new()
    }

    fn apply_concrete(
        mut state: Self::ConcreteState,
        transition: Transition,
    ) -> Self::ConcreteState {
        match transition {
            Transition::Pop => {
                let was_empty = state.is_empty();
                // We use the broken implementation of pop, which should be
                // discovered by the test
                let result = state.pop_wrong();
                // A post-condition
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

    fn invariants(state: &Self::ConcreteState) {
        assert_eq!(0 == state.len(), state.is_empty());
    }
}

// Run the state machine test without the [`prop_state_machine`] macro
proptest! {
    #![proptest_config(Config {
        // Turn failure persistence off for demonstration
        failure_persistence: None,
        .. Config::default()
    })]
    // #[test]
    fn run_without_macro(
        (initial_state, transitions) in HeapStateMachine::sequential_strategy(1..20)
    ) {
        MyHeapTest::test_sequential(initial_state, transitions)
    }
}

// Run the state machine test using the [`prop_state_machine`] macro
prop_state_machine! {
    #![proptest_config(Config {
        // Turn failure persistence off for demonstration
        failure_persistence: None,
        .. Config::default()
    })]
    #[test]
    fn run_with_macro(sequential 1..20 => MyHeapTest);
}

fn main() {
    run_without_macro();
}
