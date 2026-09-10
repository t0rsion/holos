use super::digest::diagrams_equal;
use super::digest::graph_digest;
use super::model::{
    CheckedGraded, GradedComplex, GradedReductionCertificate, GradedReductionRepair,
    GradedReductionRepairWork,
};
use super::reduction::{check_all, reduce_all_dimensions};
use super::repair::{graded_repair_mode, repair_all_dimensions, require_fixed_envelope};
use super::validation::{check_compute_diagram, checked_threshold, validate};
use crate::certificate::{CertificateError, CertificateLimits, ChangeColumn};
use crate::{Diagram, RipsParams, SparseDistanceMatrix};

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
