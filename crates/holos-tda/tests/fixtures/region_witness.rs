use holos_tda::{Diagram, ReductionGuard, RipsParams, SparseDistanceMatrix};

#[cfg(test)]
use holos_tda::Bar;

pub const VERTEX_COUNT: usize = 7;
pub const MODULUS: u32 = 3;
pub const INITIAL_EDGES: [(usize, usize, f64); 8] = [
    (0, 1, 1.0),
    (1, 2, 2.0),
    (2, 3, 3.0),
    (0, 3, 4.0),
    (0, 4, 5.0),
    (4, 5, 6.0),
    (5, 6, 7.0),
    (0, 6, 8.0),
];
pub const UPDATED_EDGES: [(usize, usize, f64); 8] = [
    (0, 1, 1.0),
    (1, 2, 2.0),
    (2, 3, 3.0),
    (0, 3, 5.0),
    (0, 4, 4.0),
    (4, 5, 6.0),
    (5, 6, 7.0),
    (0, 6, 8.0),
];
pub const REVERSED_FIRST: [usize; 2] = [0, 3];
pub const REVERSED_SECOND: [usize; 2] = [0, 4];

pub fn initial_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(VERTEX_COUNT, &INITIAL_EDGES).unwrap()
}

pub fn updated_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(VERTEX_COUNT, &UPDATED_EDGES).unwrap()
}

pub fn params() -> RipsParams {
    RipsParams::new(1).with_modulus(MODULUS)
}

pub fn diagram_bits_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

pub fn guards_hold(graph: &SparseDistanceMatrix, guards: &[ReductionGuard]) -> bool {
    guards.iter().all(|guard| {
        let earlier = simplex_value(graph, guard.earlier().vertices());
        let later = simplex_value(graph, guard.later().vertices());
        let order = earlier.total_cmp(&later).then_with(|| {
            simplex_rank(guard.later().vertices()).cmp(&simplex_rank(guard.earlier().vertices()))
        });
        !order.is_gt()
    })
}

fn simplex_value(graph: &SparseDistanceMatrix, vertices: &[usize]) -> f64 {
    match vertices {
        [_] => 0.0,
        [u, v] => graph.get(*u, *v),
        [u, v, w] => graph
            .get(*u, *v)
            .max(graph.get(*u, *w))
            .max(graph.get(*v, *w)),
        _ => f64::INFINITY,
    }
}

fn simplex_rank(vertices: &[usize]) -> u128 {
    match vertices {
        [u] => *u as u128,
        [u, v] => edge_rank(*u, *v),
        [u, v, w] => {
            *u as u128
                + (*v as u128) * (*v as u128 - 1) / 2
                + (*w as u128) * (*w as u128 - 1) * (*w as u128 - 2) / 6
        }
        _ => u128::MAX,
    }
}

fn edge_rank(u: usize, v: usize) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

#[cfg(test)]
pub fn expected_initial_bars() -> Vec<Bar> {
    expected_bars([1.0, 2.0, 3.0, 5.0, 6.0, 7.0], [4.0, 8.0])
}

#[cfg(test)]
pub fn expected_updated_bars() -> Vec<Bar> {
    expected_bars([1.0, 2.0, 3.0, 4.0, 6.0, 7.0], [5.0, 8.0])
}

#[cfg(test)]
fn expected_bars(h0_deaths: [f64; 6], h1_births: [f64; 2]) -> Vec<Bar> {
    let mut bars: Vec<_> = h0_deaths
        .into_iter()
        .map(|death| Bar {
            dim: 0,
            birth: 0.0,
            death,
        })
        .collect();
    bars.push(Bar {
        dim: 0,
        birth: 0.0,
        death: f64::INFINITY,
    });
    bars.extend(h1_births.into_iter().map(|birth| Bar {
        dim: 1,
        birth,
        death: f64::INFINITY,
    }));
    bars
}
