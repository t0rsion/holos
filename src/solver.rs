//! Implicit persistent cohomology over Z/2, following ripser's algorithm
//! (Bauer 2021): anti-transpose convention, clearing, emergent and apparent
//! pair shortcuts, lazy-cancellation heaps, and on-demand regeneration of
//! reducer columns. Simplices exist only as combinadic indices; no simplex
//! list of dimension max_dim+1 is ever materialized.

use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;

use crate::combinadic::{BinomialTable, CofacetIter, FacetIter};
use crate::union_find::UnionFind;
use crate::{Bar, Diagram, DistanceMatrix, Error, Result, RipsParams};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Simplex {
    diameter: f64,
    index: u64,
}

/// Heap order matches ripser's working-column priority queue: pops the
/// cofacet minimal in the (d+1)-simplex order, i.e. smallest diameter,
/// then largest index.
#[derive(Debug, Clone, Copy)]
struct HeapSimplex(Simplex);

impl PartialEq for HeapSimplex {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for HeapSimplex {}
impl PartialOrd for HeapSimplex {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapSimplex {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .diameter
            .total_cmp(&self.0.diameter)
            .then(self.0.index.cmp(&other.0.index))
    }
}

pub fn compute(dist: &DistanceMatrix, params: &RipsParams) -> Result<Diagram> {
    if let Some(t) = params.threshold {
        if t.is_nan() || t < 0.0 {
            return Err(Error::InvalidInput(format!(
                "threshold must be non-negative, got {t}"
            )));
        }
    }
    let n = dist.len();
    let mut diagram = Diagram::default();
    if n == 0 {
        return Ok(diagram);
    }
    let threshold = params.threshold.unwrap_or_else(|| dist.enclosing_radius());
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
        vertex_buf: Vec::new(),
    };
    engine.run(&mut diagram);
    diagram.canonicalize();
    Ok(diagram)
}

struct Engine<'a> {
    dist: &'a DistanceMatrix,
    bt: BinomialTable,
    n: usize,
    threshold: f64,
    max_dim: usize,
    params: &'a RipsParams,
    vertex_buf: Vec<usize>,
}

