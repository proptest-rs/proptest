use syn::{ItemFn, parse_str};

use crate::property_test::codegen;

#[test]
fn basic_derive_example() {
    let f: ItemFn = parse_str("fn foo(x: i32, y: String) {}").unwrap();
    let tokens = codegen::generate(f);

}
