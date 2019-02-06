// The point of this test is to check that `#[derive(Arbitrary)]` has
// no effect and that `#[cfg(test)]` is on the implementation generated.

#[macro_use]
extern crate proptest_derive;

extern crate proptest;
use proptest::prelude::Arbitrary;

#[derive(Debug, Arbitrary)]
struct Foo;

fn assert_arbitrary<T: Arbitrary>() {}

fn test() {
    assert_arbitrary::<Foo>(); //~ Arbitrary` is not satisfied [E0277]
}
