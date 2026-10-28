//-
// Copyright 2017, 2018, 2019, 2024, 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::std_facade::{Arc, BTreeMap, Box, String, Vec};
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::SeqCst;
use core::{fmt, iter};
#[cfg(feature = "std")]
use std::panic::{self, AssertUnwindSafe};

#[cfg(feature = "fork")]
use rusty_fork;
#[cfg(feature = "fork")]
use std::cell::{Cell, RefCell};
#[cfg(feature = "fork")]
use std::env;
#[cfg(feature = "fork")]
use std::fs;
#[cfg(feature = "fork")]
use tempfile;

use crate::strategy::*;
use crate::test_runner::config::*;
use crate::test_runner::errors::*;
use crate::test_runner::failure_persistence::PersistedSeed;
#[cfg(feature = "std")]
use crate::test_runner::observer::{
    ResultSource, RunOutcome, TestCaseInput, TestCaseOutcome, TestCaseVerdict,
};
use crate::test_runner::reason::*;
#[cfg(feature = "fork")]
use crate::test_runner::replay;
use crate::test_runner::result_cache::*;
use crate::test_runner::rng::TestRng;

#[cfg(feature = "fork")]
const ENV_FORK_FILE: &'static str = "_PROPTEST_FORKFILE";

const ALWAYS: u32 = 0;
/// Verbose level 1 to show failures. In state machine tests this level is used
/// to print transitions.
pub const INFO_LOG: u32 = 1;
const TRACE: u32 = 2;

#[cfg(feature = "std")]
macro_rules! verbose_message {
    ($runner:expr, $level:expr, $fmt:tt $($arg:tt)*) => { {
        #[allow(unused_comparisons)]
        {
            if $runner.config.verbose >= $level {
                eprintln!(concat!("proptest: ", $fmt) $($arg)*);
            }
        };
        ()
    } }
}

#[cfg(not(feature = "std"))]
macro_rules! verbose_message {
    ($runner:expr, $level:expr, $fmt:tt $($arg:tt)*) => {
        let _ = $level;
    };
}

type RejectionDetail = BTreeMap<Reason, u32>;

/// State used when running a proptest test.
#[derive(Clone)]
pub struct TestRunner {
    config: Config,
    successes: u32,
    local_rejects: u32,
    global_rejects: u32,
    rng: TestRng,
    flat_map_regens: Arc<AtomicUsize>,

    local_reject_detail: RejectionDetail,
    global_reject_detail: RejectionDetail,
}

impl fmt::Debug for TestRunner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("TestRunner")
            .field("config", &self.config)
            .field("successes", &self.successes)
            .field("local_rejects", &self.local_rejects)
            .field("global_rejects", &self.global_rejects)
            .field("rng", &"<TestRng>")
            .field("flat_map_regens", &self.flat_map_regens)
            .field("local_reject_detail", &self.local_reject_detail)
            .field("global_reject_detail", &self.global_reject_detail)
            .finish()
    }
}

impl fmt::Display for TestRunner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "\tsuccesses: {}\n\
             \tlocal rejects: {}\n",
            self.successes, self.local_rejects
        )?;
        for (whence, count) in &self.local_reject_detail {
            writeln!(f, "\t\t{} times at {}", count, whence)?;
        }
        writeln!(f, "\tglobal rejects: {}", self.global_rejects)?;
        for (whence, count) in &self.global_reject_detail {
            writeln!(f, "\t\t{} times at {}", count, whence)?;
        }

        Ok(())
    }
}

/// Equivalent to: `TestRunner::new(Config::default())`.
impl Default for TestRunner {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

#[cfg(feature = "fork")]
#[derive(Debug)]
struct ForkOutput {
    file: Option<fs::File>,
}

#[cfg(feature = "fork")]
impl ForkOutput {
    fn append(&mut self, result: &TestCaseResult) {
        if let Some(ref mut file) = self.file {
            replay::append(file, result)
                .expect("Failed to append to replay file");
        }
    }

    fn ping(&mut self) {
        if let Some(ref mut file) = self.file {
            replay::ping(file).expect("Failed to append to replay file");
        }
    }

    fn terminate(&mut self) {
        if let Some(ref mut file) = self.file {
            replay::terminate(file).expect("Failed to append to replay file");
        }
    }

    fn empty() -> Self {
        ForkOutput { file: None }
    }

    fn is_in_fork(&self) -> bool {
        self.file.is_some()
    }
}

#[cfg(not(feature = "fork"))]
#[derive(Debug)]
struct ForkOutput;

#[cfg(not(feature = "fork"))]
impl ForkOutput {
    fn append(&mut self, _result: &TestCaseResult) {}
    fn ping(&mut self) {}
    fn terminate(&mut self) {}
    fn empty() -> Self {
        ForkOutput
    }
    fn is_in_fork(&self) -> bool {
        false
    }
}

#[cfg(not(feature = "std"))]
fn call_test<V, F, R>(
    _runner: &mut TestRunner,
    case: V,
    test: &F,
    replay_from_fork: &mut R,
    result_cache: &mut dyn ResultCache,
    _: &mut ForkOutput,
    is_from_persisted_seed: bool,
) -> TestCaseResultV2
where
    V: fmt::Debug,
    F: Fn(V) -> TestCaseResult,
    R: Iterator<Item = TestCaseResult>,
{
    if let Some(result) = replay_from_fork.next() {
        return result.map(|_| TestCaseOk::ReplayFromForkSuccess);
    }

    let cache_key = result_cache.key(&ResultCacheKey::new(&case));
    if let Some(result) = result_cache.get(cache_key) {
        return result.clone().map(|_| TestCaseOk::CacheHitSuccess);
    }

    let result = test(case);
    result_cache.put(cache_key, &result);
    result.map(|_| {
        if is_from_persisted_seed {
            TestCaseOk::PersistedCaseSuccess
        } else {
            TestCaseOk::NewCaseSuccess
        }
    })
}

#[cfg(feature = "std")]
fn call_test<V, F, R>(
    runner: &mut TestRunner,
    case: V,
    test: &F,
    replay_from_fork: &mut R,
    result_cache: &mut dyn ResultCache,
    fork_output: &mut ForkOutput,
    is_from_persisted_seed: bool,
) -> TestCaseResultV2
where
    V: fmt::Debug,
    F: Fn(V) -> TestCaseResult,
    R: Iterator<Item = TestCaseResult>,
{
    #[cfg(feature = "timeout")]
    let timeout = runner.config.timeout();

    if let Some(result) = replay_from_fork.next() {
        return result.map(|_| TestCaseOk::ReplayFromForkSuccess);
    }

    // Now that we're about to start a new test (as far as the replay system is
    // concerned), ping the replay file so the parent process can determine
    // that we made it this far.
    fork_output.ping();

    verbose_message!(runner, TRACE, "Next test input: {:?}", case);

    let cache_key = result_cache.key(&ResultCacheKey::new(&case));
    if let Some(result) = result_cache.get(cache_key) {
        verbose_message!(
            runner,
            TRACE,
            "Test input hit cache, skipping execution"
        );
        return result.clone().map(|_| TestCaseOk::CacheHitSuccess);
    }

    #[cfg(feature = "timeout")]
    let time_start = std::time::Instant::now();

    let mut result = unwrap_or!(
        super::scoped_panic_hook::with_hook(
            |_| { /* Silence out panic backtrace */ },
            || panic::catch_unwind(AssertUnwindSafe(|| test(case)))
        ),
        what => Err(TestCaseError::Fail(
            what.downcast::<&'static str>().map(|s| (*s).into())
                .or_else(|what| what.downcast::<String>().map(|b| (*b).into()))
                .or_else(|what| what.downcast::<Box<str>>().map(|b| (*b).into()))
                .unwrap_or_else(|_| "<unknown panic value>".into()))));

    // If there is a timeout and we exceeded it, fail the test here so we get
    // consistent behaviour. (The parent process cannot precisely time the test
    // cases itself.)
    #[cfg(feature = "timeout")]
    if timeout > 0 && result.is_ok() {
        let elapsed = time_start.elapsed();
        let elapsed_millis = elapsed.as_secs() as u32 * 1000
            + elapsed.subsec_nanos() / 1_000_000;

        if elapsed_millis > timeout {
            result = Err(TestCaseError::fail(format!(
                "Timeout of {} ms exceeded: test took {} ms",
                timeout, elapsed_millis
            )));
        }
    }

    result_cache.put(cache_key, &result);
    fork_output.append(&result);

    match result {
        Ok(()) => verbose_message!(runner, TRACE, "Test case passed"),
        Err(TestCaseError::Reject(ref reason)) => {
            verbose_message!(runner, INFO_LOG, "Test case rejected: {}", reason)
        }
        Err(TestCaseError::Fail(ref reason)) => {
            verbose_message!(runner, INFO_LOG, "Test case failed: {}", reason)
        }
    }

    result.map(|_| {
        if is_from_persisted_seed {
            TestCaseOk::PersistedCaseSuccess
        } else {
            TestCaseOk::NewCaseSuccess
        }
    })
}

type TestRunResult<S> = Result<(), TestError<<S as Strategy>::Value>>;

/// Tells the configured observer, if any, that a run is starting.
///
/// In a child process started by `fork` or `timeout`, turns the observer off
/// instead: the parent process reports every test case, from the results the
/// children record.
#[cfg(feature = "std")]
fn observe_run_start(config: &mut Config) {
    #[cfg(feature = "fork")]
    if config.observer.is_some() && env::var_os(ENV_FORK_FILE).is_some() {
        config.observer = None;
    }
    if let Some(observer) = config.observer.as_ref() {
        observer.run_started(config);
    }
}

/// Without `std` there is no observer.
#[cfg(not(feature = "std"))]
fn observe_run_start(_: &mut Config) {}

/// Tells the configured observer, if any, how the run ended.
#[cfg(feature = "std")]
fn observe_run_finish<T>(config: &Config, result: &Result<(), TestError<T>>) {
    if let Some(observer) = config.observer.as_ref() {
        observer.run_finished(match result {
            Ok(()) => RunOutcome::Passed,
            Err(TestError::Fail(..)) => RunOutcome::Failed,
            Err(TestError::Abort(..)) => RunOutcome::Aborted,
        });
    }
}

/// Without `std` there is no observer.
#[cfg(not(feature = "std"))]
fn observe_run_finish<T>(_: &Config, _: &Result<(), TestError<T>>) {}

/// Whether the observer was told that a test case started, and when, where
/// proptest reads the clock: like the runner's own timers, not on wasm32,
/// where `Instant::now` panics.
#[cfg(feature = "std")]
type CaseStart = Option<Option<std::time::Instant>>;

/// Without `std` there is no observer.
#[cfg(not(feature = "std"))]
type CaseStart = Option<()>;

/// Tells the configured observer, if any, that a test case is about to run.
#[cfg(feature = "std")]
fn observe_case_start(config: &Config) -> CaseStart {
    let observer = config.observer.as_ref()?;
    observer.case_started();
    #[cfg(not(target_arch = "wasm32"))]
    return Some(Some(std::time::Instant::now()));
    #[cfg(target_arch = "wasm32")]
    return Some(None);
}

/// Without `std` there is no observer.
#[cfg(not(feature = "std"))]
fn observe_case_start(_: &Config) -> CaseStart {
    None
}

/// Tells the configured observer, if any, what happened to the test case
/// that `observe_case_start` reported.
#[cfg(feature = "std")]
fn observe_case_finish<T>(
    config: &Config,
    started: CaseStart,
    result: &Result<TestCaseOk, TestError<T>>,
    is_from_persisted_seed: bool,
    cache_hit: bool,
) {
    let (Some(observer), Some(started)) = (config.observer.as_ref(), started)
    else {
        return;
    };
    let input = if is_from_persisted_seed {
        TestCaseInput::Persisted
    } else {
        TestCaseInput::Generated
    };
    // With `fork`, this is the parent process, since the observer is off in
    // child processes, and every result is replayed from a child process.
    let result_from = if config.fork() {
        ResultSource::ChildProcess
    } else if cache_hit {
        ResultSource::ResultCache
    } else {
        ResultSource::TestBody
    };
    let verdict = match result {
        Ok(TestCaseOk::Reject) => TestCaseVerdict::Reject,
        Ok(_) => TestCaseVerdict::Pass,
        Err(TestError::Fail(..)) => TestCaseVerdict::Fail,
        // `run_one_with_replay` aborts only when a rejection is one too many.
        Err(TestError::Abort(..)) => TestCaseVerdict::Reject,
    };
    let duration = match result_from {
        ResultSource::TestBody => started.map(|started| started.elapsed()),
        _ => None,
    };
    observer.case_finished(TestCaseOutcome {
        input,
        result_from,
        verdict,
        duration,
    });
}

/// Without `std` there is no observer.
#[cfg(not(feature = "std"))]
fn observe_case_finish<T>(
    _: &Config,
    _: CaseStart,
    _: &Result<TestCaseOk, TestError<T>>,
    _: bool,
    _: bool,
) {
}

/// Passes every call on to a result cache, and notes whether the first
/// lookup, the one for the test case itself rather than for shrinking it,
/// found a result.
struct NoteCacheHit<'a> {
    cache: &'a mut dyn ResultCache,
    first_lookup_hit: core::cell::Cell<Option<bool>>,
}

