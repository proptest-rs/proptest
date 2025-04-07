//-
// Copyright 2017, 2018, 2019 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::std_facade::Box;
use core::{fmt, str, u32};

use crate::test_runner::result_cache::{noop_result_cache, ResultCache};
use crate::test_runner::rng::RngAlgorithm;
use crate::test_runner::FailurePersistence;

/// Override the config fields from environment variables, if any are set.
/// Without the `std` feature this function returns config unchanged.
#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
pub fn contextualize_config(mut result: Config) -> Config {
    use std::env;
    use std::ffi::OsString;
    use std::fmt;
    use std::str::FromStr;

    const CASES: &str = "PROPTEST_CASES";

    const MAX_LOCAL_REJECTS: &str = "PROPTEST_MAX_LOCAL_REJECTS";
    const MAX_GLOBAL_REJECTS: &str = "PROPTEST_MAX_GLOBAL_REJECTS";
    const MAX_FLAT_MAP_REGENS: &str = "PROPTEST_MAX_FLAT_MAP_REGENS";
    const MAX_SHRINK_TIME: &str = "PROPTEST_MAX_SHRINK_TIME";
    const MAX_SHRINK_ITERS: &str = "PROPTEST_MAX_SHRINK_ITERS";
    const MAX_DEFAULT_SIZE_RANGE: &str = "PROPTEST_MAX_DEFAULT_SIZE_RANGE";
    #[cfg(feature = "fork")]
    const FORK: &str = "PROPTEST_FORK";
    #[cfg(feature = "timeout")]
    const TIMEOUT: &str = "PROPTEST_TIMEOUT";
    const VERBOSE: &str = "PROPTEST_VERBOSE";
    const RNG_ALGORITHM: &str = "PROPTEST_RNG_ALGORITHM";
    const RNG_SEED: &str = "PROPTEST_RNG_SEED";
    const DISABLE_FAILURE_PERSISTENCE: &str =
        "PROPTEST_DISABLE_FAILURE_PERSISTENCE";

    fn parse_or_warn<T: FromStr + fmt::Display>(
        src: &OsString,
        dst: &mut T,
        typ: &str,
        var: &str,
    ) {
        if let Some(src) = src.to_str() {
            if let Ok(value) = src.parse() {
                *dst = value;
            } else {
                eprintln!(
                    "proptest: The env-var {}={} can't be parsed as {}, \
                     using default of {}.",
                    var, src, typ, *dst
                );
            }
        } else {
            eprintln!(
                "proptest: The env-var {} is not valid, using \
                 default of {}.",
                var, *dst
            );
        }
    }

    for (var, value) in
        env::vars_os().filter_map(|(k, v)| k.into_string().ok().map(|k| (k, v)))
    {
        let var = var.as_str();

        #[cfg(feature = "fork")]
        if var == FORK {
            parse_or_warn(&value, &mut result.fork, "bool", FORK);
            continue;
        }

        #[cfg(feature = "timeout")]
        if var == TIMEOUT {
            parse_or_warn(&value, &mut result.timeout, "timeout", TIMEOUT);
            continue;
        }

        if var == CASES {
            parse_or_warn(&value, &mut result.cases, "u32", CASES);
        } else if var == MAX_LOCAL_REJECTS {
            parse_or_warn(
                &value,
                &mut result.max_local_rejects,
                "u32",
                MAX_LOCAL_REJECTS,
            );
        } else if var == MAX_GLOBAL_REJECTS {
            parse_or_warn(
                &value,
                &mut result.max_global_rejects,
                "u32",
                MAX_GLOBAL_REJECTS,
            );
        } else if var == MAX_FLAT_MAP_REGENS {
            parse_or_warn(
                &value,
                &mut result.max_flat_map_regens,
                "u32",
                MAX_FLAT_MAP_REGENS,
            );
        } else if var == MAX_SHRINK_TIME {
            parse_or_warn(
                &value,
                &mut result.max_shrink_time,
                "u32",
                MAX_SHRINK_TIME,
            );
        } else if var == MAX_SHRINK_ITERS {
            parse_or_warn(
                &value,
                &mut result.max_shrink_iters,
                "u32",
                MAX_SHRINK_ITERS,
            );
        } else if var == MAX_DEFAULT_SIZE_RANGE {
            parse_or_warn(
                &value,
                &mut result.max_default_size_range,
                "usize",
                MAX_DEFAULT_SIZE_RANGE,
            );
        } else if var == VERBOSE {
            parse_or_warn(&value, &mut result.verbose, "u32", VERBOSE);
        } else if var == RNG_ALGORITHM {
            parse_or_warn(
                &value,
                &mut result.rng_algorithm,
                "RngAlgorithm",
                RNG_ALGORITHM,
            );
        } else if var == RNG_SEED {
            // this is a hacky workaround to deal with the fact that the
            // entire code path surrounding parsing and contextualizing
            // the RngSeed is only fallible within the parse function, however
            // RngSeed, specifically the hex-encoded version, needs to ensure
            // that the hex-encoded string matches the length of the seed that
            // the configured `RngAlgorithm` expects.
            //
            // to work around this, we'll stash the existing seed, attempt to parse
            // then attempt to validate, and if there is a validation failure,
            // reset the config value back to the existing seed
            let existing_seed = result.rng_seed;

            parse_or_warn(
                &value,
                &mut result.rng_seed,
                "RngSeed",
                RNG_SEED,
            );

            if let RngSeed::FullHexEncodedSeed(seed) = &result.rng_seed {
                match result.rng_algorithm {
                    RngAlgorithm::XorShift => {
                        // 16-byte seed, hex-encoded with 2 chars per byte
                        if seed.len() != 16 {
                            eprintln!("proptest: Invalid FullHexEncodedSeed length. Expected a 16-byte seed but got: {:?}, len={}", seed, seed.len());
                            result.rng_seed = existing_seed;
                        }
                    }
                    RngAlgorithm::ChaCha => {
                        // 32-byte seed, hex-encoded with 2 chars per byte
                        if seed.len() != 32 {
                            eprintln!("proptest: Invalid FullHexEncodedSeed length. Expected a 32-byte seed but got: {:?}, len={}", seed, seed.len());
                            result.rng_seed = existing_seed;
                        }
                    }
                    _ => {}
                }
            }

        } else if var == DISABLE_FAILURE_PERSISTENCE {
            result.failure_persistence = None;
        } else if var.starts_with("PROPTEST_") {
            eprintln!("proptest: Ignoring unknown env-var {}.", var);
        }
    }

    result
}

