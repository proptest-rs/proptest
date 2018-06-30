//-
// Copyright 2018 Jason Lingle
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Support code for state-machine testing.
//!
//! Please see the documentation for the `proptest_state_machine!` macro for
//! typical usage.

use std_facade::{Arc, Box, Cow, Vec};
use std_facade::fmt;
use std_facade::PhantomData;

use collection::{self, SizeRange};
use strategy::{BoxedStrategy, Strategy, Union, W};
use test_runner::{TestCaseError, TestCaseResult};

/**
Syntax sugar to define a set of rules for a state-machine-based test.

A tutorial follows the reference.

## Reference

The macro must begin with a statement of the form `type = SomeType;`, where
`SomeType` names the type which holds the state being tested. Following that is
any number of clauses. While the clauses have syntax similar to normal Rust
syntax, they generally need to conform closely to the syntax proscribed here
since the expand into rather different code.

A _mutator_ clause has the form

```rust,ignore
fn function_name(&mut state, arguments...) {
  // code...
}
```

A mutator clause defines a single mutating operation against the state. The
`&mut state` argument implicitly has the type given in the `type = SomeType;`
clause at the beginning (variable names other than `state` are allowed). The
arguments list is optional; if present, it has the same syntax as the arguments
to test functions defined by the [`proptest!`](macro.proptest.html) macro.
Values for the arguments are generated and shrunken as one might expect.

The body can use all the proptest macros usable from `proptest!` tests.

An _invariant_ clause has the form

```rust,ignore
invariant function_name(&state) {
  // code...
}
```

Invariants are checked after every mutating operation and the test fails if the
invariant check panics or returns an error. As with mutator clauses, the usual
proptest macros are supported.

Finally, a _delegate_ clause has the form

```rust,ignore
delegate (state_machine_expression...)
to function_name(&mut state, arguments...) -> &mut ChildType {
  // expression...
}
```

The function arguments work as with mutator clauses. However, instead of
mutating the state, the code is expected to evaluate to a value of `&mut
ChildType`. `state_machine_expression` is any expression which evaluates to a
`StateMachine<ChildType>`. The delegate clause will cause a mutation for the
child state machine to be applied to the interior value chosen by the user
code, allowing state machine tests to be composed.

Both mutator and delegate clauses can be preceded by the syntax
`#[weight = expression]`, where `expression` evaluates to a non-zero `u32`.
This can be used to control the relative probabilities of each clause being
activated, where a weight-2 clause is activated twice as often as a weight-1
clause. If omitted, the default weight is 1.

The macro evaluates to a `StateMachine<SomeType>`, where `SomeType` is the type
defined in the opening statement.

## Tutorial

This example closely follows [a Hypothesis example for that library's roughly
corresponding feature](https://hypothesis.works/articles/rule-based-stateful-testing/).

The code examples here omit some code of the implementation for the sake of
brevity. Fully compilable and runnable versions of the code snippets can be
found under the
[`examples`](https://github.com/AltSysrq/proptest/tree/master/examples)
directory in the source repository.

_State-machine-based_ property testing refers to testing properties of stateful
systems as they evolve over time via mutation. For example, this is useful for
tests around data structure implementations, which are hard to exhaustively
explore via the usual view of a property test as a pure function.

### Setup

Let's kick off by defining our own implementation of a max-heap, like
`BinaryHeap` from `std`.

```rust
# use std::cmp;
#[derive(Clone, Debug)]
pub struct MyHeap<T> { data: Vec<T> }

impl<T : cmp::Ord> MyHeap<T> {
    pub fn new() -> Self { MyHeap { data: vec![] } }

    pub fn is_empty(&self) -> bool { self.data.is_empty() }

    pub fn len(&self) -> usize { self.data.len() }

    pub fn iter(&self) -> impl Iterator<Item = &T> { self.data.iter() }

    pub fn push(&mut self, value: T) {
        // Implementation omitted for brevity
        // Full implementation can be found in the `examples` directory in the
        // source repository.
    }

    pub fn pop(&mut self) -> Option<T> {
        // THIS IS WRONG
        if self.is_empty() {
            None
        } else {
            Some(self.data.swap_remove(0))
        }
    }
}
# fn main() { }
```

To make a state-machine test around this, we need four things:

1. The state we'll be operating on.
2. A `Strategy` to generate starting states.
3. A `StateMachine` that can operate on the state.
4. One or more tests that use (2) and (3).

For the first point, it would be possible to directly use `MyHeap<T>` and be
done with it. However, we're going to be a bit more thorough here and within
the test will also track what _should_ be in the heap with a `Vec<T>`, the idea
being that it is a slow but obviously correct way of tracking that information.
Thus we define a wrapper:

```rust
# #[derive(Debug, Clone)] struct MyHeap<T>(T);
#[derive(Debug, Clone)]
struct MyHeapState<T> {
    heap: MyHeap<T>,
    existing_elements: Vec<T>,
}
# fn main() { }
```

Now for (2). There aren't any useful starting states other than an empty heap,
so a function producing that strategy is quite straightforward.

```rust
# use std::{cmp, fmt};
# use proptest::prelude::*;
# #[derive(Clone, Debug)] struct MyHeap<T>(Option<T>);
# #[derive(Clone, Debug)] struct MyHeapState<T> {
#    heap: MyHeap<T>,
#    existing_elements: Vec<T>,
# }
# impl<T> MyHeap<T> { fn new() -> Self { MyHeap(None) } }
fn initial_heap_strategy<T>() -> impl Strategy<Value = MyHeapState<T>>
// Ord: due to requirement on `impl MyHeap<T>`
// Debug: Strategy values need to be `Debug` in general
// Clone: So we can use `Just`
where T : cmp::Ord + fmt::Debug + Clone {
    Just(MyHeapState {
        heap: MyHeap::new(),
        existing_elements: vec![],
    })
}
# fn main() { }
```

Step (3) is the interesting part, where this macro comes in. We'll put the
state machine definition in its own function so we can reuse it later, and
because it would be hard to read in-line. We'll start with the boilerplate.

```rust,ignore
use proptest::state_machine::StateMachine;

fn heap_state_machine<T>() -> StateMachine<MyHeapState<T>>
// Ord and Debug required for the same reason as in the strategy above.
// We'll be using Arbitrary and Clone in our implementation below.
where T : cmp::Ord + fmt::Debug + Arbitrary + Clone + 'static
{ prop_state_machine! {
    type = MyHeapState<T>;

    // More code will go here
} }
```

The function declaration is verbose and a bit redundant with information in the
macro, but proptest does not currently provide a way to combine the function
declaration and the `prop_state_machine!` macro itself, since the function may
need modifiers or complex generics.

At the start of the macro, we tell the macro what type of state we'll be
operating on. This must match the type inside `StateMachine<...>` in the
function return type when `prop_state_machine!` is used as the return
expression.

### Defining Invariants

The first thing to think about is what the _invariants_ of the state machine
are. That is, what conditions should be true _at all times_, regardless of
mutations? For the purposes of our example, we'll check that the heap and our
`Vec` tracker of the heap's contents agree on the number of items in the heap,
and that `is_empty()` works. Add the following inside the macro invocation:

```rust,ignore
    invariant len_equals_number_of_elements(&state) {
        assert_eq!(state.existing_elements.len(), state.heap.len());
    }

    invariant is_empty_if_length_zero(&state) {
        assert_eq!(0 == state.heap.len(), state.heap.is_empty());
    }
```

Each invariant defines a function which is run against the state. All
invariants are tested after each mutation. An invariant function may panic or
return `Err` to fail the test; otherwise, the test is allowed to succeed.

### Defining Mutators

Next, we start adding mutators. First, we add one for adding items to the heap.

```rust,ignore
    #[weight = 2]
    fn push(&mut state, value: T) {
        state.heap.push(value.clone());
        state.existing_elements.push(value);
    }
```

The `value: T` argument causes a `T` to be provided via its `Arbitrary`
implementation. The arguments here support the same syntax as arguments to
`proptest!` functions. The `#[weight = 2]` decoration doubles the probability
of using this mutator (as opposed to `pop`, which we'll add next) so that tests
are less likely to spend most of their time with tiny or empty heaps.

In our mutator, we simply call the actual `push` function on the heap, then
update our internal state of what the heap should look like. In a
black-box-style test, there isn't anything for us to check afterwards other
than the common invariants which are checked automatically.

We now add a mutator for the corresponding `pop` method.

```rust,ignore
    fn pop(&mut state) {
        let was_empty = state.heap.is_empty();

        match state.heap.pop() {
            None => assert!(was_empty),

            Some(value) => {
                assert!(!was_empty);

                for in_heap in state.heap.iter() {
                    assert!(value >= *in_heap); // Nicer messages elided for brevity
                }

                let matching_index = state.existing_elements.iter()
                    .enumerate()
                    .find(|&(_, existing)| value == *existing)
                    .map(|(ix, _)| ix);

                if let Some(matching_index) = matching_index {
                    state.existing_elements.swap_remove(matching_index);
                } else {
                    panic!();
                }
            },
        }
    }
```

There's a lot more going on here! First, we check whether the heap currently
thinks it's empty. This will set our expectation as to how `pop()` should
behave.

We then call `pop()`, and make sure it returned `Some` if and only if it was
non-empty. In the case of `Some`, we also check a number of other
post-conditions: the value returned was not less than any element left in the
heap, and that the value returned is actually supposed to be in the heap.
Finally, we remove that value from the `Vec` tracking what's supposed to be in
the heap.

We didn't include a `#[weight]` this time, so the weight defaults to 1. In
other words, `push()` will happen twice as often as `pop()`, so the heap size
will trend up.

### Tying the test together

Finally, we can put the components we defined above together into a runnable
test.

```rust,ignore
proptest! {
    #[test]
    fn test_state_machine(
        // Use the state machine definition to create a test case ...
        test_case in heap_state_machine::<i32>().test_case(
            // ... given the initial state ...
            initial_heap_strategy::<i32>(),
            // ... and how many steps we want to run
            1..20)
    ) {
        // The return value is `MyHeapState<i32>` representing the final
        // state, so we could do some additional checks with it if we
        // wanted to.
        test_case.run()?;
    }
}
```

(There currently is no syntax sugar for the above, but there may be in the
future.)

When we run the test, proptest quickly discovers our broken `pop`
implementation, failing with the output

```text
thread 'main' panicked at 'Test failed: Popped value -1, which was less than 0 still in the heap;
minimal failing input: test_case = StateMachineTestCase {
    state: MyHeapState {
        heap: MyHeap {
            data: []
        },
        existing_elements: []
    },
    mutations: [
        push(value = 0),
        push(value = 0),
        push(value = -1),
        pop(()),
        pop(())
    ],
    machine: "<elided for clarity>",
    trace: false
}
        successes: 0
        local rejects: 0
        global rejects: 0
', examples/state_machine_v1.rs:174:5
```

Here, we can see our initial state (always an empty heap) and more importantly,
what sequence of mutations we performed. In the minimal failing case, we pushed
the integer `0` twice, then `-1` once, then tried to pop two values. The second
pop however returned `-1` while `0` was still in the heap, violating the
contract for a max heap.

To get better visibility into what's going on during the test cases, we can
change our test body to

```rust,ignore
        test_case.trace().run()?;
```

which results in the below additional output. Here, we can not only see what
mutations occurred, but also what the state of the system was at each step.

```text
initial state: MyHeapState { heap: MyHeap { data: [] }, existing_elements: [] }
check initial invariants
enter push(value = 0)
        => MyHeapState { heap: MyHeap { data: [0] }, existing_elements: [0] }
exit  push(value = 0)
enter push(value = 0)
        => MyHeapState { heap: MyHeap { data: [0, 0] }, existing_elements: [0, 0] }
exit  push(value = 0)
enter push(value = -1)
        => MyHeapState { heap: MyHeap { data: [0, 0, -1] }, existing_elements: [0, 0, -1] }
exit  push(value = -1)
enter pop(())
        => MyHeapState { heap: MyHeap { data: [-1, 0] }, existing_elements: [-1, 0] }
exit  pop(())
enter pop(())
thread 'main' panicked at 'Popped value -1, which was less than 0 still in the heap', examples/state_machine_v1.rs:149:25
```

*/
#[macro_export]
macro_rules! prop_state_machine {
    (type = $statety:ty;
     $($token:tt)*) => {
        #[allow(unused_mut)]
        let mut ops = $crate::state_machine::helper::new_ops();
        #[allow(unused_mut)]
        let mut invariants = $crate::state_machine::helper::new_invariant_vec();

        prop_sm_helper! {
            @_MUNCH(ops, invariants, $statety) $($token)*
        }

        $crate::state_machine::StateMachine::new_from_vecs(
            ops, invariants)
    };
}