impl<'a> NoteCacheHit<'a> {
    fn new(cache: &'a mut dyn ResultCache) -> Self {
        NoteCacheHit {
            cache,
            first_lookup_hit: core::cell::Cell::new(None),
        }
    }

    fn first_lookup_hit(&self) -> bool {
        self.first_lookup_hit.get().unwrap_or(false)
    }
}

impl ResultCache for NoteCacheHit<'_> {
    fn key(&self, key: &ResultCacheKey) -> u64 {
        self.cache.key(key)
    }

    fn put(&mut self, key: u64, result: &TestCaseResult) {
        self.cache.put(key, result)
    }

    fn get(&self, key: u64) -> Option<&TestCaseResult> {
        let result = self.cache.get(key);
        if self.first_lookup_hit.get().is_none() {
            self.first_lookup_hit.set(Some(result.is_some()));
        }
        result
    }
}

impl TestRunner {
    /// Create a fresh `TestRunner` with the given configuration.
    ///
    /// The runner will use an RNG with a generated seed and the default
    /// algorithm.
    ///
    /// In `no_std` environments, every `TestRunner` will use the same
    /// hard-coded seed. This seed is not contractually guaranteed and may be
    /// changed between releases without notice.
    pub fn new(config: Config) -> Self {
        let seed = config.rng_seed;
        let algorithm = config.rng_algorithm;
        TestRunner::new_with_rng(config, TestRng::default_rng(seed, algorithm))
    }

    /// Create a fresh `TestRunner` with the standard deterministic RNG.
    ///
    /// This is sugar for the following:
    ///
    /// ```rust
    /// # use proptest::test_runner::*;
    /// let config = Config::default();
    /// let algorithm = config.rng_algorithm;
    /// TestRunner::new_with_rng(
    ///     config,
    ///     TestRng::deterministic_rng(algorithm));
    /// ```
    ///
    /// Refer to `TestRng::deterministic_rng()` for more information on the
    /// properties of the RNG used here.
    pub fn deterministic() -> Self {
        let config = Config::default();
        let algorithm = config.rng_algorithm;
        TestRunner::new_with_rng(config, TestRng::deterministic_rng(algorithm))
    }

    /// Create a fresh `TestRunner` with the given configuration and RNG.
    pub fn new_with_rng(config: Config, rng: TestRng) -> Self {
        TestRunner {
            config: config,
            successes: 0,
            local_rejects: 0,
            global_rejects: 0,
            rng: rng,
            flat_map_regens: Arc::new(AtomicUsize::new(0)),
            local_reject_detail: BTreeMap::new(),
            global_reject_detail: BTreeMap::new(),
        }
    }

    /// Create a fresh `TestRunner` with the same config and global counters as
    /// this one, but with local state reset and an independent `Rng` (but
    /// deterministic).
    pub(crate) fn partial_clone(&mut self) -> Self {
        TestRunner {
            config: self.config.clone(),
            successes: 0,
            local_rejects: 0,
            global_rejects: 0,
            rng: self.new_rng(),
            flat_map_regens: Arc::clone(&self.flat_map_regens),
            local_reject_detail: BTreeMap::new(),
            global_reject_detail: BTreeMap::new(),
        }
    }

    /// Returns the RNG for this test run.
    pub fn rng(&mut self) -> &mut TestRng {
        &mut self.rng
    }

    /// Create a new, independent but deterministic RNG from the RNG in this
    /// runner.
    pub fn new_rng(&mut self) -> TestRng {
        self.rng.gen_rng()
    }

    /// Returns the configuration of this runner.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Dumps the bytes obtained from the RNG so far (only works if the RNG is
    /// set to `Recorder`).
    ///
    /// ## Panics
    ///
    /// Panics if the RNG does not capture generated data.
    pub fn bytes_used(&self) -> Vec<u8> {
        self.rng.bytes_used()
    }

    /// Run test cases against `f`, choosing inputs via `strategy`.
    ///
    /// If any failure cases occur, try to find a minimal failure case and
    /// report that. If invoking `f` panics, the panic is turned into a
    /// `TestCaseError::Fail`.
    ///
    /// If failure persistence is enabled, all persisted failing cases are
    /// tested first. If a later non-persisted case fails, its seed is
    /// persisted before returning failure.
    ///
    /// Returns success or failure indicating why the test as a whole failed.
    pub fn run<S: Strategy>(
        &mut self,
        strategy: &S,
        test: impl Fn(S::Value) -> TestCaseResult,
    ) -> TestRunResult<S> {
        observe_run_start(&mut self.config);
        let result = if self.config.fork() {
            self.run_in_fork(strategy, test)
        } else {
            self.run_in_process(strategy, test)
        };
        observe_run_finish(&self.config, &result);
        result
    }

