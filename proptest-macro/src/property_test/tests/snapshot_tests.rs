use syn::{parse_str, ItemFn};

use crate::property_test::{codegen, options::Options};

#[test]
fn basic_derive_example() {
    let f: ItemFn =
        parse_str("fn foo(x: i32, y: String) { let x = 1; }").unwrap();
    let tokens = codegen::generate(f, Options::default());
    insta::assert_debug_snapshot!(tokens);
}
