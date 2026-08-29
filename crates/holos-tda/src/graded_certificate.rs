//! Dimension-generic algebraic certificates for separator interfaces.
//!
//! A graded certificate records one unit-triangular change of basis for each
//! boundary dimension through `max_dim + 1`. The verifier reconstructs the
//! filtered flag complex, checks every `D V = R` relation, and derives the
//! diagram from the checked pivots. This module does not assign canonical
//! identities to classes above H1.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::certificate::{
    CertificateError, CertificateLimits, CertificateTerm, ChangeColumn, ReductionRepairMode,
};
use crate::filtration::{FilteredSimplicialComplex, ScalarGrade};
use crate::{Bar, Diagram, RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

/// Work retained and recomputed in one boundary dimension.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GradedDimensionWork {
    /// Dimension of the source simplices in this boundary matrix.
    pub simplex_dimension: usize,
    /// Change-of-basis columns retained without reduction.
    pub columns_reused: usize,
    /// Change-of-basis columns processed by reduction.
    pub columns_reduced: usize,
    /// Sparse reduced-column additions.
    pub column_additions: usize,
}

/// Exact work charged to a dimension-generic reduction repair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GradedReductionRepairWork {
    dimensions: Vec<GradedDimensionWork>,
}

impl GradedReductionRepairWork {
    /// Per-dimension work in ascending simplex dimension.
    pub fn dimensions(&self) -> &[GradedDimensionWork] {
        &self.dimensions
    }

    /// Columns retained without reduction.
    pub fn columns_reused(&self) -> usize {
        self.dimensions.iter().map(|work| work.columns_reused).sum()
    }

    /// Columns processed by reduction.
    pub fn columns_reduced(&self) -> usize {
        self.dimensions
            .iter()
            .map(|work| work.columns_reduced)
            .sum()
    }

    /// Sparse reduced-column additions.
    pub fn column_additions(&self) -> usize {
        self.dimensions
            .iter()
            .map(|work| work.column_additions)
            .sum()
    }
}

/// A checked graded reduction adapted to a new filtration.
#[derive(Debug, Clone)]
pub struct GradedReductionRepair {
    certificate: GradedReductionCertificate,
    mode: ReductionRepairMode,
    work: GradedReductionRepairWork,
}

impl GradedReductionRepair {
    /// Repaired certificate bound to the updated graph.
    pub fn certificate(&self) -> &GradedReductionCertificate {
        &self.certificate
    }

    /// Whether the repair reused, repaired, or rebuilt its columns.
    pub fn mode(&self) -> ReductionRepairMode {
        self.mode
    }

    /// Exact work charged by the repair.
    pub fn work(&self) -> &GradedReductionRepairWork {
        &self.work
    }

    pub(crate) fn into_certificate(self) -> GradedReductionCertificate {
        self.certificate
    }
}

/// A dimension-generic `D V = R` certificate for a filtered flag complex.
#[derive(Debug, Clone)]
pub struct GradedReductionCertificate {
    vertex_count: usize,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    graph_digest: [u8; 32],
    columns: Vec<Vec<ChangeColumn>>,
    diagram: Diagram,
}

