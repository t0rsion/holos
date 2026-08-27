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
    Ok(VerifiedRelativeInterface {
        digest: certificate.digest,
        max_dim: certificate.max_dim,
        input_cells: count_cells(&certificate.input),
        cancellations: certificate.steps.len(),
        core_cells: count_cells(&certificate.core),
        reduction_columns: certificate.columns.iter().map(Vec::len).sum(),
        bars: certificate.diagram.len(),
    })
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
    if children.is_empty() {
        return Err(ProofError::new(
            "relative composition requires a child certificate",
        ));
    }
    let parent = decode_verified(parent, limits)?;
    if expected_protected_vertices
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || parent.protected_vertices != expected_protected_vertices
    {
        return Err(ProofError::new(
            "relative parent has the wrong protected vertex set",
        ));
    }
    let mut union = vec![BTreeMap::<Vec<usize>, Cell>::new(); parent.max_dim + 2];
    for bytes in children {
        let child = decode_verified(bytes, limits)?;
        if child.max_dim != parent.max_dim || child.modulus != parent.modulus {
            return Err(ProofError::new(
                "relative composition changes dimension or coefficient field",
            ));
        }
        for (dimension, cells) in child.core.iter().enumerate() {
            for cell in cells {
                match union[dimension].get(&cell.vertices) {
                    Some(existing) if existing != cell => {
                        return Err(ProofError::new(
                            "relative composition identifies conflicting cells",
                        ));
                    }
                    Some(_) => {}
                    None => {
                        union[dimension].insert(cell.vertices.clone(), cell.clone());
                    }
                }
            }
        }
    }
    let expected: Vec<Vec<Cell>> = union
        .into_iter()
        .map(|dimension| {
            let mut cells: Vec<_> = dimension.into_values().collect();
            cells.sort_by(cell_order);
            cells
        })
        .collect();
    if expected != parent.input {
        return Err(ProofError::new(
            "relative parent input differs from the keyed child-core union",
        ));
    }
    Ok(VerifiedRelativeInterface {
        digest: parent.digest,
        max_dim: parent.max_dim,
        input_cells: count_cells(&parent.input),
        cancellations: parent.steps.len(),
        core_cells: count_cells(&parent.core),
        reduction_columns: parent.columns.iter().map(Vec::len).sum(),
        bars: parent.diagram.len(),
    })
}