/// Without the `std` feature this function returns config unchanged.
#[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
pub fn contextualize_config(result: Config) -> Config {
    result
}

fn default_default_config() -> Config {
    Config {
        cases: 256,
        max_local_rejects: 65_536,
        max_global_rejects: 1024,
        max_flat_map_regens: 1_000_000,
        failure_persistence: None,
        source_file: None,
        test_name: None,
        #[cfg(feature = "fork")]
        fork: false,
        #[cfg(feature = "timeout")]
        timeout: 0,
        #[cfg(feature = "std")]
        max_shrink_time: 0,
        max_shrink_iters: u32::MAX,
        max_default_size_range: 100,
        result_cache: noop_result_cache,
        #[cfg(feature = "std")]
        verbose: 0,
        rng_algorithm: RngAlgorithm::default(),
        rng_seed: RngSeed::Random,
        _non_exhaustive: (),
    }
}

// The default config, computed by combining environment variables and
// defaults.
#[cfg(feature = "std")]
lazy_static! {
    static ref DEFAULT_CONFIG: Config = {
        let mut default_config = default_default_config();
        default_config.failure_persistence = Some(Box::new(crate::test_runner::FileFailurePersistence::default()));
        contextualize_config(default_config)
    };
}

/// The seed for the RNG. Can either be random, specified as a u64, or specified
/// as a hex-encoded string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RngSeed {
    /// Default case, use a random value
    Random,
    /// Use a u64 to generate a seed
    ///
    /// NB [03-30-25] Before `FullSeed`, this was the only way to provide a seed.
    /// A u64 isn't sufficient to represent all posible seeds though, with most
    /// seeds being a 32-byte buffer. This name must stay as `Fixed` since this
    /// is part of the public API but a more appropriate name would be
    /// `AbbreviatedNumericSeed`
    Fixed(u64),
    /// Use the provided hex-encoded string as the seed. This must be exactly the
    /// size expected by the configured rng algorithm.
    ///
    /// The seed written to persistence files is a hex-encoded string, meaning you
    /// can pass a seed from those files to a TestRunner with this variant.
    FullHexEncodedSeed(&'static [u8]),
}

impl str::FromStr for RngSeed {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split("-");

        match split.next() {
            // input of the form `hex-{s}` is a full hex-encoded seed
            Some("hex") => {
                let seed_bytes = match split.next() {
                    Some(s) => {
                        let mut buf = vec![0_u8; s.len() / 2];
                        crate::test_runner::rng::from_base16(&mut buf[0..], &s);
                        buf
                    }
                    None => return Err(()),
                };

                if split.next().is_some() {
                    return Err(());
                }

                Ok(RngSeed::FullHexEncodedSeed(seed_bytes.leak()))
            }
            // any other input should be a u64 that a seed will be generated from
            Some(_) => s.parse::<u64>().map(RngSeed::Fixed).map_err(|_| ()),
            None => unreachable!("its not possible to ever return None on the first invocation of `next`. empty strings still return an empty string"),
        }
    }
}

