# Introduction

Proptest is a property testing framework (i.e., the QuickCheck family)
inspired by the [Hypothesis](https://hypothesis.works/) framework for
Python. It allows to test that certain properties of your code hold for
arbitrary inputs, and if a failure is found, automatically finds the
minimal test case to reproduce the problem. Unlike QuickCheck, generation
and shrinking is defined on a per-value basis instead of per-type, which
makes it more flexible and simplifies composition.

## Ecosystem

Proptest has evolved to be more than a single library. Currently the following
crates exist:
- `proptest` - the core testing library containing traits, implementations of those
  traits for `core`, `alloc`, `std` and some popular 3rd party crates, a test runner
  and some sugar to make working with the test runner and traits easier.
- `proptest-derive` - a derive macro for implementing `Arbitrary` when the applicable type does not
  have unique requirements for generating data.
- `proptest-macro` - procedural macros that make writing proptests easier.
- `proptest-state-machine` - a lightweight framework built on top of `proptest`
  for testing systems against a reference implementation.

## Status of the proptest ecosystem

`proptest` has been stable at a 1.x release for roughly 3 years and not seen substantial
architectural changes in quite some time -- generally receiving passive maintenance
and minor feature requests. In that time, Rust has continued evolving with the stabilization
and maturation of the async ecosystem and generic associated types (GAT). We are
evaluating what a 2.x release of proptest would look like and what features and
changes would warrant a new major version. If you have ideas, suggestions, or asks,
please [open an issue on our github repo](https://github.com/proptest-rs/proptest/issues/new)

See the [changelog](https://github.com/proptest-rs/proptest/blob/master/proptest/CHANGELOG.md)
for a full list of substantial historical changes, breaking and otherwise.

`proptest-derive`, `proptest-macro`, and `proptest-state-machine` are all 0.x
releases with the macros and state machine crates being very new additions. Breaking
changes should be assumed to happen until a 1.x release for each crate.

## What is property testing?

_Property testing_ is a system of testing code by checking that certain
properties of its output or behaviour are fulfilled for all inputs. These
inputs are generated automatically, and, critically, when a failing input
is found, the input is automatically reduced to a _minimal_ test case.

Property testing is best used to complement traditional unit testing (i.e.,
using specific inputs chosen by hand). Traditional tests can test specific
known edge cases, simple inputs, and inputs that were known in the past to
reveal bugs, whereas property tests will search for more complicated inputs
that cause problems.
