use std::collections::BTreeMap;

use num_rational::BigRational;

use crate::cohomology::{Edge, MapTerm};

/// Summary of one checked kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedKineticZigzag {
    /// Target cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Affine edge trajectory count.
    pub edges: usize,
    /// Alternating open-cell and exact-event node count.
    pub nodes: usize,
    /// Exact restriction arrow count.
    pub arrows: usize,
    /// Nonzero interval-isotypic space count.
    pub intervals: usize,
    /// Sum of all interval multiplicities.
    pub interval_copies: usize,
}

pub(crate) struct KineticClaim {
    pub(crate) vertex_count: usize,
    pub(crate) trajectories: Vec<AffineEdge>,
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) dimension: usize,
    pub(crate) scale: f64,
    pub(crate) modulus: u32,
    pub(crate) persistent_ties: usize,
    pub(crate) node_ranks: Vec<usize>,
    pub(crate) node_edges: Vec<usize>,
    pub(crate) arrow_ranks: Vec<usize>,
    pub(crate) generalized_ranks: Vec<usize>,
    pub(crate) intervals: Vec<Interval>,
}

pub(crate) struct KineticHeader {
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) dimension: usize,
    pub(crate) scale: f64,
    pub(crate) modulus: u32,
    pub(crate) persistent_ties: usize,
}

pub(crate) struct KineticOutput {
    pub(crate) node_ranks: Vec<usize>,
    pub(crate) node_edges: Vec<usize>,
    pub(crate) arrow_ranks: Vec<usize>,
    pub(crate) generalized_ranks: Vec<usize>,
    pub(crate) intervals: Vec<Interval>,
}

#[derive(Clone)]
pub(crate) struct AffineEdge {
    pub(crate) edge: Edge,
    pub(crate) intercept: f64,
    pub(crate) velocity: f64,
}

pub(crate) struct ExactSchedule {
    pub(crate) events: Vec<BigRational>,
    pub(crate) persistent_ties: usize,
}

pub(crate) struct Map {
    pub(crate) forward: bool,
    pub(crate) columns: Vec<Vec<MapTerm>>,
    pub(crate) rank: usize,
}

pub(crate) type Interval = (usize, usize, usize);

#[derive(Clone, Default)]
pub(crate) struct Vector(pub(crate) BTreeMap<usize, u32>);

impl Vector {
    pub(crate) fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    pub(crate) fn leading(&self) -> Option<(usize, u32)> {
        self.0.first_key_value().map(|(&key, &value)| (key, value))
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&position, &coefficient) in &source.0 {
            let current = u64::from(self.0.get(&position).copied().unwrap_or(0));
            let next = (current + factor * u64::from(coefficient)) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next as u32);
            }
        }
    }

    pub(crate) fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
}

pub(crate) const FORMAT_MAX_NODES: usize = 2_049;
pub(crate) const FORMAT_MAX_RANK_WORK: usize = 100_000_000;