pub(crate) fn reduce_all_dimensions(
    complex: &GradedComplex,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<ChangeColumn>>, CertificateError> {
    let mut columns = Vec::with_capacity(max_dim + 1);
    for dimension in 1..=max_dim + 1 {
        let boundaries = complex.boundaries(dimension, modulus)?;
        columns.push(reduce_with_prefix(&boundaries, &[], modulus, limits)?.0);
    }
    Ok(columns)
}

fn check_compute_diagram(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    checked: &Diagram,
) -> Result<(), CertificateError> {
    let computed = rips_persistence_sparse(input, params)
        .map_err(|error| CertificateError::new(error.to_string()))?;
    if !diagrams_equal(&computed, checked) {
        return Err(CertificateError::new(format!(
            "graded certificate diagram differs from the compute engine: expected {:?}, got {:?}",
            computed.bars, checked.bars
        )));
    }
    Ok(())
}

fn repair_all_dimensions(
    old: &GradedComplex,
    new: &GradedComplex,
    old_columns: &[Vec<ChangeColumn>],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<Vec<ChangeColumn>>, Vec<GradedDimensionWork>), CertificateError> {
    let mut columns = Vec::with_capacity(old_columns.len());
    let mut work = Vec::with_capacity(old_columns.len());
    for dimension in 1..old.simplices.len() {
        let (next, dimension_work) = repair_dimension(
            dimension,
            old,
            new,
            &old_columns[dimension - 1],
            modulus,
            limits,
        )?;
        columns.push(next);
        work.push(dimension_work);
    }
    Ok((columns, work))
}

fn repair_dimension(
    dimension: usize,
    old: &GradedComplex,
    new: &GradedComplex,
    old_columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, GradedDimensionWork), CertificateError> {
    let boundaries = new.boundaries(dimension, modulus)?;
    let candidates = reindexed_prefix_candidates(
        &old.simplices[dimension],
        &new.simplices[dimension],
        old_columns,
    )?;
    let prefix = valid_prefix_len(&boundaries, &candidates, modulus, limits)?;
    let (columns, additions) =
        reduce_with_prefix(&boundaries, &candidates[..prefix], modulus, limits)?;
    let work = GradedDimensionWork {
        simplex_dimension: dimension,
        columns_reused: prefix,
        columns_reduced: boundaries.len() - prefix,
        column_additions: additions,
    };
    Ok((columns, work))
}

fn graded_repair_mode(work: &GradedReductionRepairWork) -> ReductionRepairMode {
    if work.columns_reduced() == 0 {
        ReductionRepairMode::Reused
    } else if work.columns_reused() == 0 {
        ReductionRepairMode::Rebuilt
    } else {
        ReductionRepairMode::SuffixRepaired
    }
}

impl GradedReductionCertificate {
    /// Produce a certificate through the requested homology dimension.
    ///
    /// The producer materializes flag simplices through dimension
    /// `max_dim + 1`. [`CertificateLimits`] bounds each simplex collection
    /// before reduction.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        validate(input, params, limits)?;
        let threshold = checked_threshold(params.threshold)?;
        let complex = GradedComplex::build(input, params.max_dim, threshold, limits)?;
        let columns = reduce_all_dimensions(&complex, params.max_dim, params.modulus, limits)?;
        let checked = check_all(&complex, params.modulus, &columns, limits)?;
        check_compute_diagram(input, params, &checked.diagram)?;
        Ok(Self {
            vertex_count: input.len(),
            max_dim: params.max_dim,
            threshold: params.threshold,
            modulus: params.modulus,
            graph_digest: graph_digest(input, threshold),
            columns,
            diagram: checked.diagram,
        })
    }

    /// Adapt every boundary dimension to weights on the same listed graph.
    ///
    /// Each dimension retains its longest filtration-compatible prefix with
    /// distinct pivots. The remaining suffix is reduced from that prefix.
    pub fn repair(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> Result<GradedReductionRepair, CertificateError> {
        let old_complex = self.verify_parts(current, limits)?.0;
        require_fixed_envelope(current, updated, self.threshold)?;
        let threshold = checked_threshold(self.threshold)?;
        let new_complex = GradedComplex::build(updated, self.max_dim, threshold, limits)?;
        let (columns, work) = repair_all_dimensions(
            &old_complex,
            &new_complex,
            &self.columns,
            self.modulus,
            limits,
        )?;
        let checked = check_all(&new_complex, self.modulus, &columns, limits)?;
        let work = GradedReductionRepairWork { dimensions: work };
        Ok(GradedReductionRepair {
            certificate: Self {
                vertex_count: updated.len(),
                max_dim: self.max_dim,
                threshold: self.threshold,
                modulus: self.modulus,
                graph_digest: graph_digest(updated, threshold),
                columns,
                diagram: checked.diagram,
            },
            mode: graded_repair_mode(&work),
            work,
        })
    }

    /// Verify every `D V = R` relation without calling the persistence solver.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> Result<Diagram, CertificateError> {
        Ok(self.verify_parts(input, limits)?.1.diagram)
    }

    /// Highest homology dimension.
    pub fn max_dim(&self) -> usize {
        self.max_dim
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Digest of the thresholded graph.
    pub fn graph_digest(&self) -> &[u8; 32] {
        &self.graph_digest
    }

    /// Change-of-basis columns for source simplices of one dimension.
    pub fn columns(&self, simplex_dimension: usize) -> Option<&[ChangeColumn]> {
        simplex_dimension
            .checked_sub(1)
            .and_then(|index| self.columns.get(index))
            .map(Vec::as_slice)
    }

    /// Change-of-basis columns in ascending simplex dimension.
    pub fn graded_columns(&self) -> &[Vec<ChangeColumn>] {
        &self.columns
    }

    /// Total change-of-basis column count.
    pub fn column_count(&self) -> usize {
        self.columns.iter().map(Vec::len).sum()
    }

    /// Diagram derived from the checked reductions.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    fn verify_parts(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> Result<(GradedComplex, CheckedGraded), CertificateError> {
        let params = RipsParams::new(self.max_dim).with_modulus(self.modulus);
        validate(input, &params, limits)?;
        if input.len() != self.vertex_count {
            return Err(CertificateError::new(format!(
                "input has {} vertices, graded certificate records {}",
                input.len(),
                self.vertex_count
            )));
        }
        if self.columns.len() != self.max_dim + 1 {
            return Err(CertificateError::new(
                "graded certificate has the wrong boundary-dimension count",
            ));
        }
        let threshold = checked_threshold(self.threshold)?;
        if graph_digest(input, threshold) != self.graph_digest {
            return Err(CertificateError::new(
                "graded certificate graph binding does not match",
            ));
        }
        let complex = GradedComplex::build(input, self.max_dim, threshold, limits)?;
        let checked = check_all(&complex, self.modulus, &self.columns, limits)?;
        if !diagrams_equal(&checked.diagram, &self.diagram) {
            return Err(CertificateError::new(
                "graded certificate diagram differs from the checked reductions",
            ));
        }
        Ok((complex, checked))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SimplexKey(pub(crate) Vec<usize>);

#[derive(Debug, Clone)]
pub(crate) struct FilteredSimplex {
    pub(crate) key: SimplexKey,
    pub(crate) value: f64,
}

pub(crate) struct GradedComplex {
    pub(crate) simplices: Vec<Vec<FilteredSimplex>>,
    rows: Vec<BTreeMap<SimplexKey, usize>>,
}

impl GradedComplex {
    fn build(
        input: &SparseDistanceMatrix,
        max_dim: usize,
        threshold: f64,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        let vertices: Vec<_> = (0..input.len())
            .map(|vertex| FilteredSimplex {
                key: SimplexKey(vec![vertex]),
                value: 0.0,
            })
            .collect();
        let mut simplices = vec![vertices];
        for dimension in 1..=max_dim + 1 {
            let mut next = next_simplices(
                input,
                &simplices[dimension - 1],
                dimension,
                threshold,
                simplex_limit(dimension, limits),
            )?;
            next.sort_by(filtered_simplex_order);
            simplices.push(next);
        }
        let rows = simplex_rows(&simplices);
        Ok(Self { simplices, rows })
    }

    pub(crate) fn from_filtered(
        input: &FilteredSimplicialComplex<ScalarGrade>,
        max_dim: usize,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        if max_dim > limits.max_dimension || input.max_dimension() < max_dim + 1 {
            return Err(CertificateError::new(
                "explicit complex does not cover the requested homology dimensions",
            ));
        }
        if input.vertex_labels().len() > limits.max_vertices {
            return Err(CertificateError::new(
                "explicit complex exceeds the vertex limit",
            ));
        }
        let mut simplices = Vec::with_capacity(max_dim + 2);
        for dimension in 0..=max_dim + 1 {
            let source = &input.simplices()[dimension];
            if source.len() > explicit_simplex_limit(dimension, limits) {
                return Err(CertificateError::new(format!(
                    "explicit complex dimension {dimension} exceeds its simplex limit"
                )));
            }
            let mut ordered = source
                .iter()
                .map(|simplex| FilteredSimplex {
                    key: SimplexKey(simplex.vertices().to_vec()),
                    value: simplex.grade().value(),
                })
                .collect::<Vec<_>>();
            ordered.sort_by(filtered_simplex_order);
            simplices.push(ordered);
        }
        let rows = simplex_rows(&simplices);
        Ok(Self { simplices, rows })
    }

    fn boundaries(
        &self,
        dimension: usize,
        modulus: u32,
    ) -> Result<Vec<SparseColumn>, CertificateError> {
        let modulus = modulus as u64;
        self.simplices[dimension]
            .iter()
            .map(|simplex| {
                let mut column = SparseColumn::default();
                for removed in 0..simplex.key.0.len() {
                    let mut face = simplex.key.0.clone();
                    face.remove(removed);
                    let row = self.rows[dimension - 1]
                        .get(&SimplexKey(face))
                        .copied()
                        .ok_or_else(|| CertificateError::new("simplex boundary omits a face"))?;
                    column.insert(row, if removed % 2 == 0 { 1 } else { modulus - 1 });
                }
                Ok(column)
            })
            .collect()
    }
}

fn filtered_simplex_order(left: &FilteredSimplex, right: &FilteredSimplex) -> Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.key.0.iter().rev().cmp(left.key.0.iter().rev()))
}