    #[cfg(not(feature = "fork"))]
    fn run_in_fork<S: Strategy>(
        &mut self,
        _: &S,
        _: impl Fn(S::Value) -> TestCaseResult,
    ) -> TestRunResult<S> {
        unreachable!()
    }

    #[cfg(feature = "fork")]
    fn run_in_fork<S: Strategy>(
        &mut self,
        strategy: &S,
        test: impl Fn(S::Value) -> TestCaseResult,
    ) -> TestRunResult<S> {
        let mut test = Some(test);

        let test_name = rusty_fork::fork_test::fix_module_path(
            self.config
                .test_name
                .expect("Must supply test_name when forking enabled"),
        );
        let forkfile: RefCell<Option<tempfile::NamedTempFile>> =
            RefCell::new(None);
        let init_forkfile_size = Cell::new(0u64);
        let seed = self.rng.new_rng_seed();
        let mut replay = replay::Replay {
            seed,
            steps: vec![],
        };
        let mut child_count = 0;
        let timeout = self.config.timeout();

        fn forkfile_size(forkfile: &Option<tempfile::NamedTempFile>) -> u64 {
            forkfile.as_ref().map_or(0, |ff| {
                ff.as_file().metadata().map(|md| md.len()).unwrap_or(0)
            })
        }

        loop {
            let (child_error, last_fork_file_len) = rusty_fork::fork(
                test_name,
                rusty_fork_id!(),
                |cmd| {
                    let mut forkfile = forkfile.borrow_mut();
                    if forkfile.is_none() {
                        *forkfile =
                            Some(tempfile::NamedTempFile::new().expect(
                                "Failed to create temporary file for fork",
                            ));
                        replay.init_file(forkfile.as_mut().unwrap()).expect(
                            "Failed to initialise temporary file for fork",
                        );
                    }

                    init_forkfile_size.set(forkfile_size(&forkfile));

                    cmd.env(ENV_FORK_FILE, forkfile.as_ref().unwrap().path());
                },
                |child, _| {
                    await_child(
                        child,
                        &mut forkfile.borrow_mut().as_mut().unwrap(),
                        timeout,
                    )
                },
                || match self.run_in_process(strategy, test.take().unwrap()) {
                    Ok(_) => (),
                    Err(e) => panic!(
                        "Test failed normally in child process.\n{}\n{}",
                        e, self
                    ),
                },
            )
            .expect("Fork failed");

            let parsed = replay::Replay::parse_from(
                &mut forkfile.borrow_mut().as_mut().unwrap(),
            )
            .expect("Failed to re-read fork file");
            match parsed {
                replay::ReplayFileStatus::InProgress(new_replay) => {
                    replay = new_replay
                }
                replay::ReplayFileStatus::Terminated(new_replay) => {
                    replay = new_replay;
                    break;
                }
                replay::ReplayFileStatus::Corrupt => {
                    panic!("Child process corrupted replay file")
                }
            }

            let curr_forkfile_size = forkfile_size(&forkfile.borrow());

            // If the child failed to append *anything* to the forkfile, it
            // crashed or timed out before starting even one test case, so
            // bail.
            if curr_forkfile_size == init_forkfile_size.get() {
                return Err(TestError::Abort(
                    "Child process crashed or timed out before the first test \
                     started running; giving up."
                        .into(),
                ));
            }

            // The child only terminates early if it outright crashes or we
            // kill it due to timeout, so add a synthetic failure to the
            // output. But only do this if the length of the fork file is the
            // same as when we last saw it, or if the child was not killed due
            // to timeout. (This is because the child could have appended
            // something to the file after we gave up waiting for it but before
            // we were able to kill it).
            if last_fork_file_len.map_or(true, |last_fork_file_len| {
                last_fork_file_len == curr_forkfile_size
            }) {
                let error = Err(child_error.unwrap_or(TestCaseError::fail(
                    "Child process was terminated abruptly \
                     but with successful status",
                )));
                replay::append(forkfile.borrow_mut().as_mut().unwrap(), &error)
                    .expect("Failed to append to replay file");
                replay.steps.push(error);
            }

            // Bail if we've gone through too many processes in case the
            // shrinking process itself is crashing.
            child_count += 1;
            if child_count >= 10000 {
                return Err(TestError::Abort(
                    "Giving up after 10000 child processes crashed".into(),
                ));
            }
        }

        // Run through the steps in-process (without ever running the actual
        // tests) to produce the shrunken value and update the persistence
        // file.
        self.rng.set_seed(replay.seed);
        self.run_in_process_with_replay(
            strategy,
            |_| panic!("Ran past the end of the replay"),
            replay.steps.into_iter(),
            ForkOutput::empty(),
        )
    }

    fn run_in_process<S: Strategy>(
        &mut self,
        strategy: &S,
        test: impl Fn(S::Value) -> TestCaseResult,
    ) -> TestRunResult<S> {
        let (replay_steps, fork_output) = init_replay(&mut self.rng);
        self.run_in_process_with_replay(
            strategy,
            test,
            replay_steps.into_iter(),
            fork_output,
        )
    }

    fn run_in_process_with_replay<S: Strategy>(
        &mut self,
        strategy: &S,
        test: impl Fn(S::Value) -> TestCaseResult,
        mut replay_from_fork: impl Iterator<Item = TestCaseResult>,
        mut fork_output: ForkOutput,
    ) -> TestRunResult<S> {
        let old_rng = self.rng.clone();

        let persisted_failure_seeds: Vec<PersistedSeed> = self
            .config
            .failure_persistence
            .as_ref()
            .map(|f| f.load_persisted_failures2(self.config.source_file))
            .unwrap_or_default();

        let mut result_cache = self.new_cache();

        for PersistedSeed(persisted_seed) in
            persisted_failure_seeds.into_iter().rev()
        {
            self.rng.set_seed(persisted_seed);
            let result = self.gen_and_run_case(
                strategy,
                &test,
                &mut replay_from_fork,
                &mut *result_cache,
                &mut fork_output,
                true,
            );

            // Terminate the replay, as a new case's failure does below. A child
            // that exits without doing so looks to the parent like a crash,
            // and the parent forks another to replay it instead of reporting.
            if let Err(e) = result {
                fork_output.terminate();
                return Err(e);
            }
        }
        self.rng = old_rng;

        while self.successes < self.config.cases {
            // Generate a new seed and make an RNG from that so that we know
            // what seed to persist if this case fails.
            let seed = self.rng.gen_get_seed();
            let result = self.gen_and_run_case(
                strategy,
                &test,
                &mut replay_from_fork,
                &mut *result_cache,
                &mut fork_output,
                false,
            );
            if let Err(TestError::Fail(_, ref value)) = result {
                if let Some(ref mut failure_persistence) =
                    self.config.failure_persistence
                {
                    let source_file = &self.config.source_file;

                    // Don't update the persistence file if we're a child
                    // process. The parent relies on it remaining consistent
                    // and will take care of updating it itself.
                    if !fork_output.is_in_fork() {
                        failure_persistence.save_persisted_failure2(
                            *source_file,
                            PersistedSeed(seed),
                            value,
                        );
                    }
                }
            }

            if let Err(e) = result {
                fork_output.terminate();
                return Err(e.into());
            }
        }

        fork_output.terminate();
        Ok(())
    }

    fn gen_and_run_case<S: Strategy>(
        &mut self,
        strategy: &S,
        f: &impl Fn(S::Value) -> TestCaseResult,
        replay_from_fork: &mut impl Iterator<Item = TestCaseResult>,
        result_cache: &mut dyn ResultCache,
        fork_output: &mut ForkOutput,
        is_from_persisted_seed: bool,
    ) -> TestRunResult<S> {
        let case = unwrap_or!(strategy.new_tree(self), msg =>
                return Err(TestError::Abort(msg)));

        // The observer, if any, sees the test case but not the shrinking of a
        // failure, which happens inside `run_one_with_replay`.
        let started = observe_case_start(&self.config);
        let mut result_cache = NoteCacheHit::new(result_cache);
        let result = self.run_one_with_replay(
            case,
            f,
            replay_from_fork,
            &mut result_cache,
            fork_output,
            is_from_persisted_seed,
        );
        observe_case_finish(
            &self.config,
            started,
            &result,
            is_from_persisted_seed,
            result_cache.first_lookup_hit(),
        );

        // We only count new cases to our set of successful runs against
        // `PROPTEST_CASES` config.
        let ok_type = result?;
        match ok_type {
            TestCaseOk::NewCaseSuccess | TestCaseOk::ReplayFromForkSuccess => {
                self.successes += 1
            }
            TestCaseOk::PersistedCaseSuccess
            | TestCaseOk::CacheHitSuccess
            | TestCaseOk::Reject => (),
        }

        Ok(())
    }

