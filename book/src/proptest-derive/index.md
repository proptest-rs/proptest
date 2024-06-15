# The `proptest-derive` crate

The `proptest-derive` crate provides a procedural macro,
`#[derive(Arbitrary)]`, which can be used to automatically generate simple
`Arbitrary` implementations for user-defined types, allowing them to be used
with `any()` and embedded in other `#[derive(Arbitrary)]` types without fuss.

It is recommended to have a basic working understanding of the [`proptest`
crate](/proptest/index.md) before getting into this part of the
documentation.

**This crate is currently 0.x.** It may see breaking changes breaking changes from
time to time.

We are currently looking to stabilize proptest-derive -- if you have any suggestions
or asks to be included in a 1.x stable release, [open an issue on our github repo](https://github.com/proptest-rs/proptest/issues/new)
