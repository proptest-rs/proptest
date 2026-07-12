//-
// Copyright 2026 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! The choice tape: a Conjecture-style typed record of every decision made
//! while generating a test case, used by the experimental tape shrink engine
//! (`Config::shrink_engine = ShrinkEngine::Tape`).
//!
//! See `design/choice-tape-shrinking.md` in the repository root for the full
//! design. In short: generation records each random draw as a `Choice`;
//! shrinking edits the recorded tape and re-runs generation in `Replaying`
//! mode, accepting an edit iff the test still fails and the re-recorded
//! output tape is shortlex-smaller than the incumbent.

use crate::std_facade::Vec;
use core::cmp::Ordering;

#[cfg(not(feature = "std"))]
use num_traits::float::FloatCore;

/// One recorded decision.
///
/// `Integer` values (and their constraints) are stored in u128 "offset
/// space": an order-preserving embedding of the original integer type
/// (unsigned: identity; signed: offset binary, i.e. `x ^ (1 << 127)` of the
/// sign-extended value). This gives all integer widths a single canonical
/// unsigned ordering.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Choice {
    Integer {
        value: u128,
        min: u128,
        max: u128,
        /// The value shrinking moves toward; `encode(0)` clamped into
        /// `[min, max]` for numeric strategies.
        shrink_to: u128,
    },
    Float {
        value: f64,
        min: f64,
        max: f64,
        allow_nan: bool,
    },
    #[allow(dead_code)] // constructed starting with the collection encoding
    Bool {
        value: bool,
    },
    /// Untyped entropy recorded by the `RngCore` compat wrapper.
    RawU32 {
        value: u32,
    },
    RawU64 {
        value: u64,
    },
    RawBytes {
        value: Vec<u8>,
    },
}

impl Choice {
    /// Complexity of a scalar choice. Lower is "simpler"; the shrink
    /// target keys to 0. `None` for RawBytes, which order after all
    /// scalars by (len, bytes) in `cmp_complexity`.
    fn scalar_key(&self) -> Option<u128> {
        match self {
            Choice::Integer {
                value, shrink_to, ..
            } => Some(zigzag(*value, *shrink_to)),
            Choice::Float { value, .. } => Some(float_key(*value)),
            Choice::Bool { value } => Some(*value as u128),
            Choice::RawU32 { value } => Some(*value as u128),
            Choice::RawU64 { value } => Some(*value as u128),
            Choice::RawBytes { .. } => None,
        }
    }

    /// Allocation-free complexity comparison between two choices,
    /// consistent with the historical ordering (all scalar keys order
    /// before byte blobs; byte blobs order by length then contents).
    pub(crate) fn cmp_complexity(&self, other: &Choice) -> Ordering {
        match (self.scalar_key(), other.scalar_key()) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => match (self, other) {
                (
                    Choice::RawBytes { value: a },
                    Choice::RawBytes { value: b },
                ) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
                _ => unreachable!(),
            },
        }
    }

    /// The same choice with its value replaced by its shrink target.
    pub(crate) fn trivial(&self) -> Choice {
        match self {
            Choice::Integer {
                min,
                max,
                shrink_to,
                ..
            } => Choice::Integer {
                value: *shrink_to,
                min: *min,
                max: *max,
                shrink_to: *shrink_to,
            },
            Choice::Float {
                min,
                max,
                allow_nan,
                ..
            } => Choice::Float {
                value: float_shrink_target(*min, *max),
                min: *min,
                max: *max,
                allow_nan: *allow_nan,
            },
            Choice::Bool { .. } => Choice::Bool { value: false },
            Choice::RawU32 { .. } => Choice::RawU32 { value: 0 },
            Choice::RawU64 { .. } => Choice::RawU64 { value: 0 },
            Choice::RawBytes { value } => Choice::RawBytes {
                value: vec![0; value.len()],
            },
        }
    }
}

/// Distance of `v` from the shrink target `t`, zigzag-encoded so that the
/// target keys to 0 and, at equal distance, the positive direction is
/// preferred (`t` < `t+1` < `t-1` < `t+2` < ...).
///
/// Saturates at the extremes of u128, which slightly coarsens the ordering
/// for astronomically distant values; that only weakens tie-breaking, never
/// soundness.
pub(crate) fn zigzag(v: u128, t: u128) -> u128 {
    if v >= t {
        (v - t).saturating_mul(2).saturating_sub(1)
    } else {
        (t - v).saturating_mul(2)
    }
}

/// Complexity key for floats: NaN worst, then by sign (non-negative
/// simpler), then by the lexicographic encoding of the magnitude.
pub(crate) fn float_key(v: f64) -> u128 {
    if v.is_nan() {
        return u128::MAX;
    }
    let sign = if v.is_sign_negative() { 1u128 << 64 } else { 0 };
    sign | float_to_lex(v.abs()) as u128
}

/// The shrink target of a float draw constrained to `[min, max]`: the
/// in-range value closest to +0.0.
pub(crate) fn float_shrink_target(min: f64, max: f64) -> f64 {
    min.max(0.0).min(max)
}

