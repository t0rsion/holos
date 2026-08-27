//! Exact filtered chain cores relative to separator subcomplexes.
//!
//! Equal-filtration unit cancellations leave every protected separator cell
//! fixed. Cores with the same labeled separator compose by identifying equal
//! cells. The resulting chain complex retains exact persistence, including
//! separators with nonzero homology.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::filtration::{ComplexLimits, FilteredSimplicialComplex, FlagComplexParams, ScalarGrade};
use crate::{Bar, Diagram, RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

const MAGIC: &[u8; 8] = b"HOLOSRI\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// One nonzero term in a filtered cellular boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceChainTerm {
    /// Labeled vertices of the boundary cell.
    pub cell: Vec<usize>,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One labeled cell in a filtered relative interface.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCell {
    /// Labeled vertices in ascending order.
    pub vertices: Vec<usize>,
    /// Filtration value of this cell.
    pub value: f64,
    /// Sparse cellular boundary in ascending cell order.
    pub boundary: Vec<InterfaceChainTerm>,
}

impl InterfaceCell {
    /// Dimension of this cell.
    pub fn dimension(&self) -> usize {
        self.vertices.len() - 1
    }
}

/// One checked equal-filtration unit cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCancellation {
    /// Higher-dimensional cell removed by this step.
    pub upper: Vec<usize>,
    /// Codimension-one cell removed by this step.
    pub lower: Vec<usize>,
    /// Incidence coefficient before the cancellation.
    pub coefficient: u32,
}

/// Exact size and work of one relative interface certificate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelativeInterfaceWork {
    /// Cells before relative cancellation.
    pub input_cells: usize,
    /// Equal-filtration unit pairs removed.
    pub cancellations: usize,
    /// Cells in the retained core.
    pub core_cells: usize,
    /// Sparse additions used to reduce the retained core.
    pub reduction_additions: usize,
}

/// A proof-carrying filtered chain core relative to protected vertices.
///
/// The record includes its input chain complex, cancellation trace, retained
/// core, and one `D V = R` reduction per boundary dimension. [`Self::verify`]
/// checks these objects without the implicit persistence engine.
#[derive(Debug, Clone)]
pub struct RelativeInterfaceCertificate {
    max_dim: usize,
    modulus: u32,
    protected_vertices: Vec<usize>,
    input_cells: Vec<Vec<InterfaceCell>>,
    cancellations: Vec<InterfaceCancellation>,
    core_cells: Vec<Vec<InterfaceCell>>,
    columns: Vec<Vec<ChangeColumn>>,
    diagram: Diagram,
    digest: [u8; 32],
    work: RelativeInterfaceWork,
}

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
    /// increasing. This form lets independently built child cores identify a
    /// shared separator during [`Self::compose`].
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
    /// This entry point accepts filtrations built by methods other than the
    /// Vietoris-Rips flag construction.
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
    /// The protected vertices describe the separator retained for the next
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

    /// Replay every cancellation and check the retained reduction.
    pub fn verify(&self, limits: CertificateLimits) -> Result<Diagram, CertificateError> {
        check_interface_parameters(self.max_dim, self.modulus, limits)?;
        enforce_cell_limits(&self.input_cells, limits)?;
        enforce_cell_limits(&self.core_cells, limits)?;
        check_chain_complex(&self.input_cells, self.max_dim, self.modulus, limits)?;
        let protected: BTreeSet<_> = self.protected_vertices.iter().copied().collect();
        check_cancellation_replay(self, &protected)?;
        check_protected_cells(&self.input_cells, &self.core_cells, &protected)?;
        let (diagram, _) = check_reduction(&self.core_cells, self.modulus, &self.columns, limits)?;
        check_interface_result(self, &diagram)?;
        Ok(diagram)
    }

    /// Encode the canonical `HOLOSRI` version 1 certificate.
    pub fn encode(&self, limits: CertificateLimits) -> Result<Vec<u8>, CertificateError> {
        self.verify(limits)?;
        let mut output = Vec::new();
        encode_interface_header(&mut output, self)?;
        encode_usizes(&mut output, &self.protected_vertices)?;
        encode_cells(&mut output, &self.input_cells)?;
        encode_cancellations(&mut output, &self.cancellations)?;
        encode_cells(&mut output, &self.core_cells)?;
        encode_columns(&mut output, &self.columns)?;
        encode_diagram(&mut output, &self.diagram)?;
        output.extend_from_slice(&self.digest);
        check_encoded_size(output.len(), limits.max_bytes)?;
        Ok(output)
    }

    /// Decode and verify one bounded `HOLOSRI` version 1 certificate.
    pub fn decode(bytes: &[u8], limits: CertificateLimits) -> Result<Self, CertificateError> {
        check_encoded_size(bytes.len(), limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_interface_header(&mut reader, limits)?;
        let body = decode_interface_body(&mut reader, &header, limits)?;
        check_no_trailing_bytes(&reader)?;
        finish_decoded_interface(header, body, limits)
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
    /// This binding distinguishes equal cores produced from different source
    /// complexes. Dynamic proof nodes use both identifiers.
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

struct InterfaceHeader {
    max_dim: usize,
    modulus: u32,
}

struct DecodedInterfaceBody {
    protected_vertices: Vec<usize>,
    input_cells: Vec<Vec<InterfaceCell>>,
    cancellations: Vec<InterfaceCancellation>,
    core_cells: Vec<Vec<InterfaceCell>>,
    columns: Vec<Vec<ChangeColumn>>,
    diagram: Diagram,
    digest: [u8; 32],
}

fn check_composition_children<'a>(
    children: &[&'a RelativeInterfaceCertificate],
) -> Result<&'a RelativeInterfaceCertificate, CertificateError> {
    let first = children
        .first()
        .copied()
        .ok_or_else(|| CertificateError::new("relative composition requires a child"))?;
    for child in children {
        if child.max_dim != first.max_dim || child.modulus != first.modulus {
            return Err(CertificateError::new(
                "relative composition requires one dimension and coefficient field",
            ));
        }
    }
    Ok(first)
}

fn merge_child_cells(
    children: &[&RelativeInterfaceCertificate],
    max_dim: usize,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    let mut dimensions = vec![DimensionMap::new(); max_dim + 2];
    for child in children {
        merge_one_child(&mut dimensions, &child.core_cells)?;
    }
    Ok(maps_to_cells(dimensions))
}

fn merge_one_child(
    dimensions: &mut [DimensionMap],
    child: &[Vec<InterfaceCell>],
) -> Result<(), CertificateError> {
    for (dimension, cells) in child.iter().enumerate() {
        for cell in cells {
            merge_interface_cell(&mut dimensions[dimension], cell)?;
        }
    }
    Ok(())
}

fn merge_interface_cell(
    cells: &mut DimensionMap,
    cell: &InterfaceCell,
) -> Result<(), CertificateError> {
    if let Some(existing) = cells.get(&cell.vertices) {
        if existing != cell {
            return Err(CertificateError::new(
                "identified interface cells have different filtered boundaries",
            ));
        }
        return Ok(());
    }
    cells.insert(cell.vertices.clone(), cell.clone());
    Ok(())
}

fn check_interface_parameters(
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if max_dim > limits.max_dimension || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(CertificateError::new(
            "relative interface exceeds the dimension or modulus limit",
        ));
    }
    if !is_prime(modulus as u64) {
        return Err(CertificateError::new(
            "relative interface modulus is not prime",
        ));
    }
    Ok(())
}

