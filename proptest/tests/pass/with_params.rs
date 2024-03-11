fn main() {}

#[proptest::property_test(config = proptest::test_runner::Config {
    cases = 10,
    ..Default::default()
})]
fn my_test(x: i32) {
    assert_eq!(x, x);
}
