//! Exact degree-Rips construction on a finite parameter grid.

use crate::filtration::{FilteredSimplicialComplex, FlagComplexParams};
use crate::{Error, Result, SparseDistanceMatrix};

use super::{
    BifiltrationLimits, Bigrade, BirthAntichain, MulticriticalBifiltration, MulticriticalSimplex,
};

/// Choices for exact degree-Rips construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DegreeRipsParams {
    /// Highest requested homology dimension.
    ///
    /// The constructor materializes one additional simplex dimension.
    pub max_homology_dimension: usize,
    /// Largest included edge weight. `None` includes every listed edge.
    pub threshold: Option<f64>,
    /// Grid, birth, and simplex limits.
    pub limits: BifiltrationLimits,
}

impl Default for DegreeRipsParams {
    fn default() -> Self {
        Self {
            max_homology_dimension: 1,
            threshold: None,
            limits: BifiltrationLimits::default(),
        }
    }
}

/// Exact multicritical degree-Rips bifiltration of a listed graph.
#[derive(Debug, Clone)]
pub struct DegreeRipsBifiltration {
    source: SparseDistanceMatrix,
    threshold: f64,
    max_homology_dimension: usize,
    bifiltration: MulticriticalBifiltration,
}

impl DegreeRipsBifiltration {
    /// Construct degree-Rips from finite non-negative listed edge weights.
    ///
    /// At `(r, k)`, the value is the flag complex of the threshold graph at
    /// `r`, restricted to vertices whose degree in that threshold graph is at
    /// least `k`.
    pub fn from_graph(input: &SparseDistanceMatrix, params: DegreeRipsParams) -> Result<Self> {
        let threshold = validate_degree_rips_params(input, params)?;
        let scales = degree_rips_scales(input, threshold, params.limits)?;
        let minimum_degrees = degree_levels(input.len());
        Self::from_validated_grid(input, scales, minimum_degrees, params)
    }

    /// Construct degree-Rips on one declared finite parameter grid.
    ///
    /// `scales` must increase strictly. Its last value is the graph threshold.
    /// `minimum_degrees` must decrease strictly and end at zero. Each stored
    /// slice is exact at its declared parameters. The grid can omit critical
    /// values between adjacent entries.
    pub fn from_graph_on_grid(
        input: &SparseDistanceMatrix,
        scales: Vec<f64>,
        minimum_degrees: Vec<usize>,
        params: DegreeRipsParams,
    ) -> Result<Self> {
        let Some(&threshold) = scales.last() else {
            return Err(Error::InvalidInput(
                "a degree-Rips grid needs at least one scale".into(),
            ));
        };
        validate_degree_rips_params(input, params)?;
        if params
            .threshold
            .is_some_and(|value| value.to_bits() != threshold.to_bits())
        {
            return Err(Error::InvalidInput(
                "a degree-Rips grid threshold must equal its last scale".into(),
            ));
        }
        if minimum_degrees.last().copied() != Some(0) {
            return Err(Error::InvalidInput(
                "a degree-Rips grid must include minimum degree zero".into(),
            ));
        }
        Self::from_validated_grid(input, scales, minimum_degrees, params)
    }

