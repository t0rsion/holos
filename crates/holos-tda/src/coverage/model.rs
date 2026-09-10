use std::collections::BTreeSet;

use num_rational::BigRational;

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, KineticEdgeKey, Result};

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

    pub(crate) fn edges(&self) -> Vec<KineticEdgeKey> {
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

pub(crate) fn cycle_pairs(vertices: &[usize]) -> impl Iterator<Item = (usize, usize)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
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

pub(crate) fn validate_field(modulus: u32) -> Result<()> {
    if u64::from(modulus) >= MODULUS_LIMIT || !is_prime(u64::from(modulus)) {
        return Err(Error::InvalidInput(
            "coverage modulus must be a supported prime".into(),
        ));
    }
    Ok(())
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}