#[macro_export]
#[allow(missing_docs)]
#[doc(hidden)]
macro_rules! prop_sm_helper {
    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)) => { };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     fn $op_name:ident(&mut $statevar:ident) $body:block
     $($rest:tt)*) => {
        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*
        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::LazyJust::new(
                     || $crate::state_machine::Mutator::boxed(
                         $crate::state_machine::DirectMutator::new(
                             stringify!($op_name), (), |$statevar: &mut $statety, _| {
                                 $body;
                                 Ok(())
                             }))))));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     fn $op_name:ident(&mut $statevar:ident,
                       $($parm:pat in $strategy:expr),+) $body:block
     $($rest:tt)*) => {
        let names = proptest_helper!(@_WRAPSTR ($($parm),*));

        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*
        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::Strategy::prop_map(
                     proptest_helper!(@_WRAP ($($strategy)*)),
                     move |values| $crate::state_machine::Mutator::boxed(
                         $crate::state_machine::DirectMutator::new(
                             stringify!($op_name),
                             $crate::sugar::NamedArguments(names, values),
                             |$statevar: &mut $statety,
                              $crate::sugar::NamedArguments(
                                  _, proptest_helper!(@_WRAPPAT ($($parm),*)))|
                             {
                                 $body;
                                 Ok(())
                             }))))));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     fn $op_name:ident(&mut $statevar:ident,
                       $($arg:tt)+) $body:block
     $($rest:tt)*) => {
        let names = proptest_helper!(@_EXT _STR ($($arg)*));

        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*
        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::Strategy::prop_map(
                     proptest_helper!(@_EXT _STRAT ($($arg)*)),
                     move |values| $crate::state_machine::Mutator::boxed(
                         $crate::state_machine::DirectMutator::new(
                             stringify!($op_name),
                             $crate::sugar::NamedArguments(names, values),
                             |$statevar: &mut $statety,
                              $crate::sugar::NamedArguments(
                                  _, proptest_helper!(@_EXT _PAT ($($arg)*)))|
                             {
                                 $body;
                                 Ok(())
                             }))))));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     invariant $invname:ident (&$statevar:ident) $body:block
     $($rest:tt)*) => {
        $invariants.push(::std::boxed::Box::new(
            |$statevar: &mut $statety| {
                let $statevar: &$statety = $statevar;
                $body;
                Ok(())
            }));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     delegate ($child_machine:expr)
     to $name:ident (&mut $statevar:ident)
                     -> &mut $childty:ty $body:block
     $($rest:tt)*) => {
        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*

        let child_machine = $child_machine;

        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::Strategy::prop_map(
                     child_machine.mutation_strategy(),
                     |mutation| $crate::state_machine::Mutator::boxed(
                         $crate::state_machine::MapMutator::new(
                             stringify!($name), (),
                             |$statevar: &mut $statety, _| {
                                 let ret: &mut $childty = $body;
                                 Ok(ret)
                             },
                             mutation))))));

        let child_invariants = child_machine.invariants();
        $invariants.push(::std::boxed::Box::new(move |$statevar: &mut $statety| {
            let sub: &mut $childty = $body;
            child_invariants(sub)
        }));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     delegate ($child_machine:expr)
     to $name:ident (&mut $statevar:ident, $($parm:pat in $strategy:expr),*)
                     -> &mut $childty:ty $body:block
     $($rest:tt)*) => {
        let names = proptest_helper!(@_WRAPSTR ($($parm),*));

        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*

        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::Strategy::prop_map(
                     ($child_machine.mutation_strategy(),
                      proptest_helper!(@_WRAP ($($strategy),*))),
                     move |(mutation, values)|
                         $crate::state_machine::Mutator::boxed(
                             $crate::state_machine::MapMutator::new(
                                 stringify!($name),
                                 $crate::sugar::NamedArguments(names, values),
                                 |$statevar: &mut $statety,
                                  $crate::sugar::NamedArguments(
                                      _, proptest_helper!(@_WRAPPAT ($($parm)*)))
                                 | {
                                     let ret: &mut $childty = $body;
                                     Ok(ret)
                                 },
                                 mutation))))));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };

    (@_MUNCH($ops:ident, $invariants:ident, $statety:ty)
     $(#[$attr:ident = $attrv:expr])*
     delegate ($child_machine:expr)
     to $name:ident (&mut $statevar:ident, $($arg:tt)*)
                     -> &mut $childty:ty $body:block
     $($rest:tt)*) => {
        let names = proptest_helper!(@_EXT _STR ($($arg)*));

        #[allow(unused_mut)]
        let mut attrs = $crate::state_machine::helper::OpAttrs::DEFAULT;
        $(attrs.$attr = $attrv;)*

        $ops.push(
            (attrs.weight,
             $crate::strategy::Strategy::boxed(
                 $crate::strategy::Strategy::prop_map(
                     ($child_machine.mutation_strategy(),
                      proptest_helper!(@_EXT _STRAT ($($arg)*))),
                     move |(mutation, values)|
                         $crate::state_machine::Mutator::boxed(
                             $crate::state_machine::MapMutator::new(
                                 stringify!($name),
                                 $crate::sugar::NamedArguments(names, values),
                                 |$statevar: &mut $statety,
                                  $crate::sugar::NamedArguments(
                                      _, proptest_helper!(@_EXT _PAT ($($arg)*)))
                                 | {
                                     let ret: &mut $childty = $body;
                                     Ok(ret)
                                 },
                                 mutation))))));

        prop_sm_helper! {
            @_MUNCH($ops, $invariants, $statety) $($rest)*
        }
    };
}