    /// Run one specific test case against this runner.
    ///
    /// If the test fails, finds the minimal failing test case. If the test
    /// does not fail, returns whether it succeeded or was filtered out.
    ///
    /// This does not honour the `fork` config, and will not be able to
    /// terminate the run if it runs for longer than `timeout`. However, if the
    /// test function returns but took longer than `timeout`, the test case
    /// will fail.
    ///
    /// The observer in [`Config::observer`] is not told about this test case.
    pub fn run_one<V: ValueTree>(
        &mut self,
        case: V,
        test: impl Fn(V::Value) -> TestCaseResult,
    ) -> Result<bool, TestError<V::Value>> {
        let mut result_cache = self.new_cache();
        self.run_one_with_replay(
            case,
            test,
            &mut iter::empty::<TestCaseResult>().fuse(),
            &mut *result_cache,
            &mut ForkOutput::empty(),
            false,
        )
        .map(|ok_type| match ok_type {
            TestCaseOk::Reject => false,
            _ => true,
        })
    }

    fn run_one_with_replay<V: ValueTree>(
        &mut self,
        mut case: V,
        test: impl Fn(V::Value) -> TestCaseResult,
        replay_from_fork: &mut impl Iterator<Item = TestCaseResult>,
        result_cache: &mut dyn ResultCache,
        fork_output: &mut ForkOutput,
        is_from_persisted_seed: bool,
    ) -> Result<TestCaseOk, TestError<V::Value>> {
        let result = call_test(
            self,
            case.current(),
            &test,
            replay_from_fork,
            result_cache,
            fork_output,
            is_from_persisted_seed,
        );

        match result {
            Ok(success_type) => Ok(success_type),
            Err(TestCaseError::Fail(why)) => {
                let why = self
                    .shrink(
                        &mut case,
                        test,
                        replay_from_fork,
                        result_cache,
                        fork_output,
                        is_from_persisted_seed,
                    )
                    .unwrap_or(why);
                Err(TestError::Fail(why, case.current()))
            }
            Err(TestCaseError::Reject(whence)) => {
                self.reject_global(whence)?;
                Ok(TestCaseOk::Reject)
            }
        }
    }

    fn shrink<V: ValueTree>(
        &mut self,
        case: &mut V,
        test: impl Fn(V::Value) -> TestCaseResult,
        replay_from_fork: &mut impl Iterator<Item = TestCaseResult>,
        result_cache: &mut dyn ResultCache,
        fork_output: &mut ForkOutput,
        is_from_persisted_seed: bool,
    ) -> Option<Reason> {
        // exit early if shrink disabled
        if self.config.max_shrink_iters == 0 {
            verbose_message!(
                self,
                INFO_LOG,
                "Shrinking disabled by configuration"
            );
            return None
        }

        #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
        let start_time = std::time::Instant::now();
        let mut last_failure = None;
        let mut iterations = 0;

        verbose_message!(self, TRACE, "Starting shrinking");

        if case.simplify() {
            loop {
                let mut timed_out: Option<u64> = None;
                #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
                if self.config.max_shrink_time > 0 {
                    let elapsed = start_time.elapsed();
                    let elapsed_ms = elapsed
                        .as_secs()
                        .saturating_mul(1000)
                        .saturating_add(elapsed.subsec_millis().into());
                    if elapsed_ms > self.config.max_shrink_time as u64 {
                        timed_out = Some(elapsed_ms);
                    }
                }

                let bail = if iterations >= self.config.max_shrink_iters() {
                    #[cfg(feature = "std")]
                    const CONTROLLER: &str =
                        "the PROPTEST_MAX_SHRINK_ITERS environment \
                         variable or ProptestConfig.max_shrink_iters";
                    #[cfg(not(feature = "std"))]
                    const CONTROLLER: &str = "ProptestConfig.max_shrink_iters";
                    verbose_message!(
                        self,
                        ALWAYS,
                        "Aborting shrinking after {} iterations (set {} \
                         to a large(r) value to shrink more; current \
                         configuration: {} iterations)",
                        CONTROLLER,
                        self.config.max_shrink_iters(),
                        iterations
                    );
                    true
                } else if let Some(ms) = timed_out {
                    #[cfg(feature = "std")]
                    const CONTROLLER: &str =
                        "the PROPTEST_MAX_SHRINK_TIME environment \
                         variable or ProptestConfig.max_shrink_time";
                    #[cfg(feature = "std")]
                    let current = self.config.max_shrink_time;
                    #[cfg(not(feature = "std"))]
                    const CONTROLLER: &str = "(not configurable in no_std)";
                    #[cfg(not(feature = "std"))]
                    let current = 0;
                    verbose_message!(
                        self,
                        ALWAYS,
                        "Aborting shrinking after taking too long: {} ms \
                         (set {} to a large(r) value to shrink more; current \
                         configuration: {} ms)",
                        ms,
                        CONTROLLER,
                        current
                    );
                    true
                } else {
                    false
                };

                if bail {
                    // Move back to the most recent failing case
                    while case.complicate() {
                        fork_output.append(&Ok(()));
                    }
                    break;
                }

                iterations += 1;

                let result = call_test(
                    self,
                    case.current(),
                    &test,
                    replay_from_fork,
                    result_cache,
                    fork_output,
                    is_from_persisted_seed,
                );

                match result {
                    // Rejections are effectively a pass here,
                    // since they indicate that any behaviour of
                    // the function under test is acceptable.
                    Ok(_) | Err(TestCaseError::Reject(..)) => {
                        if !case.complicate() {
                            verbose_message!(
                                self,
                                TRACE,
                                "Cannot complicate further"
                            );

                            break;
                        }
                    }
                    Err(TestCaseError::Fail(why)) => {
                        last_failure = Some(why);
                        if !case.simplify() {
                            verbose_message!(
                                self,
                                TRACE,
                                "Cannot simplify further"
                            );

                            break;
                        }
                    }
                }
            }
        }

        last_failure
    }

    /// Update the state to account for a local rejection from `whence`, and
    /// return `Ok` if the caller should keep going or `Err` to abort.
    pub fn reject_local(
        &mut self,
        whence: impl Into<Reason>,
    ) -> Result<(), Reason> {
        if self.local_rejects >= self.config.max_local_rejects {
            Err("Too many local rejects".into())
        } else {
            self.local_rejects += 1;
            Self::insert_or_increment(
                &mut self.local_reject_detail,
                whence.into(),
            );
            Ok(())
        }
    }

    /// Update the state to account for a global rejection from `whence`, and
    /// return `Ok` if the caller should keep going or `Err` to abort.
    fn reject_global<T>(&mut self, whence: Reason) -> Result<(), TestError<T>> {
        if self.global_rejects >= self.config.max_global_rejects {
            Err(TestError::Abort("Too many global rejects".into()))
        } else {
            self.global_rejects += 1;
            Self::insert_or_increment(&mut self.global_reject_detail, whence);
            Ok(())
        }
    }

    /// Insert 1 or increment the rejection detail at key for whence.
    fn insert_or_increment(into: &mut RejectionDetail, whence: Reason) {
        into.entry(whence)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }

    /// Increment the counter of flat map regenerations and return whether it
    /// is still under the configured limit.
    pub fn flat_map_regen(&self) -> bool {
        self.flat_map_regens.fetch_add(1, SeqCst)
            < self.config.max_flat_map_regens as usize
    }

    fn new_cache(&self) -> Box<dyn ResultCache> {
        (self.config.result_cache)()
    }
}

#[cfg(feature = "fork")]
fn init_replay(rng: &mut TestRng) -> (Vec<TestCaseResult>, ForkOutput) {
    use crate::test_runner::replay::{open_file, Replay, ReplayFileStatus::*};

    if let Some(path) = env::var_os(ENV_FORK_FILE) {
        let mut file = open_file(&path).expect("Failed to open replay file");
        let loaded =
            Replay::parse_from(&mut file).expect("Failed to read replay file");
        match loaded {
            InProgress(replay) => {
                rng.set_seed(replay.seed);
                (replay.steps, ForkOutput { file: Some(file) })
            }

            Terminated(_) => {
                panic!("Replay file for child process is terminated?")
            }

            Corrupt => panic!("Replay file for child process is corrupt"),
        }
    } else {
        (vec![], ForkOutput::empty())
    }
}

#[cfg(not(feature = "fork"))]
fn init_replay(
    _rng: &mut TestRng,
) -> (iter::Empty<TestCaseResult>, ForkOutput) {
    (iter::empty(), ForkOutput::empty())
}

