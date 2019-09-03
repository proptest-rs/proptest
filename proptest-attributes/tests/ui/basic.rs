//-
// Copyright 2019, 2020 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use proptest::{strategy::Strategy, string::string_regex};
use proptest_attributes::proptest;

#[derive(Debug, Clone, PartialEq)]
struct FunctionArn(String);

fn gen_function_arn() -> impl Strategy<Value = FunctionArn> {
    let expr = "arn:aws:lambda:us-east-1:[0-9]{12}:function:custom-runtime";
    let arn = string_regex(expr).unwrap();
    arn.prop_map(FunctionArn)
}

#[proptest]
fn function_arn(#[strategy(gen_function_arn())] arn: FunctionArn) {
    let mut map = std::collections::HashMap::new();
    map.insert("arn", arn.clone());
    proptest::prop_assert_eq!(map.get("arn"), Some(&arn));
}

fn main() {}
