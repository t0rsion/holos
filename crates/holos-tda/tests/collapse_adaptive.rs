//! Correctness gates for the adaptive version 3 collapse schedule.

use holos_tda::collapse::verify::{verify_dense, verify_sparse};
use holos_tda::collapse::{
    AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective, collapse_dense_adaptive,
    collapse_sparse_adaptive,
};
use holos_tda::{
    Bar, DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence,
    rips_persistence_sparse,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn canonical(mut bars: Vec<Bar>) -> Vec<(usize, u64, u64)> {
    bars.sort_by(|a, b| {
        a.dim
            .cmp(&b.dim)
            .then(a.birth.total_cmp(&b.birth))
            .then(a.death.total_cmp(&b.death))
    });
    bars.into_iter()
        .map(|bar| (bar.dim, bar.birth.to_bits(), bar.death.to_bits()))
        .collect()
}

fn sparse_from_dense(dist: &DistanceMatrix) -> SparseDistanceMatrix {
    let mut edges = Vec::new();
    for u in 0..dist.len() {
        for v in u + 1..dist.len() {
            let value = dist.get(u, v);
            if value.is_finite() {
                edges.push((u, v, value));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(dist.len(), &edges).unwrap()
}

fn random_graph(rng: &mut Rng, n: usize) -> DistanceMatrix {
    let mut condensed = Vec::with_capacity(n * n.saturating_sub(1) / 2);
    for v in 1..n {
        for _u in 0..v {
            let draw = rng.next();
            let value = if draw % 7 == 0 {
                f64::INFINITY
            } else {
                (draw % 4) as f64
            };
            condensed.push(value);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

#[test]
fn random_complete_and_partial_certificates_verify() {
    let mut rng = Rng(0xa19d_70c3_2e51_8b47);
    for case in 0..80 {
        let dense = random_graph(&mut rng, 3 + case % 8);
        let sparse = sparse_from_dense(&dense);
        for objective in [CollapseObjective::H1, CollapseObjective::H2] {
            for work_limit in [None, Some(0), Some(1), Some(7), Some(31)] {
                let mut params = AdaptiveCollapseParams::new(objective);
                params.work_limit = work_limit;
                let a = collapse_dense_adaptive(&dense, Some(3.0), params).unwrap();
                let b = collapse_sparse_adaptive(&sparse, Some(3.0), params).unwrap();
                verify_dense(&dense, Some(3.0), &a).unwrap();
                verify_sparse(&sparse, Some(3.0), &b).unwrap();
                assert_eq!(
                    a.matrix.edges().collect::<Vec<_>>(),
                    b.matrix.edges().collect::<Vec<_>>()
                );
                assert_eq!(a.certificate, b.certificate);
                assert_eq!(a.stats, b.stats);
                assert!(
                    a.certificate
                        .work_limit()
                        .is_none_or(|limit| { a.certificate.work_used() <= limit })
                );
                if a.certificate.completeness() == CollapseCompleteness::BudgetLimited {
                    assert_eq!(a.certificate.work_used(), work_limit.unwrap());
                }
            }
        }
    }
}

#[test]
fn complete_adaptive_runs_preserve_h1_and_h2_diagrams() {
    let mut rng = Rng(0xc833_1f02_46ba_9de5);
    for case in 0..48 {
        let dense = random_graph(&mut rng, 4 + case % 7);
        let sparse = sparse_from_dense(&dense);
        let base_params = RipsParams::new(2).with_threshold(3.0);
        let expected = rips_persistence(&dense, &base_params).unwrap();
        for objective in [CollapseObjective::H1, CollapseObjective::H2] {
            let collapsed = collapse_sparse_adaptive(
                &sparse,
                Some(3.0),
                AdaptiveCollapseParams::new(objective),
            )
            .unwrap();
            let mut reduced_params = base_params.clone();
            reduced_params.threshold = Some(collapsed.certificate.terminal_level());
            let got = rips_persistence_sparse(&collapsed.matrix, &reduced_params).unwrap();
            assert_eq!(
                canonical(got.bars),
                canonical(expected.bars.clone()),
                "case {case}, objective {objective:?}"
            );
        }
    }
}

#[test]
fn pipeline_matches_the_standalone_adaptive_path() {
    let dense = DistanceMatrix::from_condensed(vec![1.0; 15]).unwrap();
    let adaptive = AdaptiveCollapseParams::new(CollapseObjective::H2);
    let pipeline = rips_persistence(
        &dense,
        &RipsParams::new(2)
            .with_threads(3)
            .with_adaptive_collapse(adaptive),
    )
    .unwrap();
    let collapsed = collapse_dense_adaptive(&dense, None, adaptive).unwrap();
    let standalone = rips_persistence_sparse(
        &collapsed.matrix,
        &RipsParams::new(2)
            .with_threads(3)
            .with_threshold(collapsed.certificate.terminal_level()),
    )
    .unwrap();
    assert_eq!(canonical(pipeline.bars), canonical(standalone.bars));
}

#[test]
fn reruns_are_field_for_field_identical() {
    let dense = DistanceMatrix::from_condensed(vec![1.0; 28]).unwrap();
    let params = AdaptiveCollapseParams::default();
    let first = collapse_dense_adaptive(&dense, None, params).unwrap();
    for _ in 0..8 {
        let next = collapse_dense_adaptive(&dense, None, params).unwrap();
        assert_eq!(
            first.matrix.edges().collect::<Vec<_>>(),
            next.matrix.edges().collect::<Vec<_>>()
        );
        assert_eq!(first.certificate, next.certificate);
        assert_eq!(first.stats, next.stats);
    }
}
