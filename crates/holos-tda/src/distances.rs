use std::ops::ControlFlow;

use crate::combinadic::{BinomialTable, CofacetIter};
use crate::simplex::Simplex;
use crate::{Error, Result};

/// Symmetric dissimilarity matrix in condensed lower-triangle form.
/// No metric assumptions: entries need not satisfy the triangle inequality.
/// Entries must be non-negative and not NaN; +inf is legal and equivalent
/// to an absent edge.
#[derive(Debug, Clone)]
pub struct DistanceMatrix {
    n: usize,
    data: Vec<f64>,
}

impl DistanceMatrix {
    /// Euclidean distances of a point cloud. Coordinates must be finite.
    pub fn from_points(points: &[Vec<f64>]) -> Result<Self> {
        let n = points.len();
        if n > 0 {
            let d = points[0].len();
            if let Some(p) = points.iter().find(|p| p.len() != d) {
                return Err(Error::InvalidInput(format!(
                    "inconsistent point dimensions: {} vs {}",
                    d,
                    p.len()
                )));
            }
            if points.iter().flatten().any(|x| !x.is_finite()) {
                return Err(Error::InvalidInput("non-finite coordinate".into()));
            }
        }
        let mut data = Vec::with_capacity(n.saturating_sub(1) * n / 2);
        for i in 1..n {
            for j in 0..i {
                data.push(euclidean(&points[i], &points[j]));
            }
        }
        Ok(Self { n, data })
    }

    /// Condensed lower triangle, row by row: d(1,0), d(2,0), d(2,1), d(3,0), ...
    /// An empty vector means one point (n = 1). Only
    /// [`DistanceMatrix::from_points`] can build an empty *space* (n = 0).
    pub fn from_condensed(mut data: Vec<f64>) -> Result<Self> {
        let m = data.len();
        let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
        if n * (n - 1) / 2 != m {
            return Err(Error::InvalidInput(format!(
                "condensed length {m} is not n(n-1)/2 for any n"
            )));
        }
        for (i, d) in data.iter_mut().enumerate() {
            if d.is_nan() {
                return Err(Error::InvalidDistance(format!(
                    "NaN at condensed index {i}"
                )));
            }
            if *d < 0.0 {
                return Err(Error::InvalidDistance(format!(
                    "negative entry {d} at condensed index {i}"
                )));
            }
            if *d == 0.0 {
                *d = 0.0;
            }
        }
        Ok(Self { n, data })
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when there are no points.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Distance between points `i` and `j` (0 on the diagonal).
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        debug_assert!(i < self.n && j < self.n);
        match i.cmp(&j) {
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => self.data[i * (i - 1) / 2 + j],
            std::cmp::Ordering::Less => self.data[j * (j - 1) / 2 + i],
        }
    }

    /// min over i of max over j of d(i,j): the radius past which the complex
    /// is a cone and acquires no further homology. This is the default
    /// threshold. It does not change the full persistence result.
    pub fn enclosing_radius(&self) -> f64 {
        if self.n < 2 {
            return 0.0;
        }
        // One contiguous pass over the condensed lower triangle. Each
        // distance folds into both endpoints' running maxima.
        let mut row_max = vec![0.0f64; self.n];
        let mut k = 0;
        for i in 1..self.n {
            for j in 0..i {
                let d = self.data[k];
                k += 1;
                row_max[i] = row_max[i].max(d);
                row_max[j] = row_max[j].max(d);
            }
        }
        row_max.into_iter().fold(f64::INFINITY, f64::min)
    }
}

/// Sparse dissimilarities: only listed pairs have finite distance. Every
/// unlisted pair is an absent edge (+inf) that never enters the filtration.
/// No metric assumptions, same entry rules as [`DistanceMatrix`].
#[derive(Debug, Clone)]
pub struct SparseDistanceMatrix {
    n: usize,
    /// Per-vertex neighbor lists, sorted by vertex index.
    neighbors: Vec<Vec<(usize, f64)>>,
}

