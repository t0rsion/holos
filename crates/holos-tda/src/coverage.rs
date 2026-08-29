//! Exact relative coverage witnesses for fenced planar Rips complexes.
//!
//! The algebraic criterion follows the controlled-boundary theorem of de
//! Silva and Ghrist. A nonzero fence cycle must bound a two-chain in the
//! active Rips complex. Physical coverage of the domain depends on
//! [`PlanarCoverageModel`].

use std::collections::{BTreeMap, BTreeSet};

use num_rational::BigRational;

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, KineticEdgeKey, Result, SparseDistanceMatrix};

/// Resource limits for one relative coverage calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverageLimits {
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted active edge count.
    pub max_edges: usize,
    /// Largest accepted active triangle count.
    pub max_triangles: usize,
    /// Largest accepted dense linear-system entry count.
    pub max_matrix_entries: usize,
    /// Largest accepted state count.
    pub max_states: usize,
    /// Largest accepted action count.
    pub max_actions: usize,
    /// Largest failure set count checked in one plan evaluation.
    pub max_failure_sets: usize,
}

impl Default for CoverageLimits {
    fn default() -> Self {
        Self {
            max_vertices: 1_000_000,
            max_edges: 20_000_000,
            max_triangles: 1_000_000,
            max_matrix_entries: 100_000_000,
            max_states: 4_096,
            max_actions: 65_536,
            max_failure_sets: 10_000_000,
        }
    }
}

/// Declared geometric contract for the planar controlled-boundary theorem.
///
/// The constructor checks the exact radius inequality from assumption A2.
/// The caller declares A3: nodes lie in one compact connected planar domain.
/// The caller declares geometric A4: the fence cycle maps to that domain's
/// connected piecewise-linear boundary. The checker validates the graph,
/// unique labels, fence edges, radii, and relative chain. It cannot recover
/// the domain from connectivity data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanarCoverageModel {
    broadcast_radius: f64,
    sensing_radius: f64,
}

impl PlanarCoverageModel {
    /// Construct the controlled-boundary radius contract.
    ///
    /// Both radii must be finite and positive. The exact dyadic values must
    /// satisfy `3 * sensing_radius^2 >= broadcast_radius^2`.
    pub fn new(broadcast_radius: f64, sensing_radius: f64) -> Result<Self> {
        if !broadcast_radius.is_finite()
            || !sensing_radius.is_finite()
            || broadcast_radius <= 0.0
            || sensing_radius <= 0.0
        {
            return Err(Error::InvalidInput(
                "coverage radii must be finite and positive".into(),
            ));
        }
        let broadcast = rational(broadcast_radius);
        let sensing = rational(sensing_radius);
        if BigRational::from_integer(3.into()) * &sensing * sensing < broadcast.clone() * broadcast
        {
            return Err(Error::InvalidInput(
                "coverage radii violate 3 * sensing_radius^2 >= broadcast_radius^2".into(),
            ));
        }
        Ok(Self {
            broadcast_radius,
            sensing_radius,
        })
    }

    /// Radius used to construct the communication graph.
    pub fn broadcast_radius(self) -> f64 {
        self.broadcast_radius
    }

    /// Radius of each declared sensing disc.
    pub fn sensing_radius(self) -> f64 {
        self.sensing_radius
    }
}

/// A simple oriented fence cycle in global vertex labels.
///
/// Construction chooses one canonical rotation and orientation. The cycle
/// remains one-dimensional even when the Rips complex contains fence chords
/// or triangles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageFence {
    vertices: Vec<usize>,
}

impl CoverageFence {
    /// Construct a canonical fence from its cyclic vertex order.
    pub fn new(vertices: Vec<usize>) -> Result<Self> {
        if vertices.len() < 3 {
            return Err(Error::InvalidInput(
                "coverage fence requires at least three vertices".into(),
            ));
        }
        if vertices.iter().copied().collect::<BTreeSet<_>>().len() != vertices.len() {
            return Err(Error::InvalidInput(
                "coverage fence repeats a vertex".into(),
            ));
        }
        let forward = rotate_to_minimum(&vertices);
        let mut reversed = vertices;
        reversed.reverse();
        let reversed = rotate_to_minimum(&reversed);
        Ok(Self {
            vertices: forward.min(reversed),
        })
    }

    /// Canonical cyclic vertex order.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }

    fn edges(&self) -> Vec<KineticEdgeKey> {
        cycle_pairs(&self.vertices)
            .map(|(u, v)| KineticEdgeKey::new(u, v))
            .collect()
    }
}

/// One nonzero coefficient of a relative coverage two-chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverageTriangleTerm {
    /// First triangle vertex.
    pub a: usize,
    /// Second triangle vertex.
    pub b: usize,
    /// Third triangle vertex.
    pub c: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Exact result of one controlled-boundary criterion calculation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageEvaluation {
    /// True when the canonical fence cycle bounds in the active Rips complex.
    pub criterion_holds: bool,
    /// Canonical solution with every free variable set to zero.
    pub witness: Vec<CoverageTriangleTerm>,
    /// Active vertex count used by the calculation.
    pub active_vertices: usize,
    /// Active edge count used by the calculation.
    pub active_edges: usize,
    /// Active triangle count used by the calculation.
    pub active_triangles: usize,
}

/// Check the planar controlled-boundary criterion on an induced active graph.
///
/// `graph` contains every possible communication edge. `active_vertices`
/// selects the nodes present in this state. The fence must be active, and each
/// consecutive fence pair must be a graph edge.
pub fn evaluate_planar_coverage(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    fence: &CoverageFence,
    modulus: u32,
    model: PlanarCoverageModel,
    limits: CoverageLimits,
) -> Result<CoverageEvaluation> {
    validate_coverage_input(graph, active_vertices, fence, modulus, model, limits)?;
    let edges = active_edges(
        graph,
        active_vertices,
        model.broadcast_radius,
        limits.max_edges,
    )?;
    check_fence_edges(fence, &edges)?;
    let triangles = flag_triangles(active_vertices, &edges, limits.max_triangles)?;
    check_matrix_size(edges.len(), triangles.len(), limits.max_matrix_entries)?;
    let target = fence_chain(fence, &edges, modulus)?;
    let solution = solve_boundary(&edges, &triangles, &target, modulus);
    let criterion_holds = solution.is_some();
    let witness = coverage_witness(solution.unwrap_or_default(), &triangles);
    Ok(CoverageEvaluation {
        criterion_holds,
        witness,
        active_vertices: active_vertices.len(),
        active_edges: edges.len(),
        active_triangles: triangles.len(),
    })
}

fn validate_coverage_input(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    fence: &CoverageFence,
    modulus: u32,
    model: PlanarCoverageModel,
    limits: CoverageLimits,
) -> Result<()> {
    validate_field(modulus)?;
    PlanarCoverageModel::new(model.broadcast_radius, model.sensing_radius)?;
    if graph.len() > limits.max_vertices || graph.is_empty() {
        return Err(Error::InvalidInput(
            "coverage graph exceeds its vertex limit or is empty".into(),
        ));
    }
    if active_vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || active_vertices.iter().any(|vertex| *vertex >= graph.len())
    {
        return Err(Error::InvalidInput(
            "coverage active vertices are not canonical".into(),
        ));
    }
    if fence
        .vertices
        .iter()
        .any(|vertex| active_vertices.binary_search(vertex).is_err())
    {
        return Err(Error::InvalidInput(
            "coverage fence contains an inactive vertex".into(),
        ));
    }
    Ok(())
}

fn active_edges(
    graph: &SparseDistanceMatrix,
    active_vertices: &[usize],
    broadcast_radius: f64,
    maximum: usize,
) -> Result<Vec<KineticEdgeKey>> {
    let active: BTreeSet<_> = active_vertices.iter().copied().collect();
    let edges = graph
        .edges()
        .filter(|(u, v, distance)| {
            active.contains(u) && active.contains(v) && *distance <= broadcast_radius
        })
        .map(|(u, v, _)| KineticEdgeKey::new(u, v))
        .collect::<Vec<_>>();
    if edges.len() > maximum {
        return Err(Error::InvalidInput(
            "coverage active edges exceed their limit".into(),
        ));
    }
    Ok(edges)
}

fn check_fence_edges(fence: &CoverageFence, edges: &[KineticEdgeKey]) -> Result<()> {
    for edge in fence.edges() {
        if edges.binary_search(&edge).is_err() {
            return Err(Error::InvalidInput(
                "coverage graph omits a consecutive fence edge".into(),
            ));
        }
    }
    Ok(())
}

