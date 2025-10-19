# State Machine testing

The state machine testing support is available in the `proptest-state-machine` crate.

## When to use State Machine testing?

State machine testing automates the checking of properties of a system under test (SUT) against an abstract reference state machine definition. It does this by trying to discover a counter-example that breaks the defined properties of the system and attempts to shrink it to a minimal sequence of transitions that still reproduce the issue.

State machines are a very useful abstraction for reasoning about code. Many things from low-level to high-level logic and anywhere in between can be modelled as a state machine. They are very effective for modelling effectful code, that is code that performs some state changes that can be too hard to test thoroughly with a more manual approach or too complex to verify formally.

Some fitting examples to give you an idea include (by no means exhaustive):

- A data structure with an API that mutates its state
- An API for a database
- Interactions between a client(s) and a server

There is some initial investment needed to set the test up and it usually takes a bit more time to run than simple prop tests, but if correctness is important for your use case, you'll be rewarded with a test that is so effective at discovering bugs it might feel almost magical, but as you'll see, [you could have easily implemented it yourself](#how-does-it-work). Also, once you have the test setup, it is much easier to extend it and add new properties to check.

## How to use it

Before using state machine testing, it is recommended to be at least familiar with the basic concepts of Proptest itself as it's built on its essential foundations. That is:

- Strategies are composed from common proptest constructs and used to generate inputs to a state machine test.
- Because the generated transitions sequence is a strategy itself, a test will attempt to shrink them on a discovery of a case that breaks some properties.
- It will capture regressions file with a seed that can be used to deterministically repeat the found case.

In short, use `ReferenceStateMachine` and `StateMachineTest` to implement your state machine test and `prop_state_machine!` macro to run it.

If you just want to get started quickly, take a look at one of the examples:

- `state_machine_heap.rs` - a simple model to test an API of a heap data structure
- `state_machine_echo_server.rs` - a more advanced model for an echo server with multiple clients talking to it

To see what transitions are being applied in standard output as the state machine test executes, run these with e.g. `PROPTEST_VERBOSE=1 cargo run --example state_machine_heap`.

State machine testing is made up of two parts, an abstract reference state machine definition that drives the inputs to a test and a test definition for a SUT that replicates the same transitions as the reference state machine to find any possible divergence or conditions under which the defined properties (in here post-conditions and invariants) start to break.

### Reference state machine strategy

You can get started with state machine testing by implementing `trait ReferenceStateMachine`, which is used to drive the generation of a sequence of transitions and can also be compared against the state of the SUT. At the minimum, this trait requires two associated types:

- `type State` that represents the state of the reference state machine.
- `type Transition` with possible transitions of the state machine. This is typically an `enum` with its variants containing input parameters for the transitions, if any.

You also have to implement three associated functions:

- To initialize the reference state machine:

  ```rust,ignore
  fn init_state() -> BoxedStrategy<Self::State>
  ```
  
  You can generate some random state with a strategy or use `Just` strategy for a constant value. Note that you can make a `BoxedStrategy` from any `Strategy` by simply calling `.boxed()` on it.

- To generate transitions:
  
  ```rust,ignore
  fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition>
  ```
  
  Most of the time, you'll use `prop_oneof!` here. If a transition takes some input parameters, you can generate those with a `Strategy` and `.prop_map` it to the `Transition` variant. In more complex state machines, the set of valid transitions may depend on the current state. To that end, you can use the `state` argument, possibly combined with `proptest::sample::select` function that allows you to create a strategy that selects a random value from an array or an array-like collection (be careful not to call `select` on an empty array as that will make it fail in a somewhat obscure way). For example, if you want to remove one of the existing keys from a hash map, you can select one of the keys from the current state and map it into a transition. Note that when you do something like this, you'll also need to override the `fn preconditions`, which are explained in more detail below.

- To apply the given transition on the reference state:

  ```rust,ignore
  fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State
  ```

Additionally, you may want to override the default implementation of:

```rust,ignore
fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool
```

By default, this simply returns `true`, which implies that there are no pre-conditions. Pre-conditions are a way of restricting what transitions are valid for a given state and you'll *only* need to restrict the transitions whose validity depends on the current state. This ensures that the reference state machine will only produce and shrink to a sequence of valid transitions. It may not be immediately apparent that the current state may be affected by shrinking. With the example of selecting of keys of a hash map for `fn transitions`, you'll need to check that the transition's key is still present in the hash map, which may no longer be true after some shrinking is applied.

You can either implement `ReferenceStateMachine` for:

- A data structure that will represent your reference state machine and set the associated `type State = Self;` or
- An empty `struct`, which may be more convenient than making a wrapper type if you're using a foreign type for the `type State`

### Definition of a state machine test

With that out of the way, you can go ahead and implement `StateMachineTest`. This also requires two associated types:

- `type SystemUnderTest` which is the type that represents the SUT.
- `type Reference` with the type for which you implemented the `ReferenceStateMachine`.

There are also three associated functions to be implemented here (some types are slightly simplified for clarity):

- Initialize the SUT state:

  ```rust,ignore
  fn init_test(ref_state: &Self::Reference::State) -> Self::SystemUnderTest
  ```
  
  If your `ReferenceStateMachine::init_state` uses a non-constant strategy, you have to use the `ref_state` to initialize this to a corresponding state to ensure that you have consistent initial states.

- Apply the `transition` on the SUT state:
  
  ```rust,ignore
  fn apply(
    mut state: Self::SystemUnderTest,
    ref_state: &Self::Reference::State,
    transition: Transition
  ) -> Self::SystemUnderTest
  ```
  
  This is also where you'll want to check any post-conditions that apply to a given transition, so after you apply the transition to the state, you can `assert!` some properties. Alternatively or additionally, you can use the `ref_state` for comparison, which will have the same transition that is given to this function already applied to it.

- Check properties that apply in any state:

  ```rust,ignore
  fn check_invariants(state: &Self::SystemUnderTest, ref_state: &Self::Reference::State)
  ```

  These must always hold and will be checked after every transition. Just like with `apply`, you have the option to use the `ref_state` for comparison.

To add some teardown logic to run at the end of each test case, you can override the `teardown` function, which by default simply drops the state:

```rust,ignore
fn teardown(state: Self::SystemUnderTest, ref_state: Self::Reference::State)
```

### Make the state machine test runnable

Finally, to run the `StateMachineTest`, you can use the `prop_state_machine!` macro. For example:

```rust,ignore
prop_state_machine! {
  #[test]
  fn name_of_the_test(sequential 1..20 => MyStateMachineTest);
}
```

You pick a `name_of_the_test` and a single numerical value or a range after the `sequential` keyword for a number of transitions to be generated for the state machine execution. The `MyStateMachineTest` is whatever you've implemented the `StateMachineTest` for.

And that's it. You can run the test, perhaps with `cargo watch` as you develop it further, and see if it can find some interesting counter-examples to your properties.

### Extra tips

Because a state machine test may be heavier than regular prop tests, if you're running your tests in a CI you may want to override the default `proptest_config`'s `cases` to include more or fewer cases in a single run. You can also use `PROPTEST_CASES` environment variable and during development it is preferable to override this to run many cases to get a better chance of catching those pesky ~~bugs~~, erm, defects.

> Given that there are thought to be in the region of another four million species that we have not yet even named, there is no doubt that scientists will be kept happily occupied studying them for millennia, so long as the insects remain to be studied. Would the world not be less rich, less surprising, less wonderful, if these peculiar creatures did not exist?
>
> -- <cite>Dave Goulson, Silent Earth</cite>

So let's leave bugs alone and only squash defects instead!

Because the output of a failed test case can be a bit hard to read, it is often convenient to print the transitions. You can do that by simply setting the `proptest_config`'s `verbose` to `1` or higher. Again, if you don't want to keep this in your test's config or if you'd prefer to override the config, you could also use the `PROPTEST_VERBOSE` environment variable instead.

Another helpful config option that is good to know about is `timeout` (`PROPTEST_TIMEOUT` via an env var) for tests that may take longer to execute.

## How does it work

This section goes into the inner workings of how the state machine is implemented, omitting some less interesting details. If you're only interested in using it, you can consider this section an optional read.

The `ReferenceStateMachine::sequential_strategy` sets up a `Sequential` strategy that generates a sequence of transitions from the definition of the `ReferenceStateMachine`. The acceptability of each transition in the sequence depends on the current state of the state machine and `ReferenceStateMachine::preconditions`, if any. The state is updated by the transitions with the `ReferenceStateMachine::apply` function.

The `Sequential` strategy is then fed into Proptest like any other strategy via the `prop_state_machine!` macro and it produces a `Vec<Transition>` that gets passed into `StateMachineTest::test_sequential` where it is applied one by one to the SUT. Its post-conditions and invariants are checked during this process and if a failing case is found, the shrinking process kicks in until it can shrink no longer.

The shrinking strategy which is defined by the associated `type Tree = SequentialValueTree` of the `Sequential` strategy is to iteratively apply `Shrink::InitialState`, `Shrink::DeleteTransition` and `Shrink::Transition` (this can be found in `proptest/src/strategy/state_machine.rs`):

1. We start by trying to delete transitions from the back of the list until we can do so no further (the list has reached the `min_size` - that is the variable that gets set from the chosen range for the number of transitions in the `prop_state_machine!` invocation).
2. Then, we again iteratively attempt to shrink the individual transitions, but this time starting from the front of the list from the first transition to be applied.
3. Finally, we try to shrink the initial state until it's not possible to shrink it any further.

The last applied shrink gets stored in the `SequentialValueTree`, so that if the shrinking process ends up in a case that no longer reproduces the discovered issue, the call to `complicate` in the `ValueTree` implementation of the `SequentialValueTree` can attempt to undo it.

## Similar technologies

The state machine testing support for Proptest is heavily inspired by the Erlang's eqc_statem (see the paper [Finding Race Conditions in Erlang with QuickCheck and PULSE](https://smallbone.se/papers/finding-race-conditions.pdf)) with some key differences. Most notably:

- Currently, only sequential strategy is supported, but a concurrent strategy is planned to be added at later point.
- There are no "symbolic" variables like in eqc_statem. The state for the abstract (reference) state machine is separate from the state of the system under test.
- The post-conditions are not defined in their own function. Instead, they are part of the `StateMachineTest::apply` function.
