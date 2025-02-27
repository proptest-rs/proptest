use proc_macro::TokenStream;

mod property_test;

/// The `property_test` procedural macro simplifies the creation of property-based tests
/// using the `proptest` crate. This macro provides a more concise syntax for writing tests
/// that automatically generate test cases based on properties.
///
/// # Example
///
/// Using the `property_test` macro:
///
/// ```
/// # use proptest_macro::property_test;
/// #[property_test]
/// fn foo(x: i32) {
///     assert_eq!(x, x);
/// }
/// ```
///
/// is roughly equivalent to:
///
/// ```ignore
/// proptest! {
///     #[test]
///     fn foo(x in any::<i32>()) {
///         assert_eq!(x, x);
///     }
/// }
/// ```
///
/// # Details
///
/// The `property_test` macro is used to define property-based tests, where the parameters
/// of the test function are automatically generated by `proptest`. The macro takes care
/// of setting up the test harness and generating input values, allowing the user to focus
/// on writing the test logic.
///
/// ## Attributes
///
/// The `property_test` macro can take an optional `config` attribute, which allows you to
/// customize the configuration of the `proptest` runner.
///
/// E.g. running 100 cases:
///
/// ```rust,ignore
/// #[property_test(config = "ProptestConfig { cases: 100, .. ProptestConfig::default() }")]
/// fn foo(x: i32) {
///     assert_eq!(x, x);
/// }
/// ```
///
/// ## Custom strategies
///
/// By default, [`property_test`] will use the `Arbitrary` impl for parameters. However, you can
/// provide a custom `Strategy` with `#[strategy = <expr>]` on an argument:
///
/// ```
/// # use proptest_macro::property_test;
/// #[property_test]
/// fn foo(#[strategy = "[0-9]*"] s: String) {
///     for c in s.chars() {
///         assert!(c.is_numeric());
///     }
/// }
/// ```
/// Multiple `#[strategy = <expr>]` attributes on an argument are not allowed.
///
#[proc_macro_attribute]
pub fn property_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    property_test::property_test(attr.into(), item.into()).into()
}
