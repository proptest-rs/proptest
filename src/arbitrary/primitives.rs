//-
// Copyright 2017, 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Arbitrary implementations for primitive types.

use bool;
use char;
use num::{isize, usize, f32, f64, i16, i32, i64, i8, u16, u32, u64, u8};

arbitrary!(
    bool, f32, f64,
    i8, i16, i32, i64, isize,
    u8, u16, u32, u64, usize
);

/*
TODO: deal with this...

#[cfg(feature = "unstable")]
arbitrary!(u128, i128);
*/

arbitrary!(char, char::CharStrategy<'static>; char::any());

#[cfg(test)]
mod test {
    no_panic_test!(
        bool => bool,
        char => char,
        f32 => f32, f64 => f64,
        isize => isize, usize => usize,
        i8 => i8, i16 => i16, i32 => i32, i64 => i64,
        u8 => u8, u16 => u16, u32 => u32, u64 => u64
    );
}