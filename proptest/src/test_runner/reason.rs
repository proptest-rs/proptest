//-
// Copyright 2017, 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::string::ToString;

use super::backtrace::Backtrace;
use crate::std_facade::{fmt, Box, Cow, String};

/// The reason for why something, such as a generated value, was rejected.
///
/// Contains message which describes reason and optionally backtrace
/// (depending on several factors like features `backtrace` and
/// `handle-panics`, and actual spot where reason was created).
///
/// This is constructed via `.into()` on a `String`, `&'static str`, or
/// `Box<str>`.
#[derive(Clone)]
pub struct Reason(Cow<'static, str>, Backtrace);

impl Reason {
    /// Creates reason from provided message
    ///
    /// # Parameters
    /// * `message` - anything convertible to shared or owned string
    ///
    /// # Returns
    /// Reason object
    pub fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self(message.into(), Backtrace::empty())
    }
    /// Creates reason from provided message, adding location info as its part
    ///
    /// # Parameters
    /// * `message` - anything convertible to shared or owned string
    ///
    /// # Returns
    /// Reason object
    #[track_caller]
    pub fn with_location(message: impl Into<Cow<'static, str>>) -> Self {
        let message: Cow<'static, str> = message.into();
        let loc = core::panic::Location::caller();
        Self(
            append_location(message.into_owned(), *loc).into(),
            Backtrace::empty(),
        )
    }
    /// Creates reason from provided message, adding location info as its part,
    /// and captures backtrace at callsite
    ///
    /// NOTE: Backtrace is actually captured only if `backtrace` feature is enabled,
    /// otherwise it'll be empty
    ///
    /// # Parameters
    /// * `message` - anything convertible to shared or owned string
    ///
    /// # Returns
    /// Reason object with provided message, augmented with location info, and captured backtrace
    #[inline(always)]
    #[track_caller]
    pub fn with_location_and_backtrace(
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self(Self::with_location(message).0, Backtrace::capture())
    }
    /// Return the message for this `Reason`.
    ///
    /// The message is intended for human consumption, and is not guaranteed to
    /// have any format in particular.
    pub fn message(&self) -> &str {
        &*self.0
    }
    /// Produces displayable value which displays all data stored in Reason,
    /// unlike normal `Display` implementation which shows only message
    pub fn display_detailed(&self) -> impl fmt::Display + '_ {
        DisplayReason(self)
    }
}

impl core::cmp::PartialEq for Reason {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl core::cmp::Eq for Reason {}

impl core::cmp::PartialOrd for Reason {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::Ord for Reason {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl core::hash::Hash for Reason {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl From<(Cow<'static, str>, Backtrace)> for Reason {
    fn from((msg, bt): (Cow<'static, str>, Backtrace)) -> Self {
        Self(msg, bt)
    }
}

impl From<&'static str> for Reason {
    fn from(s: &'static str) -> Self {
        Self(s.into(), Backtrace::empty())
    }
}

impl From<String> for Reason {
    fn from(s: String) -> Self {
        Self(s.into(), Backtrace::empty())
    }
}

impl From<Box<str>> for Reason {
    fn from(s: Box<str>) -> Self {
        Self(String::from(s).into(), Backtrace::empty())
    }
}

#[cfg(feature = "std")]
impl<'a, 'b> From<&'b std::panic::PanicInfo<'a>> for Reason {
    #[inline(always)]
    fn from(value: &'b std::panic::PanicInfo<'a>) -> Self {
        let payload = value.payload();
        let message: String = payload
            .downcast_ref::<&'static str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().map(|s| s.clone()))
            .or_else(|| {
                payload.downcast_ref::<Box<str>>().map(|s| s.to_string())
            })
            .unwrap_or_else(|| "<unknown panic value>".to_string());

        let message = if let Some(loc) = value.location() {
            append_location(message, *loc)
        } else {
            message
        };

        Self(message.into(), Backtrace::capture())
    }
}

impl fmt::Debug for Reason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Reason")
            .field(&self.0)
            .field(&"Backtrace(...)")
            .finish()
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

struct DisplayReason<'a>(&'a Reason);

impl<'a> fmt::Display for DisplayReason<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self(Reason(msg, bt)) = self;
        if bt.is_empty() {
            write!(f, "{msg}")
        } else {
            write!(f, "{msg}\nstack backtrace:\n{bt}")
        }
    }
}

fn append_location<'a>(
    message: String,
    loc: core::panic::Location<'a>,
) -> String {
    match message.rfind('\n') {
        // Message is multiline and ends with '\n'
        Some(pos) if pos == message.len() - '\n'.len_utf8() => {
            format!("{message}at {loc}")
        }
        // Message is multiline and doesn't end with '\n'
        Some(_) => format!("{message}\nat {loc}"),
        // MEssage is not multiline
        _ => format!("{message} at {loc}"),
    }
}
