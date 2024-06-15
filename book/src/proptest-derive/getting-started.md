# Getting started

## Cargo

Run `cargo add --dev proptest-derive` or add
```toml
proptest-derive = "0.4.0";
```
to the `[dev-dependencies]` section of your `Cargo.toml`

### Versioning

`proptest-derive` is currently 0.x. Once it is more stable, it will
be versioned in lock-step with the main `proptest` crate.

## Using derive

Inside any of your test modules, you can simply add `#[derive(Arbitrary)]` to a
struct or enum declaration.

```rust
#[cfg(test)]
mod test {
    use proptest::prelude::*;
    use proptest_derive::Arbitrary;

    #[derive(Arbitrary, Debug)]
    struct MyStruct {
        // ...
    }

    proptest! {
        #[test]
        fn test_one(my_struct: MyStruct) {
            // ...
        }

        // Equivalent to the above
        fn test_two(my_struct in any::<MyStruct>()) {
            // ...
        }
    }
}
```

In order to use `proptest-derive` on a type _not_ in a test module without also
depending on proptest for your main build, you must currently manually gate off
the related annotations. This is something we plan to [improve in the
future](https://github.com/proptest-rs/proptest/pull/106).


```rust
#[cfg(test)] use proptest_derive::Arbitrary;

#[derive(Debug)]
// derive(Arbitrary) is only available in tests
#[cfg_attr(test, derive(Arbitrary))]
struct MyStruct {
    // Attributes consumed proptest-derive must not be added when the
    // declaration is not being processed by derive(Arbitrary).
    #[cfg_attr(test, proptest(value = 42))]
    answer: u32,
    // ...
}
```
