//! Combinatorial number system: a d-simplex with vertices v0 < ... < vd is
//! the integer sum C(v_i, i+1). Enumerator state machines follow ripser's
//! conventions exactly (matching ripser).

use crate::{Error, Result};

pub struct BinomialTable {
    table: Vec<u64>,
    n: usize,
    k_max: usize,
}

impl BinomialTable {
    /// Table of C(i, j) for i <= n, j <= k_max. Errors if any entry
    /// overflows u64, which bounds the representable simplex index space.
    pub fn new(n: usize, k_max: usize) -> Result<Self> {
        let mut table = vec![0u64; (n + 1) * (k_max + 1)];
        for i in 0..=n {
            table[i * (k_max + 1)] = 1;
            for j in 1..=k_max.min(i) {
                let above = table[(i - 1) * (k_max + 1) + j - 1];
                let left = table[(i - 1) * (k_max + 1) + j];
                table[i * (k_max + 1) + j] =
                    above.checked_add(left).ok_or(Error::IndexOverflow {
                        n,
                        dim: k_max.saturating_sub(2),
                    })?;
            }
        }
        Ok(Self { table, n, k_max })
    }

    #[inline]
    pub fn get(&self, n: usize, k: usize) -> u64 {
        debug_assert!(k <= self.k_max && n <= self.n);
        if n < k {
            0
        } else {
            self.table[n * (self.k_max + 1) + k]
        }
    }

    #[cfg(test)]
    pub fn rank(&self, vertices_ascending: &[usize]) -> u64 {
        vertices_ascending
            .iter()
            .enumerate()
            .map(|(i, &v)| self.get(v, i + 1))
            .sum()
    }

    /// Largest j <= upper with C(j, k) <= idx.
    #[inline]
    pub fn max_vertex(&self, idx: u64, k: usize, upper: usize) -> usize {
        let mut lo = k - 1;
        let mut hi = upper;
        while hi > lo {
            let mid = lo + (hi - lo).div_ceil(2);
            if self.get(mid, k) <= idx {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    /// Vertices of the simplex with the given index, ascending.
    pub fn unrank(&self, mut idx: u64, dim: usize, n: usize, out: &mut Vec<usize>) {
        out.clear();
        let mut upper = n - 1;
        for k in (1..=dim + 1).rev() {
            let v = self.max_vertex(idx, k, upper);
            idx -= self.get(v, k);
            upper = v.saturating_sub(1);
            out.push(v);
        }
        out.reverse();
    }
}

/// Enumerates cofacets of a simplex in decreasing order of the added vertex,
/// which is decreasing order of cofacet index. `next_upper` restricts to
/// cofacets whose added vertex exceeds every simplex vertex; over all
/// d-simplices this generates each (d+1)-simplex exactly once.
pub struct CofacetIter<'a> {
    bt: &'a BinomialTable,
    idx_below: u64,
    idx_above: u64,
    j: usize,
    k: usize,
    exhausted: bool,
}

impl<'a> CofacetIter<'a> {
    pub fn new(bt: &'a BinomialTable, index: u64, dim: usize, n: usize) -> Self {
        Self {
            bt,
            idx_below: index,
            idx_above: 0,
            j: n.saturating_sub(1),
            k: dim + 1,
            exhausted: n == 0,
        }
    }

    #[inline]
    fn advance(&mut self) {
        while self.j >= self.k && self.bt.get(self.j, self.k) <= self.idx_below {
            self.idx_below -= self.bt.get(self.j, self.k);
            self.idx_above += self.bt.get(self.j, self.k + 1);
            self.j -= 1;
            self.k -= 1;
        }
    }

    /// All cofacets. Yields (cofacet_index, added_vertex, k) where k is the
    /// enumerator position at yield time; the coboundary coefficient of the
    /// cofacet is (-1)^k (ripser's `k & 1 ? modulus - 1 : 1`).
    pub fn next_all(&mut self) -> Option<(u64, usize, usize)> {
        if self.exhausted || self.j < self.k {
            return None;
        }
        self.advance();
        if self.j < self.k {
            return None;
        }
        let index = self.idx_above + self.bt.get(self.j, self.k + 1) + self.idx_below;
        let vertex = self.j;
        if self.j == 0 {
            self.exhausted = true;
        } else {
            self.j -= 1;
        }
        Some((index, vertex, self.k))
    }

    /// Only cofacets whose added vertex exceeds all simplex vertices.
    pub fn next_upper(&mut self) -> Option<(u64, usize)> {
        if self.exhausted || self.j < self.k || self.bt.get(self.j, self.k) <= self.idx_below {
            return None;
        }
        let index = self.idx_above + self.bt.get(self.j, self.k + 1) + self.idx_below;
        let vertex = self.j;
        if self.j == 0 {
            self.exhausted = true;
        } else {
            self.j -= 1;
        }
        Some((index, vertex))
    }
}

/// Enumerates facets of a simplex, removing vertices from the highest
/// position downward.
pub struct FacetIter<'a> {
    bt: &'a BinomialTable,
    idx_below: u64,
    idx_above: u64,
    j: usize,
    k: isize,
}

impl<'a> FacetIter<'a> {
    pub fn new(bt: &'a BinomialTable, index: u64, dim: usize, n: usize) -> Self {
        Self {
            bt,
            idx_below: index,
            idx_above: 0,
            j: n - 1,
            k: dim as isize,
        }
    }
}

/// Items are (facet_index, removed_vertex, k) where k is the removed
/// vertex's position in the simplex; the boundary coefficient of the facet
/// is (-1)^k.
impl Iterator for FacetIter<'_> {
    type Item = (u64, usize, usize);

