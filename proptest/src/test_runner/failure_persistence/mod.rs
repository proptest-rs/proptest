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

/// Opaque struct representing a seed which can be persisted.
///
/// The `Display` and `FromStr` implementations go to and from the format
/// Proptest uses for its persistence file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistedSeed(pub(crate) Seed);

/// Floating point edge bias as bytes to be put inside BTreeMap/set.
pub type PersistedEdgeBias = [u8; 4];

impl Display for PersistedSeed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0.to_persistence())
    }
}

impl FromStr for PersistedSeed {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Seed::from_persistence(s).map(PersistedSeed).ok_or(())
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
    ) -> Vec<(PersistedSeed, PersistedEdgeBias)> {
        self.load_persisted_failures(source_file)
            .into_iter()
            .map(|(seed, edge_bias)| {
                (PersistedSeed(Seed::XorShift(seed)), edge_bias)
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
    ) -> Vec<([u8; 16], PersistedEdgeBias)> {
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
        current_edge_bias: PersistedEdgeBias,
        shrunken_value: &dyn fmt::Debug,
    ) {
        match seed.0 {
            Seed::XorShift(seed) => self.save_persisted_failure(
                source_file,
                seed,
                current_edge_bias,
                shrunken_value,
            ),
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
        current_edge_bias: PersistedEdgeBias,
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

pub(crate) fn to_base16(dst: &mut String, src: &[u8]) {
    for byte in src {
        dst.push_str(&format!("{:02x}", byte));
    }
}

pub(crate) fn from_base16(dst: &mut [u8], src: &str) -> Option<()> {
    if dst.len() * 2 != src.len() {
        return None;
    }

    for (dst_byte, src_pair) in dst.into_iter().zip(src.as_bytes().chunks(2)) {
        *dst_byte =
            u8::from_str_radix(std::str::from_utf8(src_pair).ok()?, 16).ok()?;
    }

    Some(())
}

#[cfg(test)]
mod tests {
    use super::{PersistedEdgeBias, PersistedSeed};
    use crate::test_runner::rng::Seed;

    pub const INC_SEED: PersistedSeed = PersistedSeed(Seed::XorShift([
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    ]));

    pub const INC_EDGE_BIAS: PersistedEdgeBias = [0, 0, 0, 0];

    pub const HI_PATH: Option<&str> = Some("hi");
    pub const UNREL_PATH: Option<&str> = Some("unrelated");
}
