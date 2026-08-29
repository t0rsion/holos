use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::{ProofBar, ProofError, ProofLimits};

const MAGIC: &[u8; 8] = b"HOLOSEXP";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u32 = 32_768;

/// Summary of a checked explicit filtration.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedExplicitPersistence {
    /// Highest checked homology dimension.
    pub max_homology_dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Stable vertex-label count.
    pub vertices: usize,
    /// Simplex counts in dimensions zero through `max_homology_dimension + 1`.
    pub simplex_counts: Vec<usize>,
    /// Checked change-of-basis column count.
    pub change_columns: usize,
    /// Checked nonzero change-of-basis term count.
    pub change_terms: usize,
    /// Diagram derived from the checked pivots.
    pub bars: Vec<ProofBar>,
}

/// Return true when bytes start with an explicit-complex certificate envelope.
pub fn is_explicit_persistence(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one bounded `HOLOSEXP` certificate.
pub fn verify_explicit_persistence(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedExplicitPersistence, ProofError> {
    let (payload, _) = decode_digest(bytes, limits.max_bytes)?;
    let decoded = decode_payload(payload, limits)?;
    verify_decoded(decoded, limits)
}

struct DecodedExplicit {
    max_homology_dimension: usize,
    modulus: u32,
    labels: Vec<usize>,
    complex: Vec<Vec<Simplex>>,
    columns: Vec<Vec<ChangeColumn>>,
    bars: Vec<ProofBar>,
    change_terms: usize,
}

#[derive(Clone)]
struct Simplex {
    vertices: Vec<usize>,
    grade: f64,
}

struct ChangeColumn {
    terms: Vec<Term>,
}

struct Term {
    index: usize,
    coefficient: u32,
}

#[derive(Clone, Default)]
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
            .map(|(&index, &coefficient)| (index, coefficient))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&index, &coefficient) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * coefficient) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }
}

fn decode_digest(bytes: &[u8], maximum: usize) -> Result<(&[u8], [u8; 32]), ProofError> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(proof_error(
            "explicit certificate is truncated or exceeds its byte limit",
        ));
    }
    let payload_length = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_length]).into();
    let digest: [u8; 32] = bytes[payload_length..]
        .try_into()
        .expect("32-byte explicit certificate digest");
    if digest != expected {
        return Err(proof_error(
            "explicit certificate digest does not match its bytes",
        ));
    }
    Ok((&bytes[..payload_length], digest))
}

fn decode_payload(payload: &[u8], limits: ProofLimits) -> Result<DecodedExplicit, ProofError> {
    let mut reader = Reader::new(payload);
    decode_prefix(&mut reader)?;
    let max_homology_dimension =
        reader.bounded_usize("homology dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let labels = decode_labels(&mut reader, limits.max_vertices)?;
    let complex = decode_complex(&mut reader, &labels, max_homology_dimension, limits)?;
    let (columns, change_terms) = decode_columns(
        &mut reader,
        &complex,
        max_homology_dimension,
        modulus,
        limits,
    )?;
    let bars = decode_bars(&mut reader, max_homology_dimension, limits.max_bars)?;
    reader.finish()?;
    Ok(DecodedExplicit {
        max_homology_dimension,
        modulus,
        labels,
        complex,
        columns,
        bars,
        change_terms,
    })
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(proof_error(
            "explicit certificate envelope version is unsupported",
        ));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(modulus) || modulus >= MODULUS_LIMIT {
        return Err(proof_error(
            "explicit certificate modulus must be a prime below 32768",
        ));
    }
    Ok(())
}

fn is_prime(value: u32) -> bool {
    if value < 2 {
        return false;
    }
    let mut divisor = 2u32;
    while divisor * divisor <= value {
        if value % divisor == 0 {
            return false;
        }
        divisor += 1;
    }
    true
}

fn decode_labels(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("vertex label count", maximum)?;
    let mut labels = Vec::with_capacity(count);
    for _ in 0..count {
        labels.push(reader.usize()?);
    }
    if labels.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(proof_error(
            "explicit vertex labels are not strictly increasing",
        ));
    }
    Ok(labels)
}

