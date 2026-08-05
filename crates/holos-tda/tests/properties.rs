//! Property tests: invariances the barcode must satisfy regardless of the
//! reduction strategy, plus input edge cases.

use holos_tda::{rips_persistence, Diagram, DistanceMatrix, RipsParams};
use proptest::prelude::*;

fn canonical(d: &Diagram) -> Vec<(usize, f64, f64)> {
    let mut v: Vec<_> = d.bars.iter().map(|b| (b.dim, b.birth, b.death)).collect();
    v.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.total_cmp(&b.2))
    });
    v
}

fn compute(dist: &DistanceMatrix, max_dim: usize, threshold: Option<f64>) -> Diagram {
    let mut params = RipsParams::new(max_dim);
    params.threshold = threshold;
    rips_persistence(dist, &params).unwrap()
}

// Discrete values dominate, so ties are common. Ordering bugs hide in ties.
fn entry() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => prop::sample::select(vec![0.5, 1.0, 1.5, 2.0, 2.5]),
        1 => 0.1f64..3.0,
    ]
}

fn entry_with_inf() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => prop::sample::select(vec![0.5, 1.0, 2.0]),
        1 => Just(f64::INFINITY),
    ]
}

type Case = (usize, Vec<f64>, Vec<usize>, usize);

fn matrix_perm_and_dim() -> impl Strategy<Value = Case> {
    (2..=7usize).prop_flat_map(|n| {
        (
            Just(n),
            prop::collection::vec(entry(), n * (n - 1) / 2),
            Just((0..n).collect::<Vec<usize>>()).prop_shuffle(),
            0..=2usize,
        )
    })
}

fn permuted(n: usize, data: &[f64], perm: &[usize]) -> Vec<f64> {
    let mut out = vec![0.0; data.len()];
    for i in 1..n {
        for j in 0..i {
            let (a, b) = (perm[i].max(perm[j]), perm[i].min(perm[j]));
            out[i * (i - 1) / 2 + j] = data[a * (a - 1) / 2 + b];
        }
    }
    out
}

fn components(n: usize, dist: &DistanceMatrix, threshold: f64) -> usize {
    let mut seen = vec![false; n];
    let mut count = 0;
    for start in 0..n {
        if seen[start] {
            continue;
        }
        count += 1;
        let mut queue = vec![start];
        seen[start] = true;
        while let Some(u) = queue.pop() {
            #[allow(clippy::needless_range_loop)]
            for v in 0..n {
                let d = dist.get(u, v);
                if v != u && !seen[v] && d.is_finite() && d <= threshold {
                    seen[v] = true;
                    queue.push(v);
                }
            }
        }
    }
    count
}

