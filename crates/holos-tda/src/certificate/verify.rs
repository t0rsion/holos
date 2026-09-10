//! Certificate validation and diagram reconstruction.

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Bar, CriticalPair, CriticalSimplex, Diagram, RipsParams, SparseDistanceMatrix};

use super::model::{
    CertificateError, CertificateLimits, CertificateResult, ChangeColumn, ReductionCertificate,
};
use super::reduction::{FilteredComplex, FilteredTriangle, SparseColumn};
use super::region::critical_pair_record_order;
use super::wire::{check_bars, check_change_column, check_change_columns};

mod guards;

pub(super) use guards::{add_change_guards, add_pivot_guards, minimize_guards};

impl ReductionCertificate {
    /// Verify the change of basis, distinct pivots, and derived diagram.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<Diagram, CertificateError> {
        Ok(self.verify_checked(input, limits)?.diagram)
    }
    fn verify_checked(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<CheckedReductions, CertificateError> {
        Ok(self.verify_parts(input, limits)?.1)
    }

    pub(super) fn verify_parts(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<(FilteredComplex, CheckedReductions), CertificateError> {
        let params = RipsParams::new(1).with_modulus(self.modulus);
        validate_header(input, params.max_dim, params.modulus, limits)?;
        if input.len() != self.vertex_count {
            return Err(CertificateError::new(format!(
                "input has {} vertices, certificate records {}",
                input.len(),
                self.vertex_count
            )));
        }
        let threshold = checked_threshold(self.threshold)?;
        if graph_digest(input, threshold) != self.graph_digest {
            return Err(CertificateError::new(
                "thresholded graph binding does not match",
            ));
        }
        let complex = FilteredComplex::build(input, threshold, limits)?;
        let checked = check_reductions(
            &complex,
            self.modulus,
            &self.edge_columns,
            &self.triangle_columns,
            limits,
        )?;
        if !diagram_bits_equal(&checked.diagram, &self.diagram) {
            return Err(CertificateError::new(
                "recorded diagram differs from the checked reduction",
            ));
        }
        Ok((complex, checked))
    }

    pub(crate) fn verify_with_h1_critical_pairs(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<(Diagram, Vec<(Bar, CriticalPair)>), CertificateError> {
        let checked = self.verify_checked(input, limits)?;
        Ok((checked.diagram, checked.h1_pairs))
    }

    pub(super) fn check_envelope(
        &self,
        limits: CertificateLimits,
    ) -> std::result::Result<(), CertificateError> {
        self.check_envelope_header(limits)?;
        let mut total_terms = 0usize;
        check_change_columns(
            "edge",
            &self.edge_columns,
            self.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        check_change_columns(
            "triangle",
            &self.triangle_columns,
            self.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        check_bars(&self.diagram, limits.max_bars)
    }

    fn check_envelope_header(&self, limits: CertificateLimits) -> CertificateResult<()> {
        if self.vertex_count > limits.max_vertices {
            return Err(CertificateError::new(format!(
                "{} vertices exceed the limit {}",
                self.vertex_count, limits.max_vertices
            )));
        }
        if !is_prime(self.modulus as u64) || self.modulus as u64 >= MODULUS_LIMIT {
            return Err(CertificateError::new(format!(
                "modulus must be a prime below {MODULUS_LIMIT}, got {}",
                self.modulus
            )));
        }
        checked_threshold(self.threshold)?;
        if self.edge_columns.len() > limits.max_edges {
            return Err(CertificateError::new(format!(
                "{} edge columns exceed the limit {}",
                self.edge_columns.len(),
                limits.max_edges
            )));
        }
        if self.triangle_columns.len() > limits.max_triangles {
            return Err(CertificateError::new(format!(
                "{} triangle columns exceed the limit {}",
                self.triangle_columns.len(),
                limits.max_triangles
            )));
        }
        Ok(())
    }
}
pub(super) struct CheckedReductions {
    pub(super) diagram: Diagram,
    pub(super) h1_pairs: Vec<(Bar, CriticalPair)>,
    pub(super) reduced_edges: Vec<SparseColumn>,
    pub(super) reduced_triangles: Vec<SparseColumn>,
}

pub(super) fn check_reductions(
    complex: &FilteredComplex,
    modulus: u32,
    edge_columns: &[ChangeColumn],
    triangle_columns: &[ChangeColumn],
    limits: CertificateLimits,
) -> std::result::Result<CheckedReductions, CertificateError> {
    let edge_boundaries = complex.edge_boundaries(modulus);
    let triangle_boundaries = complex.triangle_boundaries(modulus);
    let reduced_edges = check_matrix("edge", &edge_boundaries, edge_columns, modulus, limits)?;
    let reduced_triangles = check_matrix(
        "triangle",
        &triangle_boundaries,
        triangle_columns,
        modulus,
        limits,
    )?;
    let mut diagram = checked_h0_diagram(complex, &reduced_edges);
    let h1_deaths = h1_death_simplices(complex, &reduced_triangles);
    let h1_pairs = checked_h1_pairs(complex, &reduced_edges, &h1_deaths, &mut diagram);
    diagram.canonicalize();
    let mut h1_pairs = h1_pairs;
    h1_pairs.sort_by(critical_pair_record_order);
    Ok(CheckedReductions {
        diagram,
        h1_pairs,
        reduced_edges,
        reduced_triangles,
    })
}

fn checked_h0_diagram(complex: &FilteredComplex, reduced_edges: &[SparseColumn]) -> Diagram {
    let mut diagram = Diagram::default();
    let mut killed_vertices = vec![false; complex.vertex_count];
    for (column, reduced) in reduced_edges.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            killed_vertices[pivot] = true;
            let death = complex.edges[column].value;
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
    }
    for killed in killed_vertices {
        if !killed {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
    }
    diagram
}

fn h1_death_simplices<'a>(
    complex: &'a FilteredComplex,
    reduced_triangles: &[SparseColumn],
) -> FxHashMap<usize, &'a FilteredTriangle> {
    let mut h1_deaths = FxHashMap::default();
    for (column, reduced) in reduced_triangles.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            h1_deaths.insert(pivot, &complex.triangles[column]);
        }
    }
    h1_deaths
}