fn decode_complex(
    reader: &mut Reader<'_>,
    labels: &[usize],
    max_homology_dimension: usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<Simplex>>, ProofError> {
    let dimension_limit = limits
        .max_dimension
        .checked_add(2)
        .ok_or_else(|| proof_error("explicit dimension limit overflows"))?;
    let dimension_count = reader.bounded_usize("simplex dimension count", dimension_limit)?;
    if dimension_count != max_homology_dimension + 2 {
        return Err(proof_error(
            "explicit simplex dimension count is not canonical",
        ));
    }
    let mut work = SimplexWork::default();
    let mut complex = Vec::with_capacity(dimension_count);
    for dimension in 0..dimension_count {
        let count = decode_simplex_count(reader, dimension, limits, &mut work)?;
        complex.push(decode_simplex_dimension(reader, labels, dimension, count)?);
    }
    validate_vertex_simplices(labels, &complex[0])?;
    validate_faces(&complex)?;
    Ok(complex)
}

#[derive(Default)]
struct SimplexWork {
    higher: usize,
}

fn decode_simplex_count(
    reader: &mut Reader<'_>,
    dimension: usize,
    limits: ProofLimits,
    work: &mut SimplexWork,
) -> Result<usize, ProofError> {
    let maximum = match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    };
    let count = reader.bounded_usize("simplex count", maximum)?;
    if dimension > 2 {
        work.higher = work
            .higher
            .checked_add(count)
            .ok_or_else(|| proof_error("explicit higher-simplex count overflows"))?;
        if work.higher > limits.max_higher_simplices {
            return Err(proof_error(
                "explicit higher simplices exceed their total limit",
            ));
        }
    }
    Ok(count)
}

fn decode_simplex_dimension(
    reader: &mut Reader<'_>,
    labels: &[usize],
    dimension: usize,
    count: usize,
) -> Result<Vec<Simplex>, ProofError> {
    let mut simplices = Vec::with_capacity(count);
    for _ in 0..count {
        simplices.push(decode_simplex(reader, labels, dimension)?);
    }
    if simplices
        .windows(2)
        .any(|pair| pair[0].vertices >= pair[1].vertices)
    {
        return Err(proof_error(
            "explicit simplices are not in canonical vertex order",
        ));
    }
    Ok(simplices)
}

fn decode_simplex(
    reader: &mut Reader<'_>,
    labels: &[usize],
    dimension: usize,
) -> Result<Simplex, ProofError> {
    let vertex_count = reader.bounded_usize("simplex vertex count", dimension + 1)?;
    if vertex_count != dimension + 1 {
        return Err(proof_error("explicit simplex has the wrong vertex count"));
    }
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(reader.usize()?);
    }
    validate_simplex_vertices(&vertices, labels)?;
    let grade = f64::from_bits(reader.u64()?);
    if !grade.is_finite() || grade < 0.0 || grade.to_bits() == (-0.0f64).to_bits() {
        return Err(proof_error("explicit simplex grade is not canonical"));
    }
    Ok(Simplex { vertices, grade })
}

fn validate_simplex_vertices(vertices: &[usize], labels: &[usize]) -> Result<(), ProofError> {
    if vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || vertices
            .iter()
            .any(|vertex| labels.binary_search(vertex).is_err())
    {
        return Err(proof_error(
            "explicit simplex has a noncanonical or unknown vertex",
        ));
    }
    Ok(())
}

fn validate_vertex_simplices(labels: &[usize], vertices: &[Simplex]) -> Result<(), ProofError> {
    if vertices.len() != labels.len()
        || vertices
            .iter()
            .zip(labels)
            .any(|(simplex, label)| simplex.vertices != [*label])
    {
        return Err(proof_error(
            "explicit zero-dimensional simplices differ from the labels",
        ));
    }
    Ok(())
}

fn validate_faces(complex: &[Vec<Simplex>]) -> Result<(), ProofError> {
    let grades = complex
        .iter()
        .flatten()
        .map(|simplex| (simplex.vertices.clone(), simplex.grade))
        .collect::<BTreeMap<_, _>>();
    for simplex in complex.iter().skip(1).flatten() {
        validate_simplex_faces(simplex, &grades)?;
    }
    Ok(())
}