impl SparseDistanceMatrix {
    /// Build from `(i, j, d)` triplets over `n` points. A repeated unordered
    /// pair must carry an identical distance. Entries must be finite and
    /// non-negative. Omit a pair to make it absent.
    pub fn from_triplets(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Self> {
        let mut neighbors: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        for (idx, &(i, j, d)) in triplets.iter().enumerate() {
            if i >= n || j >= n {
                return Err(Error::InvalidInput(format!(
                    "triplet {idx}: vertex out of range ({i}, {j}) for n = {n}"
                )));
            }
            if i == j {
                return Err(Error::InvalidInput(format!(
                    "triplet {idx}: self-distance for vertex {i}"
                )));
            }
            if !d.is_finite() || d < 0.0 {
                return Err(Error::InvalidDistance(format!(
                    "triplet {idx}: distance must be finite and non-negative, got {d}"
                )));
            }
            let d = if d == 0.0 { 0.0 } else { d };
            neighbors[i].push((j, d));
            neighbors[j].push((i, d));
        }
        for (v, list) in neighbors.iter_mut().enumerate() {
            list.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            for w in list.windows(2) {
                if w[0].0 == w[1].0 && w[0].1 != w[1].1 {
                    return Err(Error::InvalidInput(format!(
                        "conflicting distances for pair ({v}, {}): {} vs {}",
                        w[0].0, w[0].1, w[1].1
                    )));
                }
            }
            list.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        }
        Ok(Self { n, neighbors })
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when there are no points.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Number of stored (present) edges.
    pub fn num_edges(&self) -> usize {
        self.neighbors.iter().map(Vec::len).sum::<usize>() / 2
    }

    /// Distance between `i` and `j`; +inf when the pair is not listed.
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        debug_assert!(i < self.n && j < self.n);
        if i == j {
            return 0.0;
        }
        match self.neighbors[i].binary_search_by(|&(v, _)| v.cmp(&j)) {
            Ok(pos) => self.neighbors[i][pos].1,
            Err(_) => f64::INFINITY,
        }
    }
}

/// A cofacet produced during enumeration: its combinadic index, the position
/// `k` of the added vertex in the cofacet (the coboundary sign exponent), and
/// the cofacet's filtration diameter.
pub(crate) struct Cofacet {
    pub(crate) index: u64,
    pub(crate) k: usize,
    pub(crate) diameter: f64,
}

/// What the solver needs from a distance source. Dense and sparse inputs
/// share the whole engine through this trait. An absent pair reads as +inf.
pub(crate) trait Distances {
    fn len(&self) -> usize;
    fn get(&self, i: usize, j: usize) -> f64;
    /// Threshold to use when the caller gives none.
    fn default_threshold(&self) -> f64;
    /// Visit every pair that could be an edge, as (i, j, d) with j < i.
    fn for_each_edge(&self, f: &mut dyn FnMut(usize, usize, f64));

    /// Enumerate cofacets of `simplex` (vertex set `verts`, ascending) in
    /// dimension `dim`, in strictly descending index order. `f` runs on each
    /// cofacet. With `upper_only`, restrict to cofacets whose added vertex
    /// exceeds every simplex vertex. Over all d-simplices that generates each
    /// (d+1)-simplex exactly once. Diameters may exceed the threshold or be
    /// infinite: the caller filters. `f` may short-circuit with `Break`.
    ///
    /// The default walks the full combinadic cofacet set. A sparse source
    /// overrides it to visit only common neighbors.
    fn for_each_cofacet<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        mut f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        let cofacet_diameter = |added: usize| {
            verts
                .iter()
                .fold(simplex.diameter, |d, &v| d.max(self.get(added, v)))
        };
        let mut iter = CofacetIter::new(bt, simplex.index, dim, self.len());
        if upper_only {
            while let Some((index, vertex)) = iter.next_upper() {
                let cofacet = Cofacet {
                    index,
                    k: 0,
                    diameter: cofacet_diameter(vertex),
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        } else {
            while let Some((index, vertex, k)) = iter.next_all() {
                let cofacet = Cofacet {
                    index,
                    k,
                    diameter: cofacet_diameter(vertex),
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        }
        None
    }
}

impl Distances for DistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        DistanceMatrix::get(self, i, j)
    }
    fn default_threshold(&self) -> f64 {
        self.enclosing_radius()
    }
    fn for_each_edge(&self, f: &mut dyn FnMut(usize, usize, f64)) {
        for i in 1..self.n {
            for j in 0..i {
                f(i, j, DistanceMatrix::get(self, i, j));
            }
        }
    }
}

impl Distances for SparseDistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        SparseDistanceMatrix::get(self, i, j)
    }
    /// Sparse input has no enclosing radius: absent edges are absent at
    /// every scale. The default therefore includes all listed edges.
    fn default_threshold(&self) -> f64 {
        f64::INFINITY
    }
    fn for_each_edge(&self, f: &mut dyn FnMut(usize, usize, f64)) {
        for (i, list) in self.neighbors.iter().enumerate() {
            for &(j, d) in list {
                if j < i {
                    f(i, j, d);
                }
            }
        }
    }