pub(super) fn decode_verified(
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
    if reader.take(8)? != MAGIC {
        return Err(ProofError::new("wrong relative-interface magic bytes"));
    }
    if reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new(
            "unsupported relative-interface wire version or scalar codec",
        ));
    }
    let max_dim = reader.bounded_usize("relative dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "relative-interface modulus is not a supported prime",
        ));
    }
    let protected_count = reader.bounded_usize("protected vertex count", limits.max_vertices)?;
    let mut protected_vertices = Vec::with_capacity(protected_count);
    for _ in 0..protected_count {
        protected_vertices.push(reader.usize()?);
    }
    if !protected_vertices.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(ProofError::new(
            "relative-interface protected vertices are not canonical",
        ));
    }
    let input = decode_cells(&mut reader, max_dim, modulus, limits)?;
    let input_count = count_cells(&input);
    let cancellation_limit = input_count / 2;
    let cancellation_count =
        reader.bounded_usize("relative cancellation count", cancellation_limit)?;
    let mut steps = Vec::with_capacity(cancellation_count);
    for _ in 0..cancellation_count {
        steps.push(Step {
            upper: decode_key(&mut reader, max_dim + 2, limits.max_vertices)?,
            lower: decode_key(&mut reader, max_dim + 1, limits.max_vertices)?,
            coefficient: reader.u32()?,
        });
    }
    let core = decode_cells(&mut reader, max_dim, modulus, limits)?;
    let columns = decode_columns(&mut reader, max_dim, modulus, limits)?;
    let bar_count = reader.bounded_usize("relative bar count", limits.max_bars)?;
    let mut diagram = Vec::with_capacity(bar_count);
    for _ in 0..bar_count {
        diagram.push(ProofBar {
            dimension: reader.bounded_usize("bar dimension", max_dim)?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "trailing bytes after the relative-interface certificate",
        ));
    }

    check_chain(&input, max_dim, modulus)?;
    check_chain(&core, max_dim, modulus)?;
    let protected: BTreeSet<_> = protected_vertices.iter().copied().collect();
    let replayed = replay(input.clone(), &steps, &protected, modulus)?;
    if replayed != core {
        return Err(ProofError::new(
            "relative cancellation trace does not produce the declared core",
        ));
    }
    check_protected(&input, &core, &protected)?;
    let checked_diagram = check_reduction(&core, modulus, &columns, limits)?;
    if !diagrams_equal(&checked_diagram, &diagram) {
        return Err(ProofError::new(
            "relative-interface diagram differs from the checked reduction",
        ));
    }
    let computed = certificate_digest(
        max_dim,
        modulus,
        &protected_vertices,
        &core,
        &columns,
        &diagram,
    );
    if computed != digest {
        return Err(ProofError::new(
            "relative-interface digest differs from checked content",
        ));
    }
    Ok(VerifiedCertificate {
        digest,
        max_dim,
        modulus,
        protected_vertices,
        input,
        steps,
        core,
        columns,
        diagram,
    })
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
    let mut vertices = labels
        .iter()
        .map(|&vertex| Cell {
            vertices: vec![vertex],
            value: 0.0,
            boundary: Vec::new(),
        })
        .collect::<Vec<_>>();
    vertices.sort_by(cell_order);
    let mut cells = vec![vertices];
    for dimension in 1..=max_dim + 1 {
        let limit = match dimension {
            1 => limits.max_edges,
            2 => limits.max_triangles,
            _ => limits.max_higher_simplices,
        };
        let mut next = Vec::new();
        for simplex in &cells[dimension - 1] {
            let start = labels
                .binary_search(simplex.vertices.last().expect("a simplex is nonempty"))
                .expect("the preceding simplex uses declared labels")
                + 1;
            for &vertex in &labels[start..] {
                let mut value = simplex.value;
                let mut clique = true;
                for &member in &simplex.vertices {
                    let edge = graph.get(member, vertex);
                    if !edge.is_finite() || edge > threshold {
                        clique = false;
                        break;
                    }
                    value = value.max(edge);
                }
                if clique {
                    if next.len() == limit {
                        return Err(ProofError::new(format!(
                            "relative leaf dimension {dimension} exceeds the simplex limit"
                        )));
                    }
                    let mut vertices = simplex.vertices.clone();
                    vertices.push(vertex);
                    let mut boundary = (0..vertices.len())
                        .map(|removed| {
                            let mut cell = vertices.clone();
                            cell.remove(removed);
                            Term {
                                cell,
                                coefficient: if removed % 2 == 0 { 1 } else { modulus - 1 },
                            }
                        })
                        .collect::<Vec<_>>();
                    boundary.sort();
                    next.push(Cell {
                        vertices,
                        value,
                        boundary,
                    });
                }
            }
        }
        next.sort_by(cell_order);
        cells.push(next);
    }
    Ok(cells)
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
        let limit = dimension_limit(dimension, limits);
        let count = reader.bounded_usize("dimension cell count", limit)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let vertices = decode_key(reader, dimension + 1, limits.max_vertices)?;
            if vertices.len() != dimension + 1 {
                return Err(ProofError::new(
                    "relative-interface cell has the wrong dimension",
                ));
            }
            let value = f64::from_bits(reader.u64()?);
            let term_count = reader.bounded_usize("cell boundary term count", limit)?;
            total_terms = total_terms
                .checked_add(term_count)
                .ok_or_else(|| ProofError::new("relative boundary term count overflows"))?;
            if total_terms > limits.max_terms {
                return Err(ProofError::new("relative boundary terms exceed the limit"));
            }
            let mut boundary = Vec::with_capacity(term_count);
            for _ in 0..term_count {
                boundary.push(Term {
                    cell: decode_key(reader, dimension, limits.max_vertices)?,
                    coefficient: reader.u32()?,
                });
            }
            if boundary.iter().any(|term| {
                term.cell.len() != dimension || term.coefficient == 0 || term.coefficient >= modulus
            }) {
                return Err(ProofError::new(
                    "relative boundary term has the wrong dimension or coefficient",
                ));
            }
            values.push(Cell {
                vertices,
                value,
                boundary,
            });
        }
        cells.push(values);
    }
    Ok(cells)
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
        let count =
            reader.bounded_usize("reduction column count", dimension_limit(dimension, limits))?;
        let mut values = Vec::with_capacity(count);
        for target in 0..count {
            let term_count = reader.bounded_usize("change term count", limits.max_terms)?;
            total_terms = total_terms
                .checked_add(term_count)
                .ok_or_else(|| ProofError::new("change term count overflows"))?;
            if total_terms > limits.max_terms {
                return Err(ProofError::new("change terms exceed the limit"));
            }
            let mut terms = Vec::with_capacity(term_count);
            for _ in 0..term_count {
                terms.push(ProofTerm {
                    index: reader.usize()?,
                    coefficient: reader.u32()?,
                });
            }
            check_column(target, &terms, modulus)?;
            values.push(ProofColumn { terms });
        }
        columns.push(values);
    }
    Ok(columns)
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
    let dimension = step
        .upper
        .len()
        .checked_sub(1)
        .ok_or_else(|| ProofError::new("relative cancellation upper cell is empty"))?;
    if dimension == 0 || step.lower.len() != dimension || dimension >= cells.len() {
        return Err(ProofError::new(
            "relative cancellation dimensions are incompatible",
        ));
    }
    if is_protected(&step.upper, protected) || is_protected(&step.lower, protected) {
        return Err(ProofError::new(
            "relative cancellation removes a protected cell",
        ));
    }
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
    let inverse = inverse_mod(coefficient as u64, modulus as u64) as u32;
    let keys: Vec<_> = cells[dimension].keys().cloned().collect();
    for key in keys {
        if key == step.upper {
            continue;
        }
        let cell = cells[dimension].get_mut(&key).unwrap();
        let Some(value) = boundary_coefficient(&cell.boundary, &step.lower) else {
            continue;
        };
        let factor = ((modulus as u64 - value as u64 * inverse as u64 % modulus as u64)
            % modulus as u64) as u32;
        add_boundary_scaled(&mut cell.boundary, &upper.boundary, factor, modulus);
    }
    if dimension + 1 < cells.len() {
        for cell in cells[dimension + 1].values_mut() {
            remove_boundary_term(&mut cell.boundary, &step.upper);
        }
    }
    cells[dimension].remove(&step.upper);
    cells[dimension - 1].remove(&step.lower);
    Ok(())
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
        let mut previous: Option<&Cell> = None;
        for cell in values {
            if cell.vertices.len() != dimension + 1
                || !cell.value.is_finite()
                || cell.value < 0.0
                || previous.is_some_and(|prior| cell_order(prior, cell).is_gt())
            {
                return Err(ProofError::new(
                    "relative-interface cell order or value is invalid",
                ));
            }
            let mut prior_term: Option<&[usize]> = None;
            for term in &cell.boundary {
                let face = maps
                    .get(dimension.wrapping_sub(1))
                    .and_then(|rows| rows.get(&term.cell))
                    .ok_or_else(|| ProofError::new("relative boundary cell is absent"))?;
                if face.value > cell.value
                    || term.coefficient == 0
                    || term.coefficient >= modulus
                    || prior_term.is_some_and(|prior| prior >= term.cell.as_slice())
                {
                    return Err(ProofError::new(
                        "relative boundary is not canonical and filtered",
                    ));
                }
                prior_term = Some(&term.cell);
            }
            previous = Some(cell);
        }
        if maps[dimension].len() != values.len() {
            return Err(ProofError::new("relative interface repeats a cell"));
        }
    }
    for dimension in 2..cells.len() {
        for cell in &cells[dimension] {
            let mut square = BTreeMap::<Vec<usize>, u64>::new();
            for term in &cell.boundary {
                for lower in &maps[dimension - 1][&term.cell].boundary {
                    let value = (square.get(&lower.cell).copied().unwrap_or(0)
                        + term.coefficient as u64 * lower.coefficient as u64)
                        % modulus as u64;
                    if value == 0 {
                        square.remove(&lower.cell);
                    } else {
                        square.insert(lower.cell.clone(), value);
                    }
                }
            }
            if !square.is_empty() {
                return Err(ProofError::new(
                    "relative-interface boundary does not square to zero",
                ));
            }
        }
    }
    Ok(())
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
    let mut total_terms = 0usize;
    let mut reduced = Vec::with_capacity(columns.len());
    for (matrix, transforms) in boundaries.iter().zip(columns) {
        if matrix.len() != transforms.len() {
            return Err(ProofError::new(
                "relative reduction has the wrong column count",
            ));
        }
        let mut values = Vec::with_capacity(matrix.len());
        let mut pivots = BTreeSet::new();
        for (target, transform) in transforms.iter().enumerate() {
            check_column(target, &transform.terms, modulus)?;
            total_terms = total_terms
                .checked_add(transform.terms.len())
                .ok_or_else(|| ProofError::new("relative change term count overflows"))?;
            if total_terms > limits.max_terms {
                return Err(ProofError::new("relative change terms exceed the limit"));
            }
            let mut result = SparseColumn::default();
            for term in &transform.terms {
                result.add_scaled(&matrix[term.index], term.coefficient as u64, modulus as u64);
            }
            if let Some((pivot, _)) = result.pivot()
                && !pivots.insert(pivot)
            {
                return Err(ProofError::new("relative reduced matrix repeats a pivot"));
            }
            values.push(result);
        }
        reduced.push(values);
    }
    let mut diagram = Vec::new();
    for dimension in 0..columns.len() {
        let births = if dimension == 0 {
            vec![true; cells[0].len()]
        } else {
            reduced[dimension - 1]
                .iter()
                .map(|column| column.0.is_empty())
                .collect()
        };
        let deaths: BTreeMap<_, _> = reduced[dimension]
            .iter()
            .enumerate()
            .filter_map(|(column, value)| value.pivot().map(|(row, _)| (row, column)))
            .collect();
        for (position, is_birth) in births.into_iter().enumerate() {
            if !is_birth {
                continue;
            }
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
    }
    canonicalize_diagram(&mut diagram);
    Ok(diagram)
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