#[cfg(feature = "fork")]
fn await_child_without_timeout(
    child: &mut rusty_fork::ChildWrapper,
) -> (Option<TestCaseError>, Option<u64>) {
    let status = child.wait().expect("Failed to wait for child process");

    if status.success() {
        (None, None)
    } else {
        (
            Some(TestCaseError::fail(format!(
                "Child process exited with {}",
                status
            ))),
            None,
        )
    }
}

#[cfg(all(feature = "fork", not(feature = "timeout")))]
fn await_child(
    child: &mut rusty_fork::ChildWrapper,
    _: &mut tempfile::NamedTempFile,
    _timeout: u32,
) -> (Option<TestCaseError>, Option<u64>) {
    await_child_without_timeout(child)
}

#[cfg(all(feature = "fork", feature = "timeout"))]
fn await_child(
    child: &mut rusty_fork::ChildWrapper,
    forkfile: &mut tempfile::NamedTempFile,
    timeout: u32,
) -> (Option<TestCaseError>, Option<u64>) {
    use std::time::Duration;

    if 0 == timeout {
        return await_child_without_timeout(child);
    }

    // The child can run for longer than the timeout since it may run
    // multiple tests. Each time the timeout expires, we check whether the
    // file has grown larger. If it has, we allow the child to keep running
    // until the next timeout.
    let mut last_forkfile_len = forkfile
        .as_file()
        .metadata()
        .map(|md| md.len())
        .unwrap_or(0);

    loop {
        if let Some(status) = child
            .wait_timeout(Duration::from_millis(timeout.into()))
            .expect("Failed to wait for child process")
        {
            if status.success() {
                return (None, None);
            } else {
                return (
                    Some(TestCaseError::fail(format!(
                        "Child process exited with {}",
                        status
                    ))),
                    None,
                );
            }
        }

        let current_len = forkfile
            .as_file()
            .metadata()
            .map(|md| md.len())
            .unwrap_or(0);
        // If we've gone a full timeout period without the file growing,
        // fail the test and kill the child.
        if current_len <= last_forkfile_len {
            return (
                Some(TestCaseError::fail(format!(
                    "Timed out waiting for child process"
                ))),
                Some(current_len),
            );
        } else {
            last_forkfile_len = current_len;
        }
    }
}

#[cfg(test)]
mod test {
    use std::cell::Cell;
    use std::fs;

    use super::*;
    use crate::strategy::Strategy;
    use crate::test_runner::{FileFailurePersistence, RngAlgorithm, TestRng};

    #[cfg(feature = "std")]
    mod observer {
        use super::*;
        use crate::test_runner::{
            FailurePersistence, MapFailurePersistence, ResultSource,
            RunOutcome, Seed, TestCaseInput, TestCaseObserver, TestCaseOutcome,
            TestCaseVerdict,
        };
        use std::sync::Mutex;