fn validate_simplex_faces(
    simplex: &Simplex,
    grades: &BTreeMap<Vec<usize>, f64>,
) -> Result<(), ProofError> {
    for removed in 0..simplex.vertices.len() {
        let mut face = simplex.vertices.clone();
        face.remove(removed);
        let Some(&grade) = grades.get(&face) else {
            return Err(proof_error("explicit simplex boundary omits a face"));
        };
        if grade > simplex.grade {
            return Err(proof_error("explicit face appears after its coface"));
        }
    }
    Ok(())
}

fn decode_columns(
    reader: &mut Reader<'_>,
    complex: &[Vec<Simplex>],
    max_homology_dimension: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<(Vec<Vec<ChangeColumn>>, usize), ProofError> {
    let count = reader.bounded_usize(
        "boundary dimension count",
        max_homology_dimension.saturating_add(1),
    )?;
    if count != max_homology_dimension + 1 {
        return Err(proof_error(
            "explicit boundary dimension count is not canonical",
        ));
    }
    let mut total_terms = 0usize;
    let mut dimensions = Vec::with_capacity(count);
    for simplices in complex.iter().take(count + 1).skip(1) {
        dimensions.push(decode_change_dimension(
            reader,
            simplices.len(),
            modulus,
            limits.max_terms,
            &mut total_terms,
        )?);
    }
    Ok((dimensions, total_terms))
}

fn decode_change_dimension(
    reader: &mut Reader<'_>,
    expected_columns: usize,
    modulus: u32,
    maximum_terms: usize,
    total_terms: &mut usize,
) -> Result<Vec<ChangeColumn>, ProofError> {
    let count = reader.bounded_usize("change column count", expected_columns)?;
    if count != expected_columns {
        return Err(proof_error("explicit change column count is not canonical"));
    }
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        columns.push(decode_change_column(
            reader,
            target,
            modulus,
            maximum_terms,
            total_terms,
        )?);
    }
    Ok(columns)
}

fn decode_change_column(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
    maximum_terms: usize,
    total_terms: &mut usize,
) -> Result<ChangeColumn, ProofError> {
    let count = reader.bounded_usize("change term count", target + 1)?;
    *total_terms = total_terms
        .checked_add(count)
        .ok_or_else(|| proof_error("explicit change term count overflows"))?;
    if *total_terms > maximum_terms {
        return Err(proof_error(
            "explicit change terms exceed their total limit",
        ));
    }
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        terms.push(decode_term(reader, target, modulus)?);
    }
    Ok(ChangeColumn { terms })
}

fn decode_term(reader: &mut Reader<'_>, target: usize, modulus: u32) -> Result<Term, ProofError> {
    let index = reader.bounded_usize("change term index", target)?;
    let coefficient = reader.u32()?;
    if coefficient == 0 || coefficient >= modulus {
        return Err(proof_error(
            "explicit change coefficient is outside the field",
        ));
    }
    Ok(Term { index, coefficient })
}

fn decode_bars(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
    maximum: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let count = reader.bounded_usize("diagram bar count", maximum)?;
    let mut bars = Vec::with_capacity(count);
    for _ in 0..count {
        bars.push(decode_bar(reader, max_homology_dimension)?);
    }
    if !bars_are_canonical(&bars) {
        return Err(proof_error("explicit diagram is not canonical"));
    }
    Ok(bars)
}

fn decode_bar(reader: &mut Reader<'_>, maximum_dimension: usize) -> Result<ProofBar, ProofError> {
    let dimension = reader.bounded_usize("bar dimension", maximum_dimension)?;
    let birth = f64::from_bits(reader.u64()?);
    let death = f64::from_bits(reader.u64()?);
    if !birth.is_finite() || birth < 0.0 || death.is_nan() || death <= birth {
        return Err(proof_error("explicit diagram contains an invalid bar"));
    }
    Ok(ProofBar {
        dimension,
        birth,
        death,
    })
}

fn bars_are_canonical(bars: &[ProofBar]) -> bool {
    bars.windows(2)
        .all(|pair| compare_bars(&pair[0], &pair[1]).is_le())
}

fn compare_bars(left: &ProofBar, right: &ProofBar) -> std::cmp::Ordering {
    left.dimension
        .cmp(&right.dimension)
        .then(left.birth.total_cmp(&right.birth))
        .then(left.death.total_cmp(&right.death))
}

