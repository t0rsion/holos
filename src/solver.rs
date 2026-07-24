//! Implicit persistent cohomology over a prime field, following ripser's
//! algorithm (Bauer 2021): anti-transpose convention, clearing, emergent and
//! apparent pair shortcuts, lazy-cancellation heaps, and on-demand
//! regeneration of reducer columns. Simplices exist only as combinadic
//! indices; no simplex list of dimension max_dim+1 is ever materialized.
//!
//! The engine is generic over the coefficient field (`Coeffs`) and the
//! distance source (`Distances`). Z/2 coefficients are a zero-sized type, so
//! that instantiation compiles to exactly the coefficient-free algorithm;
//! Z/p carries a u64 coefficient and mirrors ripser's USE_COEFFICIENTS
//! arithmetic line for line.

use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;

use crate::combinadic::{BinomialTable, CofacetIter, FacetIter};
use crate::distances::Distances;
use crate::union_find::UnionFind;
use crate::{Bar, Diagram, Error, Result, RipsParams};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Simplex {
    diameter: f64,
    index: u64,
}

/// A simplex with a field coefficient. For Z/2 the coefficient is `()` and
/// this is layout-identical to `Simplex`.
#[derive(Debug, Clone, Copy)]
struct Entry<E> {
    sx: Simplex,
    coeff: E,
}

/// Heap order matches ripser's working-column priority queue: pops the
/// cofacet minimal in the (d+1)-simplex order, i.e. smallest diameter,
/// then largest index. The coefficient does not participate.
#[derive(Debug, Clone, Copy)]
struct HeapEntry<E>(Entry<E>);

impl<E: Copy> PartialEq for HeapEntry<E> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl<E: Copy> Eq for HeapEntry<E> {}
impl<E: Copy> PartialOrd for HeapEntry<E> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<E: Copy> Ord for HeapEntry<E> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .sx
            .diameter
            .total_cmp(&self.0.sx.diameter)
            .then(self.0.sx.index.cmp(&other.0.sx.index))
    }
}

/// Field operations plus the lazy-heap cancellation rule. Implementations
/// hold whatever context they need (the modulus, an inverse table).
trait Coeffs {
    type E: Copy + PartialEq + std::fmt::Debug;
    /// (-1)^k: the boundary/coboundary sign at enumerator position k
    /// (ripser's `k & 1 ? modulus - 1 : 1`).
    fn sign(&self, k: usize) -> Self::E;
    fn mul(&self, a: Self::E, b: Self::E) -> Self::E;
    fn neg(&self, a: Self::E) -> Self::E;
    /// Ripser's reduction factor: -(pivot / other) in the field.
    fn factor(&self, pivot: Self::E, other: Self::E) -> Self::E;
    /// Pop the pivot with lazy cancellation; entries with equal index
    /// combine, and a zero combined coefficient vanishes.
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry<Self::E>>) -> Option<Entry<Self::E>>;
}

/// Z/2: every stored coefficient is 1, so nothing is stored and equal
/// adjacent indices annihilate in pairs.
struct Z2;

impl Coeffs for Z2 {
    type E = ();
    fn sign(&self, _k: usize) {}
    fn mul(&self, _a: (), _b: ()) {}
    fn neg(&self, _a: ()) {}
    fn factor(&self, _pivot: (), _other: ()) {}
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry<()>>) -> Option<Entry<()>> {
        while let Some(top) = heap.pop() {
            match heap.peek() {
                Some(next) if next.0.sx.index == top.0.sx.index => {
                    heap.pop();
                }
                _ => return Some(top.0),
            }
        }
        None
    }
}

/// Z/p for an odd prime p: coefficients stored in 1..p, multiplicative
/// inverses precomputed exactly as in ripser.
struct Fp {
    p: u64,
    inv: Vec<u64>,
}

impl Fp {
    fn new(p: u64) -> Self {
        let mut inv = vec![0u64; p as usize];
        if p > 1 {
            inv[1] = 1;
        }
        for a in 2..p {
            // inv[a] = p - (inv[p % a] * (p / a)) % p, valid for prime p.
            inv[a as usize] = p - (inv[(p % a) as usize] * (p / a)) % p;
        }
        Self { p, inv }
    }
}

