//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! An optional hook that is told about every test case the runner runs.
//!
//! Tools that report on a test run (statistics, coverage, live
//! visualisation) need to know which test is running, what happened to each
//! test case (whether its input was newly generated or a persisted failure,
//! whether the test body ran, whether it passed, was rejected or failed, and
//! how long it took) and how the run ended. The runner already knows this; a
//! [`TestCaseObserver`] set in [`Config::observer`](super::Config::observer)
//! receives it, so that such tools work with every way of writing a test
//! (`proptest!`, `#[property_test]`, `prop_state_machine!`, or calling
//! [`TestRunner::run`](super::TestRunner::run) directly) without wrapping
//! the test body.

use crate::std_facade::fmt;
use crate::test_runner::Config;
use core::time::Duration;

/// Where the input of a test case came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub enum TestCaseInput {
    /// The input was newly generated in this run.
    Generated,
    /// The input is a failure persisted by an earlier run, replayed before
    /// any new input is generated.
    Persisted,
}

/// Where the result of a test case came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub enum ResultSource {
    /// The test body ran in this process.
    TestBody,
    /// The test case ran in a child process (with `fork` or `timeout`), and
    /// this process took its result from what the child processes recorded.
    /// A child process that crashed or ran out of time is recorded by this
    /// process as a failure.
    ChildProcess,
    /// The result cache already held a result for the same input, so the
    /// test body did not run.
    ResultCache,
}

/// How a test case ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub enum TestCaseVerdict {
    /// The test passed.
    Pass,
    /// The input was rejected, for example by `prop_assume!`. If this was
    /// one rejection too many, the run is then aborted.
    Reject,
    /// The test failed. The failure has already been shrunk, and the run
    /// then ends.
    Fail,
}

/// What happened to one test case, as reported to a [`TestCaseObserver`].
///
/// Only test cases are reported, not the extra calls made while shrinking a
/// failure: a failure is shrunk before it is reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct TestCaseOutcome {
    /// Where the input came from.
    pub input: TestCaseInput,
    /// Where the result came from.
    pub result_from: ResultSource,
    /// How the test case ended.
    pub verdict: TestCaseVerdict,
    /// How long the test case took in this process: producing its value,
    /// running the test body and, for a failure, shrinking it. `None` when
    /// the test body did not run in this process (`result_from` is not
    /// [`ResultSource::TestBody`]), and on wasm32, where proptest reads no
    /// clock.
    pub duration: Option<Duration>,
}

/// How a run ended, as reported to a [`TestCaseObserver`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub enum RunOutcome {
    /// Enough test cases passed.
    Passed,
    /// A test case failed.
    Failed,
    /// The run gave up without a failure: too many inputs were rejected, an
    /// input could not be generated, or (with `fork` or `timeout`) a child
    /// process crashed before its first test case or child processes kept
    /// crashing.
    Aborted,
}

/// Receives a call when a run starts, before and after every test case that
/// [`TestRunner::run`](super::TestRunner::run) runs, and when the run ends.
///
/// Set one in [`Config::observer`](super::Config::observer). All methods do
/// nothing by default, so an observer implements only what it needs. They
/// take `&self`, so an observer that keeps counts or records uses interior
/// mutability (for example a `Mutex` or atomics).
///
/// For one call of `TestRunner::run` the calls come in this order:
/// `run_started`; then, for each test case in the order they run,
/// `case_started` and `case_finished`; then `run_finished`. These are not
/// reported:
///
/// - the calls made while shrinking a failure;
/// - an input that could not be generated, which ends the run as
///   [`RunOutcome::Aborted`];
/// - test cases run with [`TestRunner::run_one`](super::TestRunner::run_one);
/// - anything in the child processes that `fork` and `timeout` start. The
///   process that started them reports the test cases it takes from what
///   the children recorded, with [`ResultSource::ChildProcess`], after the
///   children have run them.
///
/// If anything other than the test body panics during a run (a strategy
/// producing its value, the observer itself, or the runner setting up child
/// processes), the panic ends the run, and the observer gets no further
/// calls.
///
/// The calls of a run come from the thread that called `run`. Tests that run
/// at the same time and share one observer, or a run started inside another
/// run's test body, interleave their calls; one observer per test keeps them
/// apart.
///
/// ```
/// use proptest::prelude::*;
/// use proptest::test_runner::{
///     Config, TestCaseInput, TestCaseObserver, TestCaseOutcome,
///     TestCaseVerdict, TestRunner,
/// };
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// #[derive(Debug, Default)]
/// struct CountPasses(AtomicUsize);
///
/// impl TestCaseObserver for CountPasses {
///     fn case_finished(&self, outcome: TestCaseOutcome) {
///         if outcome.input == TestCaseInput::Generated
///             && outcome.verdict == TestCaseVerdict::Pass
///         {
///             self.0.fetch_add(1, Ordering::Relaxed);
///         }
///     }
/// }
///
/// let passes = Arc::new(CountPasses::default());
/// let mut runner = TestRunner::new(Config {
///     cases: 50,
///     observer: Some(passes.clone()),
///     ..Config::default()
/// });
/// runner.run(&(0u32..10), |x| {
///     prop_assert!(x < 10);
///     Ok(())
/// }).unwrap();
/// assert_eq!(passes.0.load(Ordering::Relaxed), 50);
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub trait TestCaseObserver: Send + Sync + fmt::Debug {
    /// Called once when a run starts, before its first test case, with the
    /// runner's configuration. Its `test_name` names the test when the test
    /// is a function written with `proptest!`, `#[property_test]` or
    /// `prop_state_machine!`.
    fn run_started(&self, config: &Config) {
        let _ = config;
    }

    /// Called just before the runner runs a test case.
    fn case_started(&self) {}

    /// Called once the test case has finished, with what happened to it.
    fn case_finished(&self, outcome: TestCaseOutcome) {
        let _ = outcome;
    }

    /// Called once when the run ends, with how it ended.
    fn run_finished(&self, outcome: RunOutcome) {
        let _ = outcome;
    }
}

/// Two configurations are equal only if they share the same observer
/// object, since observers have no other notion of equality.
impl PartialEq for dyn TestCaseObserver {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self, other)
    }
}
