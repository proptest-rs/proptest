# Design: a Conjecture-style choice tape for proptest

Status: draft for review. Nothing in this document is implemented yet.

## 1. Motivation

Proptest is "inspired by Hypothesis", but it did not adopt Hypothesis's core
engine idea (Conjecture): serialize *all* generation decisions into a
replayable sequence, and implement shrinking as *edits to that sequence*
followed by re-running the generator. Instead, proptest threads shrink state
through every strategy as a `ValueTree` with `simplify()`/`complicate()`
([traits.rs](../proptest/src/strategy/traits.rs)), driven by a single greedy
loop in [runner.rs](../proptest/src/test_runner/runner.rs) (`fn shrink`).

Concrete costs of the current design, observed in the code:

- **Floats shrink to noise.** `f64::BinarySearch`
  ([num.rs](../proptest/src/num.rs), `float_bin_search!`) does numeric
  bisection toward 0 and converges to an arbitrary boundary midpoint like
  `0.7500000000000002`. Hypothesis shrinks through a lexicographic float
  encoding and produces round values (`2.0`, `1.5`).
- **`prop_filter` strands the search.**
  [filter.rs](../proptest/src/strategy/filter.rs) rejects a simplification
  whose value fails the predicate and backtracks; shrinking stalls at locally
  minimal values (the code has to relax the simplify/complicate contract for
  this, see `strict_complicate_after_simplify: false`).
- **`prop_flat_map` shrinking is regeneration-with-retry.**
  [flatten.rs](../proptest/src/strategy/flatten.rs) admits in its comments
  that complicating after the outer value shrinks is implemented by
  regenerating inner values up to a retry budget.
- **No cross-value passes.** Each `ValueTree` shrinks in isolation. We cannot
  lower two equal values together, redistribute a numeric pair, or reorder
  elements.
- **Fragile persistence.** Failures persist as RNG seeds
  (`PersistedSeed(Seed)`), which regenerate a different value as soon as the
  strategy code (or rand) changes. A persisted choice tape survives most code
  changes because typed values are replayed, not re-derived from entropy.

Decision (2026-07-07): build a choice-tape engine. Distribution changes to
default strategies are acceptable.

## 2. Goals and non-goals

Goals, in priority order:

1. Shrinking quality: round floats, no filter stalls, natural flat_map
   shrinking, collection element deletion as a generic pass, later
   cross-value passes.
2. Zero breakage: every existing `Strategy`/`ValueTree` implementation
   (including third-party and proptest-state-machine) keeps compiling and
   running unchanged. Unmigrated strategies are tape-recorded transparently
   through an RngCore compat wrapper (§6) and keep at least their current
   shrink quality.
3. Incremental migration: built-in strategies move to typed draws one module
   at a time; each migration improves shrinking for that strategy only.
4. Later: tape-based failure persistence, fuzzer interop, and
   Hypothesis-style generation upgrades (weird floats, biased integers)
   expressed at the choice level.

Non-goals (for now):

- Replacing `ValueTree`. It stays as the generation result and as the
  fallback shrinker; the tape engine is opt-in (`Config`) until proven.
- Fork mode support in the first phases (tape shrinking falls back to the
  ValueTree shrinker under `fork = true`).
- DataTree-style novelty search / targeted property testing (Hypothesis's
  `datatree.py`, `optimiser.py`). Out of scope entirely.

## 3. Architecture overview

```
generation:  Strategy::new_tree(runner)
             ├─ typed draws: runner.draw_integer(..) / draw_float(..) / draw_bool(..)
             └─ raw draws:   runner.rng() → next_u32 / next_u64 / fill_bytes
                             (compat wrapper: each raw call is an untyped
                              Choice on the same tape)
                both, per tape mode:
                ├─ tape Off:        sample fresh (exactly today's distributions)
                ├─ tape Recording:  sample fresh, append Choice to tape
                └─ tape Replaying:  pop next Choice from edited tape,
                                    clamp into current constraints,
                                    append actual Choice to the *output* tape

shrinking:   loop over passes until fixpoint / budget:
               propose edited tape
               re-run Strategy::new_tree in Replaying mode
               run the test on tree.current()
               accept iff test still fails AND output tape < best (shortlex)
```

