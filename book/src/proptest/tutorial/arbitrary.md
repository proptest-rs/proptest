# Defining a canonical `Strategy` for a type

We previously used the function `any` as in `any::<u32>()` to generate a
strategy for all `u32`s. This function works with the trait `Arbitrary`,
which QuickCheck users may be familiar with. In proptest, this trait
is already implemented for most owned types in the standard library,
but you can of course implement it for your own types.

In some cases, where it makes sense to define a canonical strategy, such as in
the [JSON AST example](recursive.md), it is a good idea to implement
`Arbitrary`.


## Deriving `Arbitrary`

The experimental [`proptest-derive` crate](../../proptest-derive/index.md) can
be used to automate implementing `Arbitrary` in common cases. For example, imagine we have a struct that represents a point in a 2-D coordinate space:
```rust
#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}
```
This struct has the property that any pair of valid `i32`s can make a valid `Point`, so that is perfect for using `#[derive(Arbitrary)]`. 

## Manual `Arbitrary` implementations

Sometimes, however, there are extra constraints that your type has, which the derive macro can't understand. In these cases, you'll need to implement `Arbitrary` for your type manually.

For example, consider this struct which represents a range (note, the derive API is can actually represent this case, it's just an example):
```rust
#[derive(Debug)]
struct Range {
    lower: i32,
    upper: i32,
}

impl Range {
    pub fn new(lower: i32, upper: i32) -> Option<Self> {
        if lower <= upper {
            Some(Self { lower, upper })
        } else {
            None
        }
    }
}
```
This struct has an invariant: `lower <= upper`. However, if we derive an `Arbitrary` implementation naively, it might generate `Range { lower: 1, upper: 0 }`.

Instead, we can write a manual implementation:
```rust
impl Arbitrary for Range {
    type Parameters = ();
    type Strategy = FilterMap<StrategyFor<(i32, i32)>, fn((i32, i32)) -> Option<Self>>;
  
    fn arbitrary_with(_parameters: Self::Parameters) -> Self::Strategy {
        any::<(i32, i32)>()  // generate 2 arbitrary i32s
            .prop_map(|(a, b)| {
                let (lower, upper) = if a < b {
                    (a, b)
                } else {
                    (b, a)
                };
                Range::new(lower, upper).unwrap()
            })
    }
}
```
Here, there are three items we need to define:
 - `type Parameters` - the type of any parameters to `arbitrary_with`. Here (and in many cases), we don't need this, so `()` is used.
 - `type Strategy` - the type of the strategy produced
 - `fn arbitrary_with` - the code that creates the canonical `Strategy` for this type

It's important to consider what type you want to use for `Strategy`. Here, we explicitly write the type out. This uses static dispatch, which is often faster and easier to optimize, but has a few downsides:
 - you need to write out the type of the strategy. Even for this small function, it's a pretty lengthy function signature. In the worst case, it's impossible, since some types are unnameable (e.g. closures which capture their environment)
 - it makes the implementation of `arbitrary_with` a part of your public API signature (if you expose `Arbitrary` impls in general from your crate). This means that changes to the implementation may require a breaking change.

There are a couple of ways around this:
 - heap-allocate the strategy by: 
   - returning `BoxedStrategy<T>`
   - calling `.boxed()` on the strategy before returning it
 - use the nightly-only `#![feature(type_alias_impl_trait)]`:
```rust
type RangeStrategy = impl Strategy<Value = Range>;

impl Arbitrary for Range {
    type Parameters = ();
    type Strategy = RangeStrategy;
    // ...
}
```

Using `BoxedStrategy` will incur some performance penalty relating to a heap allocation as well as dynamic dispatch, but it works on stable (as of November 2022).