/// Port of Hypothesis's lexicographic float encoding
/// (`hypothesis.internal.conjecture.floats.float_to_lex`).
///
/// Maps non-negative floats to u64 such that lexicographically smaller
/// integers correspond to "simpler" floats:
///
/// - Integer-valued floats below 2^56 encode as themselves (tag bit 0), so
///   0 < 1 < 2 < ... in encoded space.
/// - Everything else gets the tag bit set, an exponent reordered so that
///   integer-like exponents come first, and the fractional mantissa bits
///   reversed so that shorter decimal fractions are smaller.
pub(crate) fn float_to_lex(f: f64) -> u64 {
    debug_assert!(
        !(f < 0.0),
        "float_to_lex requires a non-negative input, got {}",
        f
    );
    if f.is_finite() && f == f.trunc() && f < (1u64 << 56) as f64 {
        return f as u64;
    }
    base_float_to_lex(f)
}

const MANTISSA_MASK: u64 = (1u64 << 52) - 1;
const MAX_EXPONENT: u64 = 0x7FF;
const BIAS: i64 = 1023;

/// Rank of an 11-bit exponent under Hypothesis's ordering: non-negative
/// unbiased exponents first in increasing order, then negative unbiased
/// exponents in decreasing order, then the inf/NaN exponent last.
///
/// This is a closed form of Hypothesis's sorted `ENCODING_TABLE`.
fn exponent_rank(e: u64) -> u64 {
    debug_assert!(e <= MAX_EXPONENT);
    if e == MAX_EXPONENT {
        MAX_EXPONENT
    } else if e >= BIAS as u64 {
        e - BIAS as u64
    } else {
        // unbiased = e - 1023 in -1023..=-1; rank 1024..=2046 with the
        // least negative exponent first.
        2046 - e
    }
}

fn update_mantissa(unbiased_exponent: i64, mut mantissa: u64) -> u64 {
    if unbiased_exponent <= 0 {
        // Subnormals and values in [0, 2): all 52 bits are fractional;
        // reverse them all so that "fewer fraction bits" sorts lower.
        mantissa = mantissa.reverse_bits() >> (64 - 52);
    } else if unbiased_exponent <= 51 {
        let n_fractional = (52 - unbiased_exponent) as u32;
        let fractional = mantissa & ((1u64 << n_fractional) - 1);
        mantissa -= fractional;
        mantissa |= fractional.reverse_bits() >> (64 - n_fractional);
    }
    mantissa
}

fn base_float_to_lex(f: f64) -> u64 {
    let bits = f.to_bits() & !(1u64 << 63);
    let exponent = bits >> 52;
    let mantissa =
        update_mantissa(exponent as i64 - BIAS, bits & MANTISSA_MASK);
    (1u64 << 63) | (exponent_rank(exponent) << 52) | mantissa
}

/// A contiguous run of choices forming one logical unit of generation
/// (e.g. one collection element together with its continuation flag).
/// Spans are metadata for the deletion pass: replay ignores them and
/// re-records them from the actual generation structure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A recorded generation run.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Tape {
    pub(crate) choices: Vec<Choice>,
    pub(crate) spans: Vec<Span>,
}

impl Tape {
    /// Shortlex comparison: fewer choices first, then elementwise by
    /// complexity key. `Less` means `self` is simpler than `other`.
    pub(crate) fn cmp_key(&self, other: &Tape) -> Ordering {
        self.choices.len().cmp(&other.choices.len()).then_with(|| {
            for (a, b) in self.choices.iter().zip(&other.choices) {
                match a.cmp_complexity(b) {
                    Ordering::Equal => continue,
                    unequal => return unequal,
                }
            }
            Ordering::Equal
        })
    }

    /// The tape with every choice at its shrink target.
    pub(crate) fn trivial(&self) -> Tape {
        Tape {
            choices: self.choices.iter().map(Choice::trivial).collect(),
            spans: self.spans.clone(),
        }
    }

    /// A copy of the tape with the choice at `idx` replaced. Like
    /// `with_span_deleted`, the copy's span list is dropped: replay
    /// ignores input spans, and an accepted proposal re-records fresh
    /// ones on its output tape.
    pub(crate) fn with_choice(&self, idx: usize, choice: Choice) -> Tape {
        let mut choices = self.choices.clone();
        choices[idx] = choice;
        Tape {
            choices,
            spans: Vec::new(),
        }
    }

    /// A copy of the tape with the choices of `span` removed. The copy's
    /// span list is dropped (it would be stale); an accepted proposal gets
    /// fresh spans from the replay's output tape anyway.
    pub(crate) fn with_span_deleted(&self, span: Span) -> Tape {
        let mut choices =
            Vec::with_capacity(self.choices.len() - (span.end - span.start));
        choices.extend_from_slice(&self.choices[..span.start]);
        choices.extend_from_slice(&self.choices[span.end..]);
        Tape {
            choices,
            spans: Vec::new(),
        }
    }
}