        /// One call to the observer.
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum Event {
            RunStarted(Option<&'static str>),
            CaseStarted,
            CaseFinished(TestCaseOutcome),
            RunFinished(RunOutcome),
        }

        /// One run as the observer saw it: the test name, the outcome of
        /// each test case, and how the run ended.
        type Run = (Option<&'static str>, Vec<TestCaseOutcome>, RunOutcome);

        /// Records every call, and fails the test if it is called in a child
        /// process that `fork` started.
        #[derive(Debug, Default)]
        struct Recorder(Mutex<Vec<Event>>);

        impl Recorder {
            fn push(&self, event: Event) {
                #[cfg(feature = "fork")]
                assert!(
                    env::var_os(ENV_FORK_FILE).is_none(),
                    "observer called in a child process"
                );
                self.0.lock().unwrap().push(event);
            }

            /// Checks that the calls come in the documented order, and that
            /// a duration is given exactly when the test body ran in this
            /// process; returns the runs.
            fn runs(&self) -> Vec<Run> {
                let events = self.0.lock().unwrap().clone();
                let mut runs = vec![];
                let mut events = events.into_iter();
                while let Some(event) = events.next() {
                    let Event::RunStarted(name) = event else {
                        panic!("expected the start of a run, got {:?}", event);
                    };
                    let mut outcomes = vec![];
                    loop {
                        match events.next() {
                            Some(Event::CaseStarted) => {
                                let Some(Event::CaseFinished(outcome)) =
                                    events.next()
                                else {
                                    panic!("a test case never finished");
                                };
                                assert_eq!(
                                    outcome.duration.is_some(),
                                    outcome.result_from
                                        == ResultSource::TestBody,
                                    "{:?}",
                                    outcome
                                );
                                outcomes.push(outcome);
                            }
                            Some(Event::RunFinished(how)) => {
                                runs.push((name, outcomes, how));
                                break;
                            }
                            other => panic!(
                                "expected a test case or the run's end: {:?}",
                                other
                            ),
                        }
                    }
                }
                runs
            }

            /// The one run: what happened to each test case (without the
            /// durations), and how the run ended.
            fn run(&self) -> (Vec<Kind>, RunOutcome) {
                let runs = self.runs();
                assert_eq!(runs.len(), 1, "{:?}", runs);
                let (_, outcomes, how) = runs.into_iter().next().unwrap();
                (outcomes.iter().map(kind).collect(), how)
            }
        }

        impl TestCaseObserver for Recorder {
            fn run_started(&self, config: &Config) {
                self.push(Event::RunStarted(config.test_name));
            }

            fn case_started(&self) {
                self.push(Event::CaseStarted);
            }

            fn case_finished(&self, outcome: TestCaseOutcome) {
                self.push(Event::CaseFinished(outcome));
            }

            fn run_finished(&self, outcome: RunOutcome) {
                self.push(Event::RunFinished(outcome));
            }
        }

        /// What happened to a test case, without its duration.
        type Kind = (TestCaseInput, ResultSource, TestCaseVerdict);

        fn kind(outcome: &TestCaseOutcome) -> Kind {
            (outcome.input, outcome.result_from, outcome.verdict)
        }

        fn config_with(recorder: &Arc<Recorder>) -> Config {
            Config {
                cases: 30,
                failure_persistence: None,
                rng_seed: RngSeed::Fixed(1),
                observer: Some(recorder.clone()),
                ..Config::default()
            }
        }

        /// A configuration with `persisted` persisted failures for the test
        /// `name`.
        fn with_persisted(
            config: Config,
            name: &'static str,
            persisted: u8,
        ) -> Config {
            let mut persistence = MapFailurePersistence::default();
            for i in 0..persisted {
                persistence.save_persisted_failure2(
                    Some(name),
                    PersistedSeed(Seed::from_bytes(
                        RngAlgorithm::ChaCha,
                        &[i; 32],
                    )),
                    &(),
                );
            }
            Config {
                failure_persistence: Some(Box::new(persistence)),
                source_file: Some(name),
                rng_algorithm: RngAlgorithm::ChaCha,
                ..config
            }
        }

        /// Gives the inputs 0, 0, 1, 2, ..., one per test case, so that
        /// with the result cache the second test case is a cache hit.
        #[derive(Debug, Default)]
        struct RepeatFirst(AtomicUsize);

        impl Strategy for RepeatFirst {
            type Tree = Just<usize>;
            type Value = usize;

            fn new_tree(&self, _: &mut TestRunner) -> NewTree<Self> {
                let n = self.0.fetch_add(1, SeqCst);
                Ok(Just(n.saturating_sub(1)))
            }
        }

        fn pass() -> TestCaseResult {
            Ok(())
        }

        fn reject() -> TestCaseResult {
            Err(TestCaseError::reject("rejected"))
        }

        fn fail() -> TestCaseResult {
            Err(TestCaseError::fail("failed"))
        }

        #[test]
        fn sees_every_test_case_in_order() {
            let recorder = Arc::new(Recorder::default());
            let calls = Cell::new(0);
            TestRunner::new(config_with(&recorder))
                .run(&(0u32..100), |x| {
                    calls.set(calls.get() + 1);
                    prop_assume!(x % 3 != 0);
                    Ok(())
                })
                .unwrap();

            let (kinds, how) = recorder.run();
            let count = |verdict| {
                let k =
                    (TestCaseInput::Generated, ResultSource::TestBody, verdict);
                kinds.iter().filter(|&&x| x == k).count()
            };
            assert_eq!(kinds.len(), calls.get());
            assert_eq!(count(TestCaseVerdict::Pass), 30);
            assert_eq!(count(TestCaseVerdict::Reject), calls.get() - 30);
            assert!(count(TestCaseVerdict::Reject) > 0);
            assert_eq!(how, RunOutcome::Passed);
        }

        #[test]
        fn does_not_see_shrinking() {
            let recorder = Arc::new(Recorder::default());
            let calls = Cell::new(0);
            let result = TestRunner::new(config_with(&recorder)).run(
                &(0u32..1000),
                |x| {
                    calls.set(calls.get() + 1);
                    prop_assert!(x < 500);
                    Ok(())
                },
            );
            assert!(matches!(result, Err(TestError::Fail(..))));

            // One failure, reported last; the shrinking calls before it run
            // the test body but are not reported.
            let (kinds, how) = recorder.run();
            let fail = (
                TestCaseInput::Generated,
                ResultSource::TestBody,
                TestCaseVerdict::Fail,
            );
            assert_eq!(kinds.iter().filter(|&&k| k == fail).count(), 1);
            assert_eq!(kinds.last(), Some(&fail));
            assert!(calls.get() > kinds.len());
            assert_eq!(how, RunOutcome::Failed);
        }

        /// Every way a test case can end without `fork`: its input is new or
        /// a persisted failure, its result comes from the test body or the
        /// result cache, and it passes, is rejected or fails.
        #[test]
        fn reports_every_kind_of_test_case() {
            use self::ResultSource::{ResultCache as C, TestBody as B};
            use self::RunOutcome::*;
            use self::TestCaseInput::{Generated as G, Persisted as P};
            use self::TestCaseVerdict::{Fail as F, Pass as S, Reject as R};

            // (persisted failures, result cache on, result of every test
            //  case, what the observer must be told, how the run ends)
            type Row = (
                u8,
                bool,
                fn() -> TestCaseResult,
                &'static [Kind],
                RunOutcome,
            );
            #[rustfmt::skip]
            let table: &[Row] = &[
                (0, false, pass, &[(G, B, S), (G, B, S)], Passed),
                (1, false, pass, &[(P, B, S), (G, B, S), (G, B, S)], Passed),
                // The inputs are 0, 0, 1, ...; a cache hit does not count
                // towards `cases`.
                (0, true, pass, &[(G, B, S), (G, C, S), (G, B, S)], Passed),
                (2, true, pass,
                 &[(P, B, S), (P, C, S), (G, B, S), (G, B, S)], Passed),
                // The third rejection is one too many.
                (0, false, reject, &[(G, B, R), (G, B, R), (G, B, R)], Aborted),
                (1, false, reject, &[(P, B, R), (G, B, R), (G, B, R)], Aborted),
                (3, false, reject, &[(P, B, R), (P, B, R), (P, B, R)], Aborted),
                (0, true, reject, &[(G, B, R), (G, C, R), (G, B, R)], Aborted),
                (2, true, reject, &[(P, B, R), (P, C, R), (G, B, R)], Aborted),
                (3, true, reject, &[(P, B, R), (P, C, R), (P, B, R)], Aborted),
                // A failure ends the run, so a failure cannot come from the
                // cache.
                (0, false, fail, &[(G, B, F)], Failed),
                (1, false, fail, &[(P, B, F)], Failed),
            ];
            for &(persisted, cache, result, expected, how) in table {
                let recorder = Arc::new(Recorder::default());
                let config = Config {
                    cases: 2,
                    max_global_rejects: 2,
                    result_cache: if cache {
                        basic_result_cache
                    } else {
                        noop_result_cache
                    },
                    ..config_with(&recorder)
                };
                let config =
                    with_persisted(config, "observer_every_kind", persisted);
                let _ = TestRunner::new(config)
                    .run(&RepeatFirst::default(), |_| result());
                assert_eq!(
                    recorder.run(),
                    (expected.to_vec(), how),
                    "{} persisted, cache {}, {:?}",
                    persisted,
                    cache,
                    result()
                );
            }
        }

        /// A failure is reported with the source of its own result, even if
        /// shrinking it then finds a result in the cache.
        #[test]
        fn a_cache_hit_while_shrinking_is_not_the_test_cases_own() {
            let recorder = Arc::new(Recorder::default());
            // Input 0 passes and is cached; input 1 fails, and shrinking it
            // tries 0, which the cache answers.
            let _ = TestRunner::new(Config {
                result_cache: basic_result_cache,
                ..config_with(&recorder)
            })
            .run(&Count::default(), |x| {
                prop_assert!(x == 0);
                Ok(())
            });
            assert_eq!(
                recorder.run(),
                (
                    vec![
                        (
                            TestCaseInput::Generated,
                            ResultSource::TestBody,
                            TestCaseVerdict::Pass
                        ),
                        (
                            TestCaseInput::Generated,
                            ResultSource::TestBody,
                            TestCaseVerdict::Fail
                        ),
                    ],
                    RunOutcome::Failed
                )
            );
        }

        /// Runs `test` with `fork` after `persisted` persisted failures, and
        /// returns what the observer in this, the parent, process was told.
        #[cfg(feature = "fork")]
        fn observe_in_fork(
            name: &'static str,
            persisted: u8,
            test: impl Fn(u32) -> TestCaseResult,
        ) -> (Vec<Kind>, RunOutcome) {
            let recorder = Arc::new(Recorder::default());
            let config = Config {
                fork: true,
                test_name: Some(name),
                cases: 2,
                max_global_rejects: 2,
                ..config_with(&recorder)
            };
            let _ = TestRunner::new(with_persisted(config, name, persisted))
                .run(&(0u32..1000), test);
            recorder.run()
        }

        /// One test per row, since `fork` runs the test again in a child
        /// process, by name: (persisted failures, result of every test case,
        /// what the observer must be told, how the run ends).
        #[cfg(feature = "fork")]
        macro_rules! fork_rows {
            ($($name:ident: ($persisted:expr, $result:expr, $expected:expr,
                             $how:expr);)*) => {$(
                #[test]
                fn $name() {
                    use self::ResultSource::ChildProcess as C;
                    use self::RunOutcome::*;
                    use self::TestCaseInput::{Generated as G, Persisted as P};
                    use self::TestCaseVerdict::{Fail as F, Pass as S};
                    use self::TestCaseVerdict::Reject as R;
                    let _ = (G, P, F, S, R, C, Passed);
                    let name = concat!(module_path!(), "::", stringify!($name));
                    assert_eq!(
                        observe_in_fork(name, $persisted, |_| $result()),
                        ($expected.to_vec(), $how)
                    );
                }
            )*};
        }

        #[cfg(feature = "fork")]
        fork_rows! {
            fork_new_pass: (0, pass, [(G, C, S), (G, C, S)], Passed);
            // The parent process counts the persisted failure's replay towards
            // `cases`, unlike the child (existing runner behaviour), so only
            // one new test case follows it.
            fork_persisted_pass: (1, pass, [(P, C, S), (G, C, S)], Passed);
            fork_new_reject:
                (0, reject, [(G, C, R), (G, C, R), (G, C, R)], Aborted);
            fork_persisted_reject:
                (1, reject, [(P, C, R), (G, C, R), (G, C, R)], Aborted);
            fork_new_fail: (0, fail, [(G, C, F)], Failed);
            fork_persisted_fail: (1, fail, [(P, C, F)], Failed);
        }

        /// Gives the inputs 0, 1, 2, ..., one per test case, each shrinking
        /// towards 0.
        #[derive(Debug, Default)]
        struct Count(AtomicUsize);

        impl Strategy for Count {
            type Tree = crate::num::u32::BinarySearch;
            type Value = u32;

            fn new_tree(&self, _: &mut TestRunner) -> NewTree<Self> {
                let n = self.0.fetch_add(1, SeqCst) as u32;
                Ok(crate::num::u32::BinarySearch::new(n))
            }
        }

        /// A child process that crashes neither reports anything itself nor
        /// leaves a test case unfinished. The next child process replays the
        /// test cases before the crash, and the parent reports them once, then
        /// the crash as a failure.
        #[cfg(feature = "fork")]
        #[test]
        fn fork_child_process_crashes() {
            let recorder = Arc::new(Recorder::default());
            let name = concat!(module_path!(), "::fork_child_process_crashes");
            let result = TestRunner::new(Config {
                fork: true,
                test_name: Some(name),
                ..config_with(&recorder)
            })
            .run(&Count::default(), |x| {
                if x == 3 {
                    std::process::abort();
                }
                Ok(())
            });
            assert!(matches!(result, Err(TestError::Fail(..))));

            let child = |verdict| {
                (
                    TestCaseInput::Generated,
                    ResultSource::ChildProcess,
                    verdict,
                )
            };
            assert_eq!(
                recorder.run(),
                (
                    vec![
                        child(TestCaseVerdict::Pass),
                        child(TestCaseVerdict::Pass),
                        child(TestCaseVerdict::Pass),
                        child(TestCaseVerdict::Fail),
                    ],
                    RunOutcome::Failed
                )
            );
        }

        #[test]
        fn reports_no_test_case_when_no_input_can_be_generated() {
            #[derive(Debug)]
            struct NoInput;

            impl Strategy for NoInput {
                type Tree = Just<u32>;
                type Value = u32;

                fn new_tree(&self, _: &mut TestRunner) -> NewTree<Self> {
                    Err("no input".into())
                }
            }

            let recorder = Arc::new(Recorder::default());
            let result = TestRunner::new(config_with(&recorder))
                .run(&NoInput, |_| Ok(()));
            assert!(matches!(result, Err(TestError::Abort(..))));
            assert_eq!(recorder.run(), (vec![], RunOutcome::Aborted));
        }

        #[test]
        fn sees_the_start_and_end_of_each_run() {
            let recorder = Arc::new(Recorder::default());
            let config = Config {
                test_name: Some("observer::a_test"),
                ..config_with(&recorder)
            };
            // Two runs, each with its own runner, reported to one observer:
            // one passes, one fails.
            TestRunner::new(config.clone())
                .run(&(0u32..1000), |_| Ok(()))
                .unwrap();
            let result = TestRunner::new(config).run(&(0u32..1000), |x| {
                prop_assert!(x < 500);
                Ok(())
            });
            assert!(result.is_err());

            let runs = recorder.runs();
            assert_eq!(runs.len(), 2);
            assert_eq!(runs[0].0, Some("observer::a_test"));
            assert_eq!(runs[0].1.len(), 30);
            assert_eq!(runs[0].2, RunOutcome::Passed);
            assert_eq!(runs[1].0, Some("observer::a_test"));
            assert_eq!(runs[1].2, RunOutcome::Failed);
        }

        #[test]
        fn run_one_reports_nothing() {
            let recorder = Arc::new(Recorder::default());
            TestRunner::new(config_with(&recorder))
                .run_one(Just(0u32), |_| Ok(()))
                .unwrap();
            assert!(recorder.runs().is_empty());
        }

        #[test]
        fn an_observer_that_implements_nothing_changes_nothing() {
            #[derive(Debug)]
            struct Silent;

            impl TestCaseObserver for Silent {}

            let config = Config {
                cases: 30,
                failure_persistence: None,
                rng_seed: RngSeed::Fixed(1),
                ..Config::default()
            };
            let test = |x: u32| {
                prop_assume!(x % 3 != 0);
                prop_assert!(x < 900);
                Ok(())
            };
            let run = |config| {
                let mut runner = TestRunner::new(config);
                let result = runner.run(&(0u32..1000), test);
                (
                    format!("{:?}", result),
                    runner.successes,
                    runner.global_rejects,
                )
            };
            let without = run(config.clone());
            let with = run(Config {
                observer: Some(Arc::new(Silent)),
                ..config
            });
            assert_eq!(with, without);
            assert!(without.1 > 0 && without.2 > 0);
        }

        #[test]
        fn configs_are_equal_only_with_the_same_observer() {
            let recorder = Arc::new(Recorder::default());
            let other = Arc::new(Recorder::default());
            assert_eq!(config_with(&recorder), config_with(&recorder));
            assert_ne!(config_with(&recorder), config_with(&other));
            assert_ne!(
                config_with(&recorder),
                Config {
                    observer: None,
                    ..config_with(&recorder)
                }
            );
        }
    }

    #[test]
    fn gives_up_after_too_many_rejections() {
        let config = Config::default();
        let mut runner = TestRunner::new(config.clone());
        let runs = Cell::new(0);
        let result = runner.run(&(0u32..), |_| {
            runs.set(runs.get() + 1);
            Err(TestCaseError::reject("reject"))
        });
        match result {
            Err(TestError::Abort(_)) => (),
            e => panic!("Unexpected result: {:?}", e),
        }
        assert_eq!(config.max_global_rejects + 1, runs.get());
    }

    #[test]
    fn test_pass() {
        let mut runner = TestRunner::default();
        let result = runner.run(&(1u32..), |v| {
            assert!(v > 0);
            Ok(())
        });
        assert_eq!(Ok(()), result);
    }

    #[test]
    fn test_fail_via_result() {
        let mut runner = TestRunner::new(Config {
            failure_persistence: None,
            ..Config::default()
        });
        let result = runner.run(&(0u32..10u32), |v| {
            if v < 5 {
                Ok(())
            } else {
                Err(TestCaseError::fail("not less than 5"))
            }
        });

        assert_eq!(Err(TestError::Fail("not less than 5".into(), 5)), result);
    }

    #[test]
    fn test_fail_via_panic() {
        let mut runner = TestRunner::new(Config {
            failure_persistence: None,
            ..Config::default()
        });
        let result = runner.run(&(0u32..10u32), |v| {
            assert!(v < 5, "not less than 5");
            Ok(())
        });
        assert_eq!(Err(TestError::Fail("not less than 5".into(), 5)), result);
    }

    #[test]
    fn persisted_cases_do_not_count_towards_total_cases() {
        const FILE: &'static str = "persistence-test.txt";
        let _ = fs::remove_file(FILE);

        let config = Config {
            failure_persistence: Some(Box::new(
                FileFailurePersistence::Direct(FILE),
            )),
            cases: 1,
            ..Config::default()
        };

        let max = 10_000_000i32;
        {
            TestRunner::new(config.clone())
                .run(&(0i32..max), |_v| {
                    Err(TestCaseError::Fail("persist a failure".into()))
                })
                .expect_err("didn't fail?");
        }

        let run_count = RefCell::new(0);
        TestRunner::new(config.clone())
            .run(&(0i32..max), |_v| {
                *run_count.borrow_mut() += 1;
                Ok(())
            })
            .expect("should succeed");

        // Persisted ran, and a new case ran, and only new case counts
        // against `cases: 1`.
        assert_eq!(run_count.into_inner(), 2);
    }

    #[derive(Clone, Copy, PartialEq)]
    struct PoorlyBehavedDebug(i32);
    impl fmt::Debug for PoorlyBehavedDebug {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "\r\n{:?}\r\n", self.0)
        }
    }

