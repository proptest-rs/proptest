## Unreleased

## 0.4.0

### Breaking Changes

- The minimum supported Rust version has been increased to 1.82.0. ([\#605](https://github.com/proptest-rs/proptest/pull/605))

## 0.3.1

### Bug Fixes

- Fix attr macro incorrectly eating mutability modifiers. ([\#602](https://github.com/proptest-rs/proptest/pull/602))

## 0.3.0

### New Features

- Update attr macro to use argument names where trivial, preserving better debugging experience. ([\#594](https://github.com/proptest-rs/proptest/pull/594))

### Bug Fixes

- Fix shorthand struct initialization lint.

## 0.2.0

### Other Notes

- Updated `rand` dependency from 0.8 to 0.9.
- Bump all dependencies to latest compatible with MSRV 1.66.

## 0.1.0

Initial release, an MVP of a #[proptest] attribute macro
