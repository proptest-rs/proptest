//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Statistics about the distribution of generated test inputs.
//!
//! Property tests are only as good as the inputs they actually exercise. A
//! filter that rejects more than intended, or a generator that almost never
//! produces the "interesting" case, can silently turn a strong property into
//! a vacuous one. The functions in this module — modelled after
//! `collect`/`classify`/`aggregate`/`measure` from QuickCheck and PropEr —
//! let a test report what kinds of inputs it actually saw, so such problems
//! become visible.
//!
//! Call [`label`], [`classify`], [`collect`] or [`measure`] from inside a
//! test case. When the test run completes successfully, proptest prints a
//! summary of the recorded statistics to stderr, e.g.:
//!
//! ```text
//! proptest: test case statistics (256 cases):
//! proptest:    52.34% (134/256): non-trivial
//! proptest:    47.66% (122/256): trivial
//! proptest: measurements:
//! proptest:   input length: count 256, min 0, max 98, mean 24.71
//! ```
//!
//! (Note that `cargo test` captures stderr of passing tests; run with
//! `--nocapture` to see the summary.)
//!
//! Labels use per-case semantics: recording the same label several times
//! within one test case counts that case once, and the reported percentage is
//! the fraction of test cases carrying the label. [`measure`], by contrast,
//! records every sample.
//!
//! Statistics are aggregated over *passing* test cases of the normal
//! generation phase only; test executions performed while shrinking a failure
//! are ignored. The aggregated data is also available programmatically via
//! [`TestRunner::statistics()`](crate::test_runner::TestRunner::statistics).
//!
//! # Example
//!
//! ```
//! use proptest::prelude::*;
//! use proptest::statistics;
//!
//! proptest! {
//!     # /*
//!     #[test]
//!     # */
//!     fn parses_all_inputs(v in prop::collection::vec(any::<u8>(), 0..100)) {
//!         statistics::classify(v.is_empty(), "empty");
//!         statistics::collect("length mod 3", v.len() % 3);
//!         statistics::measure("input length", v.len() as f64);
//!         // ... the actual property ...
//!     }
//! }
//! # parses_all_inputs();
//! ```
//!
//! # Caveats
//!
//! Recording uses thread-local state: calls made from threads spawned by the
//! test body are not captured. When the `fork` config is enabled, statistics
//! are aggregated (and printed) by the child process actually running the
//! test cases. Outside of a proptest run all functions are no-ops, so code
//! under test may call them unconditionally. In `no_std` builds the functions
//! exist but do nothing.

#[cfg(feature = "std")]
mod internal {
    use crate::std_facade::{fmt, BTreeMap, BTreeSet, Cow, Vec};
    use std::cell::RefCell;

    thread_local! {
        /// Statistics being recorded for the currently executing test case,
        /// if any. `None` whenever no test case is being recorded (outside a
        /// run, or while shrinking).
        static CURRENT_CASE: RefCell<Option<CaseStatistics>> =
            const { RefCell::new(None) };
    }