/// Recording/replaying state, owned by `TestRng` so that both the typed
/// draws on `TestRunner` and the raw `RngCore` calls write to the same tape.
#[derive(Clone, Debug)]
pub(crate) enum TapeMode {
    Off,
    Recording {
        tape: Tape,
    },
    Replaying {
        input: Tape,
        cursor: usize,
        output: Tape,
        overrun: bool,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct TapeState {
    mode: TapeMode,
    /// While positive, raw RngCore draws bypass the tape entirely. Typed
    /// draws set this while running their sample closures so the closure's
    /// raw entropy is subsumed by the single typed choice.
    suppress_raw: u32,
    /// Start indices of currently-open spans (choice count at
    /// `start_span` time).
    span_stack: Vec<usize>,
}

impl Default for TapeState {
    fn default() -> Self {
        TapeState {
            mode: TapeMode::Off,
            suppress_raw: 0,
            span_stack: Vec::new(),
        }
    }
}

impl TapeState {
    pub(crate) fn is_on(&self) -> bool {
        !matches!(self.mode, TapeMode::Off)
    }

    pub(crate) fn is_replaying(&self) -> bool {
        matches!(self.mode, TapeMode::Replaying { .. })
    }

    /// Whether raw RngCore draws should currently be recorded/replayed.
    pub(crate) fn raw_active(&self) -> bool {
        self.is_on() && 0 == self.suppress_raw
    }

    pub(crate) fn suppress_raw(&mut self) {
        self.suppress_raw += 1;
    }

    pub(crate) fn unsuppress_raw(&mut self) {
        debug_assert!(self.suppress_raw > 0);
        self.suppress_raw -= 1;
    }

    pub(crate) fn start_recording(&mut self) {
        self.mode = TapeMode::Recording {
            tape: Tape::default(),
        };
        self.span_stack.clear();
    }

    /// Stop recording and return the recorded tape. Returns an empty tape
    /// if not recording.
    pub(crate) fn take_recording(&mut self) -> Tape {
        self.span_stack.clear();
        match core::mem::replace(&mut self.mode, TapeMode::Off) {
            TapeMode::Recording { tape } => tape,
            _ => Tape::default(),
        }
    }

    pub(crate) fn start_replay(&mut self, input: Tape) {
        self.mode = TapeMode::Replaying {
            input,
            cursor: 0,
            output: Tape::default(),
            overrun: false,
        };
        self.span_stack.clear();
    }

    /// Stop replaying and return the re-recorded output tape plus whether
    /// the input was overrun. Returns an empty tape if not replaying.
    pub(crate) fn finish_replay(&mut self) -> (Tape, bool) {
        self.span_stack.clear();
        match core::mem::replace(&mut self.mode, TapeMode::Off) {
            TapeMode::Replaying {
                output, overrun, ..
            } => (output, overrun),
            _ => (Tape::default(), false),
        }
    }

    /// The tape currently being written: the recording, or the output
    /// re-recording during replay.
    fn active_tape_mut(&mut self) -> Option<&mut Tape> {
        match &mut self.mode {
            TapeMode::Off => None,
            TapeMode::Recording { tape } => Some(tape),
            TapeMode::Replaying { output, .. } => Some(output),
        }
    }

    /// Open a span at the current position of the active tape. No-op when
    /// the tape is off.
    pub(crate) fn start_span(&mut self) {
        let pos = match self.active_tape_mut() {
            Some(tape) => tape.choices.len(),
            None => return,
        };
        self.span_stack.push(pos);
    }

    /// Close the innermost open span. Robust against unbalanced calls
    /// (e.g. when generation errors out mid-span): closing with no open
    /// span is a no-op.
    pub(crate) fn end_span(&mut self) {
        let start = match self.span_stack.pop() {
            Some(start) => start,
            None => return,
        };
        if let Some(tape) = self.active_tape_mut() {
            let end = tape.choices.len();
            if end > start {
                tape.spans.push(Span { start, end });
            }
        }
    }

    /// Append the choice actually used to the active tape (the recording,
    /// or the output re-recording during replay). No-op when off.
    pub(crate) fn record(&mut self, choice: Choice) {
        match &mut self.mode {
            TapeMode::Off => (),
            TapeMode::Recording { tape } => tape.choices.push(choice),
            TapeMode::Replaying { output, .. } => output.choices.push(choice),
        }
    }

    /// Record a choice whose value is forced by generation structure
    /// rather than drawn (e.g. the "stop" continuation flag of a
    /// maximum-length collection). During replay a matching next input
    /// choice is consumed so edits stay aligned, but its value is
    /// ignored, and running off the end of the input is NOT an overrun —
    /// no information is being read.
    pub(crate) fn record_forced_bool(&mut self, value: bool) {
        if !self.is_on() {
            return;
        }
        if let TapeMode::Replaying { input, cursor, .. } = &mut self.mode {
            if *cursor < input.choices.len()
                && matches!(input.choices[*cursor], Choice::Bool { .. })
            {
                *cursor += 1;
            }
        }
        self.record(Choice::Bool { value });
    }

    /// Record a boolean that generation forces to `forced` (drawing no
    /// entropy), but that shrinking may edit: during replay the next
    /// input Bool's value is honored if present. Used for units that are
    /// structurally mandatory during generation yet deletable during
    /// shrinking, e.g. a state-machine sequence's transitions below the
    /// declared minimum length, which the classic shrinker deliberately
    /// deletes past.
    ///
    /// Contrast `record_forced_bool`, which ignores the replayed value
    /// (for markers whose value can never matter, like the stop flag of
    /// a maximum-length collection).
    pub(crate) fn draw_bool_forced(&mut self, forced: bool) -> bool {
        if !self.is_on() {
            return forced;
        }
        if let Some(Choice::Bool { value }) =
            self.pop_replay(|c| matches!(c, Choice::Bool { .. }))
        {
            self.record(Choice::Bool { value });
            return value;
        }
        // Not replaying, kind mismatch, or overrun: use the forced value.
        self.record(Choice::Bool { value: forced });
        forced
    }

    /// During replay, consume and return the next input choice if `matcher`
    /// accepts it. Returns `None` (and samples must go fresh) on kind
    /// mismatch, on overrun (also setting the overrun flag), or when not
    /// replaying.
    pub(crate) fn pop_replay(
        &mut self,
        matcher: impl FnOnce(&Choice) -> bool,
    ) -> Option<Choice> {
        if let TapeMode::Replaying {
            input,
            cursor,
            overrun,
            ..
        } = &mut self.mode
        {
            if *cursor >= input.choices.len() {
                *overrun = true;
                return None;
            }
            if matcher(&input.choices[*cursor]) {
                let choice = input.choices[*cursor].clone();
                *cursor += 1;
                return Some(choice);
            }
        }
        None
    }
}

/// Serialize a tape for failure persistence (the payload of the "ct1"
/// persisted-failure format). Spans are shrinking metadata and are not
/// persisted; replay ignores them.
pub(crate) fn serialize_tape(tape: &Tape) -> Vec<u8> {
    let mut out = Vec::new();
    for choice in &tape.choices {
        match choice {
            Choice::Integer {
                value,
                min,
                max,
                shrink_to,
            } => {
                out.push(0);
                out.extend_from_slice(&value.to_le_bytes());
                out.extend_from_slice(&min.to_le_bytes());
                out.extend_from_slice(&max.to_le_bytes());
                out.extend_from_slice(&shrink_to.to_le_bytes());
            }
            Choice::Float {
                value,
                min,
                max,
                allow_nan,
            } => {
                out.push(1);
                out.extend_from_slice(&value.to_le_bytes());
                out.extend_from_slice(&min.to_le_bytes());
                out.extend_from_slice(&max.to_le_bytes());
                out.push(*allow_nan as u8);
            }
            Choice::Bool { value } => {
                out.push(2);
                out.push(*value as u8);
            }
            Choice::RawU32 { value } => {
                out.push(3);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Choice::RawU64 { value } => {
                out.push(4);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Choice::RawBytes { value } => {
                out.push(5);
                out.extend_from_slice(&(value.len() as u32).to_le_bytes());
                out.extend_from_slice(value);
            }
        }
    }
    out
}

/// Inverse of `serialize_tape`. Strict: any malformed input yields `None`
/// (the persistence layer then ignores the entry).
pub(crate) fn deserialize_tape(bytes: &[u8]) -> Option<Tape> {
    fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
        if bytes.len() < n {
            return None;
        }
        let (head, tail) = bytes.split_at(n);
        *bytes = tail;
        Some(head)
    }
    fn take_u128(bytes: &mut &[u8]) -> Option<u128> {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(take(bytes, 16)?);
        Some(u128::from_le_bytes(buf))
    }
    fn take_f64(bytes: &mut &[u8]) -> Option<f64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(take(bytes, 8)?);
        Some(f64::from_le_bytes(buf))
    }

    let mut bytes = bytes;
    let mut choices = Vec::new();
    while !bytes.is_empty() {
        let tag = take(&mut bytes, 1)?[0];
        choices.push(match tag {
            0 => Choice::Integer {
                value: take_u128(&mut bytes)?,
                min: take_u128(&mut bytes)?,
                max: take_u128(&mut bytes)?,
                shrink_to: take_u128(&mut bytes)?,
            },
            1 => Choice::Float {
                value: take_f64(&mut bytes)?,
                min: take_f64(&mut bytes)?,
                max: take_f64(&mut bytes)?,
                allow_nan: 0 != take(&mut bytes, 1)?[0],
            },
            2 => Choice::Bool {
                value: 0 != take(&mut bytes, 1)?[0],
            },
            3 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(take(&mut bytes, 4)?);
                Choice::RawU32 {
                    value: u32::from_le_bytes(buf),
                }
            }
            4 => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(take(&mut bytes, 8)?);
                Choice::RawU64 {
                    value: u64::from_le_bytes(buf),
                }
            }
            5 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(take(&mut bytes, 4)?);
                let len = u32::from_le_bytes(buf) as usize;
                Choice::RawBytes {
                    value: take(&mut bytes, len)?.to_vec(),
                }
            }
            _ => return None,
        });
    }
    Some(Tape {
        choices,
        spans: Vec::new(),
    })
}