fn simplex_rows(simplices: &[Vec<FilteredSimplex>]) -> Vec<BTreeMap<SimplexKey, usize>> {
    simplices
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, simplex)| (simplex.key.clone(), position))
                .collect()
        })
        .collect()
}

fn explicit_simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        _ => simplex_limit(dimension, limits),
    }
}

fn simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn next_simplices(
    input: &SparseDistanceMatrix,
    previous: &[FilteredSimplex],
    dimension: usize,
    threshold: f64,
    limit: usize,
) -> Result<Vec<FilteredSimplex>, CertificateError> {
    let mut next = Vec::new();
    for simplex in previous {
        extend_simplex(input, simplex, dimension, threshold, limit, &mut next)?;
    }
    Ok(next)
}

fn extend_simplex(
    input: &SparseDistanceMatrix,
    simplex: &FilteredSimplex,
    dimension: usize,
    threshold: f64,
    limit: usize,
    next: &mut Vec<FilteredSimplex>,
) -> Result<(), CertificateError> {
    let start = simplex.key.0.last().copied().unwrap_or(0) + 1;
    for vertex in start..input.len() {
        if let Some(extension) = simplex_extension(input, simplex, vertex, threshold) {
            next.push(extension);
            if next.len() > limit {
                return Err(CertificateError::new(format!(
                    "dimension {dimension} simplex count exceeds the limit {limit}"
                )));
            }
        }
    }
    Ok(())
}

