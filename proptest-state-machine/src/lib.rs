//-
// Copyright 2023 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Strategies and test runners for Proptest State Machine tests.
//!
//! Please refer to the Proptest Book chapter "State Machine testing" to learn
//! when and how to use this and how it's made.

pub mod strategy;
pub mod test_runner;

pub use strategy::*;
pub use test_runner::*;