/// A state-machine-based test.
///
/// `StateMachine` values are simultaneously a description of mutations and
/// invariants of that state, as well as a test that the state under test
/// upholds its expected behaviours.
///
/// Instances of this struct are usually constructed by the
/// `proptest_state_machine!` macro. Please see the documentation of that macro
/// for examples of typical usage.
#[must_use = "StateMachine does nothing unless used"]
pub struct StateMachine<S : fmt::Debug> {
    mutation_strategy: BoxedStrategy<Box<dyn Mutator<S>>>,
    invariants: Arc<dyn Fn (&mut S) -> TestCaseResult>,
}

impl<S : fmt::Debug> Clone for StateMachine<S> {
    fn clone(&self) -> Self {
        StateMachine {
            mutation_strategy: self.mutation_strategy.clone(),
            invariants: Arc::clone(&self.invariants),
        }
    }
}

// TODO: Why does rustc think `S : 'static` is necessary? It seems to arise
// from `Strategy<Value = Box<dyn Mutator<S>>> + 'static` somehow implying
// `S : 'static`.
impl<S : fmt::Debug + 'static> StateMachine<S> {
    /// Create a new `StateMachine` with the given mutation strategy and
    /// invariants.
    ///
    /// `mutation_strategy` is a strategy for generating single mutators
    /// against the state value type.
    ///
    /// `invariants` is a function which is run immediately after each
    /// mutation, to check for properties of the state that are expected to
    /// hold at all times. The function takes `&mut S` to allow `delegate` to
    /// use a single mapping function. Invariants should not actually modify
    /// the value.
    pub fn new(mutation_strategy: impl Strategy<Value = Box<dyn Mutator<S>>> + 'static,
               invariants: Arc<dyn Fn (&mut S) -> TestCaseResult>) -> Self {
        StateMachine {
            mutation_strategy: mutation_strategy.boxed(),
            invariants,
        }
    }

    #[allow(missing_docs)]
    #[doc(hidden)]
    pub fn new_from_vecs(
        mutation_strategies: Vec<W<BoxedStrategy<Box<dyn Mutator<S>>>>>,
        invariants: Vec<Box<dyn Fn (&mut S) -> TestCaseResult>>) -> Self
    {
        StateMachine::new(
            Union::new_weighted(mutation_strategies),
            Arc::new(move |state| {
                for invariant in &invariants {
                    invariant(state)?;
                }
                Ok(())
            }))
    }

    /// Return the `Strategy` used to construct mutators  for this
    /// `StateMachine`.
    pub fn mutation_strategy(&self) -> BoxedStrategy<Box<dyn Mutator<S>>> {
        self.mutation_strategy.clone()
    }

    /// Return the function used for testing invariants.
    pub fn invariants(&self) -> Arc<dyn Fn (&mut S) -> TestCaseResult> {
        Arc::clone(&self.invariants)
    }

    /// Create a strategy for test cases using this state machine.
    ///
    /// `state` gives a strategy used for generating the initial state.
    ///
    /// `size` specifies the possible number of steps to take.
    pub fn test_case
        (&self, state: impl Strategy<Value = S>, size: impl Into<SizeRange>)
         -> impl Strategy<Value = StateMachineTestCase<S>>
    {
        let clone = self.clone();

        (state, collection::vec(self.mutation_strategy(), size))
            .prop_map(move |(state, mutations)| StateMachineTestCase::new(
                clone.clone(), state, mutations))
    }
}