    fn from_validated_grid(
        input: &SparseDistanceMatrix,
        scales: Vec<f64>,
        minimum_degrees: Vec<usize>,
        params: DegreeRipsParams,
    ) -> Result<Self> {
        let threshold = *scales
            .last()
            .ok_or_else(|| Error::InvalidInput("a degree-Rips grid has no scale".into()))?;
        let degrees = degree_table(input, &scales);
        let labels = (0..input.len()).collect::<Vec<_>>();
        let max_simplex_dimension = params
            .max_homology_dimension
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("degree-Rips dimension overflows".into()))?;
        let flag = FilteredSimplicialComplex::from_flag_graph(
            input,
            &labels,
            FlagComplexParams {
                max_dimension: max_simplex_dimension,
                threshold: Some(threshold),
                limits: params.limits.complex,
            },
        )
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let simplices = flag
            .simplices()
            .iter()
            .map(|dimension| {
                dimension
                    .iter()
                    .map(|simplex| {
                        let births = degree_rips_births(
                            input,
                            simplex.vertices(),
                            &scales,
                            &degrees,
                            &minimum_degrees,
                        )?;
                        Ok(MulticriticalSimplex::new(
                            simplex.vertices().to_vec(),
                            births,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let bifiltration = MulticriticalBifiltration::new(
            input.len(),
            scales,
            minimum_degrees,
            simplices,
            params.limits,
        )?;
        Ok(Self {
            source: input.clone(),
            threshold,
            max_homology_dimension: params.max_homology_dimension,
            bifiltration,
        })
    }

    /// Original listed weighted graph.
    pub fn source(&self) -> &SparseDistanceMatrix {
        &self.source
    }

    /// Largest included edge weight.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// Highest requested homology dimension.
    pub fn max_homology_dimension(&self) -> usize {
        self.max_homology_dimension
    }

    /// Native antichain representation.
    pub fn bifiltration(&self) -> &MulticriticalBifiltration {
        &self.bifiltration
    }
}

fn validate_degree_rips_params(
    input: &SparseDistanceMatrix,
    params: DegreeRipsParams,
) -> Result<f64> {
    let threshold = params
        .threshold
        .unwrap_or_else(|| input.edges().map(|edge| edge.2).fold(0.0f64, f64::max));
    if !threshold.is_finite() || threshold < 0.0 {
        return Err(Error::InvalidInput(
            "degree-Rips threshold must be finite and non-negative".into(),
        ));
    }
    if input.len() > params.limits.complex.max_vertices {
        return Err(Error::InvalidInput(
            "degree-Rips vertex count exceeds its limit".into(),
        ));
    }
    Ok(threshold)
}

fn degree_rips_scales(
    input: &SparseDistanceMatrix,
    threshold: f64,
    limits: BifiltrationLimits,
) -> Result<Vec<f64>> {
    let mut bits = input
        .edges()
        .filter(|edge| edge.2 <= threshold)
        .map(|edge| {
            let value = if edge.2 == 0.0 { 0.0 } else { edge.2 };
            value.to_bits()
        })
        .collect::<Vec<_>>();
    bits.push(0.0f64.to_bits());
    bits.sort_unstable_by(|left, right| f64::from_bits(*left).total_cmp(&f64::from_bits(*right)));
    bits.dedup();
    if bits.len() > limits.max_scales {
        return Err(Error::InvalidInput(
            "degree-Rips scale count exceeds its limit".into(),
        ));
    }
    Ok(bits.into_iter().map(f64::from_bits).collect())
}

fn degree_levels(vertex_count: usize) -> Vec<usize> {
    (0..=vertex_count.saturating_sub(1)).rev().collect()
}

fn degree_table(input: &SparseDistanceMatrix, scales: &[f64]) -> Vec<Vec<usize>> {
    scales
        .iter()
        .map(|&scale| {
            let mut degrees = vec![0usize; input.len()];
            for (u, v, value) in input.edges() {
                if value <= scale {
                    degrees[u] += 1;
                    degrees[v] += 1;
                }
            }
            degrees
        })
        .collect()
}

fn degree_rips_births(
    input: &SparseDistanceMatrix,
    simplex: &[usize],
    scales: &[f64],
    degrees: &[Vec<usize>],
    minimum_degrees: &[usize],
) -> Result<BirthAntichain> {
    let diameter = simplex_diameter(input, simplex);
    let candidates = scales
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, scale)| *scale >= diameter)
        .map(|(scale, _)| {
            let minimum = simplex
                .iter()
                .map(|&vertex| degrees[scale][vertex])
                .min()
                .unwrap_or(0);
            let density = minimum_degrees
                .iter()
                .position(|&required| required <= minimum)
                .expect("minimum degree zero admits every simplex");
            Bigrade::new(scale, density)
        });
    BirthAntichain::from_candidates(candidates)
}

fn simplex_diameter(input: &SparseDistanceMatrix, simplex: &[usize]) -> f64 {
    let mut diameter = 0.0f64;
    for left in 0..simplex.len() {
        for right in left + 1..simplex.len() {
            diameter = diameter.max(input.get(simplex[left], simplex[right]));
        }
    }
    diameter
}