impl Coeffs for Fp {
    type E = u64;
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
    fn pop_pivot(&self, heap: &mut BinaryHeap<HeapEntry<u64>>) -> Option<Entry<u64>> {
        let mut pivot: Option<Entry<u64>> = None;
        while let Some(&HeapEntry(top)) = heap.peek() {
            match pivot.as_mut() {
                None => pivot = Some(top),
                Some(p) if p.coeff == 0 => *p = top,
                Some(p) if top.sx.index != p.sx.index => return Some(*p),
                Some(p) => p.coeff = (p.coeff + top.coeff) % self.p,
            }
            heap.pop();
        }
        pivot.filter(|p| p.coeff != 0)
    }
}

fn is_prime(p: u64) -> bool {
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

/// The largest accepted modulus (exclusive). Keeps the inverse table small;
/// the arithmetic itself is exact for far larger primes.
const MODULUS_LIMIT: u64 = 1 << 15;

pub(crate) fn compute<D: Distances>(dist: &D, params: &RipsParams) -> Result<Diagram> {
    if let Some(t) = params.threshold {
        if t.is_nan() || t < 0.0 {
            return Err(Error::InvalidInput(format!(
                "threshold must be non-negative, got {t}"
            )));
        }
    }
    let p = params.modulus as u64;
    if !is_prime(p) || p >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {p}"
        )));
    }
    if p == 2 {
        compute_impl(dist, Z2, params)
    } else {
        compute_impl(dist, Fp::new(p), params)
    }
}

fn compute_impl<C: Coeffs, D: Distances>(dist: &D, ops: C, params: &RipsParams) -> Result<Diagram> {
    let n = dist.len();
    let mut diagram = Diagram::default();
    if n == 0 {
        return Ok(diagram);
    }
    let threshold = params.threshold.unwrap_or_else(|| dist.default_threshold());
    // A complex on n points has no simplex above dimension n-1; clamping also
    // bounds the binomial table for absurd max_dim requests.
    let max_dim = params.max_dim.min(n.saturating_sub(1));
    let bt = BinomialTable::new(n, max_dim + 2)?;
    let mut engine = Engine {
        dist,
        bt,
        n,
        threshold,
        max_dim,
        params,
        ops,
        vertex_buf: Vec::new(),
    };
    engine.run(&mut diagram);
    diagram.canonicalize();
    Ok(diagram)
}

struct Engine<'a, C: Coeffs, D: Distances> {
    dist: &'a D,
    bt: BinomialTable,
    n: usize,
    threshold: f64,
    max_dim: usize,
    params: &'a RipsParams,
    ops: C,
    vertex_buf: Vec<usize>,
}