impl<S : fmt::Debug> fmt::Debug for StateMachine<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("StateMachine")
            .field("mutation_strategy", &self.mutation_strategy)
            .field("invariants", &"<function>")
            .finish()
    }
}

/// A single test-case of a state machine.
///
/// The only useful operation is `.run()`.
#[must_use = "StateMachineTestCase does nothing unless .run() is called"]
pub struct StateMachineTestCase<S : fmt::Debug> {
    state: S,
    mutations: Vec<Box<dyn Mutator<S>>>,
    machine: StateMachine<S>,
    trace: bool,
}

impl<S : fmt::Debug> fmt::Debug for StateMachineTestCase<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("StateMachineTestCase")
            .field("state", &self.state)
            .field("mutations", &self.mutations)
            .field("machine", &"<elided for clarity>")
            .field("trace", &self.trace)
            .finish()
    }
}

impl<S : fmt::Debug> StateMachineTestCase<S> {
    fn new(machine: StateMachine<S>,
           state: S,
           mutations: Vec<Box<dyn Mutator<S>>>) -> Self {
        StateMachineTestCase { state, mutations, machine, trace: false }
    }

    /// Enable tracing on this test case.
    ///
    /// Tracing causes verbose logging to be emitted about each mutation as
    /// they are performed.
    pub fn trace(mut self) -> Self {
        self.trace = true;
        self
    }