(An earlier revision planned a final ValueTree simplify/complicate "polish"
pass for unmigrated strategies. It was dropped during phase 1: numeric
bisection would undo the roundness the typed passes achieve — polishing
`2.0` back into `1.75…` — and a polished value cannot be re-expressed as a
tape, so the two shrinkers cannot alternate. Unmigrated strategies shrink
via raw-choice lex-lowering instead; migrating them is the durable fix.)

The single load-bearing idea, copied from Conjecture: **the edited tape is a
suggestion; the replay's output tape is the truth.** Replay re-records what
was actually consumed, so the accepted tape is always self-consistent, and
constraint changes mid-replay (e.g. a dependent range from `flat_map`) are
handled by clamping rather than by rejecting the attempt.

## 4. The choice IR

New module `proptest/src/tape/` (name: "tape" in code, "choice tape" in
docs).

```rust
pub(crate) enum Choice {
    /// All integer draws, any width/signedness, stored order-preserving
    /// in u128 "offset space" (unsigned: identity; signed: offset binary,
    /// i.e. `(v as u128) ^ (1 << 127)`).
    Integer {
        value: u128,
        min: u128,        // constraint at draw time, offset space
        max: u128,
        shrink_to: u128,  // usually encode(clamp(0, min, max))
    },
    /// All float draws; f32 widens to f64 (as in Hypothesis).
    Float {
        value: f64,
        min: f64,         // -inf/+inf when unconstrained
        max: f64,
        allow_nan: bool,
    },
    Bool { value: bool },
    /// Untyped entropy from the RngCore compat wrapper (§6): one choice
    /// per raw RNG call. Shrink target is 0 / all-zero bytes.
    RawU32 { value: u32 },
    RawU64 { value: u64 },
    RawBytes { value: Vec<u8> },  // length fixed by the fill_bytes call
    // Later phases: Char and String choices for smarter text shrinking.
}
```

Ordering. Each choice has a complexity key, `fn complexity(&self) -> Key`
where `Key` is a small `Ord` enum (an integer-like `u128` key for
`Integer`/`Float`/`Bool`/`RawU32`/`RawU64`; `(len, bytes)` for `RawBytes`):

- `Integer`: zigzag distance from `shrink_to`, preferring the positive
  direction: `0` for the target, `2*(v-t) - 1` above, `2*(t-v)` below.
- `Float`: sign-major, then a port of Hypothesis's `float_to_lex`
  (`conjecture/floats.py`): integer-valued floats ≤ 2^56 encode as
  themselves; otherwise tag bit + reordered exponent table + bit-reversed
  mantissa. This single function is what makes float shrinking produce round
  numbers, because "smaller key" ≈ "simpler decimal". NaN keys above
  infinity.
- `Bool`: `value as u128`.
- `RawU32`/`RawU64`: the value itself; `RawBytes`: length, then bytes
  lexicographically. Untyped entropy shrinks toward zero bits, which is
  meaningful more often than not: rand's widening-multiply `Uniform` is
  monotone in the raw draw, so lex-lowering a raw choice lowers the sampled
  value for most unmigrated strategies.

A tape `A` is better than `B` iff `(A.len(), A.complexity_vec()) <
(B.len(), B.complexity_vec())` — shortlex, same as Hypothesis's
shortlex-over-choice-indices.

Spans. The tape also records a span tree (start/end markers with a label),
pushed by combinators around logical units (one collection element, one
tuple field, one union branch). Spans drive the deletion and (later)
reordering passes. Spans are metadata only; they do not affect replay.

## 5. The recording seam: typed draws on `TestRunner`