    #[test]
    fn failing_cases_persisted_and_reloaded() {
        const FILE: &'static str = "persistence-test.txt";
        let _ = fs::remove_file(FILE);

        let max = 10_000_000i32;
        let input = (0i32..max).prop_map(PoorlyBehavedDebug);
        let config = Config {
            failure_persistence: Some(Box::new(
                FileFailurePersistence::Direct(FILE),
            )),
            ..Config::default()
        };

        // First test with cases that fail above half max, and then below half
        // max, to ensure we can correctly parse both lines of the persistence
        // file.
        let first_sub_failure = {
            TestRunner::new(config.clone())
                .run(&input, |v| {
                    if v.0 < max / 2 {
                        Ok(())
                    } else {
                        Err(TestCaseError::Fail("too big".into()))
                    }
                })
                .expect_err("didn't fail?")
        };
        let first_super_failure = {
            TestRunner::new(config.clone())
                .run(&input, |v| {
                    if v.0 >= max / 2 {
                        Ok(())
                    } else {
                        Err(TestCaseError::Fail("too small".into()))
                    }
                })
                .expect_err("didn't fail?")
        };
        let second_sub_failure = {
            TestRunner::new(config.clone())
                .run(&input, |v| {
                    if v.0 < max / 2 {
                        Ok(())
                    } else {
                        Err(TestCaseError::Fail("too big".into()))
                    }
                })
                .expect_err("didn't fail?")
        };
        let second_super_failure = {
            TestRunner::new(config.clone())
                .run(&input, |v| {
                    if v.0 >= max / 2 {
                        Ok(())
                    } else {
                        Err(TestCaseError::Fail("too small".into()))
                    }
                })
                .expect_err("didn't fail?")
        };

        assert_eq!(first_sub_failure, second_sub_failure);
        assert_eq!(first_super_failure, second_super_failure);
    }

    #[test]
    fn new_rng_makes_separate_rng() {
        use rand::RngExt;
        let mut runner = TestRunner::default();
        let from_1 = runner.new_rng().random::<[u8; 16]>();
        let from_2 = runner.rng().random::<[u8; 16]>();
        assert_ne!(from_1, from_2);
    }

    #[test]
    fn record_rng_use() {
        use rand::RngExt;

        // create value with recorder rng
        let default_config = Config::default();
        let recorder_rng = TestRng::default_rng(RngSeed::Random, RngAlgorithm::Recorder);
        let mut runner =
            TestRunner::new_with_rng(default_config.clone(), recorder_rng);
        let random_byte_array1 = runner.rng().random::<[u8; 16]>();
        let bytes_used = runner.bytes_used();
        assert!(bytes_used.len() >= 16); // could use more bytes for some reason

        // re-create value with pass-through rng
        let passthrough_rng =
            TestRng::from_seed(RngAlgorithm::PassThrough, &bytes_used);
        let mut runner =
            TestRunner::new_with_rng(default_config, passthrough_rng);
        let random_byte_array2 = runner.rng().random::<[u8; 16]>();

        // make sure the same value was created
        assert_eq!(random_byte_array1, random_byte_array2);
    }