    /// Run this test case.
    ///
    /// Returns `Ok` with the final internal state if successful. On failure,
    /// returns `Err` with the appropriate error, or panics if user code
    /// triggered a panic.
    pub fn run(mut self) -> Result<S, TestCaseError> {
        if self.trace {
            println!("initial state: {:?}", self.state);
            println!("check initial invariants");
        }
        (self.machine.invariants)(&mut self.state)?;

        for mut mutation in self.mutations {
            if self.trace {
                println!("enter {:?}", mutation);
            }
            mutation.mutate(&mut self.state)?;
            if self.trace {
                println!("        => {:?}", self.state);
            }
            (self.machine.invariants)(&mut self.state)?;
            if self.trace {
                println!("exit  {:?}", mutation);
            }
        }

        Ok(self.state)
    }
}

/// A single-use callback that can mutate a particular type of `StateMachine`.
pub trait Mutator<S> : fmt::Debug {
    /// Mutate `state`.
    ///
    /// This effectively consumes `self`, but it is passed as a mutable
    /// reference so that this trait is object-safe and so that the mutator can
    /// be retained for debugging output.
    ///
    /// Implementations are not required to have any particular behaviour if
    /// this function is called more than once. However, their `Debug`
    /// implementation must continue to work sensibly.
    fn mutate(&mut self, state: &mut S) -> TestCaseResult;