/// Order-preserving embedding of primitive integers into u128 offset space.
pub(crate) trait TapeInt: Copy {
    fn encode(self) -> u128;
    fn decode(encoded: u128) -> Self;
    /// `encode(0)`, the natural shrink target before range clamping.
    fn encode_zero() -> u128;
}

macro_rules! tape_int_unsigned {
    ($($typ:ty),*) => {$(
        impl TapeInt for $typ {
            fn encode(self) -> u128 {
                self as u128
            }
            fn decode(encoded: u128) -> Self {
                encoded as $typ
            }
            fn encode_zero() -> u128 {
                0
            }
        }
    )*};
}

macro_rules! tape_int_signed {
    ($($typ:ty),*) => {$(
        impl TapeInt for $typ {
            fn encode(self) -> u128 {
                (self as i128 as u128) ^ (1u128 << 127)
            }
            fn decode(encoded: u128) -> Self {
                (encoded ^ (1u128 << 127)) as i128 as $typ
            }
            fn encode_zero() -> u128 {
                1u128 << 127
            }
        }
    )*};
}

tape_int_unsigned!(u8, u16, u32, u64, u128, usize);
tape_int_signed!(i8, i16, i32, i64, i128, isize);

/// `trunc` usable from both std and no_std builds.
pub(crate) fn float_trunc(v: f64) -> f64 {
    v.trunc()
}