impl<C: Coeffs, D: Distances> Engine<'_, C, D> {
    fn run(&mut self, diagram: &mut Diagram) {
        let edges = self.edges();
        let mut columns = self.dim0_pairs(&edges, diagram);

        // `simplices` holds all dim-d simplices in the complex, the seed for
        // canonical (d+1)-cofacet assembly.
        let mut simplices = edges;
        let mut prev_pivots: FxHashMap<u64, (C::E, usize)> = FxHashMap::default();

        for dim in 1..=self.max_dim {
            let pivots = self.reduce_dimension(&columns, dim, &prev_pivots, diagram);
            if dim < self.max_dim {
                (simplices, columns) = self.assemble(&simplices, dim + 1, &pivots);
            }
            prev_pivots = pivots;
        }
    }

    #[inline]
    fn in_complex(&self, diameter: f64) -> bool {
        diameter <= self.threshold && diameter.is_finite()
    }

    fn edges(&self) -> Vec<Simplex> {
        let mut edges = Vec::new();
        let bt = &self.bt;
        let threshold = self.threshold;
        self.dist.for_each_edge(&mut |i, j, d| {
            if d <= threshold && d.is_finite() {
                edges.push(Simplex {
                    diameter: d,
                    index: bt.get(i, 2) + j as u64,
                });
            }
        });
        edges
    }

    /// Union-find pass: emits dim-0 bars and returns the dim-1 columns
    /// (cycle edges), sorted for reduction (diameter descending, index
    /// ascending).
    fn dim0_pairs(&mut self, edges: &[Simplex], diagram: &mut Diagram) -> Vec<Simplex> {
        let mut sorted: Vec<Simplex> = edges.to_vec();
        sorted.sort_by(|a, b| {
            a.diameter
                .total_cmp(&b.diameter)
                .then(b.index.cmp(&a.index))
        });
        let mut uf = UnionFind::new(self.n);
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        for e in &sorted {
            self.bt.unrank(e.index, 1, self.n, &mut verts);
            let (ru, rv) = (uf.find(verts[0]), uf.find(verts[1]));
            if ru != rv {
                if e.diameter > 0.0 {
                    diagram.bars.push(Bar {
                        dim: 0,
                        birth: 0.0,
                        death: e.diameter,
                    });
                }
                uf.link(ru, rv);
            } else if self.max_dim > 0
                && (!self.params.use_apparent_pairs || self.zero_apparent_cofacet(*e, 1).is_none())
            {
                columns.push(*e);
            }
        }
        for v in 0..self.n {
            if uf.find(v) == v {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                });
            }
        }
        columns.reverse();
        columns
    }

    /// Canonical cofacet assembly with clearing and apparent-pair pruning.
    /// Returns (all (d)-simplices, columns to reduce in dimension d).
    fn assemble(
        &mut self,
        simplices: &[Simplex],
        dim: usize,
        prev_pivots: &FxHashMap<u64, (C::E, usize)>,
    ) -> (Vec<Simplex>, Vec<Simplex>) {
        let mut next_simplices = Vec::new();
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        for s in simplices {
            self.bt.unrank(s.index, dim - 1, self.n, &mut verts);
            let mut iter = CofacetIter::new(&self.bt, s.index, dim - 1, self.n);
            while let Some((index, vertex)) = iter.next_upper() {
                let diameter = self.extend_diameter(s.diameter, &verts, vertex);
                if !self.in_complex(diameter) {
                    continue;
                }
                let cofacet = Simplex { diameter, index };
                next_simplices.push(cofacet);
                let cleared = self.params.use_clearing && prev_pivots.contains_key(&index);
                let apparent =
                    self.params.use_apparent_pairs && self.is_in_zero_apparent_pair(cofacet, dim);
                if !cleared && !apparent {
                    columns.push(cofacet);
                }
            }
        }
        columns.sort_by(|a, b| {
            b.diameter
                .total_cmp(&a.diameter)
                .then(a.index.cmp(&b.index))
        });
        (next_simplices, columns)
    }

    /// Reduce the dim-d columns against implicit (d+1)-rows. Returns the
    /// pivot registry (pivot coefficient and column position, keyed by
    /// pivot index) for clearing in the next dimension.
    fn reduce_dimension(
        &mut self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &FxHashMap<u64, (C::E, usize)>,
        diagram: &mut Diagram,
    ) -> FxHashMap<u64, (C::E, usize)> {
        let mut pivot_map: FxHashMap<u64, (C::E, usize)> = FxHashMap::default();
        let mut v_entries: Vec<Entry<C::E>> = Vec::new();
        let mut v_offsets: Vec<usize> = vec![0];
        let mut working_cob: BinaryHeap<HeapEntry<C::E>> = BinaryHeap::new();
        let mut working_red: BinaryHeap<HeapEntry<C::E>> = BinaryHeap::new();
        let mut cofacet_buf: Vec<Entry<C::E>> = Vec::new();

        for (col_pos, &column) in columns.iter().enumerate() {
            working_cob.clear();
            working_red.clear();
            let mut pivot =
                self.init_coboundary(column, dim, &pivot_map, &mut working_cob, &mut cofacet_buf);
            loop {
                match pivot {
                    Some(p) => {
                        if let Some(&(other_coeff, other_pos)) = pivot_map.get(&p.sx.index) {
                            let factor = self.ops.factor(p.coeff, other_coeff);
                            let reducer = Entry {
                                sx: columns[other_pos],
                                coeff: factor,
                            };
                            let (lo, hi) = (v_offsets[other_pos], v_offsets[other_pos + 1]);
                            self.add_simplex_coboundary(
                                reducer,
                                dim,
                                &mut working_red,
                                &mut working_cob,
                            );
                            let reducer_v: Vec<Entry<C::E>> = v_entries[lo..hi].to_vec();
                            for s in reducer_v {
                                let scaled = Entry {
                                    sx: s.sx,
                                    coeff: self.ops.mul(s.coeff, factor),
                                };
                                self.add_simplex_coboundary(
                                    scaled,
                                    dim,
                                    &mut working_red,
                                    &mut working_cob,
                                );
                            }
                            pivot = self.get_pivot(&mut working_cob);
                        } else if self.params.use_apparent_pairs {
                            if let Some((facet, k)) = self.zero_apparent_facet(p.sx, dim + 1) {
                                // Ripser negates the facet's boundary
                                // coefficient so the pivot cancels exactly.
                                let coeff = self.ops.neg(self.ops.mul(self.ops.sign(k), p.coeff));
                                let e = Entry { sx: facet, coeff };
                                self.add_simplex_coboundary(
                                    e,
                                    dim,
                                    &mut working_red,
                                    &mut working_cob,
                                );
                                pivot = self.get_pivot(&mut working_cob);
                            } else {
                                self.emit_pair(column, p.sx, dim, diagram);
                                pivot_map.insert(p.sx.index, (p.coeff, col_pos));
                                self.drain_into(&mut working_red, &mut v_entries);
                                break;
                            }
                        } else {
                            self.emit_pair(column, p.sx, dim, diagram);
                            pivot_map.insert(p.sx.index, (p.coeff, col_pos));
                            self.drain_into(&mut working_red, &mut v_entries);
                            break;
                        }
                    }
                    None => {
                        // With clearing disabled, columns that were pivots of
                        // the previous dimension reduce to zero here; they are
                        // deaths, not essential classes.
                        let is_prior_death =
                            !self.params.use_clearing && prev_pivots.contains_key(&column.index);
                        if !is_prior_death {
                            diagram.bars.push(Bar {
                                dim,
                                birth: column.diameter,
                                death: f64::INFINITY,
                            });
                        }
                        break;
                    }
                }
            }
            v_offsets.push(v_entries.len());
        }
        pivot_map
    }

    fn emit_pair(&self, column: Simplex, pivot: Simplex, dim: usize, diagram: &mut Diagram) {
        if pivot.diameter > column.diameter {
            diagram.bars.push(Bar {
                dim,
                birth: column.diameter,
                death: pivot.diameter,
            });
        }
    }

    /// Ripser's init_coboundary_and_get_pivot: enumerate the coboundary and,
    /// when the emergent shortcut fires, return the pivot without building
    /// the working column at all.
    fn init_coboundary(
        &mut self,
        column: Simplex,
        dim: usize,
        pivot_map: &FxHashMap<u64, (C::E, usize)>,
        working_cob: &mut BinaryHeap<HeapEntry<C::E>>,
        cofacet_buf: &mut Vec<Entry<C::E>>,
    ) -> Option<Entry<C::E>> {
        let mut verts = std::mem::take(&mut self.vertex_buf);
        self.bt.unrank(column.index, dim, self.n, &mut verts);
        cofacet_buf.clear();
        let mut check_emergent = self.params.use_emergent_pairs;
        let mut iter = CofacetIter::new(&self.bt, column.index, dim, self.n);
        let mut emergent: Option<Entry<C::E>> = None;
        while let Some((index, vertex, k)) = iter.next_all() {
            let diameter = self.extend_diameter(column.diameter, &verts, vertex);
            if !self.in_complex(diameter) {
                continue;
            }
            let cofacet = Entry {
                sx: Simplex { diameter, index },
                coeff: self.ops.sign(k),
            };
            cofacet_buf.push(cofacet);
            if check_emergent && diameter == column.diameter {
                let stolen = self.params.use_apparent_pairs
                    && self.zero_apparent_facet(cofacet.sx, dim + 1).is_some();
                if !pivot_map.contains_key(&index) && !stolen {
                    emergent = Some(cofacet);
                    break;
                }
                check_emergent = false;
            }
        }
        self.vertex_buf = verts;
        if let Some(p) = emergent {
            return Some(p);
        }
        for c in cofacet_buf.iter() {
            working_cob.push(HeapEntry(*c));
        }
        self.get_pivot(working_cob)
    }

    /// Push a dim-d entry into the V column and its regenerated coboundary,
    /// scaled by the entry's coefficient, into the working column.
    fn add_simplex_coboundary(
        &mut self,
        entry: Entry<C::E>,
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry<C::E>>,
        working_cob: &mut BinaryHeap<HeapEntry<C::E>>,
    ) {
        working_red.push(HeapEntry(entry));
        let mut verts = std::mem::take(&mut self.vertex_buf);
        self.bt.unrank(entry.sx.index, dim, self.n, &mut verts);
        let mut iter = CofacetIter::new(&self.bt, entry.sx.index, dim, self.n);
        while let Some((index, vertex, k)) = iter.next_all() {
            let diameter = self.extend_diameter(entry.sx.diameter, &verts, vertex);
            if self.in_complex(diameter) {
                working_cob.push(HeapEntry(Entry {
                    sx: Simplex { diameter, index },
                    coeff: self.ops.mul(self.ops.sign(k), entry.coeff),
                }));
            }
        }
        self.vertex_buf = verts;
    }

    fn get_pivot(&self, heap: &mut BinaryHeap<HeapEntry<C::E>>) -> Option<Entry<C::E>> {
        let pivot = self.ops.pop_pivot(heap)?;
        heap.push(HeapEntry(pivot));
        Some(pivot)
    }

    /// Drain the working reduction column, cancelled in the field, into the
    /// V store.
    fn drain_into(&self, heap: &mut BinaryHeap<HeapEntry<C::E>>, out: &mut Vec<Entry<C::E>>) {
        while let Some(e) = self.ops.pop_pivot(heap) {
            out.push(e);
        }
    }

    #[inline]
    fn extend_diameter(&self, diameter: f64, vertices: &[usize], new_vertex: usize) -> f64 {
        let mut d = diameter;
        for &v in vertices {
            d = d.max(self.dist.get(new_vertex, v));
        }
        d
    }

    fn simplex_diameter(&self, vertices: &[usize]) -> f64 {
        let mut d = 0.0f64;
        for (a, &va) in vertices.iter().enumerate() {
            for &vb in &vertices[a + 1..] {
                d = d.max(self.dist.get(va, vb));
            }
        }
        d
    }

    /// First facet (in ripser's facet order) with the same diameter, along
    /// with its enumerator position k (for the boundary sign).
    /// `vertices` must be the simplex's vertex set.
    fn zero_pivot_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<(Simplex, usize)> {
        let iter = FacetIter::new(&self.bt, simplex.index, dim, self.n);
        let mut facet_verts = Vec::with_capacity(vertices.len());
        for (index, removed, k) in iter {
            facet_verts.clear();
            facet_verts.extend(vertices.iter().copied().filter(|&v| v != removed));
            let d = self.simplex_diameter(&facet_verts);
            if d == simplex.diameter {
                return Some((Simplex { diameter: d, index }, k));
            }
        }
        None
    }

    /// First cofacet (descending index order) with the same diameter.
    fn zero_pivot_cofacet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<Simplex> {
        let mut iter = CofacetIter::new(&self.bt, simplex.index, dim, self.n);
        while let Some((index, vertex, _)) = iter.next_all() {
            let d = self.extend_diameter(simplex.diameter, vertices, vertex);
            if d == simplex.diameter {
                return Some(Simplex { diameter: d, index });
            }
        }
        None
    }

    fn zero_apparent_facet(&self, simplex: Simplex, dim: usize) -> Option<(Simplex, usize)> {
        let mut verts = Vec::new();
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        self.zero_apparent_facet_with(&verts, simplex, dim)
    }

    fn zero_apparent_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<(Simplex, usize)> {
        let (facet, k) = self.zero_pivot_facet_with(vertices, simplex, dim)?;
        let facet_verts: Vec<usize> = {
            let mut fv = Vec::new();
            self.bt.unrank(facet.index, dim - 1, self.n, &mut fv);
            fv
        };
        match self.zero_pivot_cofacet_with(&facet_verts, facet, dim - 1) {
            Some(c) if c.index == simplex.index => Some((facet, k)),
            _ => None,
        }
    }

    fn zero_apparent_cofacet(&self, simplex: Simplex, dim: usize) -> Option<Simplex> {
        let mut verts = Vec::new();
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        let cofacet = self.zero_pivot_cofacet_with(&verts, simplex, dim)?;
        let mut cverts = Vec::new();
        self.bt.unrank(cofacet.index, dim + 1, self.n, &mut cverts);
        match self.zero_pivot_facet_with(&cverts, cofacet, dim + 1) {
            Some((f, _)) if f.index == simplex.index => Some(cofacet),
            _ => None,
        }
    }

    fn is_in_zero_apparent_pair(&self, simplex: Simplex, dim: usize) -> bool {
        self.zero_apparent_cofacet(simplex, dim).is_some()
            || self.zero_apparent_facet(simplex, dim).is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::{rips_persistence, DistanceMatrix, RipsParams};

    #[test]
    fn triangle_unit_distances() {
        let dist = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1)).unwrap();
        let h0: Vec<_> = d.in_dim(0).collect();
        assert_eq!(h0.len(), 3);
        assert_eq!(h0.iter().filter(|b| b.is_essential()).count(), 1);
        assert_eq!(h0.iter().filter(|b| b.death == 1.0).count(), 2);
        // The loop forms and fills at the same scale: zero persistence.
        assert_eq!(d.in_dim(1).count(), 0);
    }

    #[test]
    fn square_has_one_h1_bar() {
        let s = 2.0f64.sqrt();
        // Vertices of a unit square: sides 1, diagonals sqrt(2).
        let dist = DistanceMatrix::from_condensed(vec![1.0, s, 1.0, 1.0, s, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1).with_threshold(2.0)).unwrap();
        assert_eq!(d.in_dim(0).filter(|b| b.is_essential()).count(), 1);
        assert_eq!(d.in_dim(0).count(), 4);
        let h1: Vec<_> = d.in_dim(1).collect();
        assert_eq!(h1.len(), 1);
        assert_eq!(h1[0].birth, 1.0);
        assert_eq!(h1[0].death, s);
    }

    #[test]
    fn two_components() {
        let inf = f64::INFINITY;
        let dist = DistanceMatrix::from_condensed(vec![1.0, inf, inf, inf, inf, 1.0]).unwrap();
        let d = rips_persistence(&dist, &RipsParams::new(1).with_threshold(10.0)).unwrap();
        assert_eq!(d.in_dim(0).filter(|b| b.is_essential()).count(), 2);
    }

    #[test]
    fn square_diagram_is_field_independent() {
        // A torsion-free space: identical diagrams over every field.
        let s = 2.0f64.sqrt();
        let dist = DistanceMatrix::from_condensed(vec![1.0, s, 1.0, 1.0, s, 1.0]).unwrap();
        let base = rips_persistence(&dist, &RipsParams::new(1)).unwrap();
        for p in [3u32, 5, 7, 13] {
            let mut params = RipsParams::new(1);
            params.modulus = p;
            let d = rips_persistence(&dist, &params).unwrap();
            assert_eq!(d.bars, base.bars, "diagram differs at p = {p}");
        }
    }

    #[test]
    fn invalid_modulus_is_rejected() {
        let dist = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
        for bad in [0u32, 1, 4, 6, 9, 32768, 32770] {
            let mut params = RipsParams::new(1);
            params.modulus = bad;
            assert!(
                rips_persistence(&dist, &params).is_err(),
                "modulus {bad} must be rejected"
            );
        }
    }

    #[test]
    fn prime_check() {
        use super::is_prime;
        let primes = [2u64, 3, 5, 7, 11, 13, 32749];
        let composites = [0u64, 1, 4, 9, 15, 32767];
        assert!(primes.into_iter().all(is_prime));
        assert!(!composites.into_iter().any(is_prime));
    }
}