    /// Convenience for creating a `Box<dyn Mutator<S>>`.
    fn boxed(self) -> Box<dyn Mutator<S>>
    where Self : Sized + 'static {
        Box::new(self)
    }
}

/// A `Mutator` which directly applies a function to the state of a
/// `StateMachine`.
///
/// By default, the string format of the argument list to the inner function is
/// built and saved before the arguments are passed to the function, so that
/// `Debug` can still output the argument values even after the mutation has
/// been executed. This can add substantial overhead for tests with make large
/// numbers of small mutations. The `no_debug()` method can be used to suppress
/// this.
pub struct DirectMutator<S, A : fmt::Debug, F : FnOnce (&mut S, A) -> TestCaseResult> {
    function: Option<F>,
    arguments: Option<A>,
    formatted_arguments: Option<Cow<'static, str>>,
    name: &'static str,
    _use_s: PhantomData<S>,
}

impl<S, A : fmt::Debug, F : FnOnce (&mut S, A) -> TestCaseResult>
fmt::Debug for DirectMutator<S, A, F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (&self.arguments, &self.formatted_arguments) {
            (Some(ref args), _) =>
                write!(f, "{}({:?})", self.name, args),
            (None, Some(ref formatted)) =>
                write!(f, "{}({})", self.name, formatted),
            (None, None) =>
                panic!("Mutator arguments were consumed but not formatted"),
        }
    }
}