proptest! {
    // The default config reads PROPTEST_CASES from the environment (256
    // otherwise). Stress runs can scale the budget without editing this file.
    #![proptest_config(ProptestConfig::default())]

    #[test]
    fn permutation_invariance((n, data, perm, max_dim) in matrix_perm_and_dim()) {
        let max_dim = max_dim.min(n - 2);
        let dist = DistanceMatrix::from_condensed(data.clone()).unwrap();
        let pdist = DistanceMatrix::from_condensed(permuted(n, &data, &perm)).unwrap();
        // Same entries reordered: the canonical multisets must match exactly.
        prop_assert_eq!(
            canonical(&compute(&dist, max_dim, None)),
            canonical(&compute(&pdist, max_dim, None))
        );
    }

    #[test]
    fn scaling_equivariance(
        (n, data, _, max_dim) in matrix_perm_and_dim(),
        scale in 0.1f64..8.0,
    ) {
        let max_dim = max_dim.min(n - 2);
        let dist = DistanceMatrix::from_condensed(data.clone()).unwrap();
        let scaled = DistanceMatrix::from_condensed(
            data.iter().map(|d| d * scale).collect()
        ).unwrap();
        let a = canonical(&compute(&dist, max_dim, None));
        let b = canonical(&compute(&scaled, max_dim, None));
        prop_assert_eq!(a.len(), b.len());
        for (&(dim_a, b_a, d_a), &(dim_b, b_b, d_b)) in a.iter().zip(&b) {
            prop_assert_eq!(dim_a, dim_b);
            for (x, y) in [(b_a, b_b), (d_a, d_b)] {
                if x.is_infinite() || y.is_infinite() {
                    prop_assert!(x.is_infinite() && y.is_infinite());
                } else {
                    prop_assert!((x * scale - y).abs() <= 1e-12 * (x * scale).abs().max(1.0));
                }
            }
        }
    }

    #[test]
    fn h0_essential_count_is_component_count(
        n in 2..=8usize,
        seed_data in prop::collection::vec(entry_with_inf(), 28),
        threshold in prop_oneof![Just(None), (0.5f64..3.0).prop_map(Some)],
    ) {
        let data = seed_data[..n * (n - 1) / 2].to_vec();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let effective = threshold.unwrap_or_else(|| dist.enclosing_radius());
        let diagram = compute(&dist, 0, threshold);
        let essential = diagram.in_dim(0).filter(|b| b.is_essential()).count();
        prop_assert_eq!(essential, components(n, &dist, effective));
        // Every finite dim-0 bar is born at 0.
        prop_assert!(diagram.in_dim(0).all(|b| b.birth == 0.0));
    }

    #[test]
    fn h0_component_count_monotone_in_threshold(
        n in 2..=8usize,
        seed_data in prop::collection::vec(entry_with_inf(), 28),
        t1 in 0.25f64..3.0,
        dt in 0.0f64..2.0,
    ) {
        let data = seed_data[..n * (n - 1) / 2].to_vec();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let t2 = t1 + dt;
        let essential = |t: f64| {
            compute(&dist, 0, Some(t))
                .in_dim(0)
                .filter(|b| b.is_essential())
                .count()
        };
        // Growing the threshold can only merge components.
        prop_assert!(essential(t1) >= essential(t2));
    }

    // The solver-facing shape of boundary-of-boundary. Build the oracle-style
    // filtration-ordered Z/2 boundary matrix from explicit vertex lists (no
    // combinadic code), then check D*D = 0. That means every included simplex
    // has all its faces included, and each codim-2 face cancels.
    #[test]
    fn boundary_matrix_squares_to_zero(
        n in 3..=7usize,
        seed_data in prop::collection::vec(entry(), 21),
        max_dim in 1..=3usize,
    ) {
        let data = seed_data[..n * (n - 1) / 2].to_vec();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let threshold = dist.enclosing_radius();

        let mut simplices: Vec<Vec<usize>> = Vec::new();
        for k in 1..=max_dim + 2 {
            subsets(n, k, &mut |verts| {
                let mut diam = 0.0f64;
                for (i, &u) in verts.iter().enumerate() {
                    for &v in &verts[i + 1..] {
                        diam = diam.max(dist.get(u, v));
                    }
                }
                if diam <= threshold {
                    simplices.push(verts.to_vec());
                }
            });
        }
        let position: std::collections::HashMap<&[usize], usize> = simplices
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_slice(), i))
            .collect();

        let boundary = |verts: &[usize]| -> Vec<usize> {
            let mut col = Vec::new();
            if verts.len() > 1 {
                for k in 0..verts.len() {
                    let mut face = verts.to_vec();
                    face.remove(k);
                    col.push(position[face.as_slice()]);
                }
            }
            col
        };

        for s in &simplices {
            let mut counts = std::collections::HashMap::new();
            for &face in &boundary(s) {
                for ff in boundary(&simplices[face]) {
                    *counts.entry(ff).or_insert(0usize) += 1;
                }
            }
            prop_assert!(
                counts.values().all(|&c| c % 2 == 0),
                "boundary of boundary of {s:?} nonzero mod 2"
            );
        }
    }
}

fn subsets(n: usize, k: usize, visit: &mut impl FnMut(&[usize])) {
    fn rec(
        start: usize,
        n: usize,
        k: usize,
        cur: &mut Vec<usize>,
        visit: &mut impl FnMut(&[usize]),
    ) {
        if cur.len() == k {
            visit(cur);
            return;
        }
        for v in start..n {
            cur.push(v);
            rec(v + 1, n, k, cur, visit);
            cur.pop();
        }
    }
    rec(0, n, k, &mut Vec::with_capacity(k), visit);
}

#[test]
fn empty_input_gives_empty_diagram() {
    let dist = DistanceMatrix::from_points(&[]).unwrap();
    let diagram = compute(&dist, 1, None);
    assert!(diagram.bars.is_empty());
}

#[test]
fn single_point_gives_one_essential_class() {
    let dist = DistanceMatrix::from_points(&[vec![1.0, 2.0]]).unwrap();
    let diagram = compute(&dist, 1, None);
    assert_eq!(canonical(&diagram), vec![(0, 0.0, f64::INFINITY)]);
}

#[test]
fn duplicate_points_give_one_essential_class_and_nothing_else() {
    let dist = DistanceMatrix::from_points(&[vec![3.0, 4.0], vec![3.0, 4.0]]).unwrap();
    let diagram = compute(&dist, 1, None);
    assert_eq!(canonical(&diagram), vec![(0, 0.0, f64::INFINITY)]);
}

#[test]
fn empty_space_still_validates_threshold() {
    let dist = DistanceMatrix::from_points(&[]).unwrap();
    let params = RipsParams::new(1).with_threshold(-1.0);
    assert!(rips_persistence(&dist, &params).is_err());
    let params = RipsParams::new(1).with_threshold(f64::NAN);
    assert!(rips_persistence(&dist, &params).is_err());
}
