//! The coefficient field and the reduction's arithmetic over it.
//!
//! A working entry is a filtration diameter plus a single `u64` payload that
//! packs the combinadic index with, for Z/p, the coefficient in the low bits
//! (`index << coeff_bits | coeff`). Z/2 needs no coefficient, so it sets
//! `coeff_bits = 0` and the payload is the bare index. That gives Z/2 the
//! full 64-bit range and the coefficient-free algorithm. Z/p reserves the
//! fewest bits that hold `p - 1`, exactly as ripser packs its entries.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::simplex::Simplex;

/// A simplex carried through the reduction with its field coefficient, packed
/// into 16 bytes: an `f64` diameter and a `u64` payload.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) diameter: f64,
    pub(crate) payload: u64,
}

/// Heap order matches ripser's working-column priority queue: pop the cofacet
/// minimal in the (d+1)-simplex order, smallest diameter then largest index.
/// The payload is index-major, so comparing payloads compares indices. A
/// coefficient in the low bits is only a tiebreak among equal indices, which
/// lazy cancellation then combines.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeapEntry(pub(crate) Entry);

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .0
            .diameter
            .total_cmp(&self.0.diameter)
            .then(self.0.payload.cmp(&other.0.payload))
    }
}

/// Field arithmetic plus entry (un)packing and the lazy-heap cancellation
/// rule. The index/coefficient layout is shared through `coeff_bits`. Only
/// the arithmetic and cancellation differ between Z/2 and Z/p.
pub(crate) trait Coeffs {
    /// Bits of the payload reserved for the coefficient; 0 for Z/2.
    fn coeff_bits(&self) -> u32;

    /// Largest combinadic index the payload can hold without colliding with
    /// the coefficient bits.
    #[inline]
    fn max_index(&self) -> u64 {
        match self.coeff_bits() {
            0 => u64::MAX,
            b => (1u64 << (64 - b)) - 1,
        }
    }

    #[inline]
    fn pack(&self, diameter: f64, index: u64, coeff: u64) -> Entry {
        let mask = (1u64 << self.coeff_bits()) - 1;
        Entry {
            diameter,
            payload: (index << self.coeff_bits()) | (coeff & mask),
        }
    }

    #[inline]
    fn index(&self, e: Entry) -> u64 {
        e.payload >> self.coeff_bits()
    }

    #[inline]
    fn coeff(&self, e: Entry) -> u64 {
        let mask = (1u64 << self.coeff_bits()) - 1;
        if mask == 0 {
            1
        } else {
            e.payload & mask
        }
    }

    #[inline]
    fn simplex(&self, e: Entry) -> Simplex {
        Simplex {
            diameter: e.diameter,
            index: self.index(e),
        }
    }

    /// (-1)^k as a field element: ripser's `k & 1 ? p - 1 : 1`.
    fn sign(&self, k: usize) -> u64;
    fn mul(&self, a: u64, b: u64) -> u64;
    fn neg(&self, a: u64) -> u64;
    /// Ripser's reduction factor: -(pivot / other) in the field.
    fn factor(&self, pivot: u64, other: u64) -> u64;
    /// Pop the pivot with lazy cancellation. Entries with equal index
    /// combine, and a zero combined coefficient vanishes.
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry>) -> Option<Entry>;
}

/// Z/2: every coefficient is 1, nothing is stored, and equal adjacent indices
/// annihilate in pairs.
pub(crate) struct Z2;

impl Coeffs for Z2 {
    fn coeff_bits(&self) -> u32 {
        0
    }
    fn sign(&self, _k: usize) -> u64 {
        1
    }
    fn mul(&self, _a: u64, _b: u64) -> u64 {
        1
    }
    fn neg(&self, _a: u64) -> u64 {
        1
    }
    fn factor(&self, _pivot: u64, _other: u64) -> u64 {
        1
    }
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry>) -> Option<Entry> {
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
}

