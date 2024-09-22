//-
// Copyright 2017 Jason Lingle
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::std_facade::{fmt, Arc};

use crate::strategy::fuse::Fuse;
use crate::strategy::traits::*;
use crate::test_runner::*;
use std::mem;

/// Adaptor that flattens a `Strategy` which produces other `Strategy`s into a
/// `Strategy` that picks one of those strategies and then picks values from
/// it.
#[derive(Debug, Clone, Copy)]
#[must_use = "strategies do nothing unless used"]
pub struct Flatten<S> {
    source: S,
}

impl<S: Strategy> Flatten<S> {
    /// Wrap `source` to flatten it.
    pub fn new(source: S) -> Self {
        Flatten { source }
    }
}

impl<S: Strategy> Strategy for Flatten<S>
where
    S::Value: Strategy,
    <S::Value as Strategy>::Tree: Clone,
{
    type Tree = FlattenValueTree<S::Tree>;
    type Value = <S::Value as Strategy>::Value;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        let meta = self.source.new_tree(runner)?;
        FlattenValueTree::new(runner, meta)
    }
}

/// The `ValueTree` produced by `Flatten`.
pub struct FlattenValueTree<S: ValueTree>
where
    S::Value: Strategy,
{
    meta: Fuse<S>,
    current: Fuse<<S::Value as Strategy>::Tree>,
    last_complication: Option<Fuse<<S::Value as Strategy>::Tree>>,
    runner: TestRunner,
    complicate_regen_remaining: u32,
}

impl<S: ValueTree> Clone for FlattenValueTree<S>
where
    S::Value: Strategy + Clone,
    S: Clone,
    <S::Value as Strategy>::Tree: Clone,
{
    fn clone(&self) -> Self {
        FlattenValueTree {
            meta: self.meta.clone(),
            current: self.current.clone(),
            last_complication: self.last_complication.clone(),
            runner: self.runner.clone(),
            complicate_regen_remaining: self.complicate_regen_remaining,
        }
    }
}

impl<S: ValueTree> fmt::Debug for FlattenValueTree<S>
where
    S::Value: Strategy,
    S: fmt::Debug,
    <S::Value as Strategy>::Tree: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FlattenValueTree")
            .field("meta", &self.meta)
            .field("current", &self.current)
            .field("last_complication", &self.last_complication)
            .field(
                "complicate_regen_remaining",
                &self.complicate_regen_remaining,
            )
            .finish()
    }
}

impl<S: ValueTree> FlattenValueTree<S>
where
    S::Value: Strategy,
{
    fn new(runner: &mut TestRunner, meta: S) -> Result<Self, Reason> {
        let current = meta.current().new_tree(runner)?;
        Ok(FlattenValueTree {
            meta: Fuse::new(meta),
            current: Fuse::new(current),
            last_complication: None,
            runner: runner.partial_clone(),
            complicate_regen_remaining: 0,
        })
    }
}

impl<S: ValueTree> ValueTree for FlattenValueTree<S>
where
    S::Value: Strategy,
    <S::Value as Strategy>::Tree: Clone,
{
    type Value = <S::Value as Strategy>::Value;

    fn current(&self) -> Self::Value {
        self.current.current()
    }

    fn simplify(&mut self) -> bool {
        self.current.disallow_complicate();

        if self.meta.simplify() {
            if let Ok(v) = self.meta.current().new_tree(&mut self.runner) {
                self.last_complication = Some(Fuse::new(v));
                mem::swap(
                    self.last_complication.as_mut().unwrap(),
                    &mut self.current,
                );
                self.complicate_regen_remaining = self.runner.config().cases;
                return true;
            } else {
                self.meta.disallow_simplify();
            }
        }

        self.complicate_regen_remaining = 0;
        let mut old_current = self.current.clone();
        old_current.disallow_simplify();

        if self.current.simplify() {
            self.last_complication = Some(old_current);
            true
        } else {
            false
        }
    }

    fn complicate(&mut self) -> bool {
        if self.complicate_regen_remaining > 0 {
            if self.runner.flat_map_regen() {
                self.complicate_regen_remaining -= 1;

                if let Ok(v) = self.meta.current().new_tree(&mut self.runner) {
                    self.current = Fuse::new(v);
                    return true;
                }
            } else {
                self.complicate_regen_remaining = 0;
            }
        }

        if self.meta.complicate() {
            if let Ok(v) = self.meta.current().new_tree(&mut self.runner) {
                self.current = Fuse::new(v);
                self.complicate_regen_remaining = self.runner.config().cases;
                return true;
            } else {
            }
        }

        if self.current.complicate() {
            return true;
        }

        if let Some(v) = self.last_complication.take() {
            self.current = v;
            true
        } else {
            false
        }
    }
}