/// Round `v` to `k` binary digits of fraction, for the precision-dropping
/// shrink pass.
pub(crate) fn float_round_to_precision(v: f64, k: i32) -> f64 {
    let scale = 2.0f64.powi(k);
    let scaled = v * scale;
    if !scaled.is_finite() {
        return v;
    }
    scaled.round() / scale
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::std_facade::Box;

    fn tape_of(choices: Vec<Choice>) -> Tape {
        Tape {
            choices,
            spans: Vec::new(),
        }
    }

    #[test]
    fn zigzag_prefers_target_then_positive_side() {
        let t = 100u128;
        assert_eq!(0, zigzag(100, t));
        assert_eq!(1, zigzag(101, t));
        assert_eq!(2, zigzag(99, t));
        assert_eq!(3, zigzag(102, t));
        assert_eq!(4, zigzag(98, t));
    }

    #[test]
    fn tape_int_roundtrip_and_order() {
        fn check<T: TapeInt + PartialEq + core::fmt::Debug>(values: &[T]) {
            for &v in values {
                assert_eq!(v, T::decode(T::encode(v)));
            }
            for pair in values.windows(2) {
                assert!(pair[0].encode() < pair[1].encode());
            }
        }
        check(&[0u8, 1, 200, u8::MAX]);
        check(&[i8::MIN, -1, 0, 1, i8::MAX]);
        check(&[i64::MIN, -1, 0, 1, i64::MAX]);
        check(&[u128::MIN, 1, u128::MAX]);
        check(&[i128::MIN, -1, 0, i128::MAX]);
        assert_eq!(i64::encode(0), i64::encode_zero());
        assert_eq!(u64::encode(0), u64::encode_zero());
    }

    #[test]
    fn float_lex_simple_integers_encode_as_themselves() {
        // (Values must be exactly representable as f64, so no 2^56 - 1:
        // that rounds up to 2^56, which is no longer "simple".)
        for i in [0u64, 1, 2, 3, 10, 1000, 1 << 53, 1 << 55] {
            assert_eq!(i, float_to_lex(i as f64));
        }
    }

    #[test]
    fn float_lex_integers_are_simpler_than_fractions() {
        // Any integer below 2^56 keys below any non-integer.
        assert!(float_to_lex(1000000.0) < float_to_lex(1.5));
        assert!(float_to_lex(3.0) < float_to_lex(2.5));
        // Ties on the integer part: fewer fraction bits is simpler.
        assert!(float_to_lex(2.5) < float_to_lex(2.25));
        assert!(float_to_lex(2.25) < float_to_lex(2.125));
        // Infinity is worse than all finite values.
        assert!(float_to_lex(f64::MAX) < float_to_lex(f64::INFINITY));
        assert!(float_to_lex(1e300) < float_to_lex(f64::INFINITY));
    }

    #[test]
    fn float_key_sign_and_nan_ordering() {
        assert!(float_key(1.0) < float_key(-1.0));
        assert!(float_key(-1.0) < float_key(f64::NAN));
        assert!(float_key(f64::INFINITY) < float_key(f64::NAN));
        assert_eq!(0, float_key(0.0));
    }

    #[test]
    fn float_shrink_target_clamps_toward_zero() {
        assert_eq!(0.0, float_shrink_target(-10.0, 10.0));
        assert_eq!(1.5, float_shrink_target(1.5, 10.0));
        assert_eq!(-1.5, float_shrink_target(-10.0, -1.5));
        assert_eq!(0.0, float_shrink_target(f64::NEG_INFINITY, f64::INFINITY));
    }

    #[test]
    fn shortlex_shorter_tape_wins() {
        let long = tape_of(vec![
            Choice::RawU32 { value: 0 },
            Choice::RawU32 { value: 0 },
        ]);
        let short = tape_of(vec![Choice::RawU32 { value: u32::MAX }]);
        assert_eq!(Ordering::Less, short.cmp_key(&long));
    }

    #[test]
    fn shortlex_compares_keys_elementwise() {
        let a = tape_of(vec![
            Choice::RawU32 { value: 1 },
            Choice::RawU32 { value: 100 },
        ]);
        let b = tape_of(vec![
            Choice::RawU32 { value: 2 },
            Choice::RawU32 { value: 0 },
        ]);
        assert_eq!(Ordering::Less, a.cmp_key(&b));
    }

    #[test]
    fn trivial_tape_is_minimal() {
        let tape = tape_of(vec![
            Choice::Integer {
                value: i32::encode(57),
                min: i32::encode(-100),
                max: i32::encode(100),
                shrink_to: i32::encode_zero(),
            },
            Choice::Float {
                value: 3.7,
                min: 1.5,
                max: 10.0,
                allow_nan: false,
            },
            Choice::RawBytes {
                value: vec![1, 2, 3],
            },
        ]);
        let trivial = tape.trivial();
        assert_eq!(Ordering::Less, trivial.cmp_key(&tape));
        assert_eq!(Ordering::Equal, trivial.cmp_key(&trivial.trivial()));
        match &trivial.choices[1] {
            Choice::Float { value, .. } => assert_eq!(1.5, *value),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_float_range_to_round_value() {
        // The headline feature: a float threshold failure shrinks to the
        // smallest *round* failing value, not to an ugly boundary
        // approximation like 1.7000000000000002.
        let mut runner = engine_runner();
        let result = runner.run(&(0.0f64..10.0), |v| {
            if v >= 1.7 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(2.0, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_filtered_integers_to_boundary() {
        // The ValueTree shrinker can stall on filters because simplify()
        // has no way to skip over rejected values; the tape engine
        // re-vets every proposal through the filter.
        use crate::strategy::Strategy;
        let mut runner = engine_runner();
        let strategy = (0i32..1000).prop_filter("even", |v| 0 == v % 2);
        let result = runner.run(&strategy, |v| {
            if v >= 100 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(100, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_each_tuple_component() {
        // Per-choice minimization drives the pair onto the failure
        // boundary. (Redistribution *along* the boundary toward (0, 100)
        // is a phase-4 cross-value pass.)
        let mut runner = engine_runner();
        let result = runner.run(&(0i32..1000, 0i32..1000), |(a, b)| {
            if a + b >= 100 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, (a, b))) => {
                assert_eq!(100, a + b, "final pair: ({}, {})", a, b);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_through_flat_map() {
        // flat_map replays naturally: shrinking the outer choice reuses
        // the recorded inner choice, clamped into the new constraints.
        use crate::strategy::Strategy;
        let mut runner = engine_runner();
        let strategy = (1i32..=5).prop_flat_map(|n| 0i32..(n * 100));
        let result = runner.run(&strategy, |v| {
            if v >= 57 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(57, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_length_prefixed_vec_to_single_element() {
        // Through a bind, the collection length is an explicit earlier
        // choice, so getting from [.., 0, 100] to [100] needs the
        // lower-and-delete pass: neither deleting a zero span (the
        // length still demands the old count) nor lowering the length
        // (the tail element falls off) works alone.
        use crate::strategy::Strategy;
        let mut runner = engine_runner();
        let strategy = (1usize..=64)
            .prop_flat_map(|len| crate::collection::vec(0i32..1000, len..=len));
        let result = runner.run(&strategy, |v| {
            if v.iter().sum::<i32>() >= 100 {
                Err(crate::test_runner::TestCaseError::fail("sum too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(vec![100], value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_unmigrated_strategy_via_raw_choices() {
        // bool::ANY draws straight from the RNG; the compat wrapper
        // records that as a raw choice and the trivial pass zeroes it.
        let mut runner = engine_runner();
        let result = runner.run(&crate::bool::ANY, |_| {
            Err(crate::test_runner::TestCaseError::fail("always"))
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(false, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_any_float_to_round_value() {
        // f64::ANY (class-based generation) records its value as a typed
        // Float choice, so it gets round-value shrinking too.
        let mut runner = engine_runner();
        let result = runner.run(&crate::num::f64::ANY, |v| {
            if v.is_finite() && v >= 1.7 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(2.0, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_keeps_class_restricted_floats_in_class() {
        // Shrink proposals (0.0, truncation, ...) fall outside the
        // allowed class set; the conform hook maps them back in, so the
        // minimal example is the smallest positive subnormal, not 0.0.
        let mut runner = engine_runner();
        let strategy = crate::num::f64::POSITIVE | crate::num::f64::SUBNORMAL;
        let result = runner.run(&strategy, |_| {
            Err(crate::test_runner::TestCaseError::fail("always"))
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert!(
                    value.is_sign_positive()
                        && value.classify() == core::num::FpCategory::Subnormal,
                    "value left the strategy's class set: {:?}",
                    value
                );
                assert_eq!(f64::from_bits(1), value);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_char_range_to_boundary() {
        let mut runner = engine_runner();
        let result = runner.run(&crate::char::range('a', 'z'), |c| {
            if c >= 'd' {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!('d', value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_any_char_to_convenient_bottom() {
        // char shrink targets are the hard-wired convenient characters
        // ('a', 'A', '0', ' ', '¡', or NUL), not necessarily NUL.
        let mut runner = engine_runner();
        let result = runner.run(&crate::char::any(), |_| {
            Err(crate::test_runner::TestCaseError::fail("always"))
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert!(
                    ['\0', ' ', '0', 'A', 'a', '¡'].contains(&value),
                    "expected a convenient bottom, got {:?}",
                    value
                );
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_deletes_vec_elements() {
        // Element deletion is a generic span-deletion pass under the tape
        // engine; remaining elements minimize to their targets.
        let mut runner = engine_runner();
        let result =
            runner.run(&crate::collection::vec(0i32..100, 0..10), |v| {
                if v.len() >= 3 {
                    Err(crate::test_runner::TestCaseError::fail("too long"))
                } else {
                    Ok(())
                }
            });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(vec![0, 0, 0], value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_deletes_long_vec_via_batched_deletion() {
        // Starts around 50 elements on average; batched span deletion
        // takes out long runs in O(log n) attempts.
        let mut runner = engine_runner();
        let result =
            runner.run(&crate::collection::vec(0i32..100, 0..100), |v| {
                if v.len() >= 5 {
                    Err(crate::test_runner::TestCaseError::fail("too long"))
                } else {
                    Ok(())
                }
            });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(vec![0, 0, 0, 0, 0], value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_deletes_elements_from_max_length_vecs() {
        // Regression test: maximum-length vecs draw no natural "stop"
        // flag, and without the forced stop marker every deletion edit
        // overran the tape and was rejected. Sweep seeds so some initial
        // failing cases are at maximum length.
        for seed_byte in 0..20u8 {
            let mut runner = crate::test_runner::TestRunner::new_with_rng(
                engine_config(),
                crate::test_runner::TestRng::from_seed(
                    crate::test_runner::RngAlgorithm::ChaCha,
                    &[seed_byte; 32],
                ),
            );
            let result =
                runner.run(&crate::collection::vec(0i32..100, 0..6), |v| {
                    if v.len() >= 2 {
                        Err(crate::test_runner::TestCaseError::fail("too long"))
                    } else {
                        Ok(())
                    }
                });
            match result {
                Err(crate::test_runner::TestError::Fail(_, value)) => {
                    assert_eq!(vec![0, 0], value, "seed byte {}", seed_byte)
                }
                other => panic!(
                    "unexpected result for seed byte {}: {:?}",
                    seed_byte, other
                ),
            }
        }
    }

    #[test]
    fn replay_engine_minimizes_vec_elements() {
        let mut runner = engine_runner();
        let result = runner.run(&crate::collection::vec(0i32..100, 3), |v| {
            if v.iter().any(|&e| e >= 7) {
                Err(crate::test_runner::TestCaseError::fail("big elem"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, mut value)) => {
                value.sort();
                assert_eq!(vec![0, 0, 7], value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinks_union_to_first_branch() {
        use crate::strategy::Strategy;
        let mut runner = engine_runner();
        let strategy =
            crate::prop_oneof![crate::strategy::Just(3i32), 10i32..20,];
        let result = runner.run(&strategy.boxed(), |_| {
            Err(crate::test_runner::TestCaseError::fail("always"))
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(3, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_redistributes_vec_sum_to_single_element() {
        // Needs the cross-value redistribute pass: [27, 23] -> [0, 50],
        // then deletion of the zero -> [50].
        let mut runner = engine_runner();
        let result =
            runner.run(&crate::collection::vec(0i32..100, 0..20), |v| {
                if v.iter().sum::<i32>() >= 50 {
                    Err(crate::test_runner::TestCaseError::fail("big sum"))
                } else {
                    Ok(())
                }
            });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(vec![50], value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_redistributes_pair_within_constraints() {
        // Transfer clamps at the second component's upper bound.
        let mut runner = engine_runner();
        let result = runner.run(&(0i32..60, 0i32..60), |(a, b)| {
            if a + b >= 100 {
                Err(crate::test_runner::TestCaseError::fail("big sum"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, (a, b))) => {
                assert_eq!((41, 59), (a, b))
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_lowers_duplicates_together() {
        // No single-choice edit preserves a == b; the duplicates pass
        // lowers both together. Equal pairs are rare (1/1000 per case),
        // so give the runner enough cases to find one.
        let mut config = engine_config();
        config.cases = 20_000;
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        );
        let result = runner.run(&(0i32..1000, 0i32..1000), |(a, b)| {
            if a == b && a >= 10 {
                Err(crate::test_runner::TestCaseError::fail("equal"))
            } else {
                Ok(())
            }
        });
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!((10, 10), value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_respects_zero_shrink_budget() {
        let mut config = engine_config();
        config.max_shrink_iters = 0;
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        );
        let result = runner.run(&(0.0f64..10.0), |v| {
            if v >= 1.7 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        });
        // No shrinking happened, so we only know the value fails.
        match result {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert!(value >= 1.7)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    fn engine_config() -> crate::test_runner::Config {
        crate::test_runner::Config {
            shrink_engine: crate::test_runner::ShrinkEngine::Tape,
            failure_persistence: None,
            ..crate::test_runner::Config::default()
        }
    }

    fn engine_runner() -> crate::test_runner::TestRunner {
        crate::test_runner::TestRunner::new_with_rng(
            engine_config(),
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        )
    }

    #[test]
    fn tape_serialization_roundtrips() {
        let tape = tape_of(vec![
            Choice::Integer {
                value: i64::encode(-42),
                min: i64::encode(i64::MIN),
                max: i64::encode(i64::MAX),
                shrink_to: i64::encode_zero(),
            },
            Choice::Float {
                value: 2.5,
                min: f64::NEG_INFINITY,
                max: f64::INFINITY,
                allow_nan: true,
            },
            Choice::Bool { value: true },
            Choice::RawU32 { value: 0xDEAD },
            Choice::RawU64 { value: 0xBEEF_CAFE },
            Choice::RawBytes {
                value: vec![1, 2, 3, 4, 5],
            },
        ]);
        let bytes = serialize_tape(&tape);
        assert_eq!(Some(tape), deserialize_tape(&bytes));
        // Strictness: truncated input is rejected.
        assert_eq!(None, deserialize_tape(&bytes[..bytes.len() - 1]));
        assert_eq!(None, deserialize_tape(&[99]));
    }

    #[test]
    fn persisted_tape_form_roundtrips_as_string() {
        use core::str::FromStr;
        let seed = crate::test_runner::PersistedSeed(
            crate::test_runner::failure_persistence::PersistedFailure::Tape {
                bytes: vec![0xab, 0xcd, 0x01],
                seed: None,
            },
        );
        let string = format!("{}", seed);
        assert!(string.starts_with("ct1 "), "got: {}", string);
        assert_eq!(
            Ok(seed),
            crate::test_runner::PersistedSeed::from_str(&string)
        );
    }

    #[test]
    fn persisted_tape_replays_exact_shrunken_value() {
        // Run 1: fail on v >= 1.7, shrink to 2.0, persist.
        let mut config = engine_config();
        config.failure_persistence = Some(Box::new(
            crate::test_runner::MapFailurePersistence::default(),
        ));
        config.source_file = Some("tape_persistence_test");
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        );
        let strategy = 0.0f64..10.0;
        match runner.run(&strategy, |v| {
            if v >= 1.7 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        }) {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(2.0, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }

        // The persisted entry is a tape, not a seed.
        let map = runner
            .config()
            .failure_persistence
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<crate::test_runner::MapFailurePersistence>()
            .unwrap()
            .map
            .clone();
        let entries = &map[&"tape_persistence_test"];
        assert_eq!(1, entries.len());
        assert!(
            format!("{}", entries.iter().next().unwrap()).starts_with("ct1 ")
        );

        // Run 2: fresh runner and RNG; the test only fails at *exactly*
        // 2.0, which random generation will essentially never produce.
        // Only replaying the persisted tape can find it.
        let mut config = engine_config();
        config.cases = 10;
        config.failure_persistence =
            Some(Box::new(crate::test_runner::MapFailurePersistence { map }));
        config.source_file = Some("tape_persistence_test");
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::from_seed(
                crate::test_runner::RngAlgorithm::ChaCha,
                &[7; 32],
            ),
        );
        match runner.run(&(0.0f64..10.0), |v| {
            if v == 2.0 {
                Err(crate::test_runner::TestCaseError::fail("replayed"))
            } else {
                Ok(())
            }
        }) {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(2.0, value)
            }
            other => {
                panic!("persisted tape did not replay the failure: {:?}", other)
            }
        }
        let _ = strategy;
    }

    #[test]
    fn replay_engine_shrinks_weighted_bool_to_false() {
        // bool::weighted draws through a typed Bool choice; the raw
        // fallback would shrink toward `true` (rand's Bernoulli maps a
        // zeroed u64 to true for any p > 0).
        let mut runner = engine_runner();
        match runner.run(&crate::bool::weighted(0.5), |_| {
            Err(crate::test_runner::TestCaseError::fail("always"))
        }) {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(false, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn replay_engine_shrinking_does_not_exhaust_local_rejects() {
        // Shrink attempts re-vet proposals through filters; the local
        // rejects they incur must not drain the run-wide budget, or
        // shrinking silently stalls once it crosses max_local_rejects.
        let mut config = engine_config();
        config.max_local_rejects = 32;
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        );
        use crate::strategy::Strategy;
        let strategy = (0i32..1000).prop_filter("even", |v| 0 == v % 2);
        match runner.run(&strategy, |v| {
            if v >= 100 {
                Err(crate::test_runner::TestCaseError::fail("too big"))
            } else {
                Ok(())
            }
        }) {
            Err(crate::test_runner::TestError::Fail(_, value)) => {
                assert_eq!(100, value)
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn corrupt_persisted_tape_aborts_loudly() {
        // A regression entry that cannot replay guards nothing; it must
        // fail the run like a dead persisted seed does, not continue as
        // a pass.
        use crate::std_facade::BTreeMap;
        use crate::std_facade::BTreeSet;
        let mut entries = BTreeSet::new();
        entries.insert(crate::test_runner::PersistedSeed(
            crate::test_runner::failure_persistence::PersistedFailure::Tape {
                bytes: vec![99],
                seed: None,
            },
        ));
        let mut map = BTreeMap::new();
        map.insert("corrupt_tape_test", entries);
        let mut config = engine_config();
        config.failure_persistence =
            Some(Box::new(crate::test_runner::MapFailurePersistence { map }));
        config.source_file = Some("corrupt_tape_test");
        let mut runner = crate::test_runner::TestRunner::new_with_rng(
            config,
            crate::test_runner::TestRng::deterministic_rng(
                crate::test_runner::RngAlgorithm::default(),
            ),
        );
        match runner.run(&(0i32..10), |_| Ok(())) {
            Err(crate::test_runner::TestError::Abort(_)) => (),
            other => panic!("expected loud abort, got: {:?}", other),
        }
    }

    #[test]
    fn replay_pops_matching_choices() {
        let mut state = TapeState::default();
        state.start_replay(tape_of(vec![
            Choice::RawU32 { value: 7 },
            Choice::RawU64 { value: 9 },
        ]));
        // Matching kind pops.
        let popped = state.pop_replay(|c| matches!(c, Choice::RawU32 { .. }));
        assert_eq!(Some(Choice::RawU32 { value: 7 }), popped);
        // Kind mismatch does not consume...
        assert_eq!(
            None,
            state.pop_replay(|c| matches!(c, Choice::RawU32 { .. }))
        );
        // ...so the u64 is still there.
        let popped = state.pop_replay(|c| matches!(c, Choice::RawU64 { .. }));
        assert_eq!(Some(Choice::RawU64 { value: 9 }), popped);
        // Overrun.
        assert_eq!(
            None,
            state.pop_replay(|c| matches!(c, Choice::RawU32 { .. }))
        );
        let (output, overrun) = state.finish_replay();
        assert!(overrun);
        assert!(output.choices.is_empty());
    }
}