impl<S, A : fmt::Debug, F : FnOnce (&mut S, A) -> TestCaseResult>
Mutator<S> for DirectMutator<S, A, F> {
    fn mutate(&mut self, state: &mut S) -> TestCaseResult {
        let fun = self.function.take().expect("Mutator applied more than once");
        let args = self.arguments.take().expect("Mutator applied more than once");
        if self.formatted_arguments.is_none() {
            self.formatted_arguments = Some(Cow::Owned(format!("{:?}", args)));
        }

        fun(state, args)
    }
}

impl<S, A : fmt::Debug, F : FnOnce (&mut S, A) -> TestCaseResult>
DirectMutator<S, A, F> {
    /// Create a `DirectMutator` with the given name, function, and arguments
    /// passed to the function.
    pub fn new(name: &'static str, arguments: A, function: F) -> Self {
        DirectMutator {
            function: Some(function),
            arguments: Some(arguments),
            formatted_arguments: None,
            name,
            _use_s: PhantomData,
        }
    }

    /// Suppress generation of the argument debug string.
    pub fn no_debug(&mut self) {
        self.formatted_arguments = Some(Cow::Borrowed(
            "<argument formatting disabled>"));
    }
}

/// A `Mutator` which applies a function to extract an interior value from the
/// state, then runs another mutator against the interior value.
pub struct MapMutator<
    S, C,
    A : fmt::Debug,
    F : FnOnce (&mut S, A) -> Result<&mut C, TestCaseError>
> {
    function: Option<F>,
    arguments: Option<A>,
    formatted_arguments: Option<Cow<'static, str>>,
    name: &'static str,
    child: Box<dyn Mutator<C>>,
    _use_s: PhantomData<S>,
}

impl<S, C, A : fmt::Debug,
     F : FnOnce (&mut S, A) -> Result<&mut C, TestCaseError>>
fmt::Debug for MapMutator<S, C, A, F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (&self.arguments, &self.formatted_arguments) {
            (Some(ref args), _) =>
                write!(f, "{}({:?}) => {:?}", self.name, args, self.child),
            (None, Some(ref formatted)) =>
                write!(f, "{}({}) => {:?}", self.name, formatted, self.child),
            (None, None) =>
                panic!("Mutator arguments were consumed but not formatted"),
        }
    }
}

