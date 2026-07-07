//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Side-by-side comparison of the ValueTree and choice-tape shrink
//! engines: same strategies, same failure predicates, same RNG seed.
//! Prints the minimal counterexample each engine finds and how many
//! times it ran the test function (generation + shrinking).
//!
//! Run with: cargo run --example shrink-quality

use std::cell::Cell;
use std::fmt::Debug;

use proptest::prelude::*;
use proptest::test_runner::{
    Config, RngAlgorithm, ShrinkEngine, TestError, TestRng, TestRunner,
};

const SEED: [u8; 32] = [
    0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed,
    0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed,
    0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed, 0x5e, 0xed,
];

fn run_case<S: Strategy>(
    engine: ShrinkEngine,
    strategy: &S,
    fail: &impl Fn(&S::Value) -> bool,
) -> (String, u64)
where
    S::Value: Debug,
{
    let calls = Cell::new(0u64);
    let mut runner = TestRunner::new_with_rng(
        Config {
            shrink_engine: engine,
            failure_persistence: None,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &SEED),
    );
    let result = runner.run(strategy, |v| {
        calls.set(calls.get() + 1);
        if fail(&v) {
            Err(TestCaseError::fail("dogfood failure"))
        } else {
            Ok(())
        }
    });
    match result {
        Err(TestError::Fail(_, value)) => (format!("{:?}", value), calls.get()),
        Err(other) => (format!("ABORT: {:?}", other), calls.get()),
        Ok(()) => ("NO FAILURE FOUND".to_owned(), calls.get()),
    }
}

fn compare<S: Strategy>(
    name: &str,
    strategy: S,
    fail: impl Fn(&S::Value) -> bool,
) where
    S::Value: Debug,
{
    let (vt_value, vt_calls) = run_case(ShrinkEngine::ValueTree, &strategy, &fail);
    let (tp_value, tp_calls) = run_case(ShrinkEngine::Tape, &strategy, &fail);
    println!("{}", name);
    println!("  valuetree: {:<40} ({} test calls)", vt_value, vt_calls);
    println!("  tape:      {:<40} ({} test calls)", tp_value, tp_calls);
    println!();
}

fn main() {
    compare("f64 in 0.0..10.0, fail iff v >= 1.7", 0.0f64..10.0, |v| {
        *v >= 1.7
    });

    compare(
        "any-class f64, fail iff finite && v >= 1.7",
        proptest::num::f64::ANY,
        |v| v.is_finite() && *v >= 1.7,
    );

    compare(
        "i64 in 0..1_000_000, fail iff v >= 123_457",
        0i64..1_000_000,
        |v| *v >= 123_457,
    );

    compare(
        "pair (0..1000, 0..1000), fail iff a + b >= 100",
        (0i32..1000, 0i32..1000),
        |(a, b)| a + b >= 100,
    );

    compare(
        "vec(0..100, 0..20), fail iff len >= 3",
        proptest::collection::vec(0i32..100, 0..20),
        |v| v.len() >= 3,
    );

    compare(
        "vec(0..100, 0..20), fail iff sum >= 50",
        proptest::collection::vec(0i32..100, 0..20),
        |v| v.iter().sum::<i32>() >= 50,
    );

    compare(
        "(0..1000).prop_filter(even), fail iff v >= 100",
        (0i32..1000).prop_filter("even", |v| 0 == v % 2),
        |v| *v >= 100,
    );

    compare(
        "(1..=5).prop_flat_map(|n| 0..n*100), fail iff v >= 57",
        (1i32..=5).prop_flat_map(|n| 0..n * 100),
        |v| *v >= 57,
    );

    compare(
        "prop_oneof![10..20, 100..200], fail always",
        prop_oneof![10i32..20, 100i32..200],
        |_| true,
    );

    compare(
        "char in 'a'..='z', fail iff c >= 'd'",
        proptest::char::range('a', 'z'),
        |c| *c >= 'd',
    );

    compare(
        "string from [a-z]{0,20}, fail iff len >= 3",
        proptest::string::string_regex("[a-z]{0,20}").unwrap(),
        |s| s.len() >= 3,
    );

    compare(
        "btree_map(0..100 -> 0..100, 0..10), fail iff len >= 2",
        proptest::collection::btree_map(0i32..100, 0i32..100, 0..10),
        |m| m.len() >= 2,
    );
}
