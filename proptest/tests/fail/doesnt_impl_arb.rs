fn main() {}

#[derive(Debug)]
struct NotArbitrary;

#[proptest::property_test]
fn my_test(foo: NotArbitrary) {}

