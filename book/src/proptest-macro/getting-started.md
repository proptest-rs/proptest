# Getting started

## Cargo

Run `cargo add --dev proptest-macro` or add
```toml
proptest-macro = "0.1";
```
to the `[dev-dependencies]` section of your `Cargo.toml`

### Versioning

`proptest-macro` is currently 0.x. Once it is more stable, it will
be versioned in lock-step with the main `proptest` crate.

## Using #[property_test]

This crate provides an attribute macro for defining proptests, as an
alternative to the declarative `proptest! { .. }` macro.

To use the attribute macro you add `#[proptest_macro::property_test]`
to a proptest function

```rust
#[cfg(test)]
mod test {
    use proptest_macro::property_test;

    #[property_test]
    fn test_one(my_struct: MyStruct) {
        // ...
    }
}
```

Behind the scenes, `#[property_test]` will collect the parameters of the function into a new temporary struct, derive Arbitrary for it, construct a test runner, and run the body of your function with arbitrary permutations of function arguments.

## Why a procedural macro

Primarily for internal maintenance and feature development. As there
are more complex feature asks, it is easier to develop those in
standard rust code rather than declarative macro patterns.

## When should you use `#[property_test]` vs `proptest!`

Currently `proptest!` has more features and functionality so if you need
any type of customization in terms of configuration or which strategy
to use, continue using `proptest!`.

Eventually `#[property_test]` will likely start to receive features
that we don't plan to add to `proptest!`. We have no plans to deprecate or remove support for `proptest!`.