/// Similar to `Flatten`, but does not shrink the input strategy.
///
/// See `Strategy::prop_ind_flat_map()` fore more details.
#[derive(Clone, Copy, Debug)]
pub struct IndFlatten<S>(pub(super) S);

impl<S: Strategy> Strategy for IndFlatten<S>
where
    S::Value: Strategy,
{
    type Tree = <S::Value as Strategy>::Tree;
    type Value = <S::Value as Strategy>::Value;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        let inner = self.0.new_tree(runner)?;
        inner.current().new_tree(runner)
    }
}

/// Similar to `Map` plus `Flatten`, but does not shrink the input strategy and
/// passes the original input through.
///
/// See `Strategy::prop_ind_flat_map2()` for more details.
pub struct IndFlattenMap<S, F> {
    pub(super) source: S,
    pub(super) fun: Arc<F>,
}

impl<S: fmt::Debug, F> fmt::Debug for IndFlattenMap<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("IndFlattenMap")
            .field("source", &self.source)
            .field("fun", &"<function>")
            .finish()
    }
}

impl<S: Clone, F> Clone for IndFlattenMap<S, F> {
    fn clone(&self) -> Self {
        IndFlattenMap {
            source: self.source.clone(),
            fun: Arc::clone(&self.fun),
        }
    }
}

impl<S: Strategy, R: Strategy, F: Fn(S::Value) -> R> Strategy
    for IndFlattenMap<S, F>
{
    type Tree = crate::tuple::TupleValueTree<(S::Tree, R::Tree)>;
    type Value = (S::Value, R::Value);

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        let left = self.source.new_tree(runner)?;
        let right_source = (self.fun)(left.current());
        let right = right_source.new_tree(runner)?;

        Ok(crate::tuple::TupleValueTree::new((left, right)))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::u32;

    use crate::strategy::just::Just;
    use crate::test_runner::Config;

    #[test]
    fn test_flat_map() {
        // Pick random integer A, then random integer B which is ±5 of A and
        // assert that B <= A if A > 10000. Shrinking should always converge to
        // A=10001, B=10002.
        let input = (0..65536).prop_flat_map(|a| (Just(a), (a - 5..a + 5)));

        let mut failures = 0;
        let mut runner = TestRunner::new_with_rng(
            Config {
                max_shrink_iters: u32::MAX - 1,
                ..Config::default()
            },
            TestRng::deterministic_rng(RngAlgorithm::default()),
        );
        for _ in 0..1000 {
            let case = input.new_tree(&mut runner).unwrap();
            let result = runner.run_one(case, |(a, b)| {
                if a <= 10000 || b <= a {
                    Ok(())
                } else {
                    Err(TestCaseError::fail("fail"))
                }
            });

            match result {
                Ok(_) => {}
                Err(TestError::Fail(_, v)) => {
                    failures += 1;
                    assert_eq!((10001, 10002), v);
                }
                result => panic!("Unexpected result: {:?}", result),
            }
        }

        assert!(failures > 250);
    }

    #[test]
    fn test_flat_map_sanity() {
        check_strategy_sanity(
            (0..65536).prop_flat_map(|a| (Just(a), (a - 5..a + 5))),
            None,
        );
    }

    #[test]
    fn flat_map_respects_regen_limit() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let input = (0..65536)
            .prop_flat_map(|_| 0..65536)
            .prop_flat_map(|_| 0..65536)
            .prop_flat_map(|_| 0..65536)
            .prop_flat_map(|_| 0..65536)
            .prop_flat_map(|_| 0..65536);

        // Arteficially make the first case fail and all others pass, so that
        // the regeneration logic futilely searches for another failing
        // example and eventually gives up. Unfortunately, the test is sort of
        // semi-decidable; if the limit *doesn't* work, the test just runs
        // almost forever.
        let pass = AtomicBool::new(false);
        let mut runner = TestRunner::new(Config {
            max_flat_map_regens: 1000,
            ..Config::default()
        });
        let case = input.new_tree(&mut runner).unwrap();
        let _ = runner.run_one(case, |_| {
            // Only the first run fails, all others succeed
            prop_assert!(pass.fetch_or(true, Ordering::SeqCst));
            Ok(())
        });
    }

    #[test]
    fn test_ind_flat_map_sanity() {
        check_strategy_sanity(
            (0..65536).prop_ind_flat_map(|a| (Just(a), (a - 5..a + 5))),
            None,
        );
    }

    #[test]
    fn test_ind_flat_map2_sanity() {
        check_strategy_sanity(
            (0..65536).prop_ind_flat_map2(|a| a - 5..a + 5),
            None,
        );
    }
}
