use proptest::{strategy::Strategy, string::string_regex};
use proptest_attributes::proptest;

fn gen_date() -> impl Strategy<Value = String> {
    let expr = "[0-9]{4}-[0-9]{2}-[0-9]{2}";
    string_regex(expr).unwrap()
}

#[proptest(gen_date())]
fn parse_date(date: String, _second_arg: String) {}
