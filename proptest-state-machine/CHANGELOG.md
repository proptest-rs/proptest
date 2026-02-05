## 0.7.0

### Breaking Changes

- The minimum supported Rust version has been increased to 1.84.0. ([\#612](https://github.com/proptest-rs/proptest/pull/612))

### New Features

- Extended `Sequential` test definition to accept closures in its function fields. ([\#609](https://github.com/proptest-rs/proptest/pull/609))

### Other Notes

- Added license files to the crate. ([\#618](https://github.com/proptest-rs/proptest/pull/618))

## 0.6.0

### Breaking Changes

- The minimum supported Rust version has been increased to 1.82.0. ([\#605](https://github.com/proptest-rs/proptest/pull/605))

## 0.5.0

### New Features

- Added reference state machine argument to the teardown function to allow comparison against the SUT.
  ([\#595](https://github.com/proptest-rs/proptest/pull/595))

## 0.4.0

### Other Notes

- Set MSRV to 1.82, which is what minimally compiles and completes testing.
- Updated `rand` dependency from 0.8 to 0.9.

## 0.3.1

- Fixed checking of pre-conditions with a shrinked or complicated initial state.
  ([\#482](https://github.com/proptest-rs/proptest/pull/482))

## 0.3.0

### New Features

- Remove unseen transitions on a first step of shrinking.
  ([\#388](https://github.com/proptest-rs/proptest/pull/388))

## 0.2.0

### Other Notes

- `message-io` updated from 0.17 to 0.18

### Bug Fixes

- Removed the limit of number of transitions that can be deleted in shrinking that depended on the number the of transitions given to `prop_state_machine!` or `ReferenceStateMachine::sequential_strategy`.
- Fixed state-machine macro's inability to handle missing config
- Fixed logging of state machine transitions to be enabled when verbose config is >= 1. The "std" feature is added to proptest-state-machine as a default feature that allows to switch the logging off in non-std env.
- Fixed an issue where after simplification of the initial state causes the test to succeed, the initial state would not be re-complicated - causing the test to report a succeeding input as the simplest failing input.