    /// Ripser's sparse coboundary: the only in-complex cofacets add a vertex
    /// adjacent to every simplex vertex, so intersect the vertices' neighbor
    /// lists instead of scanning all `n` candidates. This reproduces the
    /// dense enumerator's index and `k` exactly. It tracks
    /// `idx_below`/`idx_above` as the added vertex descends past the simplex
    /// vertices, the same way [`CofacetIter::advance`] does.
    fn for_each_cofacet<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        mut f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        // Candidate added vertices: neighbors shared by every simplex vertex.
        // Pivot on the shortest list, then confirm membership in the rest.
        // The same pass folds the cofacet diameter. Simplex vertices are
        // mutual neighbors, so they surface here and must be excluded.
        let pivot = *verts
            .iter()
            .min_by_key(|&&v| self.neighbors[v].len())
            .expect("cofacet enumeration needs a non-empty simplex");
        let mut candidates: Vec<(usize, f64)> = Vec::new();
        'w: for &(w, _) in &self.neighbors[pivot] {
            if verts.binary_search(&w).is_ok() {
                continue;
            }
            let mut diameter = simplex.diameter;
            for &v in verts {
                let d = self.get(w, v);
                if !d.is_finite() {
                    continue 'w;
                }
                diameter = diameter.max(d);
            }
            candidates.push((w, diameter));
        }

        // Descending candidate order is descending cofacet-index order. Move
        // each simplex vertex the added vertex overtakes from the below-set to
        // the above-set exactly as `advance` does.
        let mut idx_below = simplex.index;
        let mut idx_above = 0u64;
        let mut k = dim + 1;
        for &(w, diameter) in candidates.iter().rev() {
            while k >= 1 && verts[k - 1] > w {
                idx_below -= bt.get(verts[k - 1], k);
                idx_above += bt.get(verts[k - 1], k + 1);
                k -= 1;
            }
            if upper_only && k != dim + 1 {
                break;
            }
            let index = idx_above + bt.get(w, k + 1) + idx_below;
            let cofacet = Cofacet {
                index,
                k: if upper_only { 0 } else { k },
                diameter,
            };
            if let ControlFlow::Break(t) = f(cofacet) {
                return Some(t);
            }
        }
        None
    }
}