    /// Statistics recorded during a single test case.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct CaseStatistics {
        labels: BTreeSet<Cow<'static, str>>,
        measurements: BTreeMap<Cow<'static, str>, Vec<f64>>,
    }

    /// Record `label` for the current test case.
    ///
    /// The summary printed at the end of a successful run shows, for each
    /// label, the percentage of test cases which recorded it. Recording the
    /// same label more than once in one test case counts the case only once.
    ///
    /// Does nothing if called outside a proptest test case.
    pub fn label(label: impl Into<Cow<'static, str>>) {
        CURRENT_CASE.with(|c| {
            if let Some(case) = c.borrow_mut().as_mut() {
                case.labels.insert(label.into());
            }
        });
    }

    /// Record `label` for the current test case if `condition` holds.
    ///
    /// Equivalent to `if condition { label(label) }`; see [`label`].
    pub fn classify(condition: bool, label: impl Into<Cow<'static, str>>) {
        if condition {
            self::label(label);
        }
    }

    /// Record the value of some feature of the current test case, grouping
    /// cases by value.
    ///
    /// This is sugar for [`label`] with the label `"{name} = {value:?}"` and
    /// reports what fraction of test cases observed each distinct value, e.g.
    /// `collect("length", v.len())`. Best used with features of low
    /// cardinality.
    pub fn collect(name: impl Into<Cow<'static, str>>, value: impl fmt::Debug) {
        label(format!("{} = {:?}", name.into(), value));
    }

    /// Record one numeric sample under `name`.
    ///
    /// The summary printed at the end of a successful run shows the count,
    /// minimum, maximum and mean of all samples recorded under each name.
    /// Unlike [`label`], every call records a sample, so a test case may
    /// contribute several samples to the same name.
    ///
    /// Does nothing if called outside a proptest test case.
    pub fn measure(name: impl Into<Cow<'static, str>>, value: impl Into<f64>) {
        CURRENT_CASE.with(|c| {
            if let Some(case) = c.borrow_mut().as_mut() {
                case.measurements
                    .entry(name.into())
                    .or_default()
                    .push(value.into());
            }
        });
    }

    /// Run `body` while recording statistics into a fresh per-case
    /// accumulator, and return the body's result together with whatever was
    /// recorded.
    ///
    /// Any previously installed accumulator is restored afterwards (also on
    /// panic), so nested `TestRunner`s record independently.
    pub(crate) fn record_case<R>(
        body: impl FnOnce() -> R,
    ) -> (R, CaseStatistics) {
        struct RestoreGuard(Option<CaseStatistics>);
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                CURRENT_CASE.with(|c| *c.borrow_mut() = self.0.take());
            }
        }

        let prev = CURRENT_CASE
            .with(|c| c.borrow_mut().replace(CaseStatistics::default()));
        let _guard = RestoreGuard(prev);
        let result = body();
        let stats = CURRENT_CASE
            .with(|c| c.borrow_mut().take())
            .unwrap_or_default();
        (result, stats)
    }

    /// Summary of all numeric samples recorded under one name via
    /// [`measure`].
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct MeasurementSummary {
        count: u64,
        sum: f64,
        min: f64,
        max: f64,
    }

    impl Default for MeasurementSummary {
        fn default() -> Self {
            MeasurementSummary {
                count: 0,
                sum: 0.0,
                min: f64::INFINITY,
                max: f64::NEG_INFINITY,
            }
        }
    }

    impl MeasurementSummary {
        fn add(&mut self, sample: f64) {
            self.count += 1;
            self.sum += sample;
            self.min = self.min.min(sample);
            self.max = self.max.max(sample);
        }

        /// The number of recorded samples.
        pub fn count(&self) -> u64 {
            self.count
        }

        /// The sum of all recorded samples.
        pub fn sum(&self) -> f64 {
            self.sum
        }

        /// The smallest recorded sample.
        pub fn min(&self) -> f64 {
            self.min
        }

        /// The largest recorded sample.
        pub fn max(&self) -> f64 {
            self.max
        }

        /// The arithmetic mean of all recorded samples.
        pub fn mean(&self) -> f64 {
            if 0 == self.count {
                0.0
            } else {
                self.sum / self.count as f64
            }
        }
    }

    /// Statistics aggregated over all passing test cases of one test run.
    ///
    /// Obtained from
    /// [`TestRunner::statistics()`](crate::test_runner::TestRunner::statistics).
    /// The `Display` implementation renders the human-readable summary that
    /// proptest prints after a successful run.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct RunStatistics {
        cases: u32,
        labels: BTreeMap<Cow<'static, str>, u32>,
        measurements: BTreeMap<Cow<'static, str>, MeasurementSummary>,
    }

    impl RunStatistics {
        /// The number of test cases over which statistics were aggregated.
        pub fn cases(&self) -> u32 {
            self.cases
        }

        /// For each label recorded via [`label`], [`classify`] or
        /// [`collect`], the number of test cases that recorded it.
        pub fn labels(&self) -> &BTreeMap<Cow<'static, str>, u32> {
            &self.labels
        }

        /// For each name sampled via [`measure`], the summary of all
        /// recorded samples.
        pub fn measurements(
            &self,
        ) -> &BTreeMap<Cow<'static, str>, MeasurementSummary> {
            &self.measurements
        }

        /// Whether no statistics at all were recorded during the run.
        pub fn is_empty(&self) -> bool {
            self.labels.is_empty() && self.measurements.is_empty()
        }

        /// Fold the statistics of one passing test case into the aggregate.
        pub(crate) fn merge_case(&mut self, case: CaseStatistics) {
            self.cases += 1;
            for label in case.labels {
                *self.labels.entry(label).or_insert(0) += 1;
            }
            for (name, samples) in case.measurements {
                let summary = self.measurements.entry(name).or_default();
                for sample in samples {
                    summary.add(sample);
                }
            }
        }
    }

    impl fmt::Display for RunStatistics {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            if !self.labels.is_empty() {
                writeln!(f, "test case statistics ({} cases):", self.cases)?;
                // Most frequent labels first; ties in label order.
                let mut sorted: Vec<(&Cow<'static, str>, &u32)> =
                    self.labels.iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                for (label, count) in sorted {
                    let percent = if 0 == self.cases {
                        0.0
                    } else {
                        *count as f64 * 100.0 / self.cases as f64
                    };
                    writeln!(
                        f,
                        "  {:>6.2}% ({}/{}): {}",
                        percent, count, self.cases, label
                    )?;
                }
            }
            if !self.measurements.is_empty() {
                writeln!(f, "measurements:")?;
                for (name, summary) in &self.measurements {
                    writeln!(
                        f,
                        "  {}: count {}, min {}, max {}, mean {:.2}",
                        name,
                        summary.count(),
                        summary.min(),
                        summary.max(),
                        summary.mean()
                    )?;
                }
            }
            Ok(())
        }
    }
}