    fn next(&mut self) -> Option<(u64, usize, usize)> {
        if self.k < 0 {
            return None;
        }
        let k = self.k as usize;
        self.j = self.bt.max_vertex(self.idx_below, k + 1, self.j);
        let removed = self.j;
        let face_index = self.idx_below - self.bt.get(self.j, k + 1) + self.idx_above;
        self.idx_below -= self.bt.get(self.j, k + 1);
        self.idx_above += self.bt.get(self.j, k);
        self.k -= 1;
        Some((face_index, removed, k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent of BinomialTable: its own Pascal-triangle binomial, so
    // enumerator tests never validate rank against itself.
    fn naive_binomial(n: usize, k: usize) -> u64 {
        let mut row = vec![0u64; k + 1];
        row[0] = 1;
        for _ in 0..n {
            for j in (1..=k).rev() {
                row[j] += row[j - 1];
            }
        }
        row[k]
    }

    fn naive_rank(vertices: &[usize]) -> u64 {
        vertices
            .iter()
            .enumerate()
            .map(|(i, &v)| naive_binomial(v, i + 1))
            .sum()
    }

    fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut current = Vec::new();
        fn rec(
            start: usize,
            n: usize,
            k: usize,
            current: &mut Vec<usize>,
            out: &mut Vec<Vec<usize>>,
        ) {
            if current.len() == k {
                out.push(current.clone());
                return;
            }
            for v in start..n {
                current.push(v);
                rec(v + 1, n, k, current, out);
                current.pop();
            }
        }
        rec(0, n, k, &mut current, &mut out);
        out
    }

    #[test]
    fn rank_unrank_round_trip() {
        let n = 9;
        let bt = BinomialTable::new(n, 5).unwrap();
        for dim in 0..=3 {
            let mut seen = std::collections::HashSet::new();
            let mut buf = Vec::new();
            for verts in combinations(n, dim + 1) {
                let idx = bt.rank(&verts);
                assert_eq!(idx, naive_rank(&verts), "rank disagrees with naive sum");
                assert!(seen.insert(idx), "rank collision at {verts:?}");
                bt.unrank(idx, dim, n, &mut buf);
                assert_eq!(buf, verts);
            }
            assert_eq!(seen.len(), combinations(n, dim + 1).len());
        }
    }

    #[test]
    fn cofacets_match_direct_ranks_and_descend() {
        let n = 8;
        let bt = BinomialTable::new(n, 5).unwrap();
        for dim in 0..=2 {
            for verts in combinations(n, dim + 1) {
                let idx = bt.rank(&verts);
                let mut iter = CofacetIter::new(&bt, idx, dim, n);
                let mut got = Vec::new();
                while let Some((ci, v, k)) = iter.next_all() {
                    assert!(!verts.contains(&v));
                    let mut cv = verts.clone();
                    cv.push(v);
                    cv.sort_unstable();
                    assert_eq!(ci, naive_rank(&cv), "cofacet index mismatch");
                    // k is the added vertex's position in the cofacet.
                    assert_eq!(k, cv.iter().position(|&x| x == v).unwrap());
                    got.push((ci, v));
                }
                assert_eq!(got.len(), n - (dim + 1));
                for w in got.windows(2) {
                    assert!(w[0].0 > w[1].0, "cofacet indices must strictly descend");
                }
            }
        }
    }

    #[test]
    fn upper_cofacets_are_exactly_above_max_vertex() {
        let n = 8;
        let bt = BinomialTable::new(n, 5).unwrap();
        for dim in 0..=2 {
            for verts in combinations(n, dim + 1) {
                let idx = bt.rank(&verts);
                let max_v = *verts.last().unwrap();
                let mut iter = CofacetIter::new(&bt, idx, dim, n);
                let mut got = Vec::new();
                while let Some((_, v)) = iter.next_upper() {
                    got.push(v);
                }
                let expected: Vec<usize> = (max_v + 1..n).rev().collect();
                assert_eq!(got, expected);
            }
        }
    }

    #[test]
    fn facets_match_direct_ranks() {
        let n = 8;
        let bt = BinomialTable::new(n, 5).unwrap();
        for dim in 1..=3 {
            for verts in combinations(n, dim + 1) {
                let idx = bt.rank(&verts);
                let iter = FacetIter::new(&bt, idx, dim, n);
                let mut removed_order = Vec::new();
                for (fi, v, k) in iter {
                    let fv: Vec<usize> = verts.iter().copied().filter(|&x| x != v).collect();
                    assert_eq!(fi, naive_rank(&fv), "facet index mismatch");
                    // k is the removed vertex's position in the simplex.
                    assert_eq!(k, verts.iter().position(|&x| x == v).unwrap());
                    removed_order.push(v);
                }
                let mut expected = verts.clone();
                expected.reverse();
                assert_eq!(
                    removed_order, expected,
                    "facets remove vertices high to low"
                );
            }
        }
    }

    #[test]
    fn facets_of_facets_cancel_with_signs() {
        // Signed del o del = 0 over the integers: for every simplex, the
        // (dim-2)-faces each appear twice with opposite signs (-1)^{k1+k2}.
        let n = 9;
        let bt = BinomialTable::new(n, 6).unwrap();
        for dim in 2..=4 {
            for verts in combinations(n, dim + 1) {
                let idx = bt.rank(&verts);
                let mut sums = std::collections::HashMap::new();
                let mut counts = std::collections::HashMap::new();
                for (f, _, k1) in FacetIter::new(&bt, idx, dim, n) {
                    for (ff, _, k2) in FacetIter::new(&bt, f, dim - 1, n) {
                        let sign: i64 = if (k1 + k2) % 2 == 0 { 1 } else { -1 };
                        *sums.entry(ff).or_insert(0i64) += sign;
                        *counts.entry(ff).or_insert(0u32) += 1;
                    }
                }
                assert_eq!(counts.len(), (dim + 1) * dim / 2);
                assert!(counts.values().all(|&c| c == 2));
                assert!(
                    sums.values().all(|&s| s == 0),
                    "signed del o del != 0 at {verts:?}"
                );
            }
        }
    }

    #[test]
    fn overflow_is_an_error() {
        assert!(BinomialTable::new(100_000, 60).is_err());
        assert!(BinomialTable::new(1000, 4).is_ok());
    }
}