fn simplex_extension(
    input: &SparseDistanceMatrix,
    simplex: &FilteredSimplex,
    vertex: usize,
    threshold: f64,
) -> Option<FilteredSimplex> {
    let mut value = simplex.value;
    for &member in &simplex.key.0 {
        let edge = input.get(member, vertex);
        if !edge.is_finite() || edge > threshold {
            return None;
        }
        value = value.max(edge);
    }
    let mut key = simplex.key.0.clone();
    key.push(vertex);
    Some(FilteredSimplex {
        key: SimplexKey(key),
        value,
    })
}

#[derive(Debug, Clone, Default)]
struct SparseColumn(BTreeMap<usize, u64>);

impl SparseColumn {
    fn insert(&mut self, index: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(index, coefficient);
        }
    }

    fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&index, &value)| (index, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&index, &value) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }
}

pub(crate) struct CheckedGraded {
    pub(crate) diagram: Diagram,
}

pub(crate) fn check_all(
    complex: &GradedComplex,
    modulus: u32,
    columns: &[Vec<ChangeColumn>],
    limits: CertificateLimits,
) -> Result<CheckedGraded, CertificateError> {
    if columns.len() + 1 != complex.simplices.len() {
        return Err(CertificateError::new(
            "graded reduction count differs from the filtered complex",
        ));
    }
    let mut reduced = Vec::with_capacity(columns.len());
    let mut total_terms = 0usize;
    for dimension in 1..complex.simplices.len() {
        reduced.push(check_matrix(
            dimension,
            &complex.boundaries(dimension, modulus)?,
            &columns[dimension - 1],
            modulus,
            limits,
            &mut total_terms,
        )?);
    }
    let mut diagram = Diagram::default();
    for homology_dimension in 0..columns.len() {
        let births = if homology_dimension == 0 {
            vec![true; complex.simplices[0].len()]
        } else {
            reduced[homology_dimension - 1]
                .iter()
                .map(|column| column.0.is_empty())
                .collect()
        };
        let deaths: FxHashMap<_, _> = reduced[homology_dimension]
            .iter()
            .enumerate()
            .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
            .collect();
        for (birth_position, is_birth) in births.into_iter().enumerate() {
            if !is_birth {
                continue;
            }
            let birth = complex.simplices[homology_dimension][birth_position].value;
            let death = deaths
                .get(&birth_position)
                .map_or(f64::INFINITY, |&position| {
                    complex.simplices[homology_dimension + 1][position].value
                });
            if death > birth {
                diagram.bars.push(Bar {
                    dim: homology_dimension,
                    birth,
                    death,
                });
            }
        }
    }
    diagram.canonicalize();
    Ok(CheckedGraded { diagram })
}

