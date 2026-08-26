//! Test-only gate on the heap's bit keys.
//!
//! The working column's heap orders diameters by their IEEE bit pattern
//! read as `u64`. It used to order them with `f64::total_cmp`. The two
//! agree on every value the engine can produce: not NaN, not negative, and
//! with no negative zero. This module holds the arbitrary-pattern gate on
//! that claim, and a trace gate that drains the same column through both
//! comparators.
//!
//! The gate lives in the crate because the heap, the entry packing, and the
//! cancellation rule are internal.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::field::{Coeffs, Entry, Fp, HeapEntry, Z2};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// What the pipeline does to a distance before it reaches a diameter:
/// negative zero becomes positive zero. Every other value passes through.
fn normalize(d: f64) -> f64 {
    if d == 0.0 {
        0.0
    } else {
        d
    }
}

/// An arbitrary non-negative, non-NaN `f64`, drawn as a bit pattern. The
/// draw covers subnormals, normals of every exponent, both zeros, and
/// infinity.
fn arbitrary_nonnegative(rng: &mut Rng) -> f64 {
    loop {
        let d = f64::from_bits(rng.next_u64() & !(1u64 << 63));
        if !d.is_nan() {
            return normalize(d);
        }
    }
}

/// The values the heap must order the same way under both comparators.
fn key_values() -> Vec<f64> {
    let mut values = vec![
        normalize(-0.0),
        0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE / 2.0,
        f64::MIN_POSITIVE,
        1e-300,
        1e-8,
        0.5,
        1.0,
        1.0 + f64::EPSILON,
        2.0,
        1e8,
        1e300,
        f64::MAX,
        f64::INFINITY,
    ];
    let mut rng = Rng::new(0x6269_745f_6b65_7901);
    values.extend((0..497).map(|_| arbitrary_nonnegative(&mut rng)));
    values
}

#[test]
fn bit_order_matches_total_cmp_on_arbitrary_patterns() {
    assert_eq!(normalize(-0.0f64).to_bits(), 0.0f64.to_bits());
    assert_eq!(normalize(0.0f64).to_bits(), 0.0f64.to_bits());

    let values = key_values();
    assert!(
        values.iter().any(|d| d.is_subnormal()),
        "no subnormal drawn"
    );
    assert!(values.iter().any(|d| d.is_infinite()));
    for &a in &values {
        assert!(!a.is_nan() && a >= 0.0 && a.is_sign_positive(), "{a}");
        for &b in &values {
            assert_eq!(
                a.to_bits().cmp(&b.to_bits()),
                a.total_cmp(&b),
                "bit order and total_cmp disagree on {a} and {b}"
            );
        }
    }
}

/// The comparator the heap used before the bit keys: the same order,
/// written with `total_cmp`. Kept here so a trace can run through it.
#[derive(Debug, Clone, Copy)]
struct OldHeapEntry(Entry);

impl PartialEq for OldHeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for OldHeapEntry {}
impl PartialOrd for OldHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OldHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .0
            .diameter
            .total_cmp(&self.0.diameter)
            .then(self.0.payload.cmp(&other.0.payload))
    }
}

/// The Z/2 cancellation rule over the old comparator: equal adjacent
/// payloads annihilate in pairs.
fn old_pop_pivot_z2(heap: &mut BinaryHeap<OldHeapEntry>) -> Option<Entry> {
    while let Some(top) = heap.pop() {
        match heap.peek() {
            Some(next) if next.0.payload == top.0.payload => {
                heap.pop();
            }
            _ => return Some(top.0),
        }
    }
    None
}

/// The Z/p cancellation rule over the old comparator: coefficients of
/// equal indices add, and a zero sum vanishes.
fn old_pop_pivot_fp(ops: &Fp, p: u64, heap: &mut BinaryHeap<OldHeapEntry>) -> Option<Entry> {
    let mut acc: Option<(f64, u64, u64)> = None;
    while let Some(&OldHeapEntry(top)) = heap.peek() {
        let index = ops.index(top);
        let coeff = ops.coeff(top);
        match acc.as_mut() {
            None => acc = Some((top.diameter, index, coeff)),
            Some((_, _, c)) if *c == 0 => acc = Some((top.diameter, index, coeff)),
            Some((_, i, _)) if index != *i => break,
            Some((_, _, c)) => *c = (*c + coeff) % p,
        }
        heap.pop();
    }
    acc.filter(|&(_, _, c)| c != 0)
        .map(|(diameter, index, coeff)| ops.pack(diameter, index, coeff))
}

/// A random working column as `(diameter, index, coefficient)`: diameters
/// from a palette that repeats, so ties are common, and indices from a
/// range small enough that entries cancel.
fn random_column(
    rng: &mut Rng,
    palette: &[f64],
    indices: usize,
    p: u64,
    len: usize,
) -> Vec<(f64, u64, u64)> {
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        let diameter = palette[rng.below(palette.len())];
        let index = rng.below(indices) as u64;
        let coeff = if p == 2 {
            1
        } else {
            1 + rng.below(p as usize - 1) as u64
        };
        out.push((diameter, index, coeff));
    }
    out
}

/// A popped entry as the test compares it: bits, not numbers.
fn trace_key(e: Entry) -> (u64, u64) {
    (e.diameter.to_bits(), e.payload)
}

// Drain the same column through both comparators and compare the traces.
// A pop is a cancellation step as well as an ordering step, so an equal
// trace pins the order and the arithmetic together.
#[test]
fn heap_traces_agree_under_both_comparators() {
    let palette: Vec<f64> = {
        let mut rng = Rng::new(0x6269_745f_6b65_7902);
        let mut v = vec![
            0.0,
            normalize(-0.0),
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            1.0,
            1.0,
            2.0,
            2.0,
            f64::MAX,
            f64::INFINITY,
        ];
        v.extend((0..10).map(|_| arbitrary_nonnegative(&mut rng)));
        v
    };
    let mut rng = Rng::new(0x6269_745f_6b65_7903);
    let mut popped = 0usize;
    for p in [2u64, 3, 5, 7, 13, 32749] {
        let ops = Fp::new(p);
        for case in 0..200 {
            let len = 1 + rng.below(64);
            let indices = 1 + rng.below(8);
            let column = random_column(&mut rng, &palette, indices, p, len);

            let mut new_heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
            let mut old_heap: BinaryHeap<OldHeapEntry> = BinaryHeap::new();
            for &(diameter, index, coeff) in &column {
                let entry = if p == 2 {
                    Z2.pack(diameter, index, coeff)
                } else {
                    ops.pack(diameter, index, coeff)
                };
                new_heap.push(HeapEntry::new(entry));
                old_heap.push(OldHeapEntry(entry));
            }

            let mut new_trace = Vec::new();
            let mut old_trace = Vec::new();
            if p == 2 {
                while let Some(e) = Z2.pop_pivot(&mut new_heap) {
                    new_trace.push(trace_key(e));
                }
                while let Some(e) = old_pop_pivot_z2(&mut old_heap) {
                    old_trace.push(trace_key(e));
                }
            } else {
                while let Some(e) = ops.pop_pivot(&mut new_heap) {
                    new_trace.push(trace_key(e));
                }
                while let Some(e) = old_pop_pivot_fp(&ops, p, &mut old_heap) {
                    old_trace.push(trace_key(e));
                }
            }
            assert_eq!(new_trace, old_trace, "p {p}, case {case}");
            assert!(new_heap.is_empty() && old_heap.is_empty());
            popped += new_trace.len();
        }
    }
    assert!(popped > 10_000, "the traces were nearly empty: {popped}");
}
