use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::{
    Graph, MODULUS_LIMIT, ProofBar, ProofColumn, ProofError, ProofLimits, ProofTerm, Reader,
    SparseColumn, canonicalize_diagram, check_column, checked_threshold, diagrams_equal, is_prime,
};

const MAGIC: &[u8; 8] = b"HOLOSRI\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Whether bytes start with the relative-interface certificate magic.
pub fn is_relative_interface(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Counts derived from one independently checked relative interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRelativeInterface {
    /// Content identifier of the retained core and reduction.
    pub digest: [u8; 32],
    /// Highest checked homology dimension.
    pub max_dim: usize,
    /// Cells before relative cancellation.
    pub input_cells: usize,
    /// Equal-filtration unit cancellations checked.
    pub cancellations: usize,
    /// Cells in the retained core.
    pub core_cells: usize,
    /// Change-of-basis columns checked.
    pub reduction_columns: usize,
    /// Diagram bars derived from the reduction.
    pub bars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Term {
    cell: Vec<usize>,
    coefficient: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct Cell {
    vertices: Vec<usize>,
    value: f64,
    boundary: Vec<Term>,
}

#[derive(Debug, Clone)]
struct Step {
    upper: Vec<usize>,
    lower: Vec<usize>,
    coefficient: u32,
}

pub(super) struct VerifiedCertificate {
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) protected_vertices: Vec<usize>,
    input: Vec<Vec<Cell>>,
    steps: Vec<Step>,
    core: Vec<Vec<Cell>>,
    pub(super) columns: Vec<Vec<ProofColumn>>,
    pub(super) diagram: Vec<ProofBar>,
    pub(super) digest: [u8; 32],
}

impl VerifiedCertificate {
    pub(super) fn source_digest(&self) -> [u8; 32] {
        source_digest(
            self.max_dim,
            self.modulus,
            &self.protected_vertices,
            &self.input,
        )
    }
}

type DimensionMap = BTreeMap<Vec<usize>, Cell>;

/// Decode and verify one bounded `HOLOSRI` certificate.
pub fn verify_relative_interface(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedRelativeInterface, ProofError> {
    let certificate = decode_verified(bytes, limits)?;
    Ok(relative_summary(&certificate))
}

/// Verify that a parent input is the keyed union of checked child cores.
///
/// This checks one proof-exchange fold. The parent certificate separately
/// proves every cancellation and its final reduction.
pub fn verify_relative_composition(
    parent: &[u8],
    children: &[&[u8]],
    expected_protected_vertices: &[usize],
    limits: ProofLimits,
) -> Result<VerifiedRelativeInterface, ProofError> {
    require_children(children)?;
    let parent = decode_verified(parent, limits)?;
    verify_parent_protected(&parent, expected_protected_vertices)?;
    let mut union = vec![BTreeMap::<Vec<usize>, Cell>::new(); parent.max_dim + 2];
    for bytes in children {
        let child = decode_verified(bytes, limits)?;
        merge_child(&parent, &child, &mut union)?;
    }
    let expected = union_cells(union);
    if expected != parent.input {
        return Err(ProofError::new(
            "relative parent input differs from the keyed child-core union",
        ));
    }
    Ok(relative_summary(&parent))
}

fn relative_summary(certificate: &VerifiedCertificate) -> VerifiedRelativeInterface {
    VerifiedRelativeInterface {
        digest: certificate.digest,
        max_dim: certificate.max_dim,
        input_cells: count_cells(&certificate.input),
        cancellations: certificate.steps.len(),
        core_cells: count_cells(&certificate.core),
        reduction_columns: certificate.columns.iter().map(Vec::len).sum(),
        bars: certificate.diagram.len(),
    }
}

fn require_children(children: &[&[u8]]) -> Result<(), ProofError> {
    if children.is_empty() {
        Err(ProofError::new(
            "relative composition requires a child certificate",
        ))
    } else {
        Ok(())
    }
}

fn verify_parent_protected(
    parent: &VerifiedCertificate,
    expected: &[usize],
) -> Result<(), ProofError> {
    if expected.windows(2).any(|pair| pair[0] >= pair[1]) || parent.protected_vertices != expected {
        Err(ProofError::new(
            "relative parent has the wrong protected vertex set",
        ))
    } else {
        Ok(())
    }
}

fn merge_child(
    parent: &VerifiedCertificate,
    child: &VerifiedCertificate,
    union: &mut [DimensionMap],
) -> Result<(), ProofError> {
    if child.max_dim != parent.max_dim || child.modulus != parent.modulus {
        return Err(ProofError::new(
            "relative composition changes dimension or coefficient field",
        ));
    }
    for (dimension, cells) in child.core.iter().enumerate() {
        for cell in cells {
            merge_child_cell(&mut union[dimension], cell)?;
        }
    }
    Ok(())
}

fn merge_child_cell(dimension: &mut DimensionMap, cell: &Cell) -> Result<(), ProofError> {
    match dimension.get(&cell.vertices) {
        Some(existing) if existing != cell => Err(ProofError::new(
            "relative composition identifies conflicting cells",
        )),
        Some(_) => Ok(()),
        None => {
            dimension.insert(cell.vertices.clone(), cell.clone());
            Ok(())
        }
    }
}

fn union_cells(union: Vec<DimensionMap>) -> Vec<Vec<Cell>> {
    union
        .into_iter()
        .map(|dimension| {
            let mut cells = dimension.into_values().collect::<Vec<_>>();
            cells.sort_by(cell_order);
            cells
        })
        .collect()
}

pub(super) fn decode_verified(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCertificate, ProofError> {
    let certificate = decode_certificate(bytes, limits)?;
    verify_certificate(&certificate, limits)?;
    Ok(certificate)
}

struct CertificateHeader {
    max_dim: usize,
    modulus: u32,
    protected_vertices: Vec<usize>,
}

fn decode_certificate(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCertificate, ProofError> {
    if bytes.len() > limits.max_bytes {
        return Err(ProofError::new(format!(
            "{} bytes exceed the limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_certificate_header(&mut reader, limits)?;
    let input = decode_cells(&mut reader, header.max_dim, header.modulus, limits)?;
    let input_count = count_cells(&input);
    let steps = decode_steps(&mut reader, header.max_dim, input_count, limits)?;
    let core = decode_cells(&mut reader, header.max_dim, header.modulus, limits)?;
    let columns = decode_columns(&mut reader, header.max_dim, header.modulus, limits)?;
    let diagram = decode_diagram(&mut reader, header.max_dim, limits)?;
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "trailing bytes after the relative-interface certificate",
        ));
    }

    Ok(VerifiedCertificate {
        digest,
        max_dim: header.max_dim,
        modulus: header.modulus,
        protected_vertices: header.protected_vertices,
        input,
        steps,
        core,
        columns,
        diagram,
    })
}

fn decode_certificate_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<CertificateHeader, ProofError> {
    decode_certificate_prefix(reader)?;
    let max_dim = reader.bounded_usize("relative dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let protected_vertices = decode_protected_vertices(reader, limits)?;
    Ok(CertificateHeader {
        max_dim,
        modulus,
        protected_vertices,
    })
}

fn decode_certificate_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC {
        return Err(ProofError::new("wrong relative-interface magic bytes"));
    }
    if reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new(
            "unsupported relative-interface wire version or scalar codec",
        ));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(
            "relative-interface modulus is not a supported prime",
        ))
    } else {
        Ok(())
    }
}

fn decode_protected_vertices(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("protected vertex count", limits.max_vertices)?;
    let vertices = (0..count)
        .map(|_| reader.usize())
        .collect::<Result<Vec<_>, _>>()?;
    if vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(ProofError::new(
            "relative-interface protected vertices are not canonical",
        ))
    } else {
        Ok(vertices)
    }
}

fn decode_steps(
    reader: &mut Reader<'_>,
    max_dim: usize,
    input_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Step>, ProofError> {
    let count = reader.bounded_usize("relative cancellation count", input_count / 2)?;
    (0..count)
        .map(|_| {
            Ok(Step {
                upper: decode_key(reader, max_dim + 2, limits.max_vertices)?,
                lower: decode_key(reader, max_dim + 1, limits.max_vertices)?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    max_dim: usize,
    limits: ProofLimits,
) -> Result<Vec<ProofBar>, ProofError> {
    let count = reader.bounded_usize("relative bar count", limits.max_bars)?;
    (0..count)
        .map(|_| {
            Ok(ProofBar {
                dimension: reader.bounded_usize("bar dimension", max_dim)?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}

fn verify_certificate(
    certificate: &VerifiedCertificate,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    check_chain(&certificate.input, certificate.max_dim, certificate.modulus)?;
    check_chain(&certificate.core, certificate.max_dim, certificate.modulus)?;
    let protected = certificate
        .protected_vertices
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let replayed = replay(
        certificate.input.clone(),
        &certificate.steps,
        &protected,
        certificate.modulus,
    )?;
    verify_replay(certificate, replayed, &protected)?;
    verify_certificate_reduction(certificate, limits)?;
    verify_certificate_digest(certificate)
}

fn verify_replay(
    certificate: &VerifiedCertificate,
    replayed: Vec<Vec<Cell>>,
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    if replayed != certificate.core {
        return Err(ProofError::new(
            "relative cancellation trace does not produce the declared core",
        ));
    }
    check_protected(&certificate.input, &certificate.core, protected)
}

fn verify_certificate_reduction(
    certificate: &VerifiedCertificate,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    let checked = check_reduction(
        &certificate.core,
        certificate.modulus,
        &certificate.columns,
        limits,
    )?;
    if !diagrams_equal(&checked, &certificate.diagram) {
        return Err(ProofError::new(
            "relative-interface diagram differs from the checked reduction",
        ));
    }
    Ok(())
}

fn verify_certificate_digest(certificate: &VerifiedCertificate) -> Result<(), ProofError> {
    let computed = certificate_digest(
        certificate.max_dim,
        certificate.modulus,
        &certificate.protected_vertices,
        &certificate.core,
        &certificate.columns,
        &certificate.diagram,
    );
    if computed != certificate.digest {
        Err(ProofError::new(
            "relative-interface digest differs from checked content",
        ))
    } else {
        Ok(())
    }
}

pub(super) struct IndexLeafContext<'a> {
    pub(super) graph: &'a Graph,
    pub(super) labels: &'a [usize],
    pub(super) threshold: Option<f64>,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) protected_vertices: &'a [usize],
    pub(super) limits: ProofLimits,
}

pub(super) fn verify_index_leaf(
    bytes: &[u8],
    context: IndexLeafContext<'_>,
) -> Result<VerifiedCertificate, ProofError> {
    let certificate = decode_verified(bytes, context.limits)?;
    if certificate.max_dim != context.max_dim
        || certificate.modulus != context.modulus
        || certificate.protected_vertices != context.protected_vertices
    {
        return Err(ProofError::new(
            "relative leaf changes the dimension, field, or protected vertices",
        ));
    }
    let expected = enumerate_flag_cells(
        context.graph,
        context.labels,
        context.threshold,
        context.max_dim,
        context.modulus,
        context.limits,
    )?;
    if certificate.input != expected {
        let dimension = certificate
            .input
            .iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(0);
        return Err(ProofError::new(format!(
            "relative leaf input differs from its induced filtered flag complex in dimension {dimension}: certificate has {:?}, graph has {:?}",
            certificate.input[dimension], expected[dimension]
        )));
    }
    Ok(certificate)
}

fn enumerate_flag_cells(
    graph: &Graph,
    labels: &[usize],
    threshold: Option<f64>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    if labels.len() > limits.max_vertices {
        return Err(ProofError::new("relative leaf exceeds the vertex limit"));
    }
    let mut cells = vec![vertex_cells(labels)];
    for dimension in 1..=max_dim + 1 {
        let next = enumerate_dimension(
            graph,
            labels,
            threshold,
            dimension,
            &cells[dimension - 1],
            dimension_limit(dimension, limits),
            modulus,
        )?;
        cells.push(next);
    }
    Ok(cells)
}

fn vertex_cells(labels: &[usize]) -> Vec<Cell> {
    let mut vertices = labels
        .iter()
        .map(|&vertex| Cell {
            vertices: vec![vertex],
            value: 0.0,
            boundary: Vec::new(),
        })
        .collect::<Vec<_>>();
    vertices.sort_by(cell_order);
    vertices
}

fn enumerate_dimension(
    graph: &Graph,
    labels: &[usize],
    threshold: f64,
    dimension: usize,
    previous: &[Cell],
    limit: usize,
    modulus: u32,
) -> Result<Vec<Cell>, ProofError> {
    let mut next = Vec::new();
    for simplex in previous {
        let start = labels
            .binary_search(simplex.vertices.last().expect("a simplex is nonempty"))
            .expect("the preceding simplex uses declared labels")
            + 1;
        for &vertex in &labels[start..] {
            if let Some(cell) = extend_simplex(graph, simplex, vertex, threshold, modulus) {
                if next.len() == limit {
                    return Err(ProofError::new(format!(
                        "relative leaf dimension {dimension} exceeds the simplex limit"
                    )));
                }
                next.push(cell);
            }
        }
    }
    next.sort_by(cell_order);
    Ok(next)
}

fn extend_simplex(
    graph: &Graph,
    simplex: &Cell,
    vertex: usize,
    threshold: f64,
    modulus: u32,
) -> Option<Cell> {
    let value = simplex_value(graph, simplex, vertex, threshold)?;
    let mut vertices = simplex.vertices.clone();
    vertices.push(vertex);
    Some(Cell {
        boundary: simplex_boundary(&vertices, modulus),
        vertices,
        value,
    })
}

fn simplex_value(graph: &Graph, simplex: &Cell, vertex: usize, threshold: f64) -> Option<f64> {
    let mut value = simplex.value;
    for &member in &simplex.vertices {
        let edge = graph.get(member, vertex);
        if !edge.is_finite() || edge > threshold {
            return None;
        }
        value = value.max(edge);
    }
    Some(value)
}

fn simplex_boundary(vertices: &[usize], modulus: u32) -> Vec<Term> {
    let mut boundary = (0..vertices.len())
        .map(|removed| {
            let mut cell = vertices.to_vec();
            cell.remove(removed);
            Term {
                cell,
                coefficient: if removed % 2 == 0 { 1 } else { modulus - 1 },
            }
        })
        .collect::<Vec<_>>();
    boundary.sort();
    boundary
}

fn decode_cells(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    if reader.bounded_usize("cell dimension count", max_dim + 2)? != max_dim + 2 {
        return Err(ProofError::new(
            "relative-interface cell dimension count is wrong",
        ));
    }
    let mut cells = Vec::with_capacity(max_dim + 2);
    let mut total_terms = 0usize;
    for dimension in 0..=max_dim + 1 {
        cells.push(decode_cell_dimension(
            reader,
            dimension,
            modulus,
            &mut total_terms,
            limits,
        )?);
    }
    Ok(cells)
}

fn decode_cell_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Cell>, ProofError> {
    let limit = dimension_limit(dimension, limits);
    let count = reader.bounded_usize("dimension cell count", limit)?;
    (0..count)
        .map(|_| decode_cell(reader, dimension, modulus, total_terms, limit, limits))
        .collect()
}

fn decode_cell(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    term_limit: usize,
    limits: ProofLimits,
) -> Result<Cell, ProofError> {
    let vertices = decode_key(reader, dimension + 1, limits.max_vertices)?;
    if vertices.len() != dimension + 1 {
        return Err(ProofError::new(
            "relative-interface cell has the wrong dimension",
        ));
    }
    let value = f64::from_bits(reader.u64()?);
    let boundary = decode_boundary(reader, dimension, modulus, total_terms, term_limit, limits)?;
    Ok(Cell {
        vertices,
        value,
        boundary,
    })
}

fn decode_boundary(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    term_limit: usize,
    limits: ProofLimits,
) -> Result<Vec<Term>, ProofError> {
    let count = reader.bounded_usize("cell boundary term count", term_limit)?;
    add_term_count(total_terms, count, limits.max_terms, "relative boundary")?;
    let boundary = (0..count)
        .map(|_| {
            Ok(Term {
                cell: decode_key(reader, dimension, limits.max_vertices)?,
                coefficient: reader.u32()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    if boundary.iter().any(|term| {
        term.cell.len() != dimension || term.coefficient == 0 || term.coefficient >= modulus
    }) {
        Err(ProofError::new(
            "relative boundary term has the wrong dimension or coefficient",
        ))
    } else {
        Ok(boundary)
    }
}

fn add_term_count(
    total: &mut usize,
    add: usize,
    maximum: usize,
    kind: &str,
) -> Result<(), ProofError> {
    *total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new(format!("{kind} term count overflows")))?;
    if *total > maximum {
        Err(ProofError::new(format!("{kind} terms exceed the limit")))
    } else {
        Ok(())
    }
}

fn decode_columns(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<ProofColumn>>, ProofError> {
    if reader.bounded_usize("reduction dimension count", max_dim + 1)? != max_dim + 1 {
        return Err(ProofError::new(
            "relative reduction dimension count is wrong",
        ));
    }
    let mut columns = Vec::with_capacity(max_dim + 1);
    let mut total_terms = 0usize;
    for dimension in 1..=max_dim + 1 {
        columns.push(decode_column_dimension(
            reader,
            dimension,
            modulus,
            &mut total_terms,
            limits,
        )?);
    }
    Ok(columns)
}

fn decode_column_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<ProofColumn>, ProofError> {
    let count =
        reader.bounded_usize("reduction column count", dimension_limit(dimension, limits))?;
    (0..count)
        .map(|target| decode_column(reader, target, modulus, total_terms, limits))
        .collect()
}

fn decode_column(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofColumn, ProofError> {
    let count = reader.bounded_usize("change term count", limits.max_terms)?;
    add_term_count(total_terms, count, limits.max_terms, "change")?;
    let terms = (0..count)
        .map(|_| {
            Ok(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    check_column(target, &terms, modulus)?;
    Ok(ProofColumn { terms })
}

fn decode_key(
    reader: &mut Reader<'_>,
    maximum_len: usize,
    maximum_vertex: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("cell key length", maximum_len)?;
    let mut key = Vec::with_capacity(count);
    for _ in 0..count {
        key.push(reader.bounded_usize("cell vertex", maximum_vertex)?);
    }
    if key.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ProofError::new("cell key is not canonical"));
    }
    Ok(key)
}

fn replay(
    cells: Vec<Vec<Cell>>,
    steps: &[Step],
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    let mut maps = cells_to_maps(cells)?;
    for step in steps {
        apply_step(&mut maps, step, protected, modulus)?;
    }
    Ok(maps_to_cells(maps))
}

fn apply_step(
    cells: &mut [DimensionMap],
    step: &Step,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(), ProofError> {
    let dimension = cancellation_dimension(cells.len(), step)?;
    verify_unprotected(step, protected)?;
    let upper = prepare_cancellation(cells, step, dimension, modulus)?;
    eliminate_lower(cells, step, dimension, &upper, modulus);
    remove_upper_from_cofaces(cells, step, dimension);
    cells[dimension].remove(&step.upper);
    cells[dimension - 1].remove(&step.lower);
    Ok(())
}

fn cancellation_dimension(cell_dimensions: usize, step: &Step) -> Result<usize, ProofError> {
    let dimension = step
        .upper
        .len()
        .checked_sub(1)
        .ok_or_else(|| ProofError::new("relative cancellation upper cell is empty"))?;
    if dimension == 0 || step.lower.len() != dimension || dimension >= cell_dimensions {
        Err(ProofError::new(
            "relative cancellation dimensions are incompatible",
        ))
    } else {
        Ok(dimension)
    }
}

fn verify_unprotected(step: &Step, protected: &BTreeSet<usize>) -> Result<(), ProofError> {
    if is_protected(&step.upper, protected) || is_protected(&step.lower, protected) {
        Err(ProofError::new(
            "relative cancellation removes a protected cell",
        ))
    } else {
        Ok(())
    }
}

fn prepare_cancellation(
    cells: &[DimensionMap],
    step: &Step,
    dimension: usize,
    modulus: u32,
) -> Result<Cell, ProofError> {
    let upper = cells[dimension]
        .get(&step.upper)
        .cloned()
        .ok_or_else(|| ProofError::new("relative cancellation upper cell is absent"))?;
    let lower = cells[dimension - 1]
        .get(&step.lower)
        .ok_or_else(|| ProofError::new("relative cancellation lower cell is absent"))?;
    if upper.value.to_bits() != lower.value.to_bits() {
        return Err(ProofError::new(
            "relative cancellation crosses a filtration value",
        ));
    }
    let coefficient = boundary_coefficient(&upper.boundary, &step.lower)
        .ok_or_else(|| ProofError::new("relative cancellation cells are not incident"))?;
    if coefficient != step.coefficient || coefficient == 0 || coefficient >= modulus {
        return Err(ProofError::new(
            "relative cancellation coefficient is wrong",
        ));
    }
    Ok(upper)
}

fn eliminate_lower(
    cells: &mut [DimensionMap],
    step: &Step,
    dimension: usize,
    upper: &Cell,
    modulus: u32,
) {
    let inverse = inverse_mod(u64::from(step.coefficient), u64::from(modulus)) as u32;
    let keys: Vec<_> = cells[dimension].keys().cloned().collect();
    for key in keys {
        if key == step.upper {
            continue;
        }
        let cell = cells[dimension].get_mut(&key).unwrap();
        let Some(value) = boundary_coefficient(&cell.boundary, &step.lower) else {
            continue;
        };
        let factor = ((u64::from(modulus)
            - u64::from(value) * u64::from(inverse) % u64::from(modulus))
            % u64::from(modulus)) as u32;
        add_boundary_scaled(&mut cell.boundary, &upper.boundary, factor, modulus);
    }
}

fn remove_upper_from_cofaces(cells: &mut [DimensionMap], step: &Step, dimension: usize) {
    if dimension + 1 < cells.len() {
        for cell in cells[dimension + 1].values_mut() {
            remove_boundary_term(&mut cell.boundary, &step.upper);
        }
    }
}

fn cells_to_maps(cells: Vec<Vec<Cell>>) -> Result<Vec<DimensionMap>, ProofError> {
    cells
        .into_iter()
        .map(|dimension| {
            let expected = dimension.len();
            let map: DimensionMap = dimension
                .into_iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect();
            if map.len() == expected {
                Ok(map)
            } else {
                Err(ProofError::new("relative interface repeats a cell"))
            }
        })
        .collect()
}

fn maps_to_cells(maps: Vec<DimensionMap>) -> Vec<Vec<Cell>> {
    maps.into_iter()
        .map(|dimension| {
            let mut values: Vec<_> = dimension.into_values().collect();
            values.sort_by(cell_order);
            values
        })
        .collect()
}

fn boundary_coefficient(boundary: &[Term], key: &[usize]) -> Option<u32> {
    boundary
        .binary_search_by(|term| term.cell.as_slice().cmp(key))
        .ok()
        .map(|position| boundary[position].coefficient)
}

fn remove_boundary_term(boundary: &mut Vec<Term>, key: &[usize]) {
    if let Ok(position) = boundary.binary_search_by(|term| term.cell.as_slice().cmp(key)) {
        boundary.remove(position);
    }
}

fn add_boundary_scaled(target: &mut Vec<Term>, source: &[Term], factor: u32, modulus: u32) {
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
        .map(|(cell, coefficient)| Term { cell, coefficient })
        .collect();
}

fn check_chain(cells: &[Vec<Cell>], max_dim: usize, modulus: u32) -> Result<(), ProofError> {
    if cells.len() != max_dim + 2 {
        return Err(ProofError::new(
            "relative interface has the wrong dimension count",
        ));
    }
    let maps: Vec<BTreeMap<_, _>> = cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect()
        })
        .collect();
    for (dimension, values) in cells.iter().enumerate() {
        check_chain_dimension(dimension, values, &maps, modulus)?;
    }
    for dimension in 2..cells.len() {
        for cell in &cells[dimension] {
            check_boundary_square(cell, &maps[dimension - 1], modulus)?;
        }
    }
    Ok(())
}

fn check_chain_dimension(
    dimension: usize,
    cells: &[Cell],
    maps: &[BTreeMap<Vec<usize>, &Cell>],
    modulus: u32,
) -> Result<(), ProofError> {
    let mut previous = None;
    for cell in cells {
        check_cell(dimension, cell, previous)?;
        check_cell_boundary(dimension, cell, maps, modulus)?;
        previous = Some(cell);
    }
    if maps[dimension].len() != cells.len() {
        Err(ProofError::new("relative interface repeats a cell"))
    } else {
        Ok(())
    }
}

fn check_cell(dimension: usize, cell: &Cell, previous: Option<&Cell>) -> Result<(), ProofError> {
    if cell.vertices.len() != dimension + 1
        || !cell.value.is_finite()
        || cell.value < 0.0
        || previous.is_some_and(|prior| cell_order(prior, cell).is_gt())
    {
        Err(ProofError::new(
            "relative-interface cell order or value is invalid",
        ))
    } else {
        Ok(())
    }
}

fn check_cell_boundary(
    dimension: usize,
    cell: &Cell,
    maps: &[BTreeMap<Vec<usize>, &Cell>],
    modulus: u32,
) -> Result<(), ProofError> {
    let mut prior = None;
    for term in &cell.boundary {
        let face = maps
            .get(dimension.wrapping_sub(1))
            .and_then(|rows| rows.get(&term.cell))
            .ok_or_else(|| ProofError::new("relative boundary cell is absent"))?;
        if face.value > cell.value
            || term.coefficient == 0
            || term.coefficient >= modulus
            || prior.is_some_and(|key: &[usize]| key >= term.cell.as_slice())
        {
            return Err(ProofError::new(
                "relative boundary is not canonical and filtered",
            ));
        }
        prior = Some(term.cell.as_slice());
    }
    Ok(())
}

fn check_boundary_square(
    cell: &Cell,
    lower_cells: &BTreeMap<Vec<usize>, &Cell>,
    modulus: u32,
) -> Result<(), ProofError> {
    let mut square = BTreeMap::<Vec<usize>, u64>::new();
    for term in &cell.boundary {
        for lower in &lower_cells[&term.cell].boundary {
            add_square_term(&mut square, term, lower, modulus);
        }
    }
    if square.is_empty() {
        Ok(())
    } else {
        Err(ProofError::new(
            "relative-interface boundary does not square to zero",
        ))
    }
}

fn add_square_term(
    square: &mut BTreeMap<Vec<usize>, u64>,
    term: &Term,
    lower: &Term,
    modulus: u32,
) {
    let value = (square.get(&lower.cell).copied().unwrap_or(0)
        + u64::from(term.coefficient) * u64::from(lower.coefficient))
        % u64::from(modulus);
    if value == 0 {
        square.remove(&lower.cell);
    } else {
        square.insert(lower.cell.clone(), value);
    }
}

fn check_protected(
    input: &[Vec<Cell>],
    core: &[Vec<Cell>],
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    for (before, after) in input.iter().zip(core) {
        let retained: BTreeMap<_, _> = after.iter().map(|cell| (&cell.vertices, cell)).collect();
        for cell in before {
            if is_protected(&cell.vertices, protected)
                && retained.get(&cell.vertices).copied() != Some(cell)
            {
                return Err(ProofError::new(
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

fn check_reduction(
    cells: &[Vec<Cell>],
    modulus: u32,
    columns: &[Vec<ProofColumn>],
    limits: ProofLimits,
) -> Result<Vec<ProofBar>, ProofError> {
    let boundaries = boundary_matrices(cells)?;
    if columns.len() != boundaries.len() {
        return Err(ProofError::new(
            "relative reduction has the wrong dimension count",
        ));
    }
    let reduced = reduce_boundaries(&boundaries, columns, modulus, limits)?;
    Ok(reduction_diagram(cells, columns, &reduced))
}

fn reduce_boundaries(
    boundaries: &[Vec<SparseColumn>],
    columns: &[Vec<ProofColumn>],
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let mut total_terms = 0usize;
    let mut reduced = Vec::with_capacity(columns.len());
    for (matrix, transforms) in boundaries.iter().zip(columns) {
        reduced.push(reduce_dimension(
            matrix,
            transforms,
            modulus,
            &mut total_terms,
            limits,
        )?);
    }
    Ok(reduced)
}

fn reduce_dimension(
    matrix: &[SparseColumn],
    transforms: &[ProofColumn],
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<SparseColumn>, ProofError> {
    if matrix.len() != transforms.len() {
        return Err(ProofError::new(
            "relative reduction has the wrong column count",
        ));
    }
    let mut values = Vec::with_capacity(matrix.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in transforms.iter().enumerate() {
        let result = reduce_column(matrix, transform, target, modulus, total_terms, limits)?;
        if result
            .pivot()
            .is_some_and(|(pivot, _)| !pivots.insert(pivot))
        {
            return Err(ProofError::new("relative reduced matrix repeats a pivot"));
        }
        values.push(result);
    }
    Ok(values)
}

fn reduce_column(
    matrix: &[SparseColumn],
    transform: &ProofColumn,
    target: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<SparseColumn, ProofError> {
    check_column(target, &transform.terms, modulus)?;
    add_term_count(
        total_terms,
        transform.terms.len(),
        limits.max_terms,
        "relative change",
    )?;
    let mut result = SparseColumn::default();
    for term in &transform.terms {
        result.add_scaled(
            &matrix[term.index],
            u64::from(term.coefficient),
            u64::from(modulus),
        );
    }
    Ok(result)
}

fn reduction_diagram(
    cells: &[Vec<Cell>],
    columns: &[Vec<ProofColumn>],
    reduced: &[Vec<SparseColumn>],
) -> Vec<ProofBar> {
    let mut diagram = Vec::new();
    for dimension in 0..columns.len() {
        append_dimension_bars(&mut diagram, cells, reduced, dimension);
    }
    canonicalize_diagram(&mut diagram);
    diagram
}

fn append_dimension_bars(
    diagram: &mut Vec<ProofBar>,
    cells: &[Vec<Cell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) {
    let births = if dimension == 0 {
        vec![true; cells[0].len()]
    } else {
        reduced[dimension - 1]
            .iter()
            .map(|column| column.0.is_empty())
            .collect()
    };
    let deaths = reduced[dimension]
        .iter()
        .enumerate()
        .filter_map(|(column, value)| value.pivot().map(|(row, _)| (row, column)))
        .collect::<BTreeMap<_, _>>();
    for (position, is_birth) in births.into_iter().enumerate() {
        if is_birth {
            append_bar(diagram, cells, dimension, position, &deaths);
        }
    }
}

fn append_bar(
    diagram: &mut Vec<ProofBar>,
    cells: &[Vec<Cell>],
    dimension: usize,
    position: usize,
    deaths: &BTreeMap<usize, usize>,
) {
    let birth = cells[dimension][position].value;
    let death = deaths
        .get(&position)
        .map_or(f64::INFINITY, |&column| cells[dimension + 1][column].value);
    if death > birth {
        diagram.push(ProofBar {
            dimension,
            birth,
            death,
        });
    }
}

fn boundary_matrices(cells: &[Vec<Cell>]) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
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
                        let row = rows[dimension - 1].get(&term.cell).ok_or_else(|| {
                            ProofError::new("relative boundary references an absent cell")
                        })?;
                        column.insert(*row, term.coefficient as u64);
                    }
                    Ok(column)
                })
                .collect()
        })
        .collect()
}

fn dimension_limit(dimension: usize, limits: ProofLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn count_cells(cells: &[Vec<Cell>]) -> usize {
    cells.iter().map(Vec::len).sum()
}

fn cell_order(left: &Cell, right: &Cell) -> std::cmp::Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
}

fn certificate_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<Cell>],
    columns: &[Vec<ProofColumn>],
    diagram: &[ProofBar],
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
    hash.update((diagram.len() as u64).to_be_bytes());
    for bar in diagram {
        hash.update((bar.dimension as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn source_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<Cell>],
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