`TestRunner` ([runner.rs:71](../proptest/src/test_runner/runner.rs#L71))
gains a tape handle:

```rust
enum TapeState {
    Off,
    Recording(Tape),
    Replaying { input: Tape, cursor: usize, output: Tape, overrun: bool },
}
```

The handle is an `Rc<RefCell<TapeState>>` shared between the runner and its
`TestRng`, because there are two producers writing to one tape:

1. **Typed draws** on `TestRunner` (below) — the quality path.
2. **The `RngCore` compat wrapper**: `TestRng`'s `next_u32` / `next_u64` /
   `fill_bytes` record each call as an untyped `Raw*` choice when the tape
   is on, and pop from it when replaying. `runner.rng()` keeps its exact
   signature (`&mut TestRng`), so every existing strategy — third-party
   included — is tape-recorded with **zero code changes**. Migration to
   typed draws is purely a shrink-quality upgrade, never a compatibility
   requirement.

The typed draw API (public, since strategy authors are the eventual
audience):

```rust
impl TestRunner {
    pub fn draw_integer_in<T: TapeInt>(&mut self, min: T, max: T) -> T;
    pub fn draw_f64_in(&mut self, min: f64, max: f64,
                       sample: impl FnOnce(&mut TestRng) -> f64) -> f64;
    pub fn draw_bool(&mut self, probability_true: f64) -> bool;
    pub fn start_span(&mut self, label: &'static str);
    pub fn end_span(&mut self);
}
```

`TapeInt` is a small trait over all primitive ints providing the u128
offset-space encode/decode; `numeric_api!` in
[num.rs](../proptest/src/num.rs) switches its `new_tree` sampling from
`sample_uniform` to `draw_integer_in`, which in `Off`/`Recording` mode
samples through exactly the same `rand::Uniform` path as today (identical
distribution, identical RNG consumption), and in `Replaying` mode returns
the recorded value clamped into `[min, max]` without touching the RNG.

Floats: the range strategies pass their existing interval-splitting sampler
([float_samplers.rs](../proptest/src/num/float_samplers.rs)) as the `sample`
closure. `f64::Any`/`f32::Any` (class-based generation) selects its class
via the compat wrapper (raw choices, so the class is pinned during replay)
and then draws the assembled value through `draw_f64_in` with the
bit-masking procedure as the `sample` closure, so the value itself is a
typed `Float` choice that the float shrink passes can work on.

Replay semantics, in order:

1. Cursor past end of input → sample fresh, set `overrun = true`. The
   shrinker discards overrun attempts without running the test, following
   Hypothesis: they are usually junk, and skipping them keeps replay
   bounded. (Such an attempt could occasionally be a genuinely smaller
   failing example — the deletion passes rediscover it in aligned form.)
2. Next choice has the right kind → clamp value into the *current*
   constraints, consume it, append the clamped actual to `output`.
3. Kind mismatch (tape says Float, strategy asks for Integer) → do not
   consume; sample fresh and append. Misaligned tails are usually worse in
   shortlex order and get discarded naturally; occasionally they jump to a
   different failing example, which is fine — Hypothesis relies on the same
   effect.

Local rejects (`prop_filter` retries) during replay work unchanged: the
predicate re-runs against replayed values; if the strategy ultimately
returns `Err` (rejection budget exhausted) the attempt is discarded. This is
what fixes the filter-stall problem: the shrinker proposes *values*, the
filter re-vets them, and no simplify/complicate contract is involved.

## 6. Unmigrated strategies: the RngCore compat wrapper

Everything that still draws raw entropy via `runner.rng()` (char, string,
regex, bits, sample, third-party strategies) goes through the compat
wrapper: each `next_u32` / `next_u64` / `fill_bytes` call is one untyped
`Raw*` choice on the same tape. This was originally designed as an
RNG-reset fallback (reset the seed per attempt and hope call sequences
align); recording raw calls as first-class choices is strictly better:

- **Determinism is structural.** Unmigrated draws replay their recorded
  values *positionally* from the tape, so a tape edit elsewhere (e.g. a
  deleted span, a lowered integer) does not disturb them — no reliance on
  RNG call sequences staying aligned, no fresh-randomness hazard. The case
  seed is still reset per attempt, but only so that genuinely fresh draws
  (overrun, kind mismatch) are reproducible for debugging.
- **Free (if crude) shrinking.** Raw choices participate in the minimize
  passes lexicographically. Because rand's widening-multiply `Uniform` is
  monotone in the raw draw, lex-lowering raw choices meaningfully shrinks
  many unmigrated strategies — this is exactly how byte-level Conjecture
  worked before Hypothesis introduced the typed IR.
- **Quality floor via opt-in.** While the tape engine is opt-in, nobody
  loses shrink quality by default. Under the tape engine, an unmigrated
  strategy's shrinking can be weaker than its ValueTree shrinking (notably
  collections until phase 2 gives them deletion); the durable fix is
  migration, not a hybrid polish pass (see §3 for why polish was dropped).

One caveat inherited from byte-level Conjecture: a replayed raw value can
change how many *further* raw draws a rejection-sampling loop consumes
(rand's `Uniform` zone check, `prop_filter` retries), shifting alignment of
the tail. Such attempts usually lose the shortlex comparison and are
discarded; occasionally they land on a different valid candidate, which the
acceptance rule handles. Typed migration removes the effect per strategy.

This wrapper is the key to zero breakage and incremental migration.

## 7. Collections: continuation encoding

`vec(elem, len_range)` currently draws a size, then `size` elements
([collection.rs](../proptest/src/collection.rs)). Deleting one element's
span from such a tape misaligns replay (the size choice still says N).
Hypothesis solves this with its `many` protocol; we adopt the same in tape
mode:

```
for i in 0..max_len {
    if i >= min_len {
        if !draw_bool(p_continue) { break }   // one Bool choice per element
    }
    start_span("elem"); <generate element>; end_span();
}
```

with `p_continue` chosen so the expected length matches the middle of
today's uniform range (the size distribution changes from uniform to
truncated-geometric-ish; accepted per the 2026-07-07 decision, and only in
tape mode). Now the generic "delete span + its preceding continuation Bool"
pass shrinks any collection, and `Bool → false` truncates it — no
collection-specific shrinker code.

Non-tape mode keeps the existing size-then-elements path and `VecValueTree`
untouched.

## 8. The shrinker

`proptest/src/tape/shrink.rs`. Passes, run round-robin until a full round
makes no progress (Hypothesis's fixpoint loop, minus its pass-scheduling
sophistication), all bounded by the existing `max_shrink_iters` /
`max_shrink_time` budgets (each replay+test counts as one iteration):

1. `try_trivial`: one attempt with every choice at its shrink target.
   Catches the very common "any small input fails" case in O(1).
2. `delete_spans`: try removing each span (largest first), with the
   adaptive exponential batching trick (try deleting 1, 2, 4, ... adjacent
   spans) to make list shrinking O(log n) in the common case.
3. `minimize_individual`: per choice —
   - Integer: binary search on the complexity key toward 0 (i.e. toward
     `shrink_to`, preferring the positive side).
   - Float: candidate ladder first — `0.0`, `trunc(v)`, `ceil(v)`,
     small integers near `v` — then binary search on the integer part,
     then precision-drop by scaled rounding (port of
     `conjecture/shrinking/floats.py`).
   - Bool: `true → false`.
   - Raw: lexicographic lowering (zero the value, then binary search on
     the integer interpretation) — crude but monotone through rand's
     `Uniform`, and all the shrinking unmigrated strategies get from the
     tape itself.
4. Later phases: `minimize_duplicates` (equal choices lowered together),
   `redistribute_pairs` (shrink x while growing y for numeric pairs),
   `reorder_spans`.

Acceptance: run the test on the replayed value; accept iff it still fails
(a `Reject` counts as not-failing, same as today's shrink loop at
[runner.rs:859](../proptest/src/test_runner/runner.rs#L859)) and the output
tape is shortlex-smaller than the incumbent. The existing `result_cache`
dedups repeated identical attempts.

## 9. Runner integration

- `Config` gains `shrink_engine: ShrinkEngine` (`ValueTree` | `Tape`),
  default `ValueTree` for the spike; env var `PROPTEST_SHRINK_ENGINE`.
  Flipping the default is a later, deliberate step.
- `gen_and_run_case` ([runner.rs:650](../proptest/src/test_runner/runner.rs#L650))
  is the integration point (it is the innermost frame that still has the
  `&S: Strategy`): wrap `new_tree` in `Recording` mode; on `Fail` with the
  tape engine selected (and not under fork), call
  `tape_shrink(strategy, seed, tape, &test)` instead of the ValueTree
  `shrink`. The public `run_one(case, test)` (no strategy available) keeps
  the ValueTree path unconditionally.
- Fork mode: `shrink_engine = Tape` + `fork = true` → warn once, use
  ValueTree path. (Future: the forkfile protocol replays pass/fail
  decisions; the tape shrinker is deterministic given those, so the same
  trick can work — deferred.)

## 10. Persistence and fuzzer interop (later phase)

Persist the winning tape alongside (then instead of) the seed: a new
persisted form `ct1:<base64 of a simple length-prefixed serialization>`,
parallel to the existing `Seed::PassThrough` ("pt") machinery in
[rng.rs](../proptest/src/test_runner/rng.rs). Replaying a persisted tape is
just the Replaying mode with no shrinking. This makes regressions survive
strategy refactors and rand upgrades, and gives fuzzer harnesses a
structured format to mutate (today's `PassThrough` byte interface remains
for raw-entropy fuzzing).

## 11. Generation-side upgrades (later phase, same seam)

Once draws are typed, Hypothesis's generation tricks become one-line-ish
changes inside `draw_integer_in`/`draw_f64_in`, applied uniformly to every
strategy:

- occasional "weird" values: bounds, `next_up/next_down` of bounds, ±0,
  ±inf, NaN (where permitted), matching Hypothesis's ~5% special-value
  injection;
- biased integer magnitudes for wide ranges (Hypothesis now uses a
  piecewise uniform-within-±256 / heavy-tailed-beyond distribution,
  CDF-truncated for bounded ranges);
- these change default distributions (approved), and are deliberately
  sequenced *after* the shrinker exists, so any newly-found nasty failures
  shrink well.

## 12. Risks / open questions

- **Replay cost.** Each shrink attempt re-runs `new_tree`. Proptest's README
  already notes generation can be 10x QuickCheck's cost; shrink budgets
  (`max_shrink_iters`) bound the damage, and `try_trivial` + adaptive
  deletion keep attempt counts low. Measure on proptest's own test suite.
- **Tape size.** Recording at the RngCore level means entropy-hungry
  strategies (regex/string generation, large collections) produce long
  tapes; shortlex comparison and pass iteration are O(len). Hypothesis
  capped its buffer (8KB) for the same reason. Mitigation if it bites: a
  soft cap on recorded choices beyond which the tape degrades to
  seed-replay for the tail.
- **Union/oneof branch shrinking.** The compat wrapper pins branch
  selection during replay (it's a raw choice), but shrinking *across*
  branches (prefer earlier `prop_oneof` arms, like today's union
  ValueTree) needs the branch index as a typed `Integer` choice — phase 2.
- **`Just`-heavy tapes.** Strategies that draw nothing record nothing —
  fine; spans may be empty.
- **u128 offset space** assumes all integer draws fit u128 — true for all
  primitive types.
- **f32 round-trip:** widening to f64 and clamping back must not escape
  `[min, max]` after `as f32` narrowing; clamp in f32 space at the call
  site.
- **API commitment.** `draw_*` on `TestRunner` becomes public API for
  strategy authors. Keep the surface minimal (the five methods above) and
  document Replaying semantics as implementation-defined.

## 13. Staged plan

- **Phase 1 (spike, next):** `tape` module (Choice, Tape, complexity,
  shortlex), shared `TapeState` handle, the RngCore compat wrapper in
  `TestRng`, typed draws wired into the `numeric_api!` integer ranges +
  `int_any!` + float range strategies + float `Any`, shrinker with
  `try_trivial` + `minimize_individual` (integer binary search, float
  candidate ladder + `float_to_lex` port, raw lex-lowering),
  `ShrinkEngine` config, runner integration. Tests: round-float minimal
  examples, filter no-stall, flat_map shrink quality, unmigrated-strategy
  shrinking via raw choices, budget respect.
  **Status: DONE.**
- **Phase 2:** spans; continuation encoding for `vec`/collections;
  `delete_spans`; `prop_oneof` branch choice.
  **Status: DONE, including adaptive deletion batching.**
- **Phase 3:** migrate char/string/bits/sample; state-machine crate
  evaluation.
- **Phase 4:** cross-value passes (duplicates, redistribute, reorder).
- **Phase 5:** tape persistence (`ct1:`), corpus reuse, fuzzer entry point.
- **Phase 6:** generation upgrades (§11).
- **Phase 7:** flip `shrink_engine` default to `Tape`; ValueTree path
  remains for `run_one` and unmigrated strategies.