fn check_cancellation_replay(
    certificate: &RelativeInterfaceCertificate,
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    let replayed = replay_cancellations(
        certificate.input_cells.clone(),
        &certificate.cancellations,
        protected,
        certificate.modulus,
    )?;
    if replayed != certificate.core_cells {
        return Err(CertificateError::new(
            "relative cancellation trace does not produce the declared core",
        ));
    }
    Ok(())
}

fn check_interface_result(
    certificate: &RelativeInterfaceCertificate,
    diagram: &Diagram,
) -> Result<(), CertificateError> {
    if !diagrams_equal(diagram, &certificate.diagram) {
        return Err(CertificateError::new(
            "relative interface diagram differs from its checked reduction",
        ));
    }
    let digest = certificate_digest(
        certificate.max_dim,
        certificate.modulus,
        &certificate.protected_vertices,
        &certificate.core_cells,
        &certificate.columns,
        &certificate.diagram,
    );
    if digest != certificate.digest {
        return Err(CertificateError::new(
            "relative interface digest differs from its checked content",
        ));
    }
    Ok(())
}

fn encode_interface_header(
    output: &mut Vec<u8>,
    certificate: &RelativeInterfaceCertificate,
) -> Result<(), CertificateError> {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
    put_usize(output, certificate.max_dim)?;
    output.extend_from_slice(&certificate.modulus.to_be_bytes());
    put_usize(output, certificate.protected_vertices.len())?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), CertificateError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_cancellations(
    output: &mut Vec<u8>,
    cancellations: &[InterfaceCancellation],
) -> Result<(), CertificateError> {
    put_usize(output, cancellations.len())?;
    for step in cancellations {
        encode_cancellation(output, step)?;
    }
    Ok(())
}

fn encode_cancellation(
    output: &mut Vec<u8>,
    step: &InterfaceCancellation,
) -> Result<(), CertificateError> {
    encode_key(output, &step.upper)?;
    encode_key(output, &step.lower)?;
    output.extend_from_slice(&step.coefficient.to_be_bytes());
    Ok(())
}

fn encode_columns(
    output: &mut Vec<u8>,
    columns: &[Vec<ChangeColumn>],
) -> Result<(), CertificateError> {
    put_usize(output, columns.len())?;
    for dimension in columns {
        encode_column_dimension(output, dimension)?;
    }
    Ok(())
}

fn encode_column_dimension(
    output: &mut Vec<u8>,
    columns: &[ChangeColumn],
) -> Result<(), CertificateError> {
    put_usize(output, columns.len())?;
    for column in columns {
        encode_change_column(output, column)?;
    }
    Ok(())
}

