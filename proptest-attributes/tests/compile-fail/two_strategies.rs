//-
// Copyright 2019, 2020 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use proptest_attributes::proptest;

#[proptest]
fn function_arn(
    #[strategy("[A-Z]{1}")]
    #[strategy("[a-z]{1}")]
    arg: String,
) {
}

fn main() {}