/// Scaled two-norm: exact where the naive sum of squares would overflow or
/// underflow. Finite coordinates whose difference still overflows f64 give
/// +inf. The complex treats +inf as an absent edge.
fn euclidean(a: &[f64], b: &[f64]) -> f64 {
    let m = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f64, f64::max);
    if m == 0.0 {
        return 0.0;
    }
    if m.is_infinite() {
        return f64::INFINITY;
    }
    let s: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| {
            let r = (x - y) / m;
            r * r
        })
        .sum();
    m * s.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Collect the full (index, k, diameter) sequence for a base simplex.
    fn cofacets<D: Distances>(
        d: &D,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
    ) -> Vec<(u64, usize, f64)> {
        let mut out = Vec::new();
        d.for_each_cofacet(bt, simplex, verts, dim, upper_only, |cf| {
            out.push((cf.index, cf.k, cf.diameter));
            ControlFlow::<()>::Continue(())
        });
        out
    }

    // Rank a sorted vertex set into its combinadic index.
    fn rank(bt: &BinomialTable, verts: &[usize]) -> u64 {
        verts
            .iter()
            .enumerate()
            .map(|(i, &v)| bt.get(v, i + 1))
            .sum()
    }

    // A random sparse graph plus the dense matrix that uses +inf for every
    // absent pair. The dense default then enumerates the same cofacets. It
    // gives the missing ones an infinite diameter that the sparse side omits.
    fn random_graph(rng: &mut Rng, n: usize) -> (SparseDistanceMatrix, DistanceMatrix) {
        // Duplicates and a zero make sure the diameter fold is exercised.
        let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0];
        let mut triplets = Vec::new();
        let mut condensed = Vec::new();
        for i in 1..n {
            for j in 0..i {
                if rng.below(3) > 0 {
                    let w = palette[rng.below(palette.len())];
                    triplets.push((i, j, w));
                    condensed.push(w);
                } else {
                    condensed.push(f64::INFINITY);
                }
            }
        }
        (
            SparseDistanceMatrix::from_triplets(n, &triplets).unwrap(),
            DistanceMatrix::from_condensed(condensed).unwrap(),
        )
    }

    // The sparse override must yield exactly what the dense default yields
    // once its infinite-diameter (absent-neighbor) cofacets are dropped: the
    // same indices, k, diameters, and descending order.
    #[test]
    fn sparse_cofacets_match_dense_default() {
        let mut rng = Rng::new(0xc0fa_ce75_0000_0001);
        let mut trials = 0usize;
        for _ in 0..4000 {
            let n = 4 + rng.below(9);
            let (sparse, dense) = random_graph(&mut rng, n);
            let bt = BinomialTable::new(n, 6).unwrap();
            let dim = 1 + rng.below(3); // base simplex dimension 1..=3
            if dim + 1 > n {
                continue;
            }
            // A genuine base simplex: distinct vertices, all pairs present.
            let mut verts: Vec<usize> = Vec::new();
            while verts.len() < dim + 1 {
                let v = rng.below(n);
                if !verts.contains(&v) {
                    verts.push(v);
                }
            }
            verts.sort_unstable();
            let mut diameter = 0.0f64;
            let mut real = true;
            for a in 0..verts.len() {
                for b in 0..a {
                    let d = dense.get(verts[a], verts[b]);
                    if !d.is_finite() {
                        real = false;
                    }
                    diameter = diameter.max(d);
                }
            }
            if !real {
                continue;
            }
            let simplex = Simplex {
                diameter,
                index: rank(&bt, &verts),
            };
            for upper_only in [false, true] {
                let got = cofacets(&sparse, &bt, simplex, &verts, dim, upper_only);
                let expected: Vec<_> = cofacets(&dense, &bt, simplex, &verts, dim, upper_only)
                    .into_iter()
                    .filter(|&(_, _, diam)| diam.is_finite())
                    .collect();
                assert_eq!(got, expected, "verts {verts:?}, upper_only {upper_only}");
            }
            trials += 1;
        }
        assert!(
            trials > 500,
            "too few genuine simplices exercised: {trials}"
        );
    }

    // Degenerate intersections stay in lockstep with the dense default: an
    // empty pivot neighbor list (isolated vertex), an empty intersection with
    // both endpoints non-empty, and an ordinary non-empty case.
    #[test]
    fn sparse_cofacets_empty_intersections() {
        // Triangle {0,1,2}, a disjoint edge 3-4, and an isolated vertex 5.
        let sparse = SparseDistanceMatrix::from_triplets(
            6,
            &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0), (3, 4, 2.0)],
        )
        .unwrap();
        let inf = f64::INFINITY;
        let dense = DistanceMatrix::from_condensed(vec![
            1.0, // 1-0
            1.0, 1.0, // 2-0, 2-1
            inf, inf, inf, // 3-*
            inf, inf, inf, 2.0, // 4-*, 4-3
            inf, inf, inf, inf, inf, // 5-*
        ])
        .unwrap();
        let bt = BinomialTable::new(6, 6).unwrap();

        // {0,1}: common neighbor 2 (non-empty). {0,3}: 0->{1,2}, 3->{4}, no
        // common vertex (empty intersection, both lists non-empty). {0,5}:
        // vertex 5 is isolated, so the pivot list is empty.
        for verts in [[0usize, 1usize], [0, 3], [0, 5]] {
            let d01 = dense.get(verts[0], verts[1]);
            let simplex = Simplex {
                diameter: d01,
                index: rank(&bt, &verts),
            };
            for upper_only in [false, true] {
                let got = cofacets(&sparse, &bt, simplex, &verts, 1, upper_only);
                let expected: Vec<_> = cofacets(&dense, &bt, simplex, &verts, 1, upper_only)
                    .into_iter()
                    .filter(|&(_, _, d)| d.is_finite())
                    .collect();
                assert_eq!(got, expected, "verts {verts:?}, upper_only {upper_only}");
            }
        }
    }

    #[test]
    fn scaled_norm_survives_extreme_magnitudes() {
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e200);
        let d = DistanceMatrix::from_points(&[vec![0.0], vec![1e-200]]).unwrap();
        assert_eq!(d.get(0, 1), 1e-200);
        let d = DistanceMatrix::from_points(&[vec![3e200, 0.0], vec![0.0, 4e200]]).unwrap();
        assert!((d.get(0, 1) / 5e200 - 1.0).abs() < 1e-15);
    }

    #[test]
    fn overflowing_difference_is_an_absent_edge() {
        let d = DistanceMatrix::from_points(&[vec![1e308], vec![-1e308]]).unwrap();
        assert_eq!(d.get(0, 1), f64::INFINITY);
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        assert!(DistanceMatrix::from_points(&[vec![f64::INFINITY], vec![0.0]]).is_err());
        assert!(DistanceMatrix::from_points(&[vec![f64::NAN], vec![0.0]]).is_err());
    }

    #[test]
    fn negative_zero_entries_are_normalized() {
        let d = DistanceMatrix::from_condensed(vec![-0.0]).unwrap();
        assert!(d.get(0, 1).is_sign_positive());
    }

    #[test]
    fn validation_errors_carry_the_condensed_index() {
        let err = DistanceMatrix::from_condensed(vec![1.0, f64::NAN, 1.0]).unwrap_err();
        assert!(err.to_string().contains("index 1"), "{err}");
        let err = DistanceMatrix::from_condensed(vec![1.0, 1.0, -2.0]).unwrap_err();
        assert!(err.to_string().contains("index 2"), "{err}");
    }

    #[test]
    fn empty_condensed_means_one_point() {
        assert_eq!(DistanceMatrix::from_condensed(vec![]).unwrap().len(), 1);
    }
}
