## Unreleased

### Features

- Add `boxed_union` feature which when turned on uses heap allocation for
  `#[derive(Arbitrary)]` strategy synthesis preventing stack overflow for
  exceptionally large structures.

### Dependencies

- Upgraded `syn` to 2.x

### 0.4.0

### Other Notes

- `compiletest_rs` updated from 0.9 to 0.10

### Other Notes

- Upgraded `syn`, `quote`, and `proc-macro2` to 1.0

## 0.3.0

### Breaking changes

- The minimum supported Rust version has been increased to 1.50.0.

### Bug Fixes

- Certain `enum`s could not be derived before, and now can be.

- Structs with more than 10 fields can now be derived.

## 0.2.0

### Breaking changes

- Generated code now requires `proptest` 0.10.0.

## 0.1.2

### Other Notes

- Derived enums now use `LazyTupleUnion` instead of `TupleUnion` for better
  efficiency.

## 0.1.1

This is a minor release to correct a packaging error. The license files are now
included in the files published to crates.io.
