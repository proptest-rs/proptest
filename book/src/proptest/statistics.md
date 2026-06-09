# Test Case Statistics

Property tests are only as good as the inputs they actually exercise. A
filter that rejects more than intended, or a generator that almost never
produces the "interesting" case, can silently turn a strong property into a
vacuous one: the test passes, but mostly because it never tested anything
hard.

The [`proptest::statistics`](https://docs.rs/proptest/latest/proptest/statistics/index.html)
module — modelled after the `collect`/`classify`/`aggregate`/`measure`
facilities of QuickCheck and PropEr — lets a test report what kinds of inputs
it actually saw, so such problems become visible.

## Recording statistics

Call the recording functions from inside the test body:

```rust
use proptest::prelude::*;
use proptest::statistics;

proptest! {
    #[test]
    fn parses_all_inputs(v in prop::collection::vec(any::<u8>(), 0..100)) {
        // Percentage of cases for which a condition held:
        statistics::classify(v.is_empty(), "empty");
        // Group cases by the value of some feature:
        statistics::collect("length mod 3", v.len() % 3);
        // Numeric samples, reported as count/min/max/mean:
        statistics::measure("input length", v.len() as f64);
        // Unconditional label (typically used inside `if`/`match` arms):
        statistics::label("ran");

        // ... the actual property ...
    }
}
```

When the test run completes successfully, proptest prints a summary to
stderr:

```text
proptest: test case statistics (256 cases):
proptest:   100.00% (256/256): ran
proptest:    35.55% (91/256): length mod 3 = 0
proptest:    33.20% (85/256): length mod 3 = 1
proptest:    31.25% (80/256): length mod 3 = 2
proptest:     1.17% (3/256): empty
proptest: measurements:
proptest:   input length: count 256, min 0, max 99, mean 49.04
```

Note that `cargo test` captures the stderr of passing tests, so to see the
summary run

```text
cargo test parses_all_inputs -- --nocapture
```

## Enforcing coverage

Reading the summary is useful, but nobody reads stderr in CI. `cover` turns
a label into an enforceable requirement: if fewer than the given percentage
of test cases record it, the run fails.

```rust
use proptest::prelude::*;
use proptest::statistics;

proptest! {
    #[test]
    fn handles_collisions(v in prop::collection::vec(0u8..10, 0..10)) {
        let has_duplicates = (1..v.len()).any(|i| v[i..].contains(&v[i - 1]));
        // At least 30% of the generated vectors must contain a duplicate,
        // or the test fails even though every case passed.
        statistics::cover(has_duplicates, 30.0, "duplicate elements");

        // ... the actual property ...
    }
}
```

An unmet requirement aborts the test after all cases have run, with the
distribution summary printed for diagnosis:

```text
proptest: test case statistics (256 cases):
proptest:     1.17% (3/256): duplicate elements
Test aborted: Coverage requirements not met:
  coverage of "duplicate elements" was 1.17% (3/256 cases), required at least 30%
```

The requirement is registered even if the condition never holds, so 0%
coverage still fails. A failing test case always takes precedence over a
coverage failure.

Because coverage is measured on a random sample, a run can fail by chance if
the threshold sits close to the actual rate. Leave a generous margin (e.g.
require 30% when you expect 50%) or increase the number of test cases.

## Semantics

* Labels recorded by `label`, `classify` and `collect` use *per-case*
  semantics: recording the same label several times within one test case
  counts that case once, and the reported percentage is the fraction of test
  cases carrying the label.
* `measure` records *every* sample, so one test case may contribute several
  samples to the same name.
* Statistics are aggregated over passing test cases of the normal generation
  phase only. Rejected cases (e.g. via `prop_assume!`) and test executions
  performed while shrinking a failure are excluded.
* Outside of a proptest run all recording functions are no-ops, so code
  shared with other tests may call them unconditionally.

## Programmatic access

When driving a [`TestRunner`](https://docs.rs/proptest/latest/proptest/test_runner/struct.TestRunner.html)
by hand, the aggregated data is available from
[`TestRunner::statistics()`](https://docs.rs/proptest/latest/proptest/test_runner/struct.TestRunner.html#method.statistics):

```rust
use proptest::statistics;
use proptest::test_runner::{Config, TestRunner};

let mut runner = TestRunner::new(Config::default());
runner.run(&(0u32..100), |v| {
    statistics::classify(v < 50, "small");
    Ok(())
}).unwrap();

let stats = runner.statistics();
println!("{} of {} cases were small",
         stats.labels()["small"], stats.cases());
```

For asserting a minimum input quality in CI, prefer `cover` (above); the
programmatic API is for everything beyond a simple threshold, such as
comparing distributions between runs. `RunStatistics::unmet_coverage()`
exposes the result of the coverage check itself.

## Caveats

* Recording uses thread-local state, so calls made from threads spawned by
  the test body are not captured.
* When [forking](forking.md) is enabled, statistics are aggregated and
  printed by the child process actually running the test cases.
* In `no_std` builds the recording functions exist but do nothing.