/// Z/p for an odd prime p: coefficients in 1..p, multiplicative inverses
/// precomputed exactly as in ripser.
pub(crate) struct Fp {
    p: u64,
    coeff_bits: u32,
    inv: Vec<u64>,
}

impl Fp {
    pub(crate) fn new(p: u64) -> Self {
        let mut inv = vec![0u64; p as usize];
        if p > 1 {
            inv[1] = 1;
        }
        for a in 2..p {
            // inv[a] = p - (inv[p % a] * (p / a)) % p, valid for prime p.
            inv[a as usize] = p - (inv[(p % a) as usize] * (p / a)) % p;
        }
        // Fewest bits that hold a coefficient in 0..p (i.e. up to p - 1).
        let coeff_bits = (u64::BITS - (p - 1).leading_zeros()).max(1);
        Self { p, coeff_bits, inv }
    }
}

impl Coeffs for Fp {
    fn coeff_bits(&self) -> u32 {
        self.coeff_bits
    }
    fn sign(&self, k: usize) -> u64 {
        if k & 1 == 1 {
            self.p - 1
        } else {
            1
        }
    }
    fn mul(&self, a: u64, b: u64) -> u64 {
        a * b % self.p
    }
    fn neg(&self, a: u64) -> u64 {
        (self.p - a) % self.p
    }
    fn factor(&self, pivot: u64, other: u64) -> u64 {
        (self.p - pivot * self.inv[other as usize] % self.p) % self.p
    }
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry>) -> Option<Entry> {
        // Accumulate coefficients across equal indices. Return the first
        // index whose sum is non-zero. The heap stays positioned at it.
        let mut acc: Option<(f64, u64, u64)> = None; // (diameter, index, coeff)
        while let Some(&HeapEntry(top)) = heap.peek() {
            let index = self.index(top);
            let coeff = self.coeff(top);
            match acc.as_mut() {
                None => acc = Some((top.diameter, index, coeff)),
                Some((_, _, c)) if *c == 0 => acc = Some((top.diameter, index, coeff)),
                Some((_, i, _)) if index != *i => break,
                Some((_, _, c)) => *c = (*c + coeff) % self.p,
            }
            heap.pop();
        }
        acc.filter(|&(_, _, c)| c != 0)
            .map(|(diameter, index, coeff)| self.pack(diameter, index, coeff))
    }
}

pub(crate) fn is_prime(p: u64) -> bool {
    if p < 2 {
        return false;
    }
    if p % 2 == 0 {
        return p == 2;
    }
    let mut d = 3;
    while d * d <= p {
        if p % d == 0 {
            return false;
        }
        d += 2;
    }
    true
}

/// The largest accepted modulus (exclusive). The limit keeps the inverse
/// table small. The arithmetic itself is exact for far larger primes.
pub(crate) const MODULUS_LIMIT: u64 = 1 << 15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_check() {
        let primes = [2u64, 3, 5, 7, 11, 13, 32749];
        let composites = [0u64, 1, 4, 9, 15, 32767];
        assert!(primes.into_iter().all(is_prime));
        assert!(!composites.into_iter().any(is_prime));
    }

    #[test]
    fn pack_round_trips_index_and_coeff() {
        for p in [3u64, 5, 251, 32749] {
            let f = Fp::new(p);
            for &index in &[0u64, 1, 1000, (1 << 40) + 7] {
                for coeff in 1..p.min(20) {
                    let e = f.pack(1.5, index, coeff);
                    assert_eq!(f.index(e), index);
                    assert_eq!(f.coeff(e), coeff);
                }
            }
            // The coefficient never disturbs the index ordering.
            assert!(f.pack(1.0, 5, p - 1).payload < f.pack(1.0, 6, 1).payload);
        }
    }

    #[test]
    fn z2_payload_is_the_bare_index() {
        let z = Z2;
        let e = z.pack(2.0, u64::MAX >> 1, 1);
        assert_eq!(z.index(e), u64::MAX >> 1);
        assert_eq!(z.coeff(e), 1);
    }
}