fn verify_decoded(
    decoded: DecodedExplicit,
    limits: ProofLimits,
) -> Result<VerifiedExplicitPersistence, ProofError> {
    let ordered = order_complex(&decoded.complex);
    let boundaries = build_boundaries(&ordered, decoded.modulus)?;
    let reduced = verify_reductions(
        &boundaries,
        &decoded.columns,
        decoded.modulus,
        limits.max_terms,
    )?;
    let bars = derive_bars(&ordered, &reduced);
    if !same_bars(&bars, &decoded.bars) {
        return Err(proof_error(
            "explicit diagram differs from the checked reductions",
        ));
    }
    let simplex_counts = decoded.complex.iter().map(Vec::len).collect();
    let change_columns = decoded.columns.iter().map(Vec::len).sum();
    Ok(VerifiedExplicitPersistence {
        max_homology_dimension: decoded.max_homology_dimension,
        modulus: decoded.modulus,
        vertices: decoded.labels.len(),
        simplex_counts,
        change_columns,
        change_terms: decoded.change_terms,
        bars,
    })
}

fn order_complex(complex: &[Vec<Simplex>]) -> Vec<Vec<Simplex>> {
    complex
        .iter()
        .map(|dimension| {
            let mut ordered = dimension.clone();
            ordered.sort_by(|left, right| {
                left.grade
                    .total_cmp(&right.grade)
                    .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
            });
            ordered
        })
        .collect()
}

fn build_boundaries(
    complex: &[Vec<Simplex>],
    modulus: u32,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let rows = complex
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, simplex)| (simplex.vertices.clone(), position))
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let mut boundaries = Vec::with_capacity(complex.len().saturating_sub(1));
    for dimension in 1..complex.len() {
        boundaries.push(boundary_dimension(
            &complex[dimension],
            &rows[dimension - 1],
            modulus,
        )?);
    }
    Ok(boundaries)
}

fn boundary_dimension(
    simplices: &[Simplex],
    rows: &BTreeMap<Vec<usize>, usize>,
    modulus: u32,
) -> Result<Vec<SparseColumn>, ProofError> {
    simplices
        .iter()
        .map(|simplex| simplex_boundary(simplex, rows, modulus))
        .collect()
}

fn simplex_boundary(
    simplex: &Simplex,
    rows: &BTreeMap<Vec<usize>, usize>,
    modulus: u32,
) -> Result<SparseColumn, ProofError> {
    let mut column = SparseColumn::default();
    for removed in 0..simplex.vertices.len() {
        let mut face = simplex.vertices.clone();
        face.remove(removed);
        let row = rows
            .get(&face)
            .copied()
            .ok_or_else(|| proof_error("explicit simplex boundary omits a face"))?;
        let coefficient = if removed % 2 == 0 {
            1
        } else {
            u64::from(modulus - 1)
        };
        column.insert(row, coefficient);
    }
    Ok(column)
}

fn verify_reductions(
    boundaries: &[Vec<SparseColumn>],
    dimensions: &[Vec<ChangeColumn>],
    modulus: u32,
    maximum_terms: usize,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    if boundaries.len() != dimensions.len() {
        return Err(proof_error(
            "explicit reduction count differs from the complex",
        ));
    }
    let mut total_terms = 0usize;
    boundaries
        .iter()
        .zip(dimensions)
        .enumerate()
        .map(|(offset, (boundary, columns))| {
            verify_matrix(
                offset + 1,
                boundary,
                columns,
                modulus,
                maximum_terms,
                &mut total_terms,
            )
        })
        .collect()
}

