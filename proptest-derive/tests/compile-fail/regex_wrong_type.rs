// Copyright 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[macro_use]
extern crate proptest_derive;
use proptest_derive::Arbitrary;

fn main() {}

fn make_regex() -> &'static str {
    "a|b"
}

// struct:

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T0 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T0 {
    // The derive expansion also produces this associated-type equality error.
    #[proptest(regex = "a+")]
    f0: (), //~ StrategyFromRegex` is not satisfied [E0277]
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T1 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T1 {
    #[proptest(regex("a*"))]
    f0: u8, //~ StrategyFromRegex` is not satisfied [E0277]
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T2 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T2 {
    #[proptest(regex(make_regex))]
    f0: Vec<u16>, //~ StrategyFromRegex` is not satisfied [E0277]
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T3 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T3(
    #[proptest(regex = "a+")] (), //~ StrategyFromRegex` is not satisfied [E0277]
);

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T4 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T4(
    #[proptest(regex("a*"))] u8, //~ StrategyFromRegex` is not satisfied [E0277]
);

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T5 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
struct T5(
    #[proptest(regex(make_regex))] Vec<u16>, //~ StrategyFromRegex` is not satisfied [E0277]
);

// enum:

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T6 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T6 {
    V0 {
        #[proptest(regex = "a+")]
        f0: (), //~ StrategyFromRegex` is not satisfied [E0277]
    },
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T7 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T7 {
    V0 {
        #[proptest(regex("a*"))]
        f0: u8, //~ StrategyFromRegex` is not satisfied [E0277]
    },
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T8 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T8 {
    V0 {
        #[proptest(regex(make_regex))]
        f0: Vec<u16>, //~ StrategyFromRegex` is not satisfied [E0277]
    },
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T9 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T9 {
    V0(
        #[proptest(regex = "a+")] (), //~ StrategyFromRegex` is not satisfied [E0277]
    ),
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T10 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T10 {
    V0(
        #[proptest(regex("a*"))] u8, //~ StrategyFromRegex` is not satisfied [E0277]
    ),
}

#[derive(Debug, Arbitrary)] //~ StrategyFromRegex` is not satisfied [E0277]
                            //~| `<T11 as Arbitrary>::Strategy == _` [E0271]
                            //~| is not well-formed
enum T11 {
    V0(
        #[proptest(regex(make_regex))] Vec<u16>, //~ StrategyFromRegex` is not satisfied [E0277]
    ),
}
