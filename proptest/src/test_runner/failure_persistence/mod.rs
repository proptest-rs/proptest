//-
// Copyright 2017, 2018, 2019 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::std_facade::{fmt, Box, String, Vec};
use core::any::Any;
use core::cmp::Ordering;
use core::fmt::Display;
use core::result::Result;
use core::str::FromStr;

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
mod file;
mod map;
mod noop;

#[cfg(feature = "std")]
pub use self::file::*;
pub use self::map::*;

use crate::test_runner::Seed;

/// Opaque struct representing a persisted failure: either an RNG seed
/// (regenerates and re-shrinks the historical failure) or, under the tape
/// shrink engine, a recorded choice tape (replays the already-shrunken
/// values exactly, surviving strategy refactors and RNG changes).
///
/// The `Display` and `FromStr` implementations go to and from the format
/// Proptest uses for its persistence file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistedSeed(pub(crate) PersistedFailure);

#[derive(Clone, Debug)]
pub(crate) enum PersistedFailure {
    Seed(Seed),
    /// A serialized choice tape (see `tape::serialize_tape`), written as
    /// `ct1 <base16>`.
    Tape {
        bytes: Vec<u8>,
        /// The RNG seed of the run that produced the failure. Not
        /// persisted (the tape supersedes it for replay); carried so the
        /// default `save_persisted_failure2` can keep forwarding seeds
        /// to implementations that only override the legacy hooks.
        /// `None` for entries loaded from a persistence file.
        seed: Option<Seed>,
    },
}

// Equality and ordering ignore the carried seed: a tape loaded from disk
// (seed: None) and the same tape as saved (seed: Some) are one entry for
// deduplication purposes.
impl PartialEq for PersistedFailure {
    fn eq(&self, other: &Self) -> bool {
        Ordering::Equal == self.cmp(other)
    }
}

impl Eq for PersistedFailure {}

impl PartialOrd for PersistedFailure {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PersistedFailure {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (PersistedFailure::Seed(a), PersistedFailure::Seed(b)) => {
                a.cmp(b)
            }
            (PersistedFailure::Seed(..), PersistedFailure::Tape { .. }) => {
                Ordering::Less
            }
            (PersistedFailure::Tape { .. }, PersistedFailure::Seed(..)) => {
                Ordering::Greater
            }
            (
                PersistedFailure::Tape { bytes: a, .. },
                PersistedFailure::Tape { bytes: b, .. },
            ) => a.cmp(b),
        }
    }
}

const TAPE_PERSISTENCE_KEY: &str = "ct1";

impl Display for PersistedSeed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.0 {
            PersistedFailure::Seed(seed) => {
                write!(f, "{}", seed.to_persistence())
            }
            PersistedFailure::Tape { bytes, .. } => {
                let mut hex = String::new();
                crate::test_runner::rng::to_base16(&mut hex, bytes);
                write!(f, "{} {}", TAPE_PERSISTENCE_KEY, hex)
            }
        }
    }
}

impl FromStr for PersistedSeed {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let trimmed = s.trim();
        if let Some(hex) = trimmed
            .strip_prefix(TAPE_PERSISTENCE_KEY)
            .and_then(|rest| rest.strip_prefix(' '))
        {
            let hex = hex.trim();
            if 0 != hex.len() % 2 {
                return Err(());
            }
            let mut bytes = vec![0u8; hex.len() / 2];
            crate::test_runner::rng::from_base16(&mut bytes, hex).ok_or(())?;
            return Ok(PersistedSeed(PersistedFailure::Tape {
                bytes,
                seed: None,
            }));
        }
        Seed::from_persistence(trimmed)
            .map(|seed| PersistedSeed(PersistedFailure::Seed(seed)))
            .ok_or(())
    }
}

/// Provides external persistence for historical test failures by storing seeds.
///
/// **Note**: Implementing `load_persisted_failures` and
/// `save_persisted_failures` is **deprecated** and these methods will be
/// removed in proptest 0.10.0. Instead, implement `load_persisted_failures2`
/// and `save_persisted_failures2`.
pub trait FailurePersistence: Send + Sync + fmt::Debug {
    /// Supply seeds associated with the given `source_file` that may be used
    /// by a `TestRunner`'s random number generator in order to consistently
    /// recreate a previously-failing `Strategy`-provided value.
    ///
    /// The default implementation is **for backwards compatibility**. It
    /// delegates to `load_persisted_failures` and converts the results into
    /// XorShift seeds.
    #[allow(deprecated)]
    fn load_persisted_failures2(
        &self,
        source_file: Option<&'static str>,
    ) -> Vec<PersistedSeed> {
        self.load_persisted_failures(source_file)
            .into_iter()
            .map(|seed| {
                PersistedSeed(PersistedFailure::Seed(Seed::XorShift(seed)))
            })
            .collect()
    }

    /// Use `load_persisted_failures2` instead.
    ///
    /// This function inadvertently exposes the implementation of seeds prior
    /// to Proptest 0.9.1 and only works with XorShift seeds.
    #[deprecated]
    #[allow(unused_variables)]
    fn load_persisted_failures(
        &self,
        source_file: Option<&'static str>,
    ) -> Vec<[u8; 16]> {
        panic!("load_persisted_failures2 not implemented");
    }

    /// Store a new failure-generating seed associated with the given `source_file`.
    ///
    /// The default implementation is **for backwards compatibility**. It
    /// delegates to `save_persisted_failure` if `seed` is a XorShift seed.
    #[allow(deprecated)]
    fn save_persisted_failure2(
        &mut self,
        source_file: Option<&'static str>,
        seed: PersistedSeed,
        shrunken_value: &dyn fmt::Debug,
    ) {
        match seed.0 {
            PersistedFailure::Seed(Seed::XorShift(seed))
            | PersistedFailure::Tape {
                seed: Some(Seed::XorShift(seed)),
                ..
            } => {
                self.save_persisted_failure(source_file, seed, shrunken_value)
            }
            _ => (),
        }
    }

    /// Use `save_persisted_failures2` instead.
    ///
    /// This function inadvertently exposes the implementation of seeds prior
    /// to Proptest 0.9.1 and only works with XorShift seeds.
    #[deprecated]
    #[allow(unused_variables)]
    fn save_persisted_failure(
        &mut self,
        source_file: Option<&'static str>,
        seed: [u8; 16],
        shrunken_value: &dyn fmt::Debug,
    ) {
        panic!("save_persisted_failure2 not implemented");
    }

    /// Delegate method for producing a trait object usable with `Clone`
    fn box_clone(&self) -> Box<dyn FailurePersistence>;

    /// Equality testing delegate required due to constraints of trait objects.
    fn eq(&self, other: &dyn FailurePersistence) -> bool;

    /// Assistant method for trait object comparison.
    fn as_any(&self) -> &dyn Any;
}

impl<'a, 'b> PartialEq<dyn FailurePersistence + 'b>
    for dyn FailurePersistence + 'a
{
    fn eq(&self, other: &(dyn FailurePersistence + 'b)) -> bool {
        FailurePersistence::eq(self, other)
    }
}

impl Clone for Box<dyn FailurePersistence> {
    fn clone(&self) -> Box<dyn FailurePersistence> {
        self.box_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{PersistedFailure, PersistedSeed};
    use crate::test_runner::rng::Seed;

    pub const INC_SEED: PersistedSeed =
        PersistedSeed(PersistedFailure::Seed(Seed::XorShift([
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        ])));

    pub const HI_PATH: Option<&str> = Some("hi");
    pub const UNREL_PATH: Option<&str> = Some("unrelated");
}