impl fmt::Display for RngSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RngSeed::Random => write!(f, "random"),
            RngSeed::Fixed(n) => write!(f, "u64-{}", n),
            RngSeed::FullHexEncodedSeed(n) => {
                let mut s = std::string::String::new();
                crate::test_runner::to_base16(&mut s, n);
                write!(f, "hex-{}", s)
            }
        }
    }
}

/// Configuration for how a proptest test should be run.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The number of successful test cases that must execute for the test as a
    /// whole to pass.
    ///
    /// This does not include implicitly-replayed persisted failing cases.
    ///
    /// The default is 256, which can be overridden by setting the
    /// `PROPTEST_CASES` environment variable. (The variable is only considered
    /// when the `std` feature is enabled, which it is by default.)
    pub cases: u32,

    /// The maximum number of individual inputs that may be rejected before the
    /// test as a whole aborts.
    ///
    /// The default is 65536, which can be overridden by setting the
    /// `PROPTEST_MAX_LOCAL_REJECTS` environment variable. (The variable is only
    /// considered when the `std` feature is enabled, which it is by default.)
    pub max_local_rejects: u32,

    /// The maximum number of combined inputs that may be rejected before the
    /// test as a whole aborts.
    ///
    /// The default is 1024, which can be overridden by setting the
    /// `PROPTEST_MAX_GLOBAL_REJECTS` environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    pub max_global_rejects: u32,

    /// The maximum number of times all `Flatten` combinators will attempt to
    /// regenerate values. This puts a limit on the worst-case exponential
    /// explosion that can happen with nested `Flatten`s.
    ///
    /// The default is 1_000_000, which can be overridden by setting the
    /// `PROPTEST_MAX_FLAT_MAP_REGENS` environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    pub max_flat_map_regens: u32,

    /// Indicates whether and how to persist failed test results.
    ///
    /// When compiling with "std" feature (i.e. the standard library is available), the default
    /// is `Some(Box::new(FileFailurePersistence::SourceParallel("proptest-regressions")))`.
    ///
    /// Without the standard library, the default is `None`, and no persistence occurs.
    ///
    /// See the docs of [`FileFailurePersistence`](enum.FileFailurePersistence.html)
    /// and [`MapFailurePersistence`](struct.MapFailurePersistence.html) for more information.
    ///
    /// You can disable failure persistence with the `PROPTEST_DISABLE_FAILURE_PERSISTENCE`
    /// environment variable but its not currently possible to set the persistence file
    /// with an environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    pub failure_persistence: Option<Box<dyn FailurePersistence>>,

    /// File location of the current test, relevant for persistence
    /// and debugging.
    ///
    /// Note the use of `&str` rather than `Path` to be compatible with
    /// `#![no_std]` use cases where `Path` is unavailable.
    ///
    /// See the docs of [`FileFailurePersistence`](enum.FileFailurePersistence.html)
    /// for more information on how it may be used for persistence.
    pub source_file: Option<&'static str>,

    /// The fully-qualified name of the test being run, as would be passed to
    /// the test executable to run just that test.
    ///
    /// This must be set if `fork` is `true`. Otherwise, it is unused. It is
    /// automatically set by `proptest!`.
    ///
    /// This must include the crate name at the beginning, as produced by
    /// `module_path!()`.
    pub test_name: Option<&'static str>,

    /// If true, tests are run in a subprocess.
    ///
    /// Forking allows proptest to work with tests which may fail by aborting
    /// the process, causing a segmentation fault, etc, but can be a lot slower
    /// in certain environments or when running a very large number of tests.
    ///
    /// For forking to work correctly, both the `Strategy` and the content of
    /// the test case itself must be deterministic.
    ///
    /// This requires the "fork" feature, enabled by default.
    ///
    /// The default is `false`, which can be overridden by setting the
    /// `PROPTEST_FORK` environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    #[cfg(feature = "fork")]
    #[cfg_attr(docsrs, doc(cfg(feature = "fork")))]
    pub fork: bool,

    /// If non-zero, tests are run in a subprocess and each generated case
    /// fails if it takes longer than this number of milliseconds.
    ///
    /// This implicitly enables forking, even if the `fork` field is `false`.
    ///
    /// The type here is plain `u32` (rather than
    /// `Option<std::time::Duration>`) for the sake of ergonomics.
    ///
    /// This requires the "timeout" feature, enabled by default.
    ///
    /// Setting a timeout to less than the time it takes the process to start
    /// up and initialise the first test case will cause the whole test to be
    /// aborted.
    ///
    /// The default is `0` (i.e., no timeout), which can be overridden by
    /// setting the `PROPTEST_TIMEOUT` environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    #[cfg(feature = "timeout")]
    #[cfg_attr(docsrs, doc(cfg(feature = "timeout")))]
    pub timeout: u32,

    /// If non-zero, give up the shrinking process after this many milliseconds
    /// have elapsed since the start of the shrinking process.
    ///
    /// This will not cause currently running test cases to be interrupted.
    ///
    /// This configuration is only available when the `std` feature is enabled
    /// (which it is by default).
    ///
    /// The default is `0` (i.e., no limit), which can be overridden by setting
    /// the `PROPTEST_MAX_SHRINK_TIME` environment variable. (The variable is
    /// only considered when the `std` feature is enabled, which it is by
    /// default.)
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    pub max_shrink_time: u32,

    /// Give up on shrinking if more than this number of iterations of the test
    /// code are run.
    ///
    /// Setting this to `std::u32::MAX` causes the actual limit to be four
    /// times the number of test cases.
    ///
    /// Setting this value to `0` disables shrinking altogether.
    ///
    /// Note that the type of this field will change in a future version of
    /// proptest to better accommodate its special values.
    ///
    /// The default is `std::u32::MAX`, which can be overridden by setting the
    /// `PROPTEST_MAX_SHRINK_ITERS` environment variable. (The variable is only
    /// considered when the `std` feature is enabled, which it is by default.)
    pub max_shrink_iters: u32,

    /// The default maximum size to `proptest::collection::SizeRange`. The default
    /// strategy for collections (like `Vec`) use collections in the range of
    /// `0..max_default_size_range`.
    ///
    /// The default is `100` which can be overridden by setting the
    /// `PROPTEST_MAX_DEFAULT_SIZE_RANGE` environment variable. (The variable
    /// is only considered when the `std` feature is enabled, which it is by
    /// default.)
    pub max_default_size_range: usize,

    /// A function to create new result caches.
    ///
    /// The default is to do no caching. The easiest way to enable caching is
    /// to set this field to `basic_result_cache` (though that is currently
    /// only available with the `std` feature).
    ///
    /// This is useful for strategies which have a tendency to produce
    /// duplicate values, or for tests where shrinking can take a very long
    /// time due to exploring the same output multiple times.
    ///
    /// When caching is enabled, generated values themselves are not stored, so
    /// this does not pose a risk of memory exhaustion for large test inputs
    /// unless using extraordinarily large test case counts.
    ///
    /// Caching incurs its own overhead, and may very well make your test run
    /// more slowly.
    pub result_cache: fn() -> Box<dyn ResultCache>,

    /// Set to non-zero values to cause proptest to emit human-targeted
    /// messages to stderr as it runs.
    ///
    /// Greater values cause greater amounts of logs to be emitted. The exact
    /// meaning of certain levels other than 0 is subject to change.
    ///
    /// - 0: No extra output.
    /// - 1: Log test failure messages. In state machine tests, this level is
    ///   used to print transitions.
    /// - 2: Trace low-level details.
    ///
    /// This is only available with the `std` feature (enabled by default)
    /// since on nostd proptest has no way to produce output.
    ///
    /// The default is `0`, which can be overridden by setting the
    /// `PROPTEST_VERBOSE` environment variable. (The variable is only considered
    /// when the `std` feature is enabled, which it is by default.)
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    pub verbose: u32,

    /// The RNG algorithm to use when not using a user-provided RNG.
    ///
    /// The default is `RngAlgorithm::default()`, which can be overridden by
    /// setting the `PROPTEST_RNG_ALGORITHM` environment variable to one of the following:
    ///
    /// - `xs` — `RngAlgorithm::XorShift`
    /// - `cc` — `RngAlgorithm::ChaCha`
    ///
    /// (The variable is only considered when the `std` feature is enabled,
    /// which it is by default.)
    pub rng_algorithm: RngAlgorithm,

    /// Seed used for the RNG. Set by using the PROPTEST_RNG_SEED environment variable
    /// If the environment variable is undefined, a random seed is generated (this is the default option).
    ///
    /// PROPTEST_RNG_SEED supports two formats:
    /// - `hex-{s}` where the string {s} is a hex-encoded seed, matching the expected length of a
    ///   seed for the configured rng algorithm.
    /// - `{n}` where the u64 number {n} is used to create a seed for the configured run algorithm
    pub rng_seed: RngSeed,

    // Needs to be public so FRU syntax can be used.
    #[doc(hidden)]
    pub _non_exhaustive: (),
}

