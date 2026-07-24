//! Independent correctness oracle: explicit simplex enumeration and textbook
//! boundary-matrix reduction over a prime field. Deliberately naive, shares
//! nothing with the solver path except the input and output types (down to
//! the modular inverses: Fermat exponentiation here, a Euclid-style table
//! there).

use std::collections::HashMap;

use crate::{Bar, Diagram, DistanceMatrix};

struct Simplex {
    verts: Vec<usize>,
    diam: f64,
}

/// Textbook persistence of the Rips filtration over Z/2; shares no code with
/// the solver path. Feasible only for small inputs.
pub fn rips_persistence_oracle(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
) -> Diagram {
    rips_persistence_oracle_mod(dist, max_dim, threshold, 2)
}

/// Textbook persistence of the Rips filtration over Z/p (p prime).
pub fn rips_persistence_oracle_mod(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> Diagram {
    let p = modulus as u64;
    // Own primality check: composite p makes pivots noninvertible.
    assert!(
        p >= 2 && (2..p).take_while(|d| d * d <= p).all(|d| p % d != 0),
        "oracle modulus must be prime, got {p}"
    );
    let n = dist.len();
    let threshold = threshold.unwrap_or_else(|| naive_enclosing_radius(dist));

    let mut simplices: Vec<Simplex> = Vec::new();
    for dim in 0..=max_dim + 1 {
        for verts in combinations(n, dim + 1) {
            let diam = diameter(dist, &verts);
            if diam.is_finite() && diam <= threshold {
                simplices.push(Simplex { verts, diam });
            }
        }
    }
    // A valid simplexwise refinement of the filtration: every face has
    // diameter <= its cofaces, and at equal diameter lower dimension first.
    simplices.sort_by(|a, b| {
        a.diam
            .total_cmp(&b.diam)
            .then(a.verts.len().cmp(&b.verts.len()))
            .then(a.verts.cmp(&b.verts))
    });

    let position: HashMap<Vec<usize>, usize> = simplices
        .iter()
        .enumerate()
        .map(|(i, s)| (s.verts.clone(), i))
        .collect();

    // Signed boundary columns: removing the vertex at position k carries
    // the coefficient (-1)^k, stored as a nonzero residue mod p.
    let m = simplices.len();
    let mut columns: Vec<Vec<(usize, u64)>> = Vec::with_capacity(m);
    for s in &simplices {
        let mut col: Vec<(usize, u64)> = Vec::new();
        if s.verts.len() > 1 {
            for k in 0..s.verts.len() {
                let mut face = s.verts.clone();
                face.remove(k);
                let coeff = if k % 2 == 0 { 1 } else { p - 1 };
                col.push((position[&face], coeff));
            }
        }
        col.sort_unstable_by_key(|&(row, _)| row);
        columns.push(col);
    }

    let mut pivot_of_row: Vec<Option<usize>> = vec![None; m];
    for j in 0..m {
        while let Some(&(low, c)) = columns[j].last() {
            match pivot_of_row[low] {
                Some(k) => {
                    let pivot_coeff = columns[k].last().unwrap().1;
                    // Eliminate the pivot: add -(c / pivot_coeff) * column k.
                    let factor = (p - c * mod_inverse(pivot_coeff, p) % p) % p;
                    let sum = add_scaled_mod_p(&columns[j], &columns[k], factor, p);
                    columns[j] = sum;
                }
                None => {
                    pivot_of_row[low] = Some(j);
                    break;
                }
            }
        }
    }

    let mut diagram = Diagram::default();
    for j in 0..m {
        if let Some(&(low, _)) = columns[j].last() {
            let birth = simplices[low].diam;
            let death = simplices[j].diam;
            if death > birth {
                diagram.bars.push(Bar {
                    dim: simplices[low].verts.len() - 1,
                    birth,
                    death,
                });
            }
        } else if pivot_of_row[j].is_none() {
            let dim = simplices[j].verts.len() - 1;
            if dim <= max_dim {
                diagram.bars.push(Bar {
                    dim,
                    birth: simplices[j].diam,
                    death: f64::INFINITY,
                });
            }
        }
    }
    diagram.canonicalize();
    diagram
}

// min over i of max over j != i of d(i, j); own loop rather than
// DistanceMatrix::enclosing_radius so the oracle shares no derived
// quantities with the solver path.
fn naive_enclosing_radius(dist: &DistanceMatrix) -> f64 {
    let n = dist.len();
    if n < 2 {
        return 0.0;
    }
    let mut radius = f64::INFINITY;
    for i in 0..n {
        let mut row_max = 0.0f64;
        for j in 0..n {
            if j != i {
                row_max = row_max.max(dist.get(i, j));
            }
        }
        radius = radius.min(row_max);
    }
    radius
}

fn diameter(dist: &DistanceMatrix, verts: &[usize]) -> f64 {
    let mut diam = 0.0f64;
    for (i, &u) in verts.iter().enumerate() {
        for &v in &verts[i + 1..] {
            diam = diam.max(dist.get(u, v));
        }
    }
    diam
}

fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, current: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
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
    let mut out = Vec::new();
    if k <= n {
        rec(0, n, k, &mut Vec::with_capacity(k), &mut out);
    }
    out
}