fn check_matrix(
    dimension: usize,
    boundaries: &[SparseColumn],
    columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, CertificateError> {
    if boundaries.len() != columns.len() {
        return Err(CertificateError::new(format!(
            "dimension {dimension} has {} boundary columns but the certificate records {}",
            boundaries.len(),
            columns.len()
        )));
    }
    let modulus64 = modulus as u64;
    let mut reduced = Vec::with_capacity(columns.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in columns.iter().enumerate() {
        *total_terms = total_terms
            .checked_add(transform.terms.len())
            .ok_or_else(|| CertificateError::new("graded certificate term count overflows"))?;
        if *total_terms > limits.max_terms {
            return Err(CertificateError::new(format!(
                "{} graded certificate terms exceed the limit {}",
                *total_terms, limits.max_terms
            )));
        }
        validate_change_column(dimension, target, transform, modulus)?;
        let mut column = SparseColumn::default();
        for term in &transform.terms {
            column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
        }
        if let Some((pivot, _)) = column.pivot() {
            if !pivots.insert(pivot) {
                return Err(CertificateError::new(format!(
                    "dimension {dimension} reduction repeats pivot {pivot}"
                )));
            }
        }
        reduced.push(column);
    }
    Ok(reduced)
}

fn validate_change_column(
    dimension: usize,
    target: usize,
    column: &ChangeColumn,
    modulus: u32,
) -> Result<(), CertificateError> {
    if column.terms.is_empty()
        || column.terms.last()
            != Some(&CertificateTerm {
                index: target,
                coefficient: 1,
            })
    {
        return Err(CertificateError::new(format!(
            "dimension {dimension} change column {target} is not unit triangular"
        )));
    }
    let mut previous = None;
    for term in &column.terms {
        if term.index > target
            || previous.is_some_and(|value| value >= term.index)
            || term.coefficient == 0
            || term.coefficient >= modulus
        {
            return Err(CertificateError::new(format!(
                "dimension {dimension} change column {target} is not canonical"
            )));
        }
        previous = Some(term.index);
    }
    Ok(())
}

fn reduce_with_prefix(
    boundaries: &[SparseColumn],
    prefix: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, usize), CertificateError> {
    if prefix.len() > boundaries.len() {
        return Err(CertificateError::new(
            "graded reduction prefix exceeds its boundary matrix",
        ));
    }
    let mut state = GradedReductionState::new(boundaries.len(), modulus);
    for (target, transform) in prefix.iter().enumerate() {
        state.retain(target, transform, boundaries, limits.max_terms)?;
    }
    for (target, boundary) in boundaries.iter().enumerate().skip(prefix.len()) {
        state.reduce(target, boundary, limits.max_terms)?;
    }
    Ok(state.finish())
}

struct GradedReductionState {
    modulus: u32,
    reduced: Vec<SparseColumn>,
    bases: Vec<SparseColumn>,
    owners: FxHashMap<usize, usize>,
    term_count: usize,
    additions: usize,
}

impl GradedReductionState {
    fn new(capacity: usize, modulus: u32) -> Self {
        Self {
            modulus,
            reduced: Vec::with_capacity(capacity),
            bases: Vec::with_capacity(capacity),
            owners: FxHashMap::default(),
            term_count: 0,
            additions: 0,
        }
    }

    fn retain(
        &mut self,
        target: usize,
        transform: &ChangeColumn,
        boundaries: &[SparseColumn],
        maximum: usize,
    ) -> Result<(), CertificateError> {
        validate_change_column(0, target, transform, self.modulus)?;
        let (column, basis) = apply_transform(transform, boundaries, self.modulus as u64);
        if column
            .pivot()
            .is_some_and(|(pivot, _)| self.owners.insert(pivot, target).is_some())
        {
            return Err(CertificateError::new(
                "graded reduction prefix repeats a pivot",
            ));
        }
        self.add_prefix_terms(basis.0.len(), maximum)?;
        self.reduced.push(column);
        self.bases.push(basis);
        Ok(())
    }

    fn reduce(
        &mut self,
        target: usize,
        boundary: &SparseColumn,
        maximum: usize,
    ) -> Result<(), CertificateError> {
        let modulus = self.modulus as u64;
        let mut column = boundary.clone();
        let mut basis = SparseColumn::default();
        basis.insert(target, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = self.owners.get(&pivot) else {
                break;
            };
            let owner_coefficient = self.reduced[owner].pivot().expect("owner has a pivot").1;
            let factor = (modulus
                - coefficient * inverse_mod(owner_coefficient, modulus) % modulus)
                % modulus;
            column.add_scaled(&self.reduced[owner], factor, modulus);
            basis.add_scaled(&self.bases[owner], factor, modulus);
            self.additions += 1;
        }
        if let Some((pivot, _)) = column.pivot() {
            self.owners.insert(pivot, target);
        }
        self.add_terms(basis.0.len(), maximum)?;
        self.reduced.push(column);
        self.bases.push(basis);
        Ok(())
    }

    fn add_prefix_terms(&mut self, count: usize, maximum: usize) -> Result<(), CertificateError> {
        self.term_count += count;
        if self.term_count > maximum {
            return Err(CertificateError::new(
                "graded reduction prefix exceeds the term limit",
            ));
        }
        Ok(())
    }

    fn add_terms(&mut self, count: usize, maximum: usize) -> Result<(), CertificateError> {
        self.term_count += count;
        if self.term_count > maximum {
            return Err(CertificateError::new(format!(
                "{} graded change terms exceed the limit {maximum}",
                self.term_count
            )));
        }
        Ok(())
    }

    fn finish(self) -> (Vec<ChangeColumn>, usize) {
        let columns = self.bases.into_iter().map(sparse_change_column).collect();
        (columns, self.additions)
    }
}

fn apply_transform(
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> (SparseColumn, SparseColumn) {
    let mut column = SparseColumn::default();
    let mut basis = SparseColumn::default();
    for term in &transform.terms {
        column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        basis.insert(term.index, term.coefficient as u64);
    }
    (column, basis)
}

fn sparse_change_column(column: SparseColumn) -> ChangeColumn {
    ChangeColumn {
        terms: column
            .0
            .into_iter()
            .map(|(index, coefficient)| CertificateTerm {
                index,
                coefficient: coefficient as u32,
            })
            .collect(),
    }
}

fn valid_prefix_len(
    boundaries: &[SparseColumn],
    candidates: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<usize, CertificateError> {
    let modulus64 = modulus as u64;
    let mut pivots = BTreeSet::new();
    let mut terms = 0usize;
    for (target, transform) in candidates.iter().enumerate() {
        validate_change_column(0, target, transform, modulus)?;
        terms += transform.terms.len();
        if terms > limits.max_terms {
            return Err(CertificateError::new(
                "graded repair candidates exceed the term limit",
            ));
        }
        let mut column = SparseColumn::default();
        for term in &transform.terms {
            column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
        }
        if let Some((pivot, _)) = column.pivot() {
            if !pivots.insert(pivot) {
                return Ok(target);
            }
        }
    }
    Ok(candidates.len())
}

fn reindexed_prefix_candidates(
    old_simplices: &[FilteredSimplex],
    new_simplices: &[FilteredSimplex],
    old_columns: &[ChangeColumn],
) -> Result<Vec<ChangeColumn>, CertificateError> {
    if old_simplices.len() != old_columns.len() || old_simplices.len() != new_simplices.len() {
        return Err(CertificateError::new(
            "graded repair requires an unchanged simplex set",
        ));
    }
    let old_positions: BTreeMap<_, _> = old_simplices
        .iter()
        .enumerate()
        .map(|(position, simplex)| (simplex.key.clone(), position))
        .collect();
    let new_positions: BTreeMap<_, _> = new_simplices
        .iter()
        .enumerate()
        .map(|(position, simplex)| (simplex.key.clone(), position))
        .collect();
    if old_positions.keys().ne(new_positions.keys()) {
        return Err(CertificateError::new(
            "graded repair requires an unchanged simplex set",
        ));
    }
    let mut output = Vec::with_capacity(new_simplices.len());
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[&simplex.key];
        let mut terms = Vec::with_capacity(old_columns[old_target].terms.len());
        for term in &old_columns[old_target].terms {
            let source = &old_simplices[term.index].key;
            let index = new_positions[source];
            if index > new_target {
                return Ok(output);
            }
            terms.push(CertificateTerm {
                index,
                coefficient: term.coefficient,
            });
        }
        terms.sort_unstable();
        if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
            return Err(CertificateError::new(
                "graded repair produced duplicate source positions",
            ));
        }
        output.push(ChangeColumn { terms });
    }
    Ok(output)
}

fn require_fixed_envelope(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<(), CertificateError> {
    if current.len() != updated.len() {
        return Err(CertificateError::new(
            "graded repair requires an unchanged vertex set",
        ));
    }
    let current_edges: Vec<_> = current.edges().map(|(u, v, _)| (u, v)).collect();
    let updated_edges: Vec<_> = updated.edges().map(|(u, v, _)| (u, v)).collect();
    if current_edges != updated_edges {
        return Err(CertificateError::new(
            "graded repair requires an unchanged listed edge set",
        ));
    }
    let threshold = checked_threshold(threshold)?;
    if current
        .edges()
        .zip(updated.edges())
        .any(|((_, _, old), (_, _, new))| (old <= threshold) != (new <= threshold))
    {
        return Err(CertificateError::new(
            "graded repair requires unchanged threshold membership",
        ));
    }
    Ok(())
}

fn validate(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if params.max_dim > limits.max_dimension {
        return Err(CertificateError::new(format!(
            "homology dimension {} exceeds the graded certificate limit {}",
            params.max_dim, limits.max_dimension
        )));
    }
    if input.len() > limits.max_vertices {
        return Err(CertificateError::new(format!(
            "{} vertices exceed the limit {}",
            input.len(),
            limits.max_vertices
        )));
    }
    if params.modulus < 2 || !is_prime(params.modulus as u64) || params.modulus >= 32_768 {
        return Err(CertificateError::new(format!(
            "modulus must be a prime below 32768, got {}",
            params.modulus
        )));
    }
    checked_threshold(params.threshold)?;
    Ok(())
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64, CertificateError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 {
        return Err(CertificateError::new(format!(
            "threshold must be non-negative, got {value}"
        )));
    }
    Ok(value)
}

fn graph_digest(input: &SparseDistanceMatrix, threshold: f64) -> [u8; 32] {
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

fn diagrams_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
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

fn is_prime(value: u64) -> bool {
    if value < 2 {
        return false;
    }
    let mut divisor = 2;
    while divisor * divisor <= value {
        if value % divisor == 0 {
            return false;
        }
        divisor += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn octahedron() -> SparseDistanceMatrix {
        let opposite = [(0, 1), (2, 3), (4, 5)];
        let edges = (0..6)
            .flat_map(|u| (u + 1..6).map(move |v| (u, v)))
            .filter(|edge| !opposite.contains(edge))
            .map(|(u, v)| (u, v, 1.0 + (u + v) as f64 / 100.0))
            .collect::<Vec<_>>();
        SparseDistanceMatrix::from_triplets(6, &edges).unwrap()
    }

    fn cross_polytope(pair_count: usize) -> SparseDistanceMatrix {
        let edges = (0..2 * pair_count)
            .flat_map(|u| (u + 1..2 * pair_count).map(move |v| (u, v)))
            .filter(|&(u, v)| u / 2 != v / 2)
            .map(|(u, v)| (u, v, 1.0 + (u + v) as f64 / 100.0))
            .collect::<Vec<_>>();
        SparseDistanceMatrix::from_triplets(2 * pair_count, &edges).unwrap()
    }

    #[test]
    fn certifies_and_repairs_h2_over_prime_fields() {
        let current = octahedron();
        let mut edges: Vec<_> = current.edges().collect();
        edges[0].2 += 0.001;
        let updated = SparseDistanceMatrix::from_triplets(6, &edges).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(2).with_modulus(modulus);
            let certificate =
                GradedReductionCertificate::build(&current, &params, CertificateLimits::default())
                    .unwrap();
            assert!(certificate.diagram().in_dim(2).next().is_some());
            assert_eq!(
                certificate
                    .verify(&current, CertificateLimits::default())
                    .unwrap()
                    .bars,
                rips_persistence_sparse(&current, &params).unwrap().bars
            );
            let repaired = certificate
                .repair(&current, &updated, CertificateLimits::default())
                .unwrap();
            assert_eq!(
                repaired.certificate().diagram().bars,
                rips_persistence_sparse(&updated, &params).unwrap().bars
            );
        }
    }

    #[test]
    fn graded_certificate_extends_through_h3() {
        let graph = cross_polytope(4);
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(3).with_modulus(modulus);
            let certificate =
                GradedReductionCertificate::build(&graph, &params, CertificateLimits::default())
                    .unwrap();
            assert_eq!(certificate.graded_columns().len(), 4);
            assert_eq!(certificate.diagram().in_dim(3).count(), 1);
            assert_eq!(
                certificate.diagram().bars,
                rips_persistence_sparse(&graph, &params).unwrap().bars
            );
        }
    }

    #[test]
    fn absent_edges_never_enter_at_an_infinite_threshold() {
        let graph =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2)])
                .unwrap();
        let params = RipsParams::new(2);
        let certificate =
            GradedReductionCertificate::build(&graph, &params, CertificateLimits::default())
                .unwrap();
        assert_eq!(certificate.columns(1).unwrap().len(), 3);
        assert!(certificate.columns(2).unwrap().is_empty());
        assert!(certificate.columns(3).unwrap().is_empty());
    }
}
