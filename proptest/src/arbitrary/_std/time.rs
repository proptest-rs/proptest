//-
// Copyright 2017, 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Arbitrary implementations for `std::time`.

use core::ops::Range;
use std::time::*;

use crate::arbitrary::*;
use crate::num;
use crate::strategy::statics::{self, static_map};

arbitrary!(Duration, SMapped<(u64, u32), Self>;
    static_map(any::<(u64, u32)>(), |(a, b)| {
        let carry = u64::from(b / 1_000_000_000);
        let b = if a.checked_add(carry).is_none() {
            b % 1_000_000_000
        } else {
            b
        };
        Duration::new(a, b)
    })
);

// Instant::now() "never" returns the same Instant, so no shrinking may occur!
arbitrary!(Instant; Self::now());

arbitrary!(
    // We can't use `any::<Duration>()` because the addition to `SystemTime`
    // can overflow and panic. To be conservative, we only allow seconds to go
    // to i32::MAX since a certain popular OS still uses `i32` to represent the
    // seconds counter.
    SystemTime, statics::Map<(num::i32::Any, Range<u32>),
                             fn ((i32, u32)) -> SystemTime>;
    static_map((num::i32::ANY, 0..1_000_000_000u32),
                |(sec, ns)| {
                    if sec >= 0 {
                        UNIX_EPOCH + Duration::new(sec as u64, ns)
                    } else {
                        UNIX_EPOCH - Duration::new((-(sec as i64)) as u64, ns)
                    }
                })
);

#[cfg(test)]
mod test {
    no_panic_test!(
        duration => Duration,
        instant  => Instant,
        system_time => SystemTime
    );

    #[test]
    fn duration_tolerates_nanosecond_carry_at_max_seconds() {
        use crate::strategy::{Strategy, ValueTree};
        use crate::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
        use std::time::Duration;

        let mut runner = TestRunner::new_with_rng(
            Config::default(),
            TestRng::from_seed(RngAlgorithm::PassThrough, &[0xFF; 12]),
        );
        let value = crate::arbitrary::any::<Duration>()
            .new_tree(&mut runner)
            .unwrap()
            .current();
        assert_eq!(Duration::new(u64::MAX, u32::MAX % 1_000_000_000), value);
    }
}