/// a + factor * b over Z/p, both columns sorted by row; zero entries drop.
fn add_scaled_mod_p(
    a: &[(usize, u64)],
    b: &[(usize, u64)],
    factor: u64,
    p: u64,
) -> Vec<(usize, u64)> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    let mut push = |row: usize, coeff: u64| {
        if coeff % p != 0 {
            out.push((row, coeff % p));
        }
    };
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            std::cmp::Ordering::Less => {
                push(a[i].0, a[i].1);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                push(b[j].0, b[j].1 * factor % p);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                push(a[i].0, (a[i].1 + b[j].1 * factor) % p);
                i += 1;
                j += 1;
            }
        }
    }
    for &(row, coeff) in &a[i..] {
        push(row, coeff);
    }
    for &(row, coeff) in &b[j..] {
        push(row, coeff * factor % p);
    }
    out
}

/// a^(p-2) mod p: the inverse by Fermat's little theorem, valid for prime p.
fn mod_inverse(a: u64, p: u64) -> u64 {
    let mut base = a % p;
    let mut exp = p - 2;
    let mut acc = 1u64;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = acc * base % p;
        }
        base = base * base % p;
        exp >>= 1;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(d: &Diagram, dim: usize) -> Vec<(f64, f64)> {
        d.in_dim(dim).map(|b| (b.birth, b.death)).collect()
    }

    #[test]
    fn unit_triangle() {
        let dist = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
        let d = rips_persistence_oracle(&dist, 1, None);
        // Two components die when the first two edges arrive; the loop is
        // filled by the triangle at the same scale it forms (suppressed).
        assert_eq!(
            bars(&d, 0),
            vec![(0.0, 1.0), (0.0, 1.0), (0.0, f64::INFINITY)]
        );
        assert_eq!(bars(&d, 1), vec![]);
    }

    #[test]
    fn unit_square() {
        let s = std::f64::consts::SQRT_2;
        // Vertices 0-1-2-3 in cycle order, sides 1, diagonals sqrt(2).
        let dist = DistanceMatrix::from_condensed(vec![1.0, s, 1.0, 1.0, s, 1.0]).unwrap();

        let capped = rips_persistence_oracle(&dist, 1, Some(1.0));
        assert_eq!(
            bars(&capped, 0),
            vec![(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, f64::INFINITY)]
        );
        assert_eq!(bars(&capped, 1), vec![(1.0, f64::INFINITY)]);

        // Enclosing radius is sqrt(2): the diagonals fill the square.
        let full = rips_persistence_oracle(&dist, 1, None);
        assert_eq!(
            bars(&full, 0),
            vec![(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, f64::INFINITY)]
        );
        assert_eq!(bars(&full, 1), vec![(1.0, s)]);
    }

    #[test]
    fn infinite_distance_is_an_absent_edge() {
        let dist = DistanceMatrix::from_condensed(vec![f64::INFINITY]).unwrap();
        let d = rips_persistence_oracle(&dist, 1, None);
        assert_eq!(
            bars(&d, 0),
            vec![(0.0, f64::INFINITY), (0.0, f64::INFINITY)]
        );
        assert_eq!(bars(&d, 1), vec![]);
    }

    #[test]
    fn single_point() {
        let dist = DistanceMatrix::from_points(&[vec![0.0, 0.0]]).unwrap();
        let d = rips_persistence_oracle(&dist, 2, None);
        assert_eq!(d.bars.len(), 1);
        assert_eq!(bars(&d, 0), vec![(0.0, f64::INFINITY)]);
    }

    #[test]
    fn zero_threshold_keeps_all_vertices() {
        let dist = DistanceMatrix::from_condensed(vec![1.0, 2.0, 3.0]).unwrap();
        let d = rips_persistence_oracle(&dist, 1, Some(0.0));
        assert_eq!(d.bars.len(), 3);
        assert!(d.bars.iter().all(|b| b.dim == 0 && b.is_essential()));
    }
}