impl Config {
    /// Constructs a `Config` only differing from the `default()` in the
    /// number of test cases required to pass the test successfully.
    ///
    /// This is simply a more concise alternative to using field-record update
    /// syntax:
    ///
    /// ```
    /// # use proptest::test_runner::Config;
    /// assert_eq!(
    ///     Config::with_cases(42),
    ///     Config { cases: 42, .. Config::default() }
    /// );
    /// ```
    pub fn with_cases(cases: u32) -> Self {
        Self {
            cases,
            ..Config::default()
        }
    }

    /// Constructs a `Config` only differing from the `default()` in the
    /// source_file of the present test.
    ///
    /// This is simply a more concise alternative to using field-record update
    /// syntax:
    ///
    /// ```
    /// # use proptest::test_runner::Config;
    /// assert_eq!(
    ///     Config::with_source_file("computer/question"),
    ///     Config { source_file: Some("computer/question"), .. Config::default() }
    /// );
    /// ```
    pub fn with_source_file(source_file: &'static str) -> Self {
        Self {
            source_file: Some(source_file),
            ..Config::default()
        }
    }

    /// Constructs a `Config` only differing from the provided Config instance, `self`,
    /// in the source_file of the present test.
    ///
    /// This is simply a more concise alternative to using field-record update
    /// syntax:
    ///
    /// ```
    /// # use proptest::test_runner::Config;
    /// let a = Config::with_source_file("computer/question");
    /// let b = a.clone_with_source_file("answer/42");
    /// assert_eq!(
    ///     a,
    ///     Config { source_file: Some("computer/question"), .. Config::default() }
    /// );
    /// assert_eq!(
    ///     b,
    ///     Config { source_file: Some("answer/42"), .. Config::default() }
    /// );
    /// ```
    pub fn clone_with_source_file(&self, source_file: &'static str) -> Self {
        let mut result = self.clone();
        result.source_file = Some(source_file);
        result
    }

