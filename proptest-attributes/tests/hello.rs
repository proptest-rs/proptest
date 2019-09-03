//-
// Copyright 2019, 2020 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use proptest::{
    prelude::*, prop_assert_eq, strategy::Strategy, string::string_regex,
};
use proptest_attributes::proptest;

fn parse_date(s: &str) -> Option<(u32, u32, u32)> {
    if 10 != s.len() {
        return None;
    }

    // NEW: Ignore non-ASCII strings so we don't need to deal with Unicode.
    if !s.is_ascii() {
        return None;
    }

    if "-" != &s[4..5] || "-" != &s[7..8] {
        return None;
    }

    let year = &s[0..4];
    let month = &s[5..7];
    let day = &s[8..10];

    year.parse::<u32>().ok().and_then(|y| {
        month
            .parse::<u32>()
            .ok()
            .and_then(|m| day.parse::<u32>().ok().map(|d| (y, m, d)))
    })
}

fn gen_valid_date() -> impl Strategy<Value = String> {
    let expr = "[0-9]{4}-[0-9]{2}-[0-9]{2}";
    string_regex(expr).unwrap()
}

fn gen_all_utf8() -> impl Strategy<Value = String> {
    let expr = "\\PC*";
    string_regex(expr).unwrap()
}

prop_compose! {
  fn gen_parsed_date()(year in 0u32..10000, month in 1u32..13, day in 1u32..32) -> (u32, u32, u32) {
    (year, month, day)
  }
}

#[proptest]
fn parses_all_valid_dates(#[strategy(gen_valid_date())] s: String) {
    parse_date(&s).unwrap();
}

#[proptest]
fn doesnt_crash(#[strategy(gen_all_utf8())] s: String) {
    parse_date(&s);
}

#[proptest]
fn parses_date_back_to_original(
    #[strategy(gen_parsed_date())] date_tuple: (u32, u32, u32),
) {
    let (y, m, d) = date_tuple;
    let (y2, m2, d2) =
        parse_date(&format!("{:04}-{:02}-{:02}", y, m, d)).unwrap();
    // prop_assert_eq! is basically the same as assert_eq!, but doesn't
    // cause a bunch of panic messages to be printed on intermediate
    // test failures. Which one to use is largely a matter of taste.
    prop_assert_eq!((y, m, d), (y2, m2, d2));
}

fn checked_add(left: u64, right: u64) -> Option<u64> {
    left.checked_add(right)
}

#[proptest]
fn default_strategy(first: u64, second: u64) {
    let sum = checked_add(first, second);

    assert_eq!(sum, first.checked_add(second));
}

#[proptest]
fn multiple_strategies(
    #[strategy("[A-z]{10}")] first: String,
    #[strategy("[A-z]{5}")] second: String,
) {
    assert_eq!(first.len(), 10);
    assert_eq!(second.len(), 5);
}

#[proptest]
fn built_in_strategies(
    #[strategy(prop::collection::vec(1..10, 5))] numbers: Vec<u32>,
) {
    assert_eq!(numbers.len(), 5);
}
