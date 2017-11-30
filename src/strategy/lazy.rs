//-
// Copyright 2017 Jason Lingle
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::cell::RefCell;
use std::sync::Arc;

use rand::{Rng, SeedableRng, XorShiftRng};

use strategy::*;
use test_runner::*;

/// A strategy adaptor which defers value generation as long as possible.
///
/// That is, the `LazyValueTree` does not start out with an initialised value,
/// but instead computes it when it becomes necessary. This means the inner
/// strategy must be **infallible** since there is no way to report failures in
/// this context.
///
/// The primary use-case for `Lazy` is in deeply nested `Union`s, which
/// ordinarily generate all their branches at once.
///
/// Use `Lazy::new` to construct this combinator. There is no corresponding
/// method on `Strategy` since the implications of this wrapper are somewhat
/// subtle.
#[derive(Debug)]
pub struct Lazy<T : Strategy> {
    inner: Arc<T>,
}

impl<T : Strategy> Clone for Lazy<T> {
    fn clone(&self) -> Self {
        Lazy { inner: Arc::clone(&self.inner) }
    }
}

impl<T : Strategy> Lazy<T> {
    /// Wrap the given strategy to make it lazy.
    ///
    /// The strategy must be **infallible**, that is, any call to `new_value`
    /// must return `Ok`. If this is not the case, using the lazy strategy may
    /// lead to panics.
    pub fn new(inner: T) -> Self {
        Lazy {
            inner: Arc::new(inner),
        }
    }
}

impl<T : Strategy> Strategy for Lazy<T> {
    type Value = LazyValueTree<T>;

    fn new_value(&self, runner: &mut TestRunner)
                 -> Result<Self::Value, String> {
        Ok(LazyValueTree {
            strategy: Arc::clone(&self.inner),
            runner: runner.partial_clone(),
            seed: runner.rng().gen(),
            value: RefCell::new(None),
        })
    }
}

/// The `ValueTree` type for a `Lazy` strategy.
#[derive(Debug)]
pub struct LazyValueTree<T : Strategy> {
    strategy: Arc<T>,
    runner: TestRunner,
    seed: [u32;4],
    value: RefCell<Option<T::Value>>,
}

impl<T : Strategy> ValueTree for LazyValueTree<T> {
    type Value = <T::Value as ValueTree>::Value;

    fn current(&self) -> Self::Value {
        let mut value = self.value.borrow_mut();
        let value = &mut*value;

        if value.is_none() {
            let mut runner = self.runner.clone();
            *runner.rng() = XorShiftRng::from_seed(self.seed);
            *value = Some(self.strategy.new_value(&mut runner)
                          .expect("Inner strategy of `Lazy` failed"));
        }

        value.as_ref()
            .expect("Inner value of `LazyValueTree` not initialised")
            .current()
    }

    fn simplify(&mut self) -> bool {
        self.value.get_mut().as_mut()
            .expect("Inner value of `LazyValueTree` not initialised")
            .simplify()
    }

    fn complicate(&mut self) -> bool {
        self.value.get_mut().as_mut()
            .expect("Inner value of `LazyValueTree` not initialised")
            .complicate()
    }
}