    #[cfg(feature = "fork")]
    #[test]
    fn run_successful_test_in_fork() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            test_name: Some(concat!(
                module_path!(),
                "::run_successful_test_in_fork"
            )),
            ..Config::default()
        });

        assert!(runner.run(&(0u32..1000), |_| Ok(())).is_ok());
    }

    #[cfg(feature = "fork")]
    #[test]
    fn normal_failure_in_fork_results_in_correct_failure() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            test_name: Some(concat!(
                module_path!(),
                "::normal_failure_in_fork_results_in_correct_failure"
            )),
            ..Config::default()
        });

        let failure = runner
            .run(&(0u32..1000), |v| {
                prop_assert!(v < 500);
                Ok(())
            })
            .err()
            .unwrap();

        match failure {
            TestError::Fail(_, value) => assert_eq!(500, value),
            failure => panic!("Unexpected failure: {:?}", failure),
        }
    }

    #[cfg(feature = "fork")]
    #[test]
    fn nonsuccessful_exit_finds_correct_failure() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            test_name: Some(concat!(
                module_path!(),
                "::nonsuccessful_exit_finds_correct_failure"
            )),
            ..Config::default()
        });

        let failure = runner
            .run(&(0u32..1000), |v| {
                if v >= 500 {
                    ::std::process::exit(1);
                }
                Ok(())
            })
            .err()
            .unwrap();

        match failure {
            TestError::Fail(_, value) => assert_eq!(500, value),
            failure => panic!("Unexpected failure: {:?}", failure),
        }
    }

    #[cfg(feature = "fork")]
    #[test]
    fn spurious_exit_finds_correct_failure() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            test_name: Some(concat!(
                module_path!(),
                "::spurious_exit_finds_correct_failure"
            )),
            ..Config::default()
        });

        let failure = runner
            .run(&(0u32..1000), |v| {
                if v >= 500 {
                    ::std::process::exit(0);
                }
                Ok(())
            })
            .err()
            .unwrap();

        match failure {
            TestError::Fail(_, value) => assert_eq!(500, value),
            failure => panic!("Unexpected failure: {:?}", failure),
        }
    }

    #[cfg(feature = "timeout")]
    #[test]
    fn long_sleep_timeout_finds_correct_failure() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            timeout: 500,
            test_name: Some(concat!(
                module_path!(),
                "::long_sleep_timeout_finds_correct_failure"
            )),
            ..Config::default()
        });

        let failure = runner
            .run(&(0u32..1000), |v| {
                if v >= 500 {
                    ::std::thread::sleep(::std::time::Duration::from_millis(
                        10_000,
                    ));
                }
                Ok(())
            })
            .err()
            .unwrap();

        match failure {
            TestError::Fail(_, value) => assert_eq!(500, value),
            failure => panic!("Unexpected failure: {:?}", failure),
        }
    }

    #[cfg(feature = "timeout")]
    #[test]
    fn mid_sleep_timeout_finds_correct_failure() {
        let mut runner = TestRunner::new(Config {
            fork: true,
            timeout: 500,
            test_name: Some(concat!(
                module_path!(),
                "::mid_sleep_timeout_finds_correct_failure"
            )),
            ..Config::default()
        });

        let failure = runner
            .run(&(0u32..1000), |v| {
                if v >= 500 {
                    // Sleep a little longer than the timeout. This means that
                    // sometimes the test case itself will return before the parent
                    // process has noticed the child is timing out, so it's up to
                    // the child to mark it as a failure.
                    ::std::thread::sleep(::std::time::Duration::from_millis(
                        600,
                    ));
                } else {
                    // Sleep a bit so that the parent and child timing don't stay
                    // in sync.
                    ::std::thread::sleep(::std::time::Duration::from_millis(
                        100,
                    ))
                }
                Ok(())
            })
            .err()
            .unwrap();

        match failure {
            TestError::Fail(_, value) => assert_eq!(500, value),
            failure => panic!("Unexpected failure: {:?}", failure),
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn duplicate_tests_not_run_with_basic_result_cache() {
        use std::cell::{Cell, RefCell};
        use std::collections::HashSet;
        use std::rc::Rc;

        for _ in 0..256 {
            let mut runner = TestRunner::new(Config {
                failure_persistence: None,
                result_cache:
                    crate::test_runner::result_cache::basic_result_cache,
                ..Config::default()
            });
            let pass = Rc::new(Cell::new(true));
            let seen = Rc::new(RefCell::new(HashSet::new()));
            let result =
                runner.run(&(0u32..65536u32).prop_map(|v| v % 10), |val| {
                    if !seen.borrow_mut().insert(val) {
                        println!("Value {} seen more than once", val);
                        pass.set(false);
                    }

                    prop_assert!(val <= 5);
                    Ok(())
                });

            assert!(pass.get());
            if let Err(TestError::Fail(_, val)) = result {
                assert_eq!(6, val);
            } else {
                panic!("Incorrect result: {:?}", result);
            }
        }
    }
}

#[cfg(all(feature = "fork", feature = "timeout", test))]
mod timeout_tests {
    use core::u32;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::test_runner::failure_persistence::FailurePersistence;
    use crate::test_runner::rng::Seed;
    use crate::test_runner::{MapFailurePersistence, RngAlgorithm};

    rusty_fork_test! {
        #![rusty_fork(timeout_ms = 4_000)]

        #[test]
        fn max_shrink_iters_works() {
            test_shrink_bail(Config {
                max_shrink_iters: 5,
                .. Config::default()
            });
        }

        #[test]
        fn max_shrink_time_works() {
            test_shrink_bail(Config {
                max_shrink_time: 1000,
                .. Config::default()
            });
        }

        #[test]
        fn max_shrink_iters_works_with_forking() {
            test_shrink_bail(Config {
                fork: true,
                test_name: Some(
                    concat!(module_path!(),
                            "::max_shrink_iters_works_with_forking")),
                max_shrink_time: 1000,
                .. Config::default()
            });
        }

        #[test]
        fn persisted_failure_in_fork_is_reported() {
            let value = run_persisted_failure(Config {
                fork: true,
                test_name: Some(
                    concat!(module_path!(),
                            "::persisted_failure_in_fork_is_reported")),
                .. persisted_failure_config()
            }, 0);
            assert_eq!(0, value);
        }

        #[test]
        fn persisted_timeout_in_fork_is_reported() {
            // Every candidate times out, so any value the shrink stops at is
            // a failing case.
            run_persisted_failure(Config {
                timeout: 100,
                max_shrink_iters: 3,
                test_name: Some(
                    concat!(module_path!(),
                            "::persisted_timeout_in_fork_is_reported")),
                .. persisted_failure_config()
            }, 10_000);
        }

        #[test]
        fn detects_child_failure_to_start() {
            let mut runner = TestRunner::new(Config {
                timeout: 100,
                test_name: Some(
                    concat!(module_path!(),
                            "::detects_child_failure_to_start")),
                .. Config::default()
            });
            let result = runner.run(&Just(()).prop_map(|()| {
                thread::sleep(Duration::from_millis(200))
            }), Ok);

            if let Err(TestError::Abort(_)) = result {
                // OK
            } else {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }

    /// A configuration whose persistence already holds one failing seed, as
    /// though an earlier run had recorded it.
    fn persisted_failure_config() -> Config {
        const SOURCE_FILE: &str = "persisted_failure";
        let mut persistence = MapFailurePersistence::default();
        persistence.save_persisted_failure2(
            Some(SOURCE_FILE),
            PersistedSeed(Seed::from_bytes(RngAlgorithm::ChaCha, &[7; 32])),
            &(),
        );
        Config {
            failure_persistence: Some(Box::new(persistence)),
            source_file: Some(SOURCE_FILE),
            rng_algorithm: RngAlgorithm::ChaCha,
            ..Config::default()
        }
    }

    /// Run a test that fails every case, after sleeping for `sleep_ms` so
    /// that a timeout can be what fails it, and return the shrunken value.
    /// The persisted seed is replayed first, so this checks that its failure
    /// is reported, rather than aborted or replayed in child after child.
    fn run_persisted_failure(config: Config, sleep_ms: u64) -> u32 {
        let mut runner = TestRunner::new(config);
        let result = runner.run(&(0u32..1000), |_| {
            thread::sleep(Duration::from_millis(sleep_ms));
            Err(TestCaseError::fail("every case fails"))
        });

        match result {
            Err(TestError::Fail(_, value)) => value,
            result => panic!("Unexpected result: {:?}", result),
        }
    }

    fn test_shrink_bail(config: Config) {
        let mut runner = TestRunner::new(config);
        let result = runner.run(&crate::num::u64::ANY, |v| {
            thread::sleep(Duration::from_millis(250));
            prop_assert!(v <= u32::MAX as u64);
            Ok(())
        });

        if let Err(TestError::Fail(_, value)) = result {
            // Ensure the final value was in fact a failing case.
            assert!(value > u32::MAX as u64);
        } else {
            panic!("Unexpected result: {:?}", result);
        }
    }
}