    /// Constructs a `Config` only differing from the `default()` in the
    /// failure_persistence member.
    ///
    /// This is simply a more concise alternative to using field-record update
    /// syntax:
    ///
    /// ```
    /// # use proptest::test_runner::{Config, FileFailurePersistence};
    /// assert_eq!(
    ///     Config::with_failure_persistence(FileFailurePersistence::WithSource("regressions")),
    ///     Config {
    ///         failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("regressions"))),
    ///         .. Config::default()
    ///     }
    /// );
    /// ```
    pub fn with_failure_persistence<T>(failure_persistence: T) -> Self
    where
        T: FailurePersistence + 'static,
    {
        Self {
            failure_persistence: Some(Box::new(failure_persistence)),
            ..Default::default()
        }
    }

    /// Return whether this configuration implies forking.
    ///
    /// This method exists even if the "fork" feature is disabled, in which
    /// case it simply returns false.
    pub fn fork(&self) -> bool {
        self._fork() || self.timeout() > 0
    }

    #[cfg(feature = "fork")]
    fn _fork(&self) -> bool {
        self.fork
    }

    #[cfg(not(feature = "fork"))]
    fn _fork(&self) -> bool {
        false
    }

    /// Returns the configured timeout.
    ///
    /// This method exists even if the "timeout" feature is disabled, in which
    /// case it simply returns 0.
    #[cfg(feature = "timeout")]
    pub fn timeout(&self) -> u32 {
        self.timeout
    }

    /// Returns the configured timeout.
    ///
    /// This method exists even if the "timeout" feature is disabled, in which
    /// case it simply returns 0.
    #[cfg(not(feature = "timeout"))]
    pub fn timeout(&self) -> u32 {
        0
    }

    /// Returns the configured limit on shrinking iterations.
    ///
    /// This takes into account the special "automatic" behaviour.
    pub fn max_shrink_iters(&self) -> u32 {
        if u32::MAX == self.max_shrink_iters {
            self.cases.saturating_mul(4)
        } else {
            self.max_shrink_iters
        }
    }

    // Used by macros to force the config to be owned without depending on
    // certain traits being `use`d.
    #[allow(missing_docs)]
    #[doc(hidden)]
    pub fn __sugar_to_owned(&self) -> Self {
        self.clone()
    }
}

#[cfg(feature = "std")]
impl Default for Config {
    fn default() -> Self {
        DEFAULT_CONFIG.clone()
    }
}

#[cfg(not(feature = "std"))]
impl Default for Config {
    fn default() -> Self {
        default_default_config()
    }
}
