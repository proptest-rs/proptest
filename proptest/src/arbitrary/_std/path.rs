//-
// Copyright 2017, 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Arbitrary implementations for `std::path`.

use std::{ffi::OsString, path::*};

use crate::{
    arbitrary::{SMapped, StrategyFor},
    prelude::{any, any_with, Arbitrary, Strategy},
    std_facade::{string::ToString, Arc, Box, Rc, Vec},
    strategy::{statics::static_map, MapInto},
};

arbitrary!(StripPrefixError; Path::new("").strip_prefix("a").unwrap_err());

/// This implementation takes two parameters: a range for the number of components, and a regex to
/// determine the string. It generates either a relative or an absolute path with equal probability.
///
/// Currently, this implementation does not generate:
///
/// * Paths that are not valid UTF-8 (this is unlikely to change)
/// * Paths with a [`PrefixComponent`](std::path::PrefixComponent) on Windows, e.g. `C:\` (this may
///   change in the future)
impl Arbitrary for PathBuf {
    type Parameters = <Vec<OsString> as Arbitrary>::Parameters;
    type Strategy = SMapped<(bool, Vec<OsString>), Self>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        static_map(
            (any::<bool>(), any_with::<Vec<OsString>>(args)),
            |(is_absolute, parts)| {
                let mut out = PathBuf::new();
                if is_absolute {
                    out.push(&MAIN_SEPARATOR.to_string());
                }
                for part in parts {
                    out.push(&part);
                }
                out
            },
        )
    }
}

macro_rules! dst_wrapped {
    ($($w: ident),*) => {
        $(
            /// This implementation is identical to [the `Arbitrary` implementation for
            /// `PathBuf`](trait.Arbitrary.html#impl-Arbitrary-for-PathBuf).
            impl Arbitrary for $w<Path> {
                type Parameters = <Vec<OsString> as Arbitrary>::Parameters;
                type Strategy = MapInto<StrategyFor<PathBuf>, Self>;

                fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
                    any_with::<PathBuf>(args).prop_map_into()
                }
            }
        )*
    }
}

dst_wrapped!(Box, Rc, Arc);

#[cfg(test)]
mod test {
    no_panic_test!(
        strip_prefix_error => StripPrefixError,
        path_buf => PathBuf,
        box_path => Box<Path>,
        rc_path => Rc<Path>,
        arc_path => Arc<Path>
    );
}