fn check_matrix_size(edges: usize, triangles: usize, maximum: usize) -> Result<()> {
    let entries = edges
        .checked_mul(triangles.saturating_add(1))
        .ok_or_else(|| Error::InvalidInput("coverage linear-system size overflows".into()))?;
    if entries > maximum {
        return Err(Error::InvalidInput(
            "coverage linear system exceeds its entry limit".into(),
        ));
    }
    Ok(())
}

fn coverage_witness(solution: Vec<u32>, triangles: &[[usize; 3]]) -> Vec<CoverageTriangleTerm> {
    solution
        .into_iter()
        .enumerate()
        .filter(|(_, coefficient)| *coefficient != 0)
        .map(|(index, coefficient)| {
            let [a, b, c] = triangles[index];
            CoverageTriangleTerm {
                a,
                b,
                c,
                coefficient,
            }
        })
        .collect()
}

fn rotate_to_minimum(values: &[usize]) -> Vec<usize> {
    let position = values
        .iter()
        .enumerate()
        .min_by_key(|(_, value)| **value)
        .map(|(position, _)| position)
        .unwrap_or(0);
    values[position..]
        .iter()
        .chain(&values[..position])
        .copied()
        .collect()
}

fn cycle_pairs(vertices: &[usize]) -> impl Iterator<Item = (usize, usize)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

fn validate_field(modulus: u32) -> Result<()> {
    if u64::from(modulus) >= MODULUS_LIMIT || !is_prime(u64::from(modulus)) {
        return Err(Error::InvalidInput(
            "coverage modulus must be a supported prime".into(),
        ));
    }
    Ok(())
}

fn flag_triangles(
    vertices: &[usize],
    edges: &[KineticEdgeKey],
    maximum: usize,
) -> Result<Vec<[usize; 3]>> {
    let edge_set: BTreeSet<_> = edges.iter().copied().collect();
    let mut triangles = Vec::new();
    for (first_position, &a) in vertices.iter().enumerate() {
        for (second_position, &b) in vertices.iter().enumerate().skip(first_position + 1) {
            if !edge_set.contains(&KineticEdgeKey::new(a, b)) {
                continue;
            }
            for &c in vertices.iter().skip(second_position + 1) {
                if edge_set.contains(&KineticEdgeKey::new(a, c))
                    && edge_set.contains(&KineticEdgeKey::new(b, c))
                {
                    triangles.push([a, b, c]);
                    if triangles.len() > maximum {
                        return Err(Error::InvalidInput(
                            "coverage active triangles exceed their limit".into(),
                        ));
                    }
                }
            }
        }
    }
    Ok(triangles)
}

fn fence_chain(fence: &CoverageFence, edges: &[KineticEdgeKey], modulus: u32) -> Result<Vec<u32>> {
    let positions: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect();
    let mut chain = vec![0u32; edges.len()];
    for (u, v) in cycle_pairs(&fence.vertices) {
        let position = positions[&KineticEdgeKey::new(u, v)];
        let coefficient = if u < v { 1 } else { modulus - 1 };
        chain[position] = add(chain[position], coefficient, modulus);
    }
    if chain.iter().all(|value| *value == 0) {
        return Err(Error::InvalidInput(
            "coverage fence cycle is zero in the declared field".into(),
        ));
    }
    Ok(chain)
}

