//-
// Copyright 2018 Jason Lingle
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// The code here is adapted from
// https://hypothesis.works/articles/rule-based-stateful-testing/

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

impl<T : cmp::Ord> MyHeap<T> {
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
            if child1 >= self.data.len() { break; }

            let child = if child2 == self.data.len() ||
                self.data[child1] > self.data[child2]
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

    // NEW: Operation to merge another heap into this one
    pub fn merge(&mut self, other: &mut MyHeap<T>) {
        // THIS IS BROKEN
        // Walk the iterators from either side, taking whichever is greater at
        // each step.
        let mut new_data = vec![];
        {
            let mut l = self.data.drain(..).peekable();
            let mut r = other.data.drain(..).peekable();
            loop {
                if l.peek().is_some() {
                    if r.peek().is_some() && r.peek() > l.peek() {
                        new_data.push(r.next().unwrap());
                    } else {
                        new_data.push(l.next().unwrap());
                    }
                } else if let Some(rv) = r.next() {
                    new_data.push(rv);
                } else {
                    break;
                }
            }
        }

        self.data = new_data;
    }
}

mod test {
    use std::fmt;

    use super::*;
    use proptest::prelude::*;
    use proptest::state_machine::StateMachine;

    // These would normally be in `#[cfg(test)]` and have `#[test]`, but those
    // are omitted since the example runs without the rust test harness so they
    // are called by main directly.

    // It is possible to use `MyHeap<T>` directly as the state under test, but
    // we add this wrapper so we can use a "known correct" container to track
    // what's supposed to be in the heap.
    #[derive(Debug, Clone)]
    struct MyHeapState<T> {
        heap: MyHeap<T>,
        // Vec is pretty inefficient for the test here, but we don't mind the
        // test being slow in exchange for something "obviously correct" to
        // test against.
        existing_elements: Vec<T>,
    }

    // We define a strategy for our starting state. Here, we always start with
    // an empty heap.
    fn initial_heap_strategy<T : cmp::Ord + fmt::Debug + Clone>
        () -> impl Strategy<Value = MyHeapState<T>>
    {
        Just(MyHeapState {
            heap: MyHeap::new(),
            existing_elements: vec![],
        })
    }

    // Now for the meat of the test: our state machine!
    fn heap_state_machine<T>() -> StateMachine<MyHeapState<T>>
    where T : cmp::Ord + fmt::Debug + Arbitrary + Clone + 'static
    { prop_state_machine! {
        type = MyHeapState<T>;

        // First, we define some invariants. That is, properties that
        // should always hold at all points between mutation operations.
        invariant len_equals_number_of_elements(&state) {
            assert_eq!(state.existing_elements.len(), state.heap.len());
        }

        invariant is_empty_if_length_zero(&state) {
            assert_eq!(0 == state.heap.len(), state.heap.is_empty());
        }

        // There are more complex invariants that could be checked, such as
        // examining whether it still has the heap property. Here, we rely
        // on the fact that other properties will break if the internal
        // invariants are omitted, for the sake of being concise.

        // Now for our mutators. We only defined two mutation operations on
        // our heap so far.

        // push gets extra weight so that we tend to build larger heaps rather
        // than keeping tiny ones.
        #[weight = 2]
        fn push(&mut state, value: T) {
            // Add it to the heap...
            state.heap.push(value.clone());
            // ... and also the Vec where we track what the heap contains.
            state.existing_elements.push(value);
        }

        fn pop(&mut state) {
            let was_empty = state.heap.is_empty();

            match state.heap.pop() {
                // If None is returned, the heap must have been empty
                // before the operation.
                None => assert!(was_empty),

                Some(value) => {
                    assert!(!was_empty);

                    // The heap must not contain any value which was
                    // greater than the "maximum" we were just given.
                    for in_heap in state.heap.iter() {
                        assert!(value >= *in_heap,
                                "Popped value {:?}, which was less \
                                 than {:?} still in the heap",
                                value, in_heap);
                    }

                    // The value we popped must have been supposed to still
                    // be in the heap.
                    let matching_index = state.existing_elements.iter()
                        .enumerate()
                        .find(|&(_, existing)| value == *existing)
                        .map(|(ix, _)| ix);

                    if let Some(matching_index) = matching_index {
                        state.existing_elements.swap_remove(matching_index);
                    } else {
                        panic!("Popped value {:?} which shouldn't \
                                have been in the heap", value);
                    }
                },
            }
        }
    } }

    // Finally, the test to put it all together.
    proptest! {
        //#[test]
        fn test_state_machine(
            // Use the state machine definition to create a test case ...
            test_case in heap_state_machine::<i32>().test_case(
                // ... given the initial state ...
                initial_heap_strategy::<i32>(),
                // ... and how many steps we want to run
                1..20)
        ) {
            // Run the test and fail if it fails
            // The return value is `MyHeapState<i32>` representing the final
            // state, so we could do some additional checks with it if we
            // wanted to.
            test_case.run()?;
        }
    }

    // NEW: Testing our merge function
    // To merge two heaps, we need to compose two copies of the earlier state
    // machine and then add an extra operation to merge the heaps.

    #[derive(Clone, Debug)]
    struct TwoHeaps<T> {
        left: MyHeapState<T>,
        right: MyHeapState<T>,
    }

    fn initial_two_heap_state<T : cmp::Ord + fmt::Debug + Clone>
        () -> impl Strategy<Value = TwoHeaps<T>>
    {
        (initial_heap_strategy(), initial_heap_strategy()).prop_map(
            |(left, right)| TwoHeaps { left, right })
    }

    fn two_heap_state_machine<T>() -> StateMachine<TwoHeaps<T>>
    where T : cmp::Ord + fmt::Debug + Arbitrary + Clone + 'static
    { prop_state_machine! {
        type = TwoHeaps<T>;

        // We inherit all invariants from the child state machines and don't
        // have any new invariants that spans both heaps.

        // We _delegate_ operations to each of our internal heaps. This is done
        // by specifying a state machine and a function to extract the current
        // interior state from our state object. Since we want to have
        // relatively full heaps before merging them, we give the delegation
        // relatively high weight.
        //
        // Since these are the no-argument variants of `delegate`, we get all
        // the invariants of the inner state machines for free.
        #[weight = 3]
        delegate (heap_state_machine())
        to left(&mut state) -> &mut MyHeapState<T> {
            &mut state.left
        }

        #[weight = 3]
        delegate (heap_state_machine())
        to right(&mut state) -> &mut MyHeapState<T> {
            &mut state.right
        }

        // And finally we have our own mutation operation, merge.
        // Since left and right are built the same way, we just always merge
        // right into left.
        fn merge(&mut state) {
            state.left.heap.merge(&mut state.right.heap);
            // We of course need to continue manually maintaining our
            // sanity-checking state on the side.
            state.left.existing_elements.extend(
                state.right.existing_elements.drain(..));
        }
    } }

    proptest! {
        //#[test]
        fn test_two_heap_machine(
            test_case in two_heap_state_machine::<i32>().test_case(
                // ... given the initial state ...
                initial_two_heap_state::<i32>(),
                // ... and how many steps we want to run
                1..40)
        ) {
            // Run the test and fail if it fails
            test_case.run()?;
        }
    }

    // So main can call the tests
    pub fn run_tests() {
        test_state_machine();
        test_two_heap_machine();
    }
}

fn main() {
    test::run_tests();
    println!("Tests pass!");
}