fn checked_h1_pairs(
    complex: &FilteredComplex,
    reduced_edges: &[SparseColumn],
    h1_deaths: &FxHashMap<usize, &FilteredTriangle>,
    diagram: &mut Diagram,
) -> Vec<(Bar, CriticalPair)> {
    let mut h1_pairs = Vec::new();
    for (edge, reduced) in reduced_edges.iter().enumerate() {
        if !reduced.0.is_empty() {
            continue;
        }
        let birth = complex.edges[edge].value;
        let death_simplex = h1_deaths.get(&edge).copied();
        let death = death_simplex.map_or(f64::INFINITY, |triangle| triangle.value);
        if death > birth {
            let interval = Bar {
                dim: 1,
                birth,
                death,
            };
            diagram.bars.push(interval);
            h1_pairs.push((
                interval,
                CriticalPair {
                    birth: CriticalSimplex {
                        vertices: complex.edges[edge].vertices.to_vec(),
                        value: birth,
                    },
                    death: death_simplex.map(|triangle| CriticalSimplex {
                        vertices: triangle.vertices.to_vec(),
                        value: triangle.value,
                    }),
                },
            ));
        }
    }
    h1_pairs
}

fn check_matrix(
    label: &str,
    boundaries: &[SparseColumn],
    columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<Vec<SparseColumn>, CertificateError> {
    if columns.len() != boundaries.len() {
        return Err(CertificateError::new(format!(
            "{label} matrix has {} columns, certificate records {}",
            boundaries.len(),
            columns.len()
        )));
    }
    let modulus64 = modulus as u64;
    let mut total_terms = 0usize;
    let mut reduced = Vec::with_capacity(columns.len());
    let mut pivots = FxHashMap::default();
    for (column_index, transform) in columns.iter().enumerate() {
        add_matrix_terms(
            &mut total_terms,
            transform.terms.len(),
            limits.max_terms,
            label,
        )?;
        let result = checked_matrix_column(
            label,
            column_index,
            transform,
            boundaries,
            modulus,
            modulus64,
        )?;
        if let Some((pivot, _)) = result.pivot() {
            if let Some(previous_column) = pivots.insert(pivot, column_index) {
                return Err(CertificateError::new(format!(
                    "{label} reduced columns {previous_column} and {column_index} share pivot {pivot}"
                )));
            }
        }
        reduced.push(result);
    }
    Ok(reduced)
}

fn add_matrix_terms(
    total: &mut usize,
    count: usize,
    maximum: usize,
    label: &str,
) -> CertificateResult<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("certificate term count overflows usize"))?;
    if *total > maximum {
        return Err(CertificateError::new(format!(
            "{} {label} terms exceed the limit {maximum}",
            *total
        )));
    }
    Ok(())
}

fn checked_matrix_column(
    label: &str,
    column_index: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u32,
    modulus64: u64,
) -> CertificateResult<SparseColumn> {
    check_change_column(label, column_index, transform, modulus)?;
    let mut result = SparseColumn::default();
    for term in &transform.terms {
        result.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
    }
    Ok(result)
}

pub(super) fn validate_header(
    input: &SparseDistanceMatrix,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<(), CertificateError> {
    if max_dim != 1 {
        return Err(CertificateError::new(
            "an algebraic certificate requires max_dim equal to 1",
        ));
    }
    if input.len() > limits.max_vertices {
        return Err(CertificateError::new(format!(
            "{} vertices exceed the limit {}",
            input.len(),
            limits.max_vertices
        )));
    }
    if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
        return Err(CertificateError::new(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )));
    }
    Ok(())
}

pub(super) fn checked_threshold(
    threshold: Option<f64>,
) -> std::result::Result<f64, CertificateError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(CertificateError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

pub(super) fn graph_digest(input: &SparseDistanceMatrix, threshold: f64) -> [u8; 32] {
    let edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((input.len() as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for (u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

pub(super) fn triangle_rank([u, v, w]: [usize; 3]) -> u128 {
    let choose2 = v as u128 * (v.saturating_sub(1)) as u128 / 2;
    let choose3 = w as u128 * (w.saturating_sub(1)) as u128 * (w.saturating_sub(2)) as u128 / 6;
    u as u128 + choose2 + choose3
}

pub(super) fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}

pub(super) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}