fn solve_boundary(
    edges: &[KineticEdgeKey],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Option<Vec<u32>> {
    let mut rows = boundary_system(edges, triangles, target, modulus);
    let pivots = reduce_boundary_system(&mut rows, triangles.len(), modulus);
    if inconsistent_system(&rows, triangles.len()) {
        return None;
    }
    Some(boundary_solution(&rows, &pivots, triangles.len()))
}

fn boundary_system(
    edges: &[KineticEdgeKey],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Vec<Vec<u32>> {
    let positions: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect();
    let mut rows = vec![vec![0u32; triangles.len() + 1]; edges.len()];
    for (column, &[a, b, c]) in triangles.iter().enumerate() {
        rows[positions[&KineticEdgeKey::new(b, c)]][column] = 1;
        rows[positions[&KineticEdgeKey::new(a, c)]][column] = modulus - 1;
        rows[positions[&KineticEdgeKey::new(a, b)]][column] = 1;
    }
    for (row, value) in rows.iter_mut().zip(target) {
        row[triangles.len()] = *value;
    }
    rows
}

fn reduce_boundary_system(rows: &mut [Vec<u32>], columns: usize, modulus: u32) -> Vec<usize> {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..columns {
        let Some(found) = (pivot_row..rows.len()).find(|row| rows[*row][column] != 0) else {
            continue;
        };
        reduce_pivot_column(rows, pivot_row, found, column, modulus);
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    pivots
}

fn reduce_pivot_column(
    rows: &mut [Vec<u32>],
    pivot_row: usize,
    found: usize,
    column: usize,
    modulus: u32,
) {
    rows.swap(pivot_row, found);
    let inverse = inverse(rows[pivot_row][column], modulus);
    for value in &mut rows[pivot_row][column..] {
        *value = multiply(*value, inverse, modulus);
    }
    let pivot = rows[pivot_row][column..].to_vec();
    for (row_index, row) in rows.iter_mut().enumerate() {
        if row_index == pivot_row || row[column] == 0 {
            continue;
        }
        let factor = row[column];
        for (value, pivot_value) in row[column..].iter_mut().zip(&pivot) {
            *value = subtract(*value, multiply(factor, *pivot_value, modulus), modulus);
        }
    }
}

fn inconsistent_system(rows: &[Vec<u32>], columns: usize) -> bool {
    rows.iter()
        .any(|row| row[..columns].iter().all(|value| *value == 0) && row[columns] != 0)
}

fn boundary_solution(rows: &[Vec<u32>], pivots: &[usize], columns: usize) -> Vec<u32> {
    let mut solution = vec![0u32; columns];
    for (row, &column) in pivots.iter().enumerate() {
        solution[column] = rows[row][columns];
    }
    solution
}

fn add(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(modulus)) as u32
}

fn subtract(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(modulus) - u64::from(right)) % u64::from(modulus)) as u32
}

fn multiply(left: u32, right: u32, modulus: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(modulus)) as u32
}

fn inverse(value: u32, modulus: u32) -> u32 {
    let mut result = 1u64;
    let mut base = u64::from(value);
    let mut exponent = u64::from(modulus - 2);
    let modulus = u64::from(modulus);
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> PlanarCoverageModel {
        PlanarCoverageModel::new(1.0, 1.0).unwrap()
    }

    fn wheel() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 1.0),
                (1, 4, 1.0),
                (2, 4, 1.0),
                (3, 4, 1.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn a_wheel_fills_its_fence_over_several_fields() {
        let fence = CoverageFence::new(vec![0, 1, 2, 3]).unwrap();
        for modulus in [2, 3, 5] {
            let evaluation = evaluate_planar_coverage(
                &wheel(),
                &[0, 1, 2, 3, 4],
                &fence,
                modulus,
                model(),
                CoverageLimits::default(),
            )
            .unwrap();
            assert!(evaluation.criterion_holds);
            assert_eq!(evaluation.active_triangles, 4);
            assert_eq!(evaluation.witness.len(), 4);
        }
    }

    #[test]
    fn the_unfilled_fence_fails_the_criterion() {
        let evaluation = evaluate_planar_coverage(
            &wheel(),
            &[0, 1, 2, 3],
            &CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            2,
            model(),
            CoverageLimits::default(),
        )
        .unwrap();
        assert!(!evaluation.criterion_holds);
        assert!(evaluation.witness.is_empty());
    }

    #[test]
    fn fence_order_has_one_canonical_orientation() {
        let expected = CoverageFence::new(vec![0, 1, 2, 3]).unwrap();
        assert_eq!(expected, CoverageFence::new(vec![2, 3, 0, 1]).unwrap());
        assert_eq!(expected, CoverageFence::new(vec![2, 1, 0, 3]).unwrap());
    }

    #[test]
    fn radius_inequality_uses_exact_dyadic_values() {
        assert!(PlanarCoverageModel::new(1.0, 0.5).is_err());
        assert!(PlanarCoverageModel::new(1.0, 0.6).is_ok());
    }

    #[test]
    fn a_missing_fence_edge_is_rejected() {
        let graph =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap();
        assert!(
            evaluate_planar_coverage(
                &graph,
                &[0, 1, 2, 3],
                &CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
                2,
                model(),
                CoverageLimits::default(),
            )
            .is_err()
        );
    }
}
