use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{ProofBar, ProofError, ProofLimits, Reader, is_prime};

use super::model::{ChangeColumn, DecodedExplicit, Simplex, Term};
use super::proof_error;

pub(super) const MAGIC: &[u8; 8] = b"HOLOSEXP";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u32 = 32_768;
const WIRE_USIZE_BYTES: usize = 8;
const CHANGE_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;
const BAR_BYTES: usize = 3 * WIRE_USIZE_BYTES;

pub(super) fn decode(bytes: &[u8], limits: ProofLimits) -> Result<DecodedExplicit, ProofError> {
    let (payload, _) = decode_digest(bytes, limits.max_bytes)?;
    decode_payload(payload, limits)
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
    finish_payload(&reader)?;
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

fn finish_payload(reader: &Reader<'_>) -> Result<(), ProofError> {
    if reader.remaining() != 0 {
        return Err(proof_error(
            "explicit certificate has trailing payload bytes",
        ));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || modulus >= MODULUS_LIMIT {
        return Err(proof_error(
            "explicit certificate modulus must be a prime below 32768",
        ));
    }
    Ok(())
}

fn decode_labels(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("vertex label count", maximum)?;
    reader.require_bytes(count, WIRE_USIZE_BYTES, "vertex labels")?;
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
    let dimension_count = decode_complex_header(reader, max_homology_dimension, limits)?;
    let mut work = SimplexWork::default();
    let complex = (0..dimension_count)
        .map(|dimension| {
            let count = decode_simplex_count(reader, dimension, limits, &mut work)?;
            decode_simplex_dimension(reader, labels, dimension, count)
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_vertex_simplices(labels, &complex[0])?;
    validate_faces(&complex)?;
    Ok(complex)
}

fn decode_complex_header(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
    limits: ProofLimits,
) -> Result<usize, ProofError> {
    let dimension_limit = limits
        .max_dimension
        .checked_add(2)
        .ok_or_else(|| proof_error("explicit dimension limit overflows"))?;
    let dimension_count = reader.bounded_usize("simplex dimension count", dimension_limit)?;
    let expected_dimensions = max_homology_dimension
        .checked_add(2)
        .ok_or_else(|| proof_error("explicit simplex dimension count overflows"))?;
    if dimension_count != expected_dimensions {
        return Err(proof_error(
            "explicit simplex dimension count is not canonical",
        ));
    }
    reader.require_bytes(
        dimension_count,
        WIRE_USIZE_BYTES,
        "simplex dimension headers",
    )?;
    Ok(dimension_count)
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
    let minimum_width = dimension
        .checked_add(3)
        .and_then(|fields| fields.checked_mul(WIRE_USIZE_BYTES))
        .ok_or_else(|| proof_error("simplex byte count overflows"))?;
    reader.require_bytes(count, minimum_width, "simplices")?;
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
    let vertex_count = decode_simplex_header(reader, dimension)?;
    let vertices = decode_simplex_vertices(reader, labels, vertex_count)?;
    let grade = decode_simplex_grade(reader)?;
    Ok(Simplex { vertices, grade })
}

fn decode_simplex_header(reader: &mut Reader<'_>, dimension: usize) -> Result<usize, ProofError> {
    let expected_vertices = dimension
        .checked_add(1)
        .ok_or_else(|| proof_error("simplex vertex count overflows"))?;
    let vertex_count = reader.bounded_usize("simplex vertex count", expected_vertices)?;
    if vertex_count != expected_vertices {
        return Err(proof_error("explicit simplex has the wrong vertex count"));
    }
    reader.require_bytes(vertex_count, WIRE_USIZE_BYTES, "simplex vertices")?;
    Ok(vertex_count)
}

fn decode_simplex_vertices(
    reader: &mut Reader<'_>,
    labels: &[usize],
    vertex_count: usize,
) -> Result<Vec<usize>, ProofError> {
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(reader.usize()?);
    }
    validate_simplex_vertices(&vertices, labels)?;
    Ok(vertices)
}

fn decode_simplex_grade(reader: &mut Reader<'_>) -> Result<f64, ProofError> {
    let grade = f64::from_bits(reader.u64()?);
    if !grade.is_finite() || grade < 0.0 || grade.to_bits() == (-0.0f64).to_bits() {
        return Err(proof_error("explicit simplex grade is not canonical"));
    }
    Ok(grade)
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
    let expected_dimensions = max_homology_dimension
        .checked_add(1)
        .ok_or_else(|| proof_error("explicit boundary dimension count overflows"))?;
    let count = reader.bounded_usize("boundary dimension count", expected_dimensions)?;
    if count != expected_dimensions {
        return Err(proof_error(
            "explicit boundary dimension count is not canonical",
        ));
    }
    reader.require_bytes(count, WIRE_USIZE_BYTES, "boundary dimension headers")?;
    let mut total_terms = 0usize;
    let mut dimensions = Vec::with_capacity(count);
    for simplices in complex.iter().skip(1).take(count) {
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
    reader.require_bytes(count, WIRE_USIZE_BYTES, "change column headers")?;
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
    reader.require_bytes(count, CHANGE_TERM_BYTES, "change terms")?;
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
    reader.require_bytes(count, BAR_BYTES, "diagram bars")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_count_requires_minimum_encoded_bytes() {
        let bytes = 1u64.to_be_bytes();
        let mut reader = Reader::new(&bytes);
        let error = decode_labels(&mut reader, 1).unwrap_err();
        assert!(
            error
                .message()
                .contains("vertex labels requires at least 8 bytes")
        );
    }
}
