//-
// Copyright 2024
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use core::fmt;
/// Holds test failure backtrace, if captured
///
/// If feature `backtrace` is disabled, it's a zero-sized struct with no logic
///
/// If `backtrace` is enabled, attempts to capture backtrace using `std::backtrace::Backtrace` -
/// if requested
#[derive(Clone, Default)]
pub struct Backtrace(internal::Backtrace);

impl Backtrace {
    /// Creates empty backtrace object
    ///
    /// Used when client code doesn't care
    pub fn empty() -> Self {
        Self(internal::Backtrace::empty())
    }
    /// Tells whether there's backtrace captured
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Attempts to capture backtrace - but only if `backtrace` feature is enabled
    #[inline(always)]
    pub fn capture() -> Self {
        Self(internal::Backtrace::capture())
    }
}

impl fmt::Debug for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for Backtrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(feature = "backtrace")]
mod internal {
    use core::fmt;
    use std::backtrace as bt;
    use std::sync::Arc;

    // `std::backtrace::Backtrace` isn't `Clone`, so we have
    // to use `Arc` to also maintain `Send + Sync`
    #[derive(Clone, Default)]
    pub struct Backtrace(Option<Arc<bt::Backtrace>>);

    impl Backtrace {
        pub fn empty() -> Self {
            Self(None)
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_none()
        }

        #[inline(always)]
        pub fn capture() -> Self {
            let bt = bt::Backtrace::capture();
            // Store only if we have backtrace
            if bt.status() == bt::BacktraceStatus::Captured {
                Self(Some(Arc::new(bt)))
            } else {
                Self(None)
            }
        }
    }

    impl fmt::Debug for Backtrace {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(ref arc) = self.0 {
                fmt::Debug::fmt(arc.as_ref(), f)
            } else {
                Ok(())
            }
        }
    }

    impl fmt::Display for Backtrace {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(ref arc) = self.0 {
                fmt::Display::fmt(arc.as_ref(), f)
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(not(feature = "backtrace"))]
mod internal {
    use core::fmt;

    #[derive(Clone, Default)]
    pub struct Backtrace;

    impl Backtrace {
        pub fn empty() -> Self {
            Self
        }

        pub fn is_empty(&self) -> bool {
            true
        }

        pub fn capture() -> Self {
            Self
        }
    }

    impl fmt::Debug for Backtrace {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    impl fmt::Display for Backtrace {
        fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            Ok(())
        }
    }
}