fn encode_change_column(
    output: &mut Vec<u8>,
    column: &ChangeColumn,
) -> Result<(), CertificateError> {
    put_usize(output, column.terms.len())?;
    for term in &column.terms {
        put_usize(output, term.index)?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn encode_diagram(output: &mut Vec<u8>, diagram: &Diagram) -> Result<(), CertificateError> {
    put_usize(output, diagram.bars.len())?;
    for bar in &diagram.bars {
        put_usize(output, bar.dim)?;
        output.extend_from_slice(&bar.birth.to_bits().to_be_bytes());
        output.extend_from_slice(&bar.death.to_bits().to_be_bytes());
    }
    Ok(())
}

fn check_encoded_size(actual: usize, limit: usize) -> Result<(), CertificateError> {
    if actual > limit {
        return Err(CertificateError::new(format!(
            "relative interface has {actual} bytes, above the limit {limit}"
        )));
    }
    Ok(())
}

fn decode_interface_header(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> Result<InterfaceHeader, CertificateError> {
    check_interface_identity(reader)?;
    let max_dim = reader.bounded_usize("dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    check_decoded_modulus(modulus)?;
    Ok(InterfaceHeader { max_dim, modulus })
}

fn decode_interface_body(
    reader: &mut Reader<'_>,
    header: &InterfaceHeader,
    limits: CertificateLimits,
) -> Result<DecodedInterfaceBody, CertificateError> {
    let protected_vertices = decode_protected_vertices(reader, limits)?;
    let input_cells = decode_cells(reader, header.max_dim, header.modulus, limits)?;
    let input_count = count_cells(&input_cells);
    let cancellations = decode_cancellations(reader, header.max_dim, input_count, limits)?;
    Ok(DecodedInterfaceBody {
        protected_vertices,
        input_cells,
        cancellations,
        core_cells: decode_cells(reader, header.max_dim, header.modulus, limits)?,
        columns: decode_columns(reader, header.max_dim, header.modulus, limits)?,
        diagram: decode_diagram(reader, header.max_dim, limits)?,
        digest: reader.array32()?,
    })
}

fn finish_decoded_interface(
    header: InterfaceHeader,
    body: DecodedInterfaceBody,
    limits: CertificateLimits,
) -> Result<RelativeInterfaceCertificate, CertificateError> {
    let (_, _, reduction_additions) = reduce_core(&body.core_cells, header.modulus, limits)?;
    let work = RelativeInterfaceWork {
        input_cells: count_cells(&body.input_cells),
        cancellations: body.cancellations.len(),
        core_cells: count_cells(&body.core_cells),
        reduction_additions,
    };
    let certificate = RelativeInterfaceCertificate {
        max_dim: header.max_dim,
        modulus: header.modulus,
        protected_vertices: body.protected_vertices,
        input_cells: body.input_cells,
        cancellations: body.cancellations,
        core_cells: body.core_cells,
        columns: body.columns,
        diagram: body.diagram,
        digest: body.digest,
        work,
    };
    certificate.verify(limits)?;
    Ok(certificate)
}

fn check_interface_identity(reader: &mut Reader<'_>) -> Result<(), CertificateError> {
    let valid =
        reader.take(8)? == MAGIC && reader.u16()? == VERSION && reader.u8()? == F64_BITS_CODEC;
    if !valid {
        return Err(CertificateError::new(
            "relative interface has unsupported magic, version, or scalar codec",
        ));
    }
    Ok(())
}

fn check_decoded_modulus(modulus: u32) -> Result<(), CertificateError> {
    if u64::from(modulus) >= MODULUS_LIMIT || !is_prime(modulus as u64) {
        return Err(CertificateError::new(
            "relative interface modulus is not a supported prime",
        ));
    }
    Ok(())
}

fn decode_protected_vertices(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> Result<Vec<usize>, CertificateError> {
    let count = reader.bounded_usize("protected vertex count", limits.max_vertices)?;
    let mut vertices = Vec::with_capacity(count);
    for _ in 0..count {
        vertices.push(reader.usize()?);
    }
    if vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(CertificateError::new(
            "relative interface protected vertices are not canonical",
        ));
    }
    Ok(vertices)
}

fn decode_cancellations(
    reader: &mut Reader<'_>,
    max_dim: usize,
    input_count: usize,
    limits: CertificateLimits,
) -> Result<Vec<InterfaceCancellation>, CertificateError> {
    let count = reader.bounded_usize("cancellation count", input_count / 2)?;
    let mut cancellations = Vec::with_capacity(count);
    for _ in 0..count {
        cancellations.push(InterfaceCancellation {
            upper: decode_key(reader, max_dim + 2, limits.max_vertices)?,
            lower: decode_key(reader, max_dim + 1, limits.max_vertices)?,
            coefficient: reader.u32()?,
        });
    }
    Ok(cancellations)
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    max_dim: usize,
    limits: CertificateLimits,
) -> Result<Diagram, CertificateError> {
    let count = reader.bounded_usize("bar count", limits.max_bars)?;
    let mut diagram = Diagram::default();
    for _ in 0..count {
        diagram.bars.push(Bar {
            dim: reader.bounded_usize("bar dimension", max_dim)?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    Ok(diagram)
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), CertificateError> {
    if reader.remaining() != 0 {
        return Err(CertificateError::new(
            "relative interface has trailing bytes",
        ));
    }
    Ok(())
}

type DimensionMap = BTreeMap<Vec<usize>, InterfaceCell>;

fn simplex_boundary(vertices: &[usize], modulus: u32) -> Vec<InterfaceChainTerm> {
    let mut boundary = (0..vertices.len())
        .map(|removed| {
            let mut face = vertices.to_vec();
            face.remove(removed);
            InterfaceChainTerm {
                cell: face,
                coefficient: if removed % 2 == 0 { 1 } else { modulus - 1 },
            }
        })
        .collect::<Vec<_>>();
    boundary.sort();
    boundary
}

fn cancel_relative(
    cells: Vec<Vec<InterfaceCell>>,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(Vec<Vec<InterfaceCell>>, Vec<InterfaceCancellation>), CertificateError> {
    let mut maps = cells_to_maps(cells)?;
    let mut steps = Vec::new();
    while let Some(step) = next_cancellation(&maps, protected) {
        apply_cancellation(&mut maps, &step, protected, modulus)?;
        steps.push(step);
    }
    Ok((maps_to_cells(maps), steps))
}

fn replay_cancellations(
    cells: Vec<Vec<InterfaceCell>>,
    steps: &[InterfaceCancellation],
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    let mut maps = cells_to_maps(cells)?;
    for step in steps {
        apply_cancellation(&mut maps, step, protected, modulus)?;
    }
    Ok(maps_to_cells(maps))
}

fn next_cancellation(
    cells: &[DimensionMap],
    protected: &BTreeSet<usize>,
) -> Option<InterfaceCancellation> {
    for dimension in 1..cells.len() {
        let mut upper_cells: Vec<_> = cells[dimension].values().collect();
        upper_cells.sort_by(|left, right| cell_order(left, right));
        for upper in upper_cells {
            if is_protected(&upper.vertices, protected) {
                continue;
            }
            for term in &upper.boundary {
                let lower = cells[dimension - 1].get(&term.cell)?;
                if lower.value.to_bits() == upper.value.to_bits()
                    && !is_protected(&lower.vertices, protected)
                {
                    return Some(InterfaceCancellation {
                        upper: upper.vertices.clone(),
                        lower: lower.vertices.clone(),
                        coefficient: term.coefficient,
                    });
                }
            }
        }
    }
    None
}

fn apply_cancellation(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let pair = prepare_cancellation(cells, step, protected, modulus)?;
    eliminate_lower_boundary(cells, step, &pair, modulus);
    remove_upper_cofaces(cells, step, pair.dimension);
    cells[pair.dimension].remove(&step.upper);
    cells[pair.dimension - 1].remove(&step.lower);
    Ok(())
}

struct CancellationPair {
    dimension: usize,
    upper: InterfaceCell,
    inverse: u32,
}

fn prepare_cancellation(
    cells: &[DimensionMap],
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<CancellationPair, CertificateError> {
    let dimension = checked_cancellation_dimension(cells, step)?;
    check_unprotected_cancellation(step, protected)?;
    let upper = cells[dimension]
        .get(&step.upper)
        .cloned()
        .ok_or_else(|| CertificateError::new("relative cancellation upper cell is absent"))?;
    let lower = cells[dimension - 1]
        .get(&step.lower)
        .ok_or_else(|| CertificateError::new("relative cancellation lower cell is absent"))?;
    check_cancellation_incidence(&upper, lower, step, modulus)?;
    Ok(CancellationPair {
        dimension,
        upper,
        inverse: inverse_mod(step.coefficient as u64, modulus as u64) as u32,
    })
}

fn checked_cancellation_dimension(
    cells: &[DimensionMap],
    step: &InterfaceCancellation,
) -> Result<usize, CertificateError> {
    let dimension =
        step.upper.len().checked_sub(1).ok_or_else(|| {
            CertificateError::new("relative cancellation has an empty upper cell")
        })?;
    if dimension == 0 || step.lower.len() != dimension || dimension >= cells.len() {
        return Err(CertificateError::new(
            "relative cancellation cells have incompatible dimensions",
        ));
    }
    Ok(dimension)
}

fn check_unprotected_cancellation(
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    if is_protected(&step.upper, protected) || is_protected(&step.lower, protected) {
        return Err(CertificateError::new(
            "relative cancellation removes a protected separator cell",
        ));
    }
    Ok(())
}

fn check_cancellation_incidence(
    upper: &InterfaceCell,
    lower: &InterfaceCell,
    step: &InterfaceCancellation,
    modulus: u32,
) -> Result<(), CertificateError> {
    if upper.value.to_bits() != lower.value.to_bits() {
        return Err(CertificateError::new(
            "relative cancellation crosses a filtration value",
        ));
    }
    let coefficient = boundary_coefficient(&upper.boundary, &step.lower)
        .ok_or_else(|| CertificateError::new("relative cancellation cells are not incident"))?;
    if coefficient != step.coefficient || coefficient == 0 || coefficient >= modulus {
        return Err(CertificateError::new(
            "relative cancellation has the wrong incidence coefficient",
        ));
    }
    Ok(())
}

fn eliminate_lower_boundary(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    pair: &CancellationPair,
    modulus: u32,
) {
    let keys: Vec<_> = cells[pair.dimension].keys().cloned().collect();
    for key in keys {
        if key == step.upper {
            continue;
        }
        let cell = cells[pair.dimension].get_mut(&key).unwrap();
        let Some(value) = boundary_coefficient(&cell.boundary, &step.lower) else {
            continue;
        };
        let factor = ((modulus as u64 - value as u64 * pair.inverse as u64 % modulus as u64)
            % modulus as u64) as u32;
        add_boundary_scaled(&mut cell.boundary, &pair.upper.boundary, factor, modulus);
    }
}

fn remove_upper_cofaces(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    dimension: usize,
) {
    if dimension + 1 < cells.len() {
        for cell in cells[dimension + 1].values_mut() {
            remove_boundary_term(&mut cell.boundary, &step.upper);
        }
    }
}

fn cells_to_maps(cells: Vec<Vec<InterfaceCell>>) -> Result<Vec<DimensionMap>, CertificateError> {
    cells
        .into_iter()
        .map(|dimension| {
            let expected = dimension.len();
            let map: DimensionMap = dimension
                .into_iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect();
            if map.len() != expected {
                Err(CertificateError::new("relative interface repeats a cell"))
            } else {
                Ok(map)
            }
        })
        .collect()
}

fn maps_to_cells(maps: Vec<DimensionMap>) -> Vec<Vec<InterfaceCell>> {
    maps.into_iter()
        .map(|dimension| {
            let mut cells: Vec<_> = dimension.into_values().collect();
            cells.sort_by(cell_order);
            cells
        })
        .collect()
}

fn boundary_coefficient(boundary: &[InterfaceChainTerm], key: &[usize]) -> Option<u32> {
    boundary
        .binary_search_by(|term| term.cell.as_slice().cmp(key))
        .ok()
        .map(|position| boundary[position].coefficient)
}

fn remove_boundary_term(boundary: &mut Vec<InterfaceChainTerm>, key: &[usize]) {
    if let Ok(position) = boundary.binary_search_by(|term| term.cell.as_slice().cmp(key)) {
        boundary.remove(position);
    }
}

fn add_boundary_scaled(
    target: &mut Vec<InterfaceChainTerm>,
    source: &[InterfaceChainTerm],
    factor: u32,
    modulus: u32,
) {
    let mut values: BTreeMap<_, _> = target
        .iter()
        .map(|term| (term.cell.clone(), term.coefficient))
        .collect();
    for term in source {
        let value = (values.get(&term.cell).copied().unwrap_or(0) as u64
            + factor as u64 * term.coefficient as u64)
            % modulus as u64;
        if value == 0 {
            values.remove(&term.cell);
        } else {
            values.insert(term.cell.clone(), value as u32);
        }
    }
    *target = values
        .into_iter()
        .map(|(cell, coefficient)| InterfaceChainTerm { cell, coefficient })
        .collect();
}

#[derive(Debug, Clone, Default)]
struct SparseColumn(BTreeMap<usize, u64>);

impl SparseColumn {
    fn insert(&mut self, row: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(row, coefficient);
        }
    }

    fn pivot(&self) -> Option<(usize, u64)> {
        self.0.last_key_value().map(|(&row, &value)| (row, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&row, &value) in &source.0 {
            let next = (self.0.get(&row).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&row);
            } else {
                self.0.insert(row, next);
            }
        }
    }
}

fn reduce_core(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<Vec<ChangeColumn>>, Diagram, usize), CertificateError> {
    let boundaries = boundary_matrices(cells, modulus)?;
    let mut columns = Vec::with_capacity(boundaries.len());
    let mut additions = 0;
    for matrix in &boundaries {
        let (next, count) = reduce_matrix(matrix, modulus, limits)?;
        columns.push(next);
        additions += count;
    }
    let (diagram, _) = check_reduction(cells, modulus, &columns, limits)?;
    Ok((columns, diagram, additions))
}

fn reduce_matrix(
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, usize), CertificateError> {
    let modulus64 = modulus as u64;
    let mut reduced: Vec<SparseColumn> = Vec::with_capacity(boundaries.len());
    let mut bases: Vec<SparseColumn> = Vec::with_capacity(boundaries.len());
    let mut owners: FxHashMap<usize, usize> = FxHashMap::default();
    let mut additions = 0;
    let mut terms = 0;
    for (target, boundary) in boundaries.iter().enumerate() {
        let mut column = boundary.clone();
        let mut basis = SparseColumn::default();
        basis.insert(target, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = owners.get(&pivot) else {
                break;
            };
            let owner_coefficient = reduced[owner].pivot().unwrap().1;
            let factor = (modulus64
                - coefficient * inverse_mod(owner_coefficient, modulus64) % modulus64)
                % modulus64;
            column.add_scaled(&reduced[owner], factor, modulus64);
            basis.add_scaled(&bases[owner], factor, modulus64);
            additions += 1;
        }
        if let Some((pivot, _)) = column.pivot() {
            owners.insert(pivot, target);
        }
        terms += basis.0.len();
        if terms > limits.max_terms {
            return Err(CertificateError::new(
                "relative core reduction exceeds the term limit",
            ));
        }
        reduced.push(column);
        bases.push(basis);
    }
    Ok((
        bases
            .into_iter()
            .map(|basis| ChangeColumn {
                terms: basis
                    .0
                    .into_iter()
                    .map(|(index, coefficient)| CertificateTerm {
                        index,
                        coefficient: coefficient as u32,
                    })
                    .collect(),
            })
            .collect(),
        additions,
    ))
}

fn check_reduction(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
    columns: &[Vec<ChangeColumn>],
    limits: CertificateLimits,
) -> Result<(Diagram, Vec<Vec<SparseColumn>>), CertificateError> {
    let boundaries = boundary_matrices(cells, modulus)?;
    check_reduction_dimension_count(columns, &boundaries)?;
    let mut total_terms = 0;
    let reduced = replay_reduction(&boundaries, columns, modulus, limits, &mut total_terms)?;
    let mut diagram = diagram_from_reduction(cells, columns, &reduced);
    check_bar_count(&diagram, limits.max_bars)?;
    diagram.canonicalize();
    Ok((diagram, reduced))
}

fn check_reduction_dimension_count(
    columns: &[Vec<ChangeColumn>],
    boundaries: &[Vec<SparseColumn>],
) -> Result<(), CertificateError> {
    if columns.len() != boundaries.len() {
        return Err(CertificateError::new(
            "relative reduction has the wrong dimension count",
        ));
    }
    Ok(())
}

fn replay_reduction(
    boundaries: &[Vec<SparseColumn>],
    columns: &[Vec<ChangeColumn>],
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<Vec<SparseColumn>>, CertificateError> {
    let mut reduced = Vec::with_capacity(columns.len());
    for (dimension, (matrix, transforms)) in boundaries.iter().zip(columns).enumerate() {
        reduced.push(replay_reduction_dimension(
            matrix,
            transforms,
            dimension,
            modulus,
            limits.max_terms,
            total_terms,
        )?);
    }
    Ok(reduced)
}

fn replay_reduction_dimension(
    matrix: &[SparseColumn],
    transforms: &[ChangeColumn],
    dimension: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, CertificateError> {
    if matrix.len() != transforms.len() {
        return Err(CertificateError::new(format!(
            "relative boundary dimension {} has the wrong column count",
            dimension + 1
        )));
    }
    let mut reduced = Vec::with_capacity(matrix.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in transforms.iter().enumerate() {
        reduced.push(replay_transform(
            matrix,
            transform,
            target,
            modulus,
            term_limit,
            total_terms,
            &mut pivots,
        )?);
    }
    Ok(reduced)
}

fn replay_transform(
    matrix: &[SparseColumn],
    transform: &ChangeColumn,
    target: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
    pivots: &mut BTreeSet<usize>,
) -> Result<SparseColumn, CertificateError> {
    add_change_terms(total_terms, transform.terms.len(), term_limit)?;
    check_unit_triangular(transform, target)?;
    let column = apply_change_column(matrix, transform, target, modulus)?;
    check_unique_pivot(&column, pivots)?;
    Ok(column)
}

fn add_change_terms(total: &mut usize, count: usize, limit: usize) -> Result<(), CertificateError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("relative change term count overflows"))?;
    if *total > limit {
        return Err(CertificateError::new(
            "relative change of basis exceeds the term limit",
        ));
    }
    Ok(())
}

fn check_unit_triangular(transform: &ChangeColumn, target: usize) -> Result<(), CertificateError> {
    if transform.terms.last()
        != Some(&CertificateTerm {
            index: target,
            coefficient: 1,
        })
    {
        return Err(CertificateError::new(
            "relative change of basis is not bounded unit triangular",
        ));
    }
    Ok(())
}

fn apply_change_column(
    matrix: &[SparseColumn],
    transform: &ChangeColumn,
    target: usize,
    modulus: u32,
) -> Result<SparseColumn, CertificateError> {
    let mut previous = None;
    let mut column = SparseColumn::default();
    for term in &transform.terms {
        check_change_term(term, target, previous, modulus)?;
        previous = Some(term.index);
        column.add_scaled(&matrix[term.index], term.coefficient as u64, modulus as u64);
    }
    Ok(column)
}

fn check_change_term(
    term: &CertificateTerm,
    target: usize,
    previous: Option<usize>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let invalid = term.index > target
        || previous.is_some_and(|value| value >= term.index)
        || term.coefficient == 0
        || term.coefficient >= modulus;
    if invalid {
        return Err(CertificateError::new(
            "relative change of basis is not canonical",
        ));
    }
    Ok(())
}

fn check_unique_pivot(
    column: &SparseColumn,
    pivots: &mut BTreeSet<usize>,
) -> Result<(), CertificateError> {
    if let Some((pivot, _)) = column.pivot()
        && !pivots.insert(pivot)
    {
        return Err(CertificateError::new(
            "relative reduced matrix repeats a pivot",
        ));
    }
    Ok(())
}

fn diagram_from_reduction(
    cells: &[Vec<InterfaceCell>],
    columns: &[Vec<ChangeColumn>],
    reduced: &[Vec<SparseColumn>],
) -> Diagram {
    let mut diagram = Diagram::default();
    for dimension in 0..columns.len() {
        add_dimension_bars(&mut diagram, cells, reduced, dimension);
    }
    diagram
}

fn add_dimension_bars(
    diagram: &mut Diagram,
    cells: &[Vec<InterfaceCell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) {
    let births = birth_columns(cells, reduced, dimension);
    let deaths: FxHashMap<_, _> = reduced[dimension]
        .iter()
        .enumerate()
        .filter_map(|(column, value)| value.pivot().map(|(row, _)| (row, column)))
        .collect();
    for (position, is_birth) in births.into_iter().enumerate() {
        if is_birth {
            push_relative_bar(diagram, cells, dimension, position, &deaths);
        }
    }
}

fn birth_columns(
    cells: &[Vec<InterfaceCell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) -> Vec<bool> {
    if dimension == 0 {
        return vec![true; cells[0].len()];
    }
    reduced[dimension - 1]
        .iter()
        .map(|column| column.0.is_empty())
        .collect()
}

fn push_relative_bar(
    diagram: &mut Diagram,
    cells: &[Vec<InterfaceCell>],
    dimension: usize,
    birth_position: usize,
    deaths: &FxHashMap<usize, usize>,
) {
    let birth = cells[dimension][birth_position].value;
    let death = deaths
        .get(&birth_position)
        .map_or(f64::INFINITY, |&position| {
            cells[dimension + 1][position].value
        });
    if death > birth {
        diagram.bars.push(Bar {
            dim: dimension,
            birth,
            death,
        });
    }
}

fn check_bar_count(diagram: &Diagram, limit: usize) -> Result<(), CertificateError> {
    if diagram.bars.len() > limit {
        return Err(CertificateError::new(
            "relative interface diagram exceeds the bar limit",
        ));
    }
    Ok(())
}

fn boundary_matrices(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
) -> Result<Vec<Vec<SparseColumn>>, CertificateError> {
    let rows: Vec<BTreeMap<_, _>> = cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, cell)| (cell.vertices.clone(), position))
                .collect()
        })
        .collect();
    (1..cells.len())
        .map(|dimension| {
            cells[dimension]
                .iter()
                .map(|cell| {
                    let mut column = SparseColumn::default();
                    for term in &cell.boundary {
                        if term.coefficient == 0 || term.coefficient >= modulus {
                            return Err(CertificateError::new(
                                "relative boundary coefficient is outside the field",
                            ));
                        }
                        let row = rows[dimension - 1].get(&term.cell).ok_or_else(|| {
                            CertificateError::new(format!(
                                "relative boundary of {:?} references absent cell {:?}",
                                cell.vertices, term.cell
                            ))
                        })?;
                        column.insert(*row, term.coefficient as u64);
                    }
                    Ok(column)
                })
                .collect()
        })
        .collect()
}

fn check_chain_complex(
    cells: &[Vec<InterfaceCell>],
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    check_cell_dimension_count(cells.len(), max_dim)?;
    enforce_cell_limits(cells, limits)?;
    let maps = cell_reference_maps(cells);
    for (dimension, dimension_cells) in cells.iter().enumerate() {
        check_cell_dimension(dimension_cells, dimension, &maps, modulus)?;
    }
    check_boundary_squared(cells, &maps, modulus)?;
    Ok(())
}

fn check_cell_dimension_count(count: usize, max_dim: usize) -> Result<(), CertificateError> {
    if count != max_dim + 2 {
        return Err(CertificateError::new(
            "relative interface has the wrong dimension count",
        ));
    }
    Ok(())
}

fn cell_reference_maps(cells: &[Vec<InterfaceCell>]) -> Vec<BTreeMap<Vec<usize>, &InterfaceCell>> {
    cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect()
        })
        .collect()
}

fn check_cell_dimension(
    cells: &[InterfaceCell],
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut previous = None;
    for cell in cells {
        check_interface_cell(cell, dimension, previous, maps, modulus)?;
        previous = Some(cell);
    }
    if maps[dimension].len() != cells.len() {
        return Err(CertificateError::new("relative interface repeats a cell"));
    }
    Ok(())
}

fn check_interface_cell(
    cell: &InterfaceCell,
    dimension: usize,
    previous: Option<&InterfaceCell>,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    check_interface_cell_shape(cell, dimension, previous)?;
    check_boundary_terms(cell, dimension, maps, modulus)
}

fn check_interface_cell_shape(
    cell: &InterfaceCell,
    dimension: usize,
    previous: Option<&InterfaceCell>,
) -> Result<(), CertificateError> {
    let invalid = cell.vertices.len() != dimension + 1
        || cell.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || !cell.value.is_finite()
        || cell.value < 0.0
        || previous.is_some_and(|prior| cell_order(prior, cell).is_gt());
    if invalid {
        return Err(CertificateError::new(
            "relative interface cell order or value is invalid",
        ));
    }
    Ok(())
}

fn check_boundary_terms(
    cell: &InterfaceCell,
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut previous = None;
    for term in &cell.boundary {
        let face = find_boundary_face(cell, term, dimension, maps)?;
        check_boundary_term(term, face, cell.value, previous, modulus)?;
        previous = Some(term.cell.as_slice());
    }
    Ok(())
}

fn find_boundary_face<'a>(
    cell: &InterfaceCell,
    term: &InterfaceChainTerm,
    dimension: usize,
    maps: &'a [BTreeMap<Vec<usize>, &InterfaceCell>],
) -> Result<&'a InterfaceCell, CertificateError> {
    maps.get(dimension.wrapping_sub(1))
        .and_then(|rows| rows.get(&term.cell))
        .copied()
        .ok_or_else(|| {
            CertificateError::new(format!(
                "relative boundary of {:?} references absent cell {:?}",
                cell.vertices, term.cell
            ))
        })
}

fn check_boundary_term(
    term: &InterfaceChainTerm,
    face: &InterfaceCell,
    value: f64,
    previous: Option<&[usize]>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let invalid = term.coefficient == 0
        || term.coefficient >= modulus
        || face.value > value
        || previous.is_some_and(|prior| prior >= term.cell.as_slice());
    if invalid {
        return Err(CertificateError::new(
            "relative boundary term is not canonical or filtered",
        ));
    }
    Ok(())
}

fn check_boundary_squared(
    cells: &[Vec<InterfaceCell>],
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    for (dimension, dimension_cells) in cells.iter().enumerate().skip(2) {
        for cell in dimension_cells {
            check_cell_boundary_squared(cell, dimension, maps, modulus)?;
        }
    }
    Ok(())
}

fn check_cell_boundary_squared(
    cell: &InterfaceCell,
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut square = BTreeMap::<Vec<usize>, u64>::new();
    for term in &cell.boundary {
        let face = maps[dimension - 1][&term.cell];
        add_squared_boundary(&mut square, term.coefficient, &face.boundary, modulus);
    }
    if !square.is_empty() {
        return Err(CertificateError::new(
            "relative interface boundary does not square to zero",
        ));
    }
    Ok(())
}

fn add_squared_boundary(
    square: &mut BTreeMap<Vec<usize>, u64>,
    coefficient: u32,
    boundary: &[InterfaceChainTerm],
    modulus: u32,
) {
    for lower in boundary {
        let value = (square.get(&lower.cell).copied().unwrap_or(0)
            + coefficient as u64 * lower.coefficient as u64)
            % modulus as u64;
        if value == 0 {
            square.remove(&lower.cell);
        } else {
            square.insert(lower.cell.clone(), value);
        }
    }
}

fn check_protected_cells(
    input: &[Vec<InterfaceCell>],
    core: &[Vec<InterfaceCell>],
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    for (input_dimension, core_dimension) in input.iter().zip(core) {
        let core_map: BTreeMap<_, _> = core_dimension
            .iter()
            .map(|cell| (&cell.vertices, cell))
            .collect();
        for cell in input_dimension {
            if is_protected(&cell.vertices, protected)
                && core_map.get(&cell.vertices).copied() != Some(cell)
            {
                return Err(CertificateError::new(
                    "relative interface does not fix its protected subcomplex",
                ));
            }
        }
    }
    Ok(())
}

fn is_protected(cell: &[usize], protected: &BTreeSet<usize>) -> bool {
    !protected.is_empty() && cell.iter().all(|vertex| protected.contains(vertex))
}

fn canonical_vertices(vertices: &[usize]) -> Result<Vec<usize>, CertificateError> {
    let mut output = vertices.to_vec();
    output.sort_unstable();
    output.dedup();
    if output.len() != vertices.len() {
        return Err(CertificateError::new(
            "protected vertex list contains a duplicate",
        ));
    }
    Ok(output)
}

fn validate_parameters(
    input: &SparseDistanceMatrix,
    labels: &[usize],
    params: &RipsParams,
    protected: &[usize],
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if labels.len() != input.len()
        || labels.windows(2).any(|pair| pair[0] >= pair[1])
        || labels.len() > limits.max_vertices
    {
        return Err(CertificateError::new(
            "relative interface labels must be unique, ordered, and bounded",
        ));
    }
    let label_set: BTreeSet<_> = labels.iter().copied().collect();
    if protected.iter().any(|vertex| !label_set.contains(vertex)) {
        return Err(CertificateError::new(
            "protected vertex is outside the interface scope",
        ));
    }
    if params.max_dim > limits.max_dimension
        || u64::from(params.modulus) >= MODULUS_LIMIT
        || !is_prime(params.modulus as u64)
    {
        return Err(CertificateError::new(
            "relative interface dimension or coefficient field is invalid",
        ));
    }
    checked_threshold(params.threshold)?;
    Ok(())
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64, CertificateError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 {
        return Err(CertificateError::new(
            "relative interface threshold must be non-negative",
        ));
    }
    Ok(value)
}

fn enforce_cell_limits(
    cells: &[Vec<InterfaceCell>],
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    for (dimension, values) in cells.iter().enumerate() {
        enforce_dimension_limit(dimension, values.len(), limits)?;
    }
    Ok(())
}

fn enforce_dimension_limit(
    dimension: usize,
    count: usize,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    let limit = match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    };
    if count > limit {
        return Err(CertificateError::new(format!(
            "relative dimension {dimension} cell count exceeds the limit {limit}"
        )));
    }
    Ok(())
}

fn count_cells(cells: &[Vec<InterfaceCell>]) -> usize {
    cells.iter().map(Vec::len).sum()
}

fn cell_order(left: &InterfaceCell, right: &InterfaceCell) -> std::cmp::Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
}

fn certificate_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<InterfaceCell>],
    columns: &[Vec<ChangeColumn>],
    diagram: &Diagram,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-relative-interface-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, protected);
    for dimension in cells {
        hash.update((dimension.len() as u64).to_be_bytes());
        for cell in dimension {
            digest_usizes(&mut hash, &cell.vertices);
            hash.update(cell.value.to_bits().to_be_bytes());
            hash.update((cell.boundary.len() as u64).to_be_bytes());
            for term in &cell.boundary {
                digest_usizes(&mut hash, &term.cell);
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    for dimension in columns {
        hash.update((dimension.len() as u64).to_be_bytes());
        for column in dimension {
            hash.update((column.terms.len() as u64).to_be_bytes());
            for term in &column.terms {
                hash.update((term.index as u64).to_be_bytes());
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    hash.update((diagram.bars.len() as u64).to_be_bytes());
    for bar in &diagram.bars {
        hash.update((bar.dim as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn source_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<InterfaceCell>],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-relative-interface-source-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, protected);
    for dimension in cells {
        hash.update((dimension.len() as u64).to_be_bytes());
        for cell in dimension {
            digest_usizes(&mut hash, &cell.vertices);
            hash.update(cell.value.to_bits().to_be_bytes());
            hash.update((cell.boundary.len() as u64).to_be_bytes());
            for term in &cell.boundary {
                digest_usizes(&mut hash, &term.cell);
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    hash.finalize().into()
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
    }
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
    let mut result = 1;
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

fn encode_cells(
    output: &mut Vec<u8>,
    cells: &[Vec<InterfaceCell>],
) -> Result<(), CertificateError> {
    put_usize(output, cells.len())?;
    for dimension in cells {
        put_usize(output, dimension.len())?;
        for cell in dimension {
            encode_key(output, &cell.vertices)?;
            output.extend_from_slice(&cell.value.to_bits().to_be_bytes());
            put_usize(output, cell.boundary.len())?;
            for term in &cell.boundary {
                encode_key(output, &term.cell)?;
                output.extend_from_slice(&term.coefficient.to_be_bytes());
            }
        }
    }
    Ok(())
}

fn encode_key(output: &mut Vec<u8>, key: &[usize]) -> Result<(), CertificateError> {
    put_usize(output, key.len())?;
    for vertex in key {
        put_usize(output, *vertex)?;
    }
    Ok(())
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), CertificateError> {
    let value = u64::try_from(value)
        .map_err(|_| CertificateError::new("relative interface integer does not fit u64"))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn decode_cells(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    check_decoded_dimension_count(reader, "cell", max_dim + 2)?;
    let mut cells = Vec::with_capacity(max_dim + 2);
    let mut total_terms = 0usize;
    for dimension in 0..=max_dim + 1 {
        cells.push(decode_cell_dimension(
            reader,
            dimension,
            modulus,
            limits,
            &mut total_terms,
        )?);
    }
    Ok(cells)
}

fn check_decoded_dimension_count(
    reader: &mut Reader<'_>,
    label: &str,
    expected: usize,
) -> Result<(), CertificateError> {
    if reader.bounded_usize(&format!("{label} dimension count"), expected)? != expected {
        return Err(CertificateError::new(format!(
            "relative interface has the wrong {label} dimension count"
        )));
    }
    Ok(())
}

fn dimension_cell_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn decode_cell_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<InterfaceCell>, CertificateError> {
    let count = reader.bounded_usize("cell count", dimension_cell_limit(dimension, limits))?;
    let mut cells = Vec::with_capacity(count);
    for _ in 0..count {
        cells.push(decode_cell(
            reader,
            dimension,
            modulus,
            limits,
            total_terms,
        )?);
    }
    Ok(cells)
}

fn decode_cell(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<InterfaceCell, CertificateError> {
    let vertices = decode_key(reader, dimension + 1, limits.max_vertices)?;
    check_decoded_cell_dimension(vertices.len(), dimension)?;
    let value = f64::from_bits(reader.u64()?);
    let boundary_count = reader.bounded_usize("boundary term count", limits.max_terms)?;
    add_decoded_terms(total_terms, boundary_count, limits.max_terms, "boundary")?;
    let boundary = decode_boundary_terms(reader, dimension, boundary_count, limits.max_vertices)?;
    check_decoded_boundary(&boundary, dimension, modulus)?;
    Ok(InterfaceCell {
        vertices,
        value,
        boundary,
    })
}

fn check_decoded_cell_dimension(
    vertex_count: usize,
    dimension: usize,
) -> Result<(), CertificateError> {
    if vertex_count != dimension + 1 {
        return Err(CertificateError::new(
            "relative interface cell has the wrong dimension",
        ));
    }
    Ok(())
}

fn add_decoded_terms(
    total: &mut usize,
    count: usize,
    limit: usize,
    label: &str,
) -> Result<(), CertificateError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new(format!("{label} term count overflows")))?;
    if *total > limit {
        return Err(CertificateError::new(format!(
            "relative {label} terms exceed the limit"
        )));
    }
    Ok(())
}

fn decode_boundary_terms(
    reader: &mut Reader<'_>,
    dimension: usize,
    count: usize,
    max_vertex: usize,
) -> Result<Vec<InterfaceChainTerm>, CertificateError> {
    let mut boundary = Vec::with_capacity(count);
    for _ in 0..count {
        boundary.push(InterfaceChainTerm {
            cell: decode_key(reader, dimension, max_vertex)?,
            coefficient: reader.u32()?,
        });
    }
    Ok(boundary)
}

fn check_decoded_boundary(
    boundary: &[InterfaceChainTerm],
    dimension: usize,
    modulus: u32,
) -> Result<(), CertificateError> {
    let invalid = boundary.iter().any(|term| {
        term.cell.len() != dimension || term.coefficient == 0 || term.coefficient >= modulus
    });
    if invalid {
        return Err(CertificateError::new(
            "relative boundary term has the wrong dimension or coefficient",
        ));
    }
    Ok(())
}

fn decode_columns(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<ChangeColumn>>, CertificateError> {
    check_decoded_dimension_count(reader, "reduction", max_dim + 1)?;
    let mut columns = Vec::with_capacity(max_dim + 1);
    let mut total_terms = 0usize;
    for dimension in 1..=max_dim + 1 {
        columns.push(decode_column_dimension(
            reader,
            dimension,
            modulus,
            limits,
            &mut total_terms,
        )?);
    }
    Ok(columns)
}

fn decode_column_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<ChangeColumn>, CertificateError> {
    let count = reader.bounded_usize(
        "reduction column count",
        dimension_cell_limit(dimension, limits),
    )?;
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        columns.push(decode_change_column(
            reader,
            modulus,
            limits.max_terms,
            total_terms,
        )?);
    }
    Ok(columns)
}

fn decode_change_column(
    reader: &mut Reader<'_>,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<ChangeColumn, CertificateError> {
    let count = reader.bounded_usize("change term count", term_limit)?;
    add_decoded_terms(total_terms, count, term_limit, "change")?;
    let terms = decode_change_terms(reader, count)?;
    check_change_coefficients(&terms, modulus)?;
    Ok(ChangeColumn { terms })
}

fn decode_change_terms(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<CertificateTerm>, CertificateError> {
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        terms.push(CertificateTerm {
            index: reader.usize()?,
            coefficient: reader.u32()?,
        });
    }
    Ok(terms)
}

fn check_change_coefficients(
    terms: &[CertificateTerm],
    modulus: u32,
) -> Result<(), CertificateError> {
    if terms
        .iter()
        .any(|term| term.coefficient == 0 || term.coefficient >= modulus)
    {
        return Err(CertificateError::new(
            "relative change coefficient is outside the field",
        ));
    }
    Ok(())
}

fn decode_key(
    reader: &mut Reader<'_>,
    maximum_len: usize,
    maximum_vertex: usize,
) -> Result<Vec<usize>, CertificateError> {
    let count = reader.bounded_usize("cell key length", maximum_len)?;
    let mut key = Vec::with_capacity(count);
    for _ in 0..count {
        key.push(reader.bounded_usize("cell vertex", maximum_vertex)?);
    }
    if key.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(CertificateError::new(
            "relative interface cell key is not canonical",
        ));
    }
    Ok(key)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CertificateError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| CertificateError::new("relative byte position overflows"))?;
        if end > self.bytes.len() {
            return Err(CertificateError::new("relative interface is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, CertificateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CertificateError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, CertificateError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, CertificateError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn usize(&mut self) -> Result<usize, CertificateError> {
        usize::try_from(self.u64()?)
            .map_err(|_| CertificateError::new("relative wire integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, maximum: usize) -> Result<usize, CertificateError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(CertificateError::new(format!(
                "relative {label} {value} exceeds the limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn array32(&mut self) -> Result<[u8; 32], CertificateError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(vertex_count: usize, edges: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(vertex_count, edges).unwrap()
    }

    #[test]
    fn relative_cancellation_matches_the_implicit_engine() {
        let input = graph(
            6,
            &[
                (0, 1, 0.0),
                (1, 2, 0.0),
                (0, 2, 0.0),
                (2, 3, 1.0),
                (3, 4, 1.0),
                (2, 4, 1.0),
                (0, 5, 2.0),
            ],
        );
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(2).with_modulus(modulus);
            let certificate = RelativeInterfaceCertificate::build(
                &input,
                &params,
                &[0, 1, 2],
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(
                certificate
                    .verify(CertificateLimits::default())
                    .unwrap()
                    .bars,
                rips_persistence_sparse(&input, &params).unwrap().bars
            );
            assert!(
                certificate.core_cells()[0]
                    .iter()
                    .any(|cell| cell.vertices == [0])
            );
        }
    }

    #[test]
    fn composes_through_a_noncontractible_filtered_separator() {
        let left = graph(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 2.0),
                (1, 4, 2.0),
            ],
        );
        let right = graph(
            5,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (2, 4, 2.5),
                (3, 4, 2.5),
            ],
        );
        let complete = graph(
            6,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 2.0),
                (1, 4, 2.0),
                (2, 5, 2.5),
                (3, 5, 2.5),
            ],
        );
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(2).with_modulus(modulus);
            let a = RelativeInterfaceCertificate::build_labeled(
                &left,
                &[0, 1, 2, 3, 4],
                &params,
                &[0, 1, 2, 3],
                CertificateLimits::default(),
            )
            .unwrap();
            let b = RelativeInterfaceCertificate::build_labeled(
                &right,
                &[0, 1, 2, 3, 5],
                &params,
                &[0, 1, 2, 3],
                CertificateLimits::default(),
            )
            .unwrap();
            let composed =
                RelativeInterfaceCertificate::compose(&[&a, &b], &[], CertificateLimits::default())
                    .unwrap();
            assert_eq!(
                composed.diagram().bars,
                rips_persistence_sparse(&complete, &params).unwrap().bars
            );
            assert_eq!(composed.diagram().in_dim(1).count(), 1);
        }
    }
}