fn verify_matrix(
    dimension: usize,
    boundaries: &[SparseColumn],
    columns: &[ChangeColumn],
    modulus: u32,
    maximum_terms: usize,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, ProofError> {
    if boundaries.len() != columns.len() {
        return Err(proof_error(format!(
            "explicit dimension {dimension} has the wrong change column count"
        )));
    }
    let mut pivots = BTreeSet::new();
    let mut reduced = Vec::with_capacity(columns.len());
    for (target, transform) in columns.iter().enumerate() {
        charge_terms(transform.terms.len(), maximum_terms, total_terms)?;
        validate_change_column(dimension, target, transform, modulus)?;
        let column = apply_change_column(transform, boundaries, modulus);
        if column
            .pivot()
            .is_some_and(|(pivot, _)| !pivots.insert(pivot))
        {
            return Err(proof_error(format!(
                "explicit dimension {dimension} repeats a reduced pivot"
            )));
        }
        reduced.push(column);
    }
    Ok(reduced)
}

fn charge_terms(count: usize, maximum: usize, total: &mut usize) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| proof_error("explicit change term count overflows"))?;
    if *total > maximum {
        return Err(proof_error(
            "explicit change terms exceed their verification limit",
        ));
    }
    Ok(())
}

fn validate_change_column(
    dimension: usize,
    target: usize,
    column: &ChangeColumn,
    modulus: u32,
) -> Result<(), ProofError> {
    let unit = column
        .terms
        .last()
        .is_some_and(|term| term.index == target && term.coefficient == 1);
    if column.terms.is_empty() || !unit {
        return Err(proof_error(format!(
            "explicit dimension {dimension} change column {target} is not unit triangular"
        )));
    }
    let canonical = column
        .terms
        .iter()
        .all(|term| term.index <= target && term.coefficient > 0 && term.coefficient < modulus)
        && column
            .terms
            .windows(2)
            .all(|pair| pair[0].index < pair[1].index);
    if !canonical {
        return Err(proof_error(format!(
            "explicit dimension {dimension} change column {target} is not canonical"
        )));
    }
    Ok(())
}

fn apply_change_column(
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u32,
) -> SparseColumn {
    let mut column = SparseColumn::default();
    for term in &transform.terms {
        column.add_scaled(
            &boundaries[term.index],
            u64::from(term.coefficient),
            u64::from(modulus),
        );
    }
    column
}

fn derive_bars(complex: &[Vec<Simplex>], reduced: &[Vec<SparseColumn>]) -> Vec<ProofBar> {
    let mut bars = Vec::new();
    for dimension in 0..reduced.len() {
        let births = birth_columns(dimension, complex[dimension].len(), reduced);
        let deaths = death_columns(&reduced[dimension]);
        append_bars(dimension, &births, &deaths, complex, &mut bars);
    }
    bars.sort_by(compare_bars);
    bars
}

fn birth_columns(dimension: usize, count: usize, reduced: &[Vec<SparseColumn>]) -> Vec<bool> {
    if dimension == 0 {
        vec![true; count]
    } else {
        reduced[dimension - 1]
            .iter()
            .map(|column| column.0.is_empty())
            .collect()
    }
}

fn death_columns(reduced: &[SparseColumn]) -> BTreeMap<usize, usize> {
    reduced
        .iter()
        .enumerate()
        .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
        .collect()
}

fn append_bars(
    dimension: usize,
    births: &[bool],
    deaths: &BTreeMap<usize, usize>,
    complex: &[Vec<Simplex>],
    output: &mut Vec<ProofBar>,
) {
    for (position, &is_birth) in births.iter().enumerate() {
        if !is_birth {
            continue;
        }
        let birth = complex[dimension][position].grade;
        let death = deaths.get(&position).map_or(f64::INFINITY, |&column| {
            complex[dimension + 1][column].grade
        });
        if death > birth {
            output.push(ProofBar {
                dimension,
                birth,
                death,
            });
        }
    }
}

fn same_bars(left: &[ProofBar], right: &[ProofBar]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension == right.dimension
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| proof_error("explicit read position overflows"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| proof_error("explicit certificate is truncated"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte explicit slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte explicit slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte explicit slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?).map_err(|_| proof_error("explicit integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, maximum: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(proof_error(format!(
                "{label} {value} exceeds its limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn finish(&self) -> Result<(), ProofError> {
        if self.position != self.bytes.len() {
            return Err(proof_error(
                "explicit certificate has trailing payload bytes",
            ));
        }
        Ok(())
    }
}

fn proof_error(message: impl Into<String>) -> ProofError {
    ProofError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_probe_is_exact() {
        assert!(is_explicit_persistence(b"HOLOSEXPanything"));
        assert!(!is_explicit_persistence(b"HOLOSEXanything"));
    }
}
