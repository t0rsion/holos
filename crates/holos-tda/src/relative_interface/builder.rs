use std::collections::BTreeSet;

use crate::certificate::{CertificateError, CertificateLimits, ChangeColumn};
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::filtration::{ComplexLimits, FilteredSimplicialComplex, FlagComplexParams, ScalarGrade};
use crate::{Diagram, RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

use super::cancellation::{cancel_relative, simplex_boundary};
use super::chain::check_chain_complex;
use super::composition::{check_composition_children, merge_child_cells};
use super::digest::{certificate_digest, diagrams_equal, source_digest};
use super::model::{
    InterfaceCancellation, InterfaceCell, RelativeInterfaceCertificate, RelativeInterfaceWork,
};
use super::reduction::reduce_core;
use super::validation::{
    canonical_vertices, cell_order, check_protected_cells, count_cells, enforce_cell_limits,
    validate_parameters,
};

impl RelativeInterfaceCertificate {
    /// Build a relative core from one filtered flag complex.
    ///
    /// `protected_vertices` induces the separator subcomplex that every
    /// cancellation fixes. Vertex labels are the input positions.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        let labels: Vec<_> = (0..input.len()).collect();
        Self::build_labeled(input, &labels, params, protected_vertices, limits)
    }

    /// Build a relative core with explicit global vertex labels.
    ///
    /// `labels[local]` names each input vertex. Labels must be strictly
    /// increasing. Independent child cores identify a shared separator during
    /// [`Self::compose`].
    pub fn build_labeled(
        input: &SparseDistanceMatrix,
        labels: &[usize],
        params: &RipsParams,
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        validate_parameters(input, labels, params, protected_vertices, limits)?;
        let complex = FilteredSimplicialComplex::from_flag_graph(
            input,
            labels,
            FlagComplexParams {
                max_dimension: params.max_dim + 1,
                threshold: params.threshold,
                limits: ComplexLimits {
                    max_vertices: limits.max_vertices,
                    max_edges: limits.max_edges,
                    max_triangles: limits.max_triangles,
                    max_higher_simplices: limits.max_higher_simplices,
                },
            },
        )
        .map_err(|error| CertificateError::new(error.to_string()))?;
        let certificate = Self::build_complex(
            &complex,
            params.max_dim,
            params.modulus,
            protected_vertices,
            limits,
        )?;
        let expected = rips_persistence_sparse(input, params)
            .map_err(|error| CertificateError::new(error.to_string()))?;
        if !diagrams_equal(&expected, certificate.diagram()) {
            return Err(CertificateError::new(format!(
                "relative interface differs from the compute engine: expected {:?}, got {:?}",
                expected.bars,
                certificate.diagram().bars
            )));
        }
        Ok(certificate)
    }

    /// Build a relative core from an explicit scalar filtered complex.
    ///
    /// The complex must contain cells through dimension `max_dim + 1`.
    /// The filtration need not come from a Vietoris-Rips flag construction.
    pub fn build_complex(
        complex: &FilteredSimplicialComplex<ScalarGrade>,
        max_dim: usize,
        modulus: u32,
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        if max_dim > limits.max_dimension
            || u64::from(modulus) >= MODULUS_LIMIT
            || !is_prime(modulus as u64)
        {
            return Err(CertificateError::new(
                "relative interface dimension or coefficient field is invalid",
            ));
        }
        if complex.simplices().len() != max_dim + 2 {
            return Err(CertificateError::new(format!(
                "relative interface needs simplex dimensions zero through {}, got zero through {}",
                max_dim + 1,
                complex.max_dimension()
            )));
        }
        let labels: BTreeSet<_> = complex.vertex_labels().iter().copied().collect();
        if protected_vertices
            .iter()
            .any(|vertex| !labels.contains(vertex))
        {
            return Err(CertificateError::new(
                "protected vertex is outside the interface scope",
            ));
        }
        let mut cells = Vec::with_capacity(complex.simplices().len());
        for dimension in complex.simplices() {
            let mut output = Vec::with_capacity(dimension.len());
            for simplex in dimension {
                output.push(InterfaceCell {
                    vertices: simplex.vertices().to_vec(),
                    value: simplex.grade().value(),
                    boundary: if simplex.dimension() == 0 {
                        Vec::new()
                    } else {
                        simplex_boundary(simplex.vertices(), modulus)
                    },
                });
            }
            output.sort_by(cell_order);
            cells.push(output);
        }
        Self::from_cells(cells, max_dim, modulus, protected_vertices, limits)
    }

    /// Compose child cores by identifying cells with equal labeled vertices.
    ///
    /// Equal cells must have bit-identical filtration values and boundaries.
    /// The protected vertices are the separator retained for the next
    /// composition level.
    pub fn compose(
        children: &[&Self],
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        for child in children {
            child.verify(limits)?;
        }
        Self::compose_trusted(children, protected_vertices, limits)
    }

    pub(crate) fn compose_trusted(
        children: &[&Self],
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        let first = check_composition_children(children)?;
        let cells = merge_child_cells(children, first.max_dim)?;
        Self::from_cells(
            cells,
            first.max_dim,
            first.modulus,
            protected_vertices,
            limits,
        )
    }

    fn from_cells(
        input_cells: Vec<Vec<InterfaceCell>>,
        max_dim: usize,
        modulus: u32,
        protected_vertices: &[usize],
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        let protected_vertices = canonical_vertices(protected_vertices)?;
        let input_count = count_cells(&input_cells);
        enforce_cell_limits(&input_cells, limits)?;
        check_chain_complex(&input_cells, max_dim, modulus, limits)?;
        let protected: BTreeSet<_> = protected_vertices.iter().copied().collect();
        let (core_cells, cancellations) =
            cancel_relative(input_cells.clone(), &protected, modulus)?;
        check_protected_cells(&input_cells, &core_cells, &protected)?;
        check_chain_complex(&core_cells, max_dim, modulus, limits)?;
        let (columns, diagram, reduction_additions) = reduce_core(&core_cells, modulus, limits)?;
        let work = RelativeInterfaceWork {
            input_cells: input_count,
            cancellations: cancellations.len(),
            core_cells: count_cells(&core_cells),
            reduction_additions,
        };
        let digest = certificate_digest(
            max_dim,
            modulus,
            &protected_vertices,
            &core_cells,
            &columns,
            &diagram,
        );
        Ok(Self {
            max_dim,
            modulus,
            protected_vertices,
            input_cells,
            cancellations,
            core_cells,
            columns,
            diagram,
            digest,
            work,
        })
    }

    /// Highest homology dimension carried by this interface.
    pub fn max_dim(&self) -> usize {
        self.max_dim
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Vertices whose induced subcomplex is fixed by every cancellation.
    pub fn protected_vertices(&self) -> &[usize] {
        &self.protected_vertices
    }

    /// Input cells grouped by dimension.
    pub fn input_cells(&self) -> &[Vec<InterfaceCell>] {
        &self.input_cells
    }

    /// Checked cancellation trace in execution order.
    pub fn cancellations(&self) -> &[InterfaceCancellation] {
        &self.cancellations
    }

    /// Retained interface core grouped by dimension.
    pub fn core_cells(&self) -> &[Vec<InterfaceCell>] {
        &self.core_cells
    }

    /// Change-of-basis columns grouped by positive source dimension.
    pub fn graded_columns(&self) -> &[Vec<ChangeColumn>] {
        &self.columns
    }

    /// Diagram derived from the retained core.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Content identifier of the checked core and reduction.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Content identifier of the pre-cancellation filtered chain complex.
    ///
    /// Equal cores from different source complexes have different source
    /// identifiers. Dynamic proof nodes use both this digest and [`Self::digest`].
    pub fn source_digest(&self) -> [u8; 32] {
        source_digest(
            self.max_dim,
            self.modulus,
            &self.protected_vertices,
            &self.input_cells,
        )
    }

    /// Exact cell and reduction work for this interface.
    pub fn work(&self) -> RelativeInterfaceWork {
        self.work
    }
}