#[cfg(feature = "std")]
pub(crate) use internal::record_case;
#[cfg(feature = "std")]
pub use internal::{
    classify, collect, label, measure, MeasurementSummary, RunStatistics,
};

#[cfg(not(feature = "std"))]
mod internal {
    //! No-op stubs so that code calling into the statistics API still
    //! compiles in `no_std` builds, where the thread-local recording
    //! machinery is unavailable.

    /// Does nothing in `no_std` builds. See the `std` documentation.
    pub fn label<L>(_label: L) {}

    /// Does nothing in `no_std` builds. See the `std` documentation.
    pub fn classify<L>(_condition: bool, _label: L) {}

    /// Does nothing in `no_std` builds. See the `std` documentation.
    pub fn collect<N, V>(_name: N, _value: V) {}

    /// Does nothing in `no_std` builds. See the `std` documentation.
    pub fn measure<N, V>(_name: N, _value: V) {}
}

#[cfg(not(feature = "std"))]
pub use internal::{classify, collect, label, measure};

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::std_facade::string::ToString;
    use crate::strategy::Just;
    use crate::test_runner::{Config, TestCaseError, TestRunner};

    fn config() -> Config {
        Config {
            failure_persistence: None,
            ..Config::default()
        }
    }

    #[test]
    fn noop_outside_test_case() {
        label("ignored");
        classify(true, "ignored");
        collect("ignored", 42);
        measure("ignored", 1.0);
    }

    #[test]
    fn aggregates_labels_over_all_cases() {
        let mut runner = TestRunner::new(config());
        runner
            .run(&(0u32..100), |v| {
                label("ran");
                classify(v < 100, "in range");
                classify(v >= 100, "out of range");
                collect("parity", v % 2);
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        let cases = stats.cases();
        assert_eq!(Config::default().cases, cases);
        assert_eq!(Some(&cases), stats.labels().get("ran"));
        assert_eq!(Some(&cases), stats.labels().get("in range"));
        assert_eq!(None, stats.labels().get("out of range"));
        assert_eq!(
            cases,
            stats.labels()["parity = 0"] + stats.labels()["parity = 1"]
        );
    }

    #[test]
    fn duplicate_labels_count_once_per_case() {
        let mut runner = TestRunner::new(config());
        runner
            .run(&Just(()), |_| {
                label("twice");
                label("twice");
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        assert_eq!(Some(&stats.cases()), stats.labels().get("twice"));
    }

    #[test]
    fn measure_summarizes_samples() {
        let mut runner = TestRunner::new(config());
        runner
            .run(&Just(5u32), |v| {
                measure("v", v);
                measure("v", v * 2);
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        let summary = stats.measurements()["v"];
        assert_eq!(2 * stats.cases() as u64, summary.count());
        assert_eq!(5.0, summary.min());
        assert_eq!(10.0, summary.max());
        assert_eq!(7.5, summary.mean());
        assert_eq!(15.0 * stats.cases() as f64, summary.sum());
    }

    #[test]
    fn shrinking_does_not_pollute_statistics() {
        let calls = core::cell::Cell::new(0u32);
        let mut runner = TestRunner::new(config());
        let result = runner.run(&(0u32..1000), |_| {
            label("ran");
            calls.set(calls.get() + 1);
            if calls.get() <= 5 {
                Ok(())
            } else {
                Err(TestCaseError::fail("always fails from the 6th call"))
            }
        });
        assert!(result.is_err());

        // Only the five passing cases from the generation phase count; the
        // failing case and all shrink executions (which also record the
        // label) are excluded.
        assert!(calls.get() > 6, "test was not re-run while shrinking");
        let stats = runner.statistics();
        assert_eq!(5, stats.cases());
        assert_eq!(Some(&5), stats.labels().get("ran"));
    }

    #[test]
    fn rejected_cases_are_not_counted() {
        let mut runner = TestRunner::new(config());
        runner
            .run(&(0u32..10), |v| {
                label("ran");
                prop_assume!(v < 5);
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        assert_eq!(Some(&stats.cases()), stats.labels().get("ran"));
    }

    #[test]
    fn empty_when_nothing_recorded() {
        let mut runner = TestRunner::new(config());
        runner.run(&Just(()), |_| Ok(())).unwrap();
        assert!(runner.statistics().is_empty());
        assert_eq!(Config::default().cases, runner.statistics().cases());
    }

    #[test]
    fn display_renders_summary() {
        let mut runner = TestRunner::new(Config {
            cases: 4,
            ..config()
        });
        runner
            .run(&Just(()), |_| {
                label("always");
                measure("x", 2.0);
                Ok(())
            })
            .unwrap();

        let rendered = runner.statistics().to_string();
        assert_eq!(
            "test case statistics (4 cases):\n\
             \x20 100.00% (4/4): always\n\
             measurements:\n\
             \x20 x: count 4, min 2, max 2, mean 2.00\n",
            rendered
        );
    }

    #[test]
    fn labels_sorted_by_frequency_in_display() {
        let mut runner = TestRunner::new(config());
        runner
            .run(&(0u32..4), |v| {
                label("common");
                classify(0 == v, "rare");
                Ok(())
            })
            .unwrap();

        let rendered = runner.statistics().to_string();
        let common_at = rendered.find("common").unwrap();
        let rare_at = rendered.find("rare").unwrap();
        assert!(
            common_at < rare_at,
            "expected 'common' first:\n{}",
            rendered
        );
    }

    #[test]
    fn nested_runners_record_independently() {
        let mut runner = TestRunner::new(Config {
            cases: 8,
            ..config()
        });
        runner
            .run(&Just(()), |_| {
                label("outer");
                let mut inner = TestRunner::new(Config {
                    cases: 2,
                    ..Config {
                        failure_persistence: None,
                        ..Config::default()
                    }
                });
                inner
                    .run(&Just(()), |_| {
                        label("inner");
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(None, inner.statistics().labels().get("outer"));
                assert_eq!(Some(&2), inner.statistics().labels().get("inner"));
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        assert_eq!(Some(&8), stats.labels().get("outer"));
        assert_eq!(None, stats.labels().get("inner"));
    }

    #[test]
    fn works_in_proptest_macro() {
        proptest!(config(), |(v in 0u32..10)| {
            classify(v < 5, "small");
            prop_assert!(v < 10);
        });
    }
}