impl<S, C, A : fmt::Debug,
     F : FnOnce (&mut S, A) -> Result<&mut C, TestCaseError>>
Mutator<S> for MapMutator<S, C, A, F> {
    fn mutate(&mut self, state: &mut S) -> TestCaseResult {
        let fun = self.function.take().expect("Mutator applied more than once");
        let args = self.arguments.take().expect("Mutator applied more than once");
        if self.formatted_arguments.is_none() {
            self.formatted_arguments = Some(Cow::Owned(format!("{:?}", args)));
        }

        self.child.mutate(fun(state, args)?)
    }
}

impl<S, C, A : fmt::Debug,
     F : FnOnce (&mut S, A) -> Result<&mut C, TestCaseError>>
MapMutator<S, C, A, F> {
    /// Create a `MapMutator` with the given name, function, arguments passed
    /// to the function, and child mutator.
    pub fn new(name: &'static str, arguments: A, function: F,
               child: Box<dyn Mutator<C>>) -> Self {
        MapMutator {
            function: Some(function),
            arguments: Some(arguments),
            formatted_arguments: None,
            name, child,
            _use_s: PhantomData,
        }
    }

    /// Suppress generation of the argument debug string.
    pub fn no_debug(&mut self) {
        self.formatted_arguments = Some(Cow::Borrowed(
            "<argument formatting disabled>"));
    }
}

// Helper functions for the macros, mainly to allow specifying
// partially-inferred types.
#[doc(hidden)]
#[allow(missing_docs)]
pub mod helper {
    use super::*;

    pub fn new_ops<S : fmt::Debug>() -> Vec<W<BoxedStrategy<Box<dyn Mutator<S>>>>> {
        vec![]
    }

    pub fn new_invariant_vec<S>() -> Vec<Box<dyn Fn (&mut S) -> TestCaseResult>> {
        vec![]
    }

    #[derive(Clone, Copy, Debug)]
    pub struct OpAttrs {
        pub weight: u32,
    }

    impl OpAttrs {
        pub const DEFAULT: Self = OpAttrs { weight: 1 };
    }
}

#[cfg(test)]
mod test {
    // No `use`s; make sure the macro works without imports

    fn simple_state_machine() -> ::state_machine::StateMachine<i64> {
        prop_state_machine! {
            type = i64;

            fn negate(&mut state) {
                *state = -*state;
            }

            #[weight = 2]
            fn add_a_bit(&mut state, amt in 0i64..10i64) {
                *state += amt * 2;
            }

            fn sub_a_u8(&mut state, val: u8) {
                *state -= (val as i64) * 2;
            }

            invariant always_even(&state) {
                assert_eq!(0, *state & 1);
            }
        }
    }

    fn pair_of_ints() -> ::state_machine::StateMachine<(i64, i64)> {
        prop_state_machine! {
            type = (i64, i64);

            delegate (simple_state_machine()) to left(&mut state) -> &mut i64 {
                &mut state.0
            }

            #[weight = 2]
            delegate (simple_state_machine())
            to right(&mut state) -> &mut i64 {
                &mut state.1
            }

            delegate (simple_state_machine())
            to random(&mut state, which in 0..2) -> &mut i64 {
                if 0 == which {
                    &mut state.0
                } else {
                    &mut state.1
                }
            }

            delegate (simple_state_machine())
            to random_any(&mut state, which: i32) -> &mut i64 {
                if 0 == which & 1 {
                    &mut state.0
                } else {
                    &mut state.1
                }
            }
        }
    }

    proptest! {
        #[test]
        fn do_simple_state_machine(
            sm in simple_state_machine().test_case(
                ::strategy::Just(0), 1..10)
        ) {
            sm.trace().run().unwrap();
        }

        #[test]
        fn do_pair_of_ints(
            sm in pair_of_ints().test_case(
                (::strategy::Just(0), ::strategy::Just(2)), 1..10)
        ) {
            sm.trace().run().unwrap();
        }
    }
}
