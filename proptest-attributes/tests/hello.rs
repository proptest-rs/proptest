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

#[proptest(gen_valid_date())]
fn parses_all_valid_dates(s: String) {
    parse_date(&s).unwrap();
}

#[proptest(gen_all_utf8())]
fn doesnt_crash(s: String) {
    parse_date(&s);
}

#[proptest(gen_parsed_date())]
fn parses_date_back_to_original(date_tuple: (u32, u32, u32)) {
    let (y, m, d) = date_tuple;
    let (y2, m2, d2) =
        parse_date(&format!("{:04}-{:02}-{:02}", y, m, d)).unwrap();
    // prop_assert_eq! is basically the same as assert_eq!, but doesn't
    // cause a bunch of panic messages to be printed on intermediate
    // test failures. Which one to use is largely a matter of taste.
    prop_assert_eq!((y, m, d), (y2, m2, d2));
}