impl Engine<'_> {
    fn run(&mut self, diagram: &mut Diagram) {
        let edges = self.edges();
        let mut columns = self.dim0_pairs(&edges, diagram);

        // `simplices` holds all dim-d simplices in the complex, the seed for
        // canonical (d+1)-cofacet assembly.
        let mut simplices = edges;
        let mut prev_pivots: FxHashMap<u64, usize> = FxHashMap::default();

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
        for i in 1..self.n {
            let base = self.bt.get(i, 2);
            for j in 0..i {
                let d = self.dist.get(i, j);
                if self.in_complex(d) {
                    edges.push(Simplex {
                        diameter: d,
                        index: base + j as u64,
                    });
                }
            }
        }
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
        prev_pivots: &FxHashMap<u64, usize>,
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
    /// pivot registry for clearing in the next dimension.
    fn reduce_dimension(
        &mut self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &FxHashMap<u64, usize>,
        diagram: &mut Diagram,
    ) -> FxHashMap<u64, usize> {
        let mut pivot_map: FxHashMap<u64, usize> = FxHashMap::default();
        let mut v_entries: Vec<Simplex> = Vec::new();
        let mut v_offsets: Vec<usize> = vec![0];
        let mut working_cob: BinaryHeap<HeapSimplex> = BinaryHeap::new();
        let mut working_red: BinaryHeap<HeapSimplex> = BinaryHeap::new();
        let mut cofacet_buf: Vec<Simplex> = Vec::new();

        for (col_pos, &column) in columns.iter().enumerate() {
            working_cob.clear();
            working_red.clear();
            let mut pivot =
                self.init_coboundary(column, dim, &pivot_map, &mut working_cob, &mut cofacet_buf);
            loop {
                match pivot {
                    Some(p) => {
                        if let Some(&other_pos) = pivot_map.get(&p.index) {
                            let reducer = columns[other_pos];
                            let (lo, hi) = (v_offsets[other_pos], v_offsets[other_pos + 1]);
                            self.add_simplex_coboundary(
                                reducer,
                                dim,
                                &mut working_red,
                                &mut working_cob,
                            );
                            let reducer_v: Vec<Simplex> = v_entries[lo..hi].to_vec();
                            for s in reducer_v {
                                self.add_simplex_coboundary(
                                    s,
                                    dim,
                                    &mut working_red,
                                    &mut working_cob,
                                );
                            }
                            pivot = get_pivot(&mut working_cob);
                        } else if self.params.use_apparent_pairs {
                            if let Some(e) = self.zero_apparent_facet(p, dim + 1) {
                                self.add_simplex_coboundary(
                                    e,
                                    dim,
                                    &mut working_red,
                                    &mut working_cob,
                                );
                                pivot = get_pivot(&mut working_cob);
                            } else {
                                self.emit_pair(column, p, dim, diagram);
                                pivot_map.insert(p.index, col_pos);
                                drain_into(&mut working_red, &mut v_entries);
                                break;
                            }
                        } else {
                            self.emit_pair(column, p, dim, diagram);
                            pivot_map.insert(p.index, col_pos);
                            drain_into(&mut working_red, &mut v_entries);
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
        pivot_map: &FxHashMap<u64, usize>,
        working_cob: &mut BinaryHeap<HeapSimplex>,
        cofacet_buf: &mut Vec<Simplex>,
    ) -> Option<Simplex> {
        let mut verts = std::mem::take(&mut self.vertex_buf);
        self.bt.unrank(column.index, dim, self.n, &mut verts);
        cofacet_buf.clear();
        let mut check_emergent = self.params.use_emergent_pairs;
        let mut iter = CofacetIter::new(&self.bt, column.index, dim, self.n);
        let mut emergent: Option<Simplex> = None;
        while let Some((index, vertex)) = iter.next_all() {
            let diameter = self.extend_diameter(column.diameter, &verts, vertex);
            if !self.in_complex(diameter) {
                continue;
            }
            let cofacet = Simplex { diameter, index };
            cofacet_buf.push(cofacet);
            if check_emergent && diameter == column.diameter {
                let stolen = self.params.use_apparent_pairs
                    && self.zero_apparent_facet(cofacet, dim + 1).is_some();
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
            working_cob.push(HeapSimplex(*c));
        }
        get_pivot(working_cob)
    }

    /// Push a dim-d simplex into the V column and its regenerated coboundary
    /// into the working column.
    fn add_simplex_coboundary(
        &mut self,
        simplex: Simplex,
        dim: usize,
        working_red: &mut BinaryHeap<HeapSimplex>,
        working_cob: &mut BinaryHeap<HeapSimplex>,
    ) {
        working_red.push(HeapSimplex(simplex));
        let mut verts = std::mem::take(&mut self.vertex_buf);
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        let mut iter = CofacetIter::new(&self.bt, simplex.index, dim, self.n);
        while let Some((index, vertex)) = iter.next_all() {
            let diameter = self.extend_diameter(simplex.diameter, &verts, vertex);
            if self.in_complex(diameter) {
                working_cob.push(HeapSimplex(Simplex { diameter, index }));
            }
        }
        self.vertex_buf = verts;
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

    /// First facet (in ripser's facet order) with the same diameter.
    /// `vertices` must be the simplex's vertex set.
    fn zero_pivot_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<Simplex> {
        let iter = FacetIter::new(&self.bt, simplex.index, dim, self.n);
        let mut facet_verts = Vec::with_capacity(vertices.len());
        for (index, removed) in iter {
            facet_verts.clear();
            facet_verts.extend(vertices.iter().copied().filter(|&v| v != removed));
            let d = self.simplex_diameter(&facet_verts);
            if d == simplex.diameter {
                return Some(Simplex { diameter: d, index });
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
        while let Some((index, vertex)) = iter.next_all() {
            let d = self.extend_diameter(simplex.diameter, vertices, vertex);
            if d == simplex.diameter {
                return Some(Simplex { diameter: d, index });
            }
        }
        None
    }

    fn zero_apparent_facet(&self, simplex: Simplex, dim: usize) -> Option<Simplex> {
        let mut verts = Vec::new();
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        self.zero_apparent_facet_with(&verts, simplex, dim)
    }

    fn zero_apparent_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<Simplex> {
        let facet = self.zero_pivot_facet_with(vertices, simplex, dim)?;
        let facet_verts: Vec<usize> = {
            let mut fv = Vec::new();
            self.bt.unrank(facet.index, dim - 1, self.n, &mut fv);
            fv
        };
        match self.zero_pivot_cofacet_with(&facet_verts, facet, dim - 1) {
            Some(c) if c.index == simplex.index => Some(facet),
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
            Some(f) if f.index == simplex.index => Some(cofacet),
            _ => None,
        }
    }

    fn is_in_zero_apparent_pair(&self, simplex: Simplex, dim: usize) -> bool {
        self.zero_apparent_cofacet(simplex, dim).is_some()
            || self.zero_apparent_facet(simplex, dim).is_some()
    }
}

/// Z/2 lazy cancellation: equal adjacent indices annihilate in pairs.
fn pop_pivot(heap: &mut BinaryHeap<HeapSimplex>) -> Option<Simplex> {
    while let Some(top) = heap.pop() {
        match heap.peek() {
            Some(next) if next.0.index == top.0.index => {
                heap.pop();
            }
            _ => return Some(top.0),
        }
    }
    None
}

fn get_pivot(heap: &mut BinaryHeap<HeapSimplex>) -> Option<Simplex> {
    let pivot = pop_pivot(heap)?;
    heap.push(HeapSimplex(pivot));
    Some(pivot)
}

/// Drain the working reduction column, cancelled mod 2, into the V store.
fn drain_into(heap: &mut BinaryHeap<HeapSimplex>, out: &mut Vec<Simplex>) {
    while let Some(s) = pop_pivot(heap) {
        out.push(s);
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
}
