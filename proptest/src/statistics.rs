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
//! test case. When all test cases pass, proptest prints a summary of the
//! recorded statistics to stderr, e.g.:
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
//! [`cover`] turns a label into an enforceable requirement: if fewer than the
//! given percentage of test cases record it, the run fails. This catches
//! generators that silently stop producing the inputs a property needs.
//! Setting
//! [`Config::coverage_confidence`](crate::test_runner::Config::coverage_confidence)
//! upgrades the check to a sequential statistical test (like QuickCheck's
//! `checkCoverage`) that runs as many cases as the decision needs and cannot
//! fail by bad luck; see [`CoverageConfidence`].
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
        /// Accumulator for the test case currently being recorded, or
        /// `None` while nothing is recording (no run in progress, or
        /// shrinking).
        static CURRENT_CASE: RefCell<Option<CaseStatistics>> =
            const { RefCell::new(None) };
    }

    /// Statistics recorded during a single test case.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct CaseStatistics {
        labels: BTreeSet<Cow<'static, str>>,
        measurements: BTreeMap<Cow<'static, str>, Vec<f64>>,
        coverage_requirements: BTreeMap<Cow<'static, str>, f64>,
    }

    /// Record `label` for the current test case.
    ///
    /// The summary printed at the end of a successful run shows, for each
    /// label, the percentage of test cases which recorded it. Recording the
    /// same label more than once in one test case counts the case only once.
    ///
    /// Does nothing if called outside a proptest test case.
    pub fn label(label: impl Into<Cow<'static, str>>) {
        CURRENT_CASE.with(|cell| {
            if let Some(case) = cell.borrow_mut().as_mut() {
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
    /// `collect("length", v.len())`. Best for values with only a few
    /// distinct possibilities.
    pub fn collect(name: impl Into<Cow<'static, str>>, value: impl fmt::Debug) {
        label(format!("{} = {:?}", name.into(), value));
    }

    /// Record `label` for the current test case if `condition` holds, and
    /// require that at least `min_percent` of all test cases do.
    ///
    /// This is [`classify`] turned into an enforceable property: if fewer
    /// than `min_percent` percent of test cases record the label by the end
    /// of the run, the test as a whole fails. Use it to make sure a
    /// generator keeps producing the inputs a property actually needs:
    ///
    /// ```
    /// use proptest::prelude::*;
    /// use proptest::statistics;
    ///
    /// proptest! {
    ///     # /*
    ///     #[test]
    ///     # */
    ///     fn handles_collisions(v in prop::collection::vec(0u8..10, 0..10)) {
    ///         let has_duplicates = (1..v.len()).any(|i| v[i..].contains(&v[i - 1]));
    ///         statistics::cover(has_duplicates, 30.0, "duplicate elements");
    ///         // ... the actual property ...
    ///     }
    /// }
    /// # handles_collisions();
    /// ```
    ///
    /// The requirement is registered even if `condition` never holds, so a
    /// label whose coverage is 0% still fails the run. If the same label is
    /// given different `min_percent` values, the largest one applies.
    ///
    /// Coverage is measured on a random sample, so a run can fail by chance
    /// if `min_percent` sits close to the actual rate. Either leave a
    /// generous margin (e.g. require 30% when you expect 50%), or set
    /// [`Config::coverage_confidence`](crate::test_runner::Config::coverage_confidence)
    /// to have the requirements decided by a statistical test that runs as
    /// many cases as the decision needs.
    ///
    /// Does nothing if called outside a proptest test case.
    pub fn cover(
        condition: bool,
        min_percent: f64,
        label: impl Into<Cow<'static, str>>,
    ) {
        CURRENT_CASE.with(|cell| {
            if let Some(case) = cell.borrow_mut().as_mut() {
                let label = label.into();
                require_at_least(
                    &mut case.coverage_requirements,
                    label.clone(),
                    min_percent,
                );
                if condition {
                    case.labels.insert(label);
                }
            }
        });
    }

    /// Set `label`'s coverage requirement to `min_percent`, keeping an
    /// existing requirement if it is larger.
    fn require_at_least(
        requirements: &mut BTreeMap<Cow<'static, str>, f64>,
        label: Cow<'static, str>,
        min_percent: f64,
    ) {
        let required = requirements.entry(label).or_insert(min_percent);
        if min_percent > *required {
            *required = min_percent;
        }
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
        CURRENT_CASE.with(|cell| {
            if let Some(case) = cell.borrow_mut().as_mut() {
                case.measurements
                    .entry(name.into())
                    .or_default()
                    .push(value.into());
            }
        });
    }

    /// Run `body` with a fresh accumulator installed and return the body's
    /// result together with whatever it recorded.
    ///
    /// The enclosing accumulator is restored afterwards (also on panic), so
    /// nested `TestRunner`s record independently.
    pub(crate) fn record_case<R>(
        body: impl FnOnce() -> R,
    ) -> (R, CaseStatistics) {
        struct RestoreGuard {
            enclosing: Option<CaseStatistics>,
        }
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                CURRENT_CASE
                    .with(|cell| *cell.borrow_mut() = self.enclosing.take());
            }
        }

        let enclosing = CURRENT_CASE
            .with(|cell| cell.borrow_mut().replace(CaseStatistics::default()));
        let _restore = RestoreGuard { enclosing };
        let result = body();
        let recorded = CURRENT_CASE
            .with(|cell| cell.borrow_mut().take())
            .unwrap_or_default();
        (result, recorded)
    }

    /// Summary of all numeric samples recorded under one name via
    /// [`measure`].
    ///
    /// A summary always holds at least one sample, so `min`, `max` and
    /// `mean` are always meaningful.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct MeasurementSummary {
        count: u64,
        sum: f64,
        min: f64,
        max: f64,
    }

    impl MeasurementSummary {
        /// A summary with no samples yet; only handed out once `add` has
        /// been called.
        fn empty() -> Self {
            MeasurementSummary {
                count: 0,
                sum: 0.0,
                min: f64::INFINITY,
                max: f64::NEG_INFINITY,
            }
        }

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
            self.sum / self.count as f64
        }
    }

    /// Statistics aggregated over all passing test cases of one test run.
    ///
    /// Obtained from
    /// [`TestRunner::statistics()`](crate::test_runner::TestRunner::statistics).
    /// The `Display` implementation renders the human-readable summary that
    /// proptest prints when all test cases pass.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct RunStatistics {
        cases: u32,
        labels: BTreeMap<Cow<'static, str>, u32>,
        measurements: BTreeMap<Cow<'static, str>, MeasurementSummary>,
        coverage_requirements: BTreeMap<Cow<'static, str>, f64>,
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

        /// Whether nothing at all was recorded during the run.
        pub fn is_empty(&self) -> bool {
            self.labels.is_empty()
                && self.measurements.is_empty()
                && self.coverage_requirements.is_empty()
        }

        /// All coverage requirements declared via [`cover`] that the run
        /// did not meet. The run as a whole fails if this is non-empty.
        pub fn unmet_coverage(&self) -> Vec<UnmetCoverage> {
            self.coverage_requirements
                .iter()
                .filter_map(|(label, &required_percent)| {
                    let observed_cases =
                        self.labels.get(label).copied().unwrap_or(0);
                    let observed_percent =
                        observed_cases as f64 * 100.0 / self.cases as f64;
                    if observed_percent >= required_percent {
                        None
                    } else {
                        Some(UnmetCoverage {
                            label: label.clone(),
                            required_percent,
                            observed_percent,
                            observed_cases,
                            total_cases: self.cases,
                        })
                    }
                })
                .collect()
        }

        /// Fold the statistics of one passing test case into the aggregate.
        pub(crate) fn merge_case(&mut self, case: CaseStatistics) {
            self.cases += 1;
            for label in case.labels {
                *self.labels.entry(label).or_insert(0) += 1;
            }
            for (name, samples) in case.measurements {
                let summary = self
                    .measurements
                    .entry(name)
                    .or_insert_with(MeasurementSummary::empty);
                for sample in samples {
                    summary.add(sample);
                }
            }
            for (label, min_percent) in case.coverage_requirements {
                require_at_least(
                    &mut self.coverage_requirements,
                    label,
                    min_percent,
                );
            }
        }
    }

    /// Controls statistical checking of [`cover`] requirements.
    ///
    /// Set via
    /// [`Config::coverage_confidence`](crate::test_runner::Config::coverage_confidence).
    /// When configured, the runner keeps generating test cases beyond
    /// `Config::cases` until every requirement is *confirmed* (the true rate
    /// is confidently at least [`tolerance`](Self::tolerance) times the
    /// required one) or some requirement is *refuted* (the true rate is
    /// confidently below the required one), using Wilson score bounds as in
    /// QuickCheck's `checkCoverage`. This replaces the fixed-sample check,
    /// which can fail by chance when the true rate is close to the
    /// requirement.
    ///
    /// Construct with field-record update syntax:
    ///
    /// ```
    /// use proptest::statistics::CoverageConfidence;
    ///
    /// let confidence = CoverageConfidence {
    ///     max_cases: 100_000,
    ///     ..CoverageConfidence::default()
    /// };
    /// ```
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct CoverageConfidence {
        /// The acceptable probability of a wrong decision is
        /// `1 / certainty`. The default is `10^9`.
        pub certainty: u64,
        /// A requirement counts as confirmed once the true rate is
        /// confidently at least `tolerance` times the required rate. The
        /// gap between `tolerance * required` and `required` is what makes
        /// a decision reachable: true rates inside the gap can be neither
        /// confirmed nor refuted. The default is `0.9`.
        pub tolerance: f64,
        /// Hard cap on the number of test cases to run while deciding.
        /// Reaching it fails the run as undecided rather than letting a
        /// rate close to a requirement run unbounded. The default is
        /// `1_000_000`.
        pub max_cases: u32,
        // Needs to be public so FRU syntax can be used.
        #[doc(hidden)]
        pub _non_exhaustive: (),
    }

    impl Default for CoverageConfidence {
        fn default() -> Self {
            CoverageConfidence {
                certainty: 1_000_000_000,
                tolerance: 0.9,
                max_cases: 1_000_000,
                _non_exhaustive: (),
            }
        }
    }

    /// The outcome of one statistical evaluation of all [`cover`]
    /// requirements.
    pub(crate) enum CoverageDecision {
        /// Every requirement has been confirmed; testing can stop.
        AllConfirmed,
        /// At least one requirement is still too close to call.
        Undecided,
        /// This requirement is confidently below its required rate.
        Refuted(UnmetCoverage),
    }

    impl RunStatistics {
        /// Statistically evaluate every coverage requirement against the
        /// cases seen so far.
        ///
        /// Labels in `confirmed` are skipped and newly confirmed labels are
        /// added to it, so each requirement is decided at most once per run.
        ///
        /// Must not be called before any case has been recorded.
        pub(crate) fn decide_coverage(
            &self,
            confidence: &CoverageConfidence,
            confirmed: &mut BTreeSet<Cow<'static, str>>,
        ) -> CoverageDecision {
            let alpha = 1.0 / confidence.certainty as f64;
            let z = inverse_normal_cdf(1.0 - alpha);
            let mut all_confirmed = true;
            for (label, &required_percent) in &self.coverage_requirements {
                if confirmed.contains(label) {
                    continue;
                }
                let observed_cases =
                    self.labels.get(label).copied().unwrap_or(0);
                let required_rate = required_percent / 100.0;
                let lower = wilson_bound(observed_cases, self.cases, -z);
                let upper = wilson_bound(observed_cases, self.cases, z);
                if lower >= confidence.tolerance * required_rate {
                    confirmed.insert(label.clone());
                } else if upper < required_rate {
                    return CoverageDecision::Refuted(UnmetCoverage {
                        label: label.clone(),
                        required_percent,
                        observed_percent: observed_cases as f64 * 100.0
                            / self.cases as f64,
                        observed_cases,
                        total_cases: self.cases,
                    });
                } else {
                    all_confirmed = false;
                }
            }
            if all_confirmed {
                CoverageDecision::AllConfirmed
            } else {
                CoverageDecision::Undecided
            }
        }
    }

    /// One-sided Wilson score bound on the true success rate after seeing
    /// `successes` in `trials`. A negative `z` gives the lower bound, a
    /// positive `z` the upper bound.
    pub(crate) fn wilson_bound(successes: u32, trials: u32, z: f64) -> f64 {
        let n = trials as f64;
        let observed_rate = successes as f64 / n;
        let zz = z * z;
        (observed_rate
            + zz / (2.0 * n)
            + z * (observed_rate * (1.0 - observed_rate) / n
                + zz / (4.0 * n * n))
                .sqrt())
            / (1.0 + zz / n)
    }

    /// Quantile function of the standard normal distribution, using
    /// Acklam's rational approximation (relative error below `1.2e-9`,
    /// good enough for the default certainty of `10^9`).
    pub(crate) fn inverse_normal_cdf(p: f64) -> f64 {
        assert!(p > 0.0 && p < 1.0, "p must be in (0, 1), got {}", p);

        const A: [f64; 6] = [
            -3.969683028665376e+01,
            2.209460984245205e+02,
            -2.759285104469687e+02,
            1.38357751867269e+02,
            -3.066479806614716e+01,
            2.506628277459239e+00,
        ];
        const B: [f64; 5] = [
            -5.447609879822406e+01,
            1.615858368580409e+02,
            -1.556989798598866e+02,
            6.680131188771972e+01,
            -1.328068155288572e+01,
        ];
        const C: [f64; 6] = [
            -7.784894002430293e-03,
            -3.223964580411365e-01,
            -2.400758277161838e+00,
            -2.549732539343734e+00,
            4.374664141464968e+00,
            2.938163982698783e+00,
        ];
        const D: [f64; 4] = [
            7.784695709041462e-03,
            3.224671290700398e-01,
            2.445134137142996e+00,
            3.754408661907416e+00,
        ];
        const P_LOW: f64 = 0.02425;

        let tail = |q: f64| {
            (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q
                + C[5])
                / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
        };

        if p < P_LOW {
            tail((-2.0 * p.ln()).sqrt())
        } else if p > 1.0 - P_LOW {
            -tail((-2.0 * (1.0 - p).ln()).sqrt())
        } else {
            let q = p - 0.5;
            let r = q * q;
            (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r
                + A[5])
                * q
                / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r
                    + 1.0)
        }
    }

    /// A coverage requirement declared via [`cover`] that a test run did
    /// not meet.
    #[derive(Clone, Debug, PartialEq)]
    #[non_exhaustive]
    pub struct UnmetCoverage {
        /// The label whose coverage fell short.
        pub label: Cow<'static, str>,
        /// The smallest acceptable percentage of test cases.
        pub required_percent: f64,
        /// The percentage of test cases that recorded the label.
        pub observed_percent: f64,
        /// The number of test cases that recorded the label.
        pub observed_cases: u32,
        /// The number of test cases in the run.
        pub total_cases: u32,
    }

    impl fmt::Display for UnmetCoverage {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "coverage of \"{}\" was {:.2}% ({}/{} cases), \
                 required at least {}%",
                self.label,
                self.observed_percent,
                self.observed_cases,
                self.total_cases,
                self.required_percent
            )
        }
    }

    impl fmt::Display for RunStatistics {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            if !self.labels.is_empty() {
                writeln!(f, "test case statistics ({} cases):", self.cases)?;
                let mut most_frequent_first: Vec<(&Cow<'static, str>, &u32)> =
                    self.labels.iter().collect();
                most_frequent_first.sort_by(
                    |(a_label, a_count), (b_label, b_count)| {
                        b_count.cmp(a_count).then_with(|| a_label.cmp(b_label))
                    },
                );
                for (label, count) in most_frequent_first {
                    let percent = *count as f64 * 100.0 / self.cases as f64;
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
pub use internal::{classify, collect, cover, label, measure};
#[cfg(feature = "std")]
pub(crate) use internal::{record_case, CoverageDecision};
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub use internal::{
    CoverageConfidence, MeasurementSummary, RunStatistics, UnmetCoverage,
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

    /// Does nothing in `no_std` builds — coverage is *not* enforced. See
    /// the `std` documentation.
    pub fn cover<L>(_condition: bool, _min_percent: f64, _label: L) {}

    /// Does nothing in `no_std` builds. See the `std` documentation.
    pub fn measure<N, V>(_name: N, _value: V) {}
}

#[cfg(not(feature = "std"))]
pub use internal::{classify, collect, cover, label, measure};

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::std_facade::string::ToString;
    use crate::strategy::Just;
    use crate::test_runner::{Config, TestCaseError, TestError, TestRunner};

    // Failure persistence would write regression files for the
    // intentionally failing tests below.
    fn config_without_persistence() -> Config {
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
        let mut runner = TestRunner::new(config_without_persistence());
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
        let mut runner = TestRunner::new(config_without_persistence());
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
        let mut runner = TestRunner::new(config_without_persistence());
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
        let executions = core::cell::Cell::new(0u32);
        let mut runner = TestRunner::new(config_without_persistence());
        let result = runner.run(&(0u32..1000), |_| {
            label("ran");
            executions.set(executions.get() + 1);
            if executions.get() <= 5 {
                Ok(())
            } else {
                Err(TestCaseError::fail("always fails from the 6th call"))
            }
        });
        assert!(result.is_err());

        // The failing case and the shrink executions also record the label,
        // but only the five passing cases may be aggregated.
        assert!(executions.get() > 6, "test was not re-run while shrinking");
        let stats = runner.statistics();
        assert_eq!(5, stats.cases());
        assert_eq!(Some(&5), stats.labels().get("ran"));
    }

    #[test]
    fn rejected_cases_are_not_counted() {
        let mut runner = TestRunner::new(config_without_persistence());
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
    fn met_coverage_passes() {
        let mut runner = TestRunner::new(config_without_persistence());
        runner
            .run(&(0u32..100), |v| {
                cover(v < 90, 50.0, "small");
                Ok(())
            })
            .unwrap();
        assert!(runner.statistics().unmet_coverage().is_empty());
    }

    #[test]
    fn unmet_coverage_fails_the_run() {
        let mut runner = TestRunner::new(config_without_persistence());
        let result = runner.run(&(0u32..100), |v| {
            cover(v == 0, 90.0, "zero");
            Ok(())
        });

        match result {
            Err(TestError::Abort(reason)) => {
                let message = format!("{}", reason);
                assert!(
                    message.contains("Coverage requirements not met"),
                    "unexpected message: {}",
                    message
                );
                assert!(
                    message.contains("\"zero\""),
                    "unexpected message: {}",
                    message
                );
                assert!(
                    message.contains("required at least 90%"),
                    "unexpected message: {}",
                    message
                );
            }
            other => panic!("Unexpected result: {:?}", other),
        }

        let unmet = runner.statistics().unmet_coverage();
        assert_eq!(1, unmet.len());
        assert_eq!("zero", unmet[0].label);
        assert_eq!(90.0, unmet[0].required_percent);
        assert_eq!(runner.statistics().cases(), unmet[0].total_cases);
        assert!(unmet[0].observed_percent < 90.0);
    }

    #[test]
    fn coverage_requirement_holds_even_if_condition_never_true() {
        let mut runner = TestRunner::new(config_without_persistence());
        let result = runner.run(&Just(()), |_| {
            cover(false, 10.0, "never");
            Ok(())
        });

        assert!(matches!(result, Err(TestError::Abort(_))));
        let unmet = runner.statistics().unmet_coverage();
        assert_eq!(1, unmet.len());
        assert_eq!(0, unmet[0].observed_cases);
        assert_eq!(0.0, unmet[0].observed_percent);
    }

    #[test]
    fn largest_requirement_for_a_label_applies() {
        let mut runner = TestRunner::new(config_without_persistence());
        let result = runner.run(&Just(()), |_| {
            // Met under the first requirement, unmet under the second.
            cover(true, 50.0, "feature");
            cover(false, 200.0, "feature");
            Ok(())
        });

        assert!(matches!(result, Err(TestError::Abort(_))));
        let unmet = runner.statistics().unmet_coverage();
        assert_eq!(1, unmet.len());
        assert_eq!(200.0, unmet[0].required_percent);
    }

    #[test]
    fn test_failure_takes_precedence_over_coverage() {
        let mut runner = TestRunner::new(config_without_persistence());
        let result = runner.run(&(0u32..100), |_| {
            cover(false, 90.0, "never");
            Err(TestCaseError::fail("real failure"))
        });
        assert!(matches!(result, Err(TestError::Fail(..))));
    }

    #[test]
    fn inverse_normal_cdf_matches_known_quantiles() {
        let cases = [
            (0.5, 0.0),
            (0.975, 1.959964),
            (0.025, -1.959964),
            (0.999, 3.090232),
            (1e-9, -5.997807),
            (1.0 - 1e-9, 5.997807),
        ];
        for (p, expected) in cases {
            let actual = internal::inverse_normal_cdf(p);
            assert!(
                (actual - expected).abs() < 1e-5,
                "inverse_normal_cdf({}) = {}, expected {}",
                p,
                actual,
                expected
            );
        }
    }

    #[test]
    fn wilson_bounds_match_known_interval() {
        // The 95% Wilson interval for 50 successes in 100 trials is
        // approximately (0.4038, 0.5962).
        let z = internal::inverse_normal_cdf(0.975);
        let lower = internal::wilson_bound(50, 100, -z);
        let upper = internal::wilson_bound(50, 100, z);
        assert!((lower - 0.4038).abs() < 1e-3, "lower = {}", lower);
        assert!((upper - 0.5962).abs() < 1e-3, "upper = {}", upper);
    }

    fn config_with_confidence(confidence: CoverageConfidence) -> Config {
        Config {
            cases: 4,
            coverage_confidence: Some(confidence),
            ..config_without_persistence()
        }
    }

    #[test]
    fn coverage_confidence_extends_run_until_confirmed() {
        let mut runner = TestRunner::new(config_with_confidence(
            CoverageConfidence::default(),
        ));
        runner
            .run(&(0u32..2), |v| {
                // True rate ~50%, required 20%: confirmable, but not
                // within the configured 4 cases.
                cover(v == 0, 20.0, "zero");
                Ok(())
            })
            .unwrap();

        let stats = runner.statistics();
        assert!(
            stats.cases() > 4,
            "expected extra cases to settle coverage, ran {}",
            stats.cases()
        );
        assert!(stats.cases() < CoverageConfidence::default().max_cases);
    }

    #[test]
    fn coverage_confidence_refutes_unreachable_requirement() {
        let mut runner = TestRunner::new(config_with_confidence(
            CoverageConfidence::default(),
        ));
        let result = runner.run(&(0u32..2), |v| {
            // True rate ~50%, required 90%: refutable.
            cover(v == 0, 90.0, "zero");
            Ok(())
        });

        match result {
            Err(TestError::Abort(reason)) => {
                let message = format!("{}", reason);
                assert!(
                    message.contains("statistically refuted"),
                    "unexpected message: {}",
                    message
                );
                assert!(
                    message.contains("\"zero\""),
                    "unexpected message: {}",
                    message
                );
            }
            other => panic!("Unexpected result: {:?}", other),
        }
    }

    #[test]
    fn coverage_confidence_gives_up_at_max_cases() {
        let mut runner =
            TestRunner::new(config_with_confidence(CoverageConfidence {
                max_cases: 500,
                ..CoverageConfidence::default()
            }));
        let result = runner.run(&(0u32..100), |v| {
            // True rate ~47% sits inside the tolerance band
            // (45%..50%), so it can be neither confirmed nor refuted.
            cover(v < 47, 50.0, "in the band");
            Ok(())
        });

        match result {
            Err(TestError::Abort(reason)) => {
                let message = format!("{}", reason);
                assert!(
                    message.contains("undecided after 500 cases"),
                    "unexpected message: {}",
                    message
                );
            }
            other => panic!("Unexpected result: {:?}", other),
        }
        assert_eq!(500, runner.statistics().cases());
    }

    #[test]
    fn coverage_confidence_without_cover_runs_configured_cases() {
        let mut runner = TestRunner::new(Config {
            cases: 8,
            ..config_with_confidence(CoverageConfidence::default())
        });
        runner
            .run(&Just(()), |_| {
                label("ran");
                Ok(())
            })
            .unwrap();
        assert_eq!(8, runner.statistics().cases());
    }

    #[test]
    fn coverage_confidence_skips_fixed_sample_check() {
        // A true rate of ~48% confirms quickly against the lowered
        // confirmation threshold of tolerance * 50% ~= 33%, yet the
        // observed percentage stays below the raw 50% requirement. The
        // run may only pass if the fixed-sample end-of-run check stays
        // out of the way.
        let mut runner =
            TestRunner::new(config_with_confidence(CoverageConfidence {
                tolerance: 2.0 / 3.0,
                ..CoverageConfidence::default()
            }));
        runner
            .run(&(0u32..100), |v| {
                cover(v < 48, 50.0, "almost half");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn empty_when_nothing_recorded() {
        let mut runner = TestRunner::new(config_without_persistence());
        runner.run(&Just(()), |_| Ok(())).unwrap();
        assert!(runner.statistics().is_empty());
        assert_eq!(Config::default().cases, runner.statistics().cases());
    }

    #[test]
    fn display_renders_summary() {
        let mut runner = TestRunner::new(Config {
            cases: 4,
            ..config_without_persistence()
        });
        runner
            .run(&Just(()), |_| {
                label("always");
                measure("x", 2.0);
                Ok(())
            })
            .unwrap();

        let rendered = runner.statistics().to_string();
        let expected = [
            "test case statistics (4 cases):",
            "  100.00% (4/4): always",
            "measurements:",
            "  x: count 4, min 2, max 2, mean 2.00",
            "",
        ]
        .join("\n");
        assert_eq!(expected, rendered);
    }

    #[test]
    fn labels_sorted_by_frequency_in_display() {
        let mut runner = TestRunner::new(config_without_persistence());
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
            ..config_without_persistence()
        });
        runner
            .run(&Just(()), |_| {
                label("outer");
                let mut inner = TestRunner::new(Config {
                    cases: 2,
                    ..config_without_persistence()
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
        proptest!(config_without_persistence(), |(v in 0u32..10)| {
            classify(v < 5, "small");
            prop_assert!(v < 10);
        });
    }
}
