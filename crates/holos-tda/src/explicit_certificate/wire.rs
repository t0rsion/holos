use sha2::{Digest, Sha256};

use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::filtration::{FilteredSimplex, FilteredSimplicialComplex, ScalarGrade};
use crate::{Bar, Diagram};

use super::ExplicitReductionCertificate;
use super::validation::{certificate_error, simplex_limit};

const MAGIC: &[u8; 8] = b"HOLOSEXP";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const WIRE_USIZE_BYTES: usize = 8;
const CHANGE_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;
const BAR_BYTES: usize = 3 * WIRE_USIZE_BYTES;

pub(super) fn encode_payload(
    certificate: &ExplicitReductionCertificate,
) -> Result<Vec<u8>, CertificateError> {
    let mut output = Vec::new();
    output.extend_from_slice(MAGIC);
    put_u16(&mut output, VERSION);
    output.push(F64_BITS_CODEC);
    put_usize(
        &mut output,
        certificate.max_homology_dimension,
        "homology dimension",
    )?;
    put_u32(&mut output, certificate.modulus);
    encode_complex(&mut output, &certificate.complex)?;
    encode_columns(&mut output, &certificate.columns)?;
    encode_diagram(&mut output, &certificate.diagram)?;
    Ok(output)
}

pub(super) fn compute_digest(
    certificate: &ExplicitReductionCertificate,
) -> Result<[u8; 32], CertificateError> {
    Ok(Sha256::digest(encode_payload(certificate)?).into())
}

pub(super) fn decode_digest(
    bytes: &[u8],
    maximum: usize,
) -> Result<(&[u8], [u8; 32]), CertificateError> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(certificate_error(
            "explicit certificate is truncated or exceeds its byte limit",
        ));
    }
    let payload_length = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_length]).into();
    let digest: [u8; 32] = bytes[payload_length..]
        .try_into()
        .expect("32-byte explicit certificate digest");
    if digest != expected {
        return Err(certificate_error(
            "explicit certificate digest does not match its bytes",
        ));
    }
    Ok((&bytes[..payload_length], digest))
}

pub(super) fn decode_payload(
    payload: &[u8],
    digest: [u8; 32],
    limits: CertificateLimits,
) -> Result<ExplicitReductionCertificate, CertificateError> {
    let mut reader = Reader::new(payload);
    decode_prefix(&mut reader)?;
    let max_homology_dimension =
        reader.bounded_usize("homology dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    let complex = decode_complex(&mut reader, max_homology_dimension, limits)?;
    let columns = decode_columns(
        &mut reader,
        &complex,
        max_homology_dimension,
        modulus,
        limits,
    )?;
    let diagram = decode_diagram(&mut reader, max_homology_dimension, limits)?;
    reader.finish()?;
    Ok(ExplicitReductionCertificate {
        complex,
        max_homology_dimension,
        modulus,
        columns,
        diagram,
        digest,
    })
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), CertificateError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(certificate_error(
            "explicit certificate envelope version is unsupported",
        ));
    }
    Ok(())
}

fn encode_complex(
    output: &mut Vec<u8>,
    complex: &FilteredSimplicialComplex<ScalarGrade>,
) -> Result<(), CertificateError> {
    put_usize(output, complex.vertex_labels().len(), "vertex label count")?;
    for &label in complex.vertex_labels() {
        put_usize(output, label, "vertex label")?;
    }
    put_usize(output, complex.simplices().len(), "simplex dimension count")?;
    for dimension in complex.simplices() {
        put_usize(output, dimension.len(), "simplex count")?;
        for simplex in dimension {
            encode_simplex(output, simplex)?;
        }
    }
    Ok(())
}

fn encode_simplex(
    output: &mut Vec<u8>,
    simplex: &FilteredSimplex<ScalarGrade>,
) -> Result<(), CertificateError> {
    put_usize(output, simplex.vertices().len(), "simplex vertex count")?;
    for &vertex in simplex.vertices() {
        put_usize(output, vertex, "simplex vertex")?;
    }
    put_u64(output, simplex.grade().bits());
    Ok(())
}

fn encode_columns(
    output: &mut Vec<u8>,
    dimensions: &[Vec<ChangeColumn>],
) -> Result<(), CertificateError> {
    put_usize(output, dimensions.len(), "boundary dimension count")?;
    for columns in dimensions {
        put_usize(output, columns.len(), "change column count")?;
        for column in columns {
            put_usize(output, column.terms.len(), "change term count")?;
            for term in &column.terms {
                put_usize(output, term.index, "change term index")?;
                put_u32(output, term.coefficient);
            }
        }
    }
    Ok(())
}

fn encode_diagram(output: &mut Vec<u8>, diagram: &Diagram) -> Result<(), CertificateError> {
    put_usize(output, diagram.bars.len(), "diagram bar count")?;
    for bar in &diagram.bars {
        put_usize(output, bar.dim, "bar dimension")?;
        put_u64(output, bar.birth.to_bits());
        put_u64(output, bar.death.to_bits());
    }
    Ok(())
}

fn decode_complex(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
    limits: CertificateLimits,
) -> Result<FilteredSimplicialComplex<ScalarGrade>, CertificateError> {
    let labels = decode_labels(reader, limits.max_vertices)?;
    let expected_dimensions = max_homology_dimension
        .checked_add(2)
        .ok_or_else(|| certificate_error("explicit simplex dimension count overflows"))?;
    let dimension_limit = limits
        .max_dimension
        .checked_add(2)
        .ok_or_else(|| certificate_error("explicit dimension limit overflows"))?;
    let dimension_count = reader.bounded_usize("simplex dimension count", dimension_limit)?;
    if dimension_count != expected_dimensions {
        return Err(certificate_error(
            "explicit simplex dimension count is not canonical",
        ));
    }
    reader.require_bytes(
        dimension_count,
        WIRE_USIZE_BYTES,
        "simplex dimension headers",
    )?;
    let mut simplices = Vec::with_capacity(dimension_count);
    for dimension in 0..dimension_count {
        simplices.push(decode_simplex_dimension(reader, dimension, limits)?);
    }
    FilteredSimplicialComplex::new(labels, simplices)
        .map_err(|error| certificate_error(error.to_string()))
}

fn decode_labels(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, CertificateError> {
    let count = reader.bounded_usize("vertex label count", maximum)?;
    reader.require_bytes(count, WIRE_USIZE_BYTES, "vertex labels")?;
    let mut labels = Vec::with_capacity(count);
    for _ in 0..count {
        labels.push(reader.usize()?);
    }
    Ok(labels)
}

fn decode_simplex_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    limits: CertificateLimits,
) -> Result<Vec<FilteredSimplex<ScalarGrade>>, CertificateError> {
    let count = reader.bounded_usize("simplex count", simplex_limit(dimension, limits))?;
    let minimum_width = dimension
        .checked_add(3)
        .and_then(|fields| fields.checked_mul(WIRE_USIZE_BYTES))
        .ok_or_else(|| certificate_error("simplex byte count overflows"))?;
    reader.require_bytes(count, minimum_width, "simplices")?;
    let mut simplices = Vec::with_capacity(count);
    for _ in 0..count {
        simplices.push(decode_simplex(reader, dimension)?);
    }
    Ok(simplices)
}

fn decode_simplex(
    reader: &mut Reader<'_>,
    dimension: usize,
) -> Result<FilteredSimplex<ScalarGrade>, CertificateError> {
    let expected_vertices = dimension
        .checked_add(1)
        .ok_or_else(|| certificate_error("simplex vertex count overflows"))?;
    let vertex_count = reader.bounded_usize("simplex vertex count", expected_vertices)?;
    if vertex_count != expected_vertices {
        return Err(certificate_error(
            "explicit simplex has the wrong vertex count",
        ));
    }
    reader.require_bytes(vertex_count, WIRE_USIZE_BYTES, "simplex vertices")?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(reader.usize()?);
    }
    let grade = ScalarGrade::new(f64::from_bits(reader.u64()?))
        .map_err(|error| certificate_error(error.to_string()))?;
    Ok(FilteredSimplex::new(vertices, grade))
}

fn decode_columns(
    reader: &mut Reader<'_>,
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<ChangeColumn>>, CertificateError> {
    let expected_dimensions = max_homology_dimension
        .checked_add(1)
        .ok_or_else(|| certificate_error("explicit boundary dimension count overflows"))?;
    let count = reader.bounded_usize("boundary dimension count", expected_dimensions)?;
    if count != expected_dimensions {
        return Err(certificate_error(
            "explicit boundary dimension count is not canonical",
        ));
    }
    reader.require_bytes(count, WIRE_USIZE_BYTES, "boundary dimension headers")?;
    let mut total_terms = 0usize;
    let mut dimensions = Vec::with_capacity(count);
    for dimension in 1..=count {
        dimensions.push(decode_change_dimension(
            reader,
            complex.simplices()[dimension].len(),
            modulus,
            limits,
            &mut total_terms,
        )?);
    }
    Ok(dimensions)
}

fn decode_change_dimension(
    reader: &mut Reader<'_>,
    expected_columns: usize,
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<ChangeColumn>, CertificateError> {
    let count = reader.bounded_usize("change column count", expected_columns)?;
    if count != expected_columns {
        return Err(certificate_error(
            "explicit change column count is not canonical",
        ));
    }
    reader.require_bytes(count, WIRE_USIZE_BYTES, "change column headers")?;
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        columns.push(decode_change_column(
            reader,
            target,
            modulus,
            limits,
            total_terms,
        )?);
    }
    Ok(columns)
}

fn decode_change_column(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<ChangeColumn, CertificateError> {
    let count = reader.bounded_usize("change term count", target + 1)?;
    *total_terms = total_terms
        .checked_add(count)
        .ok_or_else(|| certificate_error("explicit change term count overflows"))?;
    if *total_terms > limits.max_terms {
        return Err(certificate_error(
            "explicit change terms exceed their limit",
        ));
    }
    reader.require_bytes(count, CHANGE_TERM_BYTES, "change terms")?;
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        terms.push(decode_change_term(reader, target, modulus)?);
    }
    Ok(ChangeColumn { terms })
}

fn decode_change_term(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
) -> Result<CertificateTerm, CertificateError> {
    let index = reader.bounded_usize("change term index", target)?;
    let coefficient = reader.u32()?;
    if coefficient == 0 || coefficient >= modulus {
        return Err(certificate_error(
            "explicit change coefficient is outside the field",
        ));
    }
    Ok(CertificateTerm { index, coefficient })
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
    limits: CertificateLimits,
) -> Result<Diagram, CertificateError> {
    let count = reader.bounded_usize("diagram bar count", limits.max_bars)?;
    reader.require_bytes(count, BAR_BYTES, "diagram bars")?;
    let mut diagram = Diagram::default();
    diagram.bars.reserve(count);
    for _ in 0..count {
        diagram
            .bars
            .push(decode_bar(reader, max_homology_dimension)?);
    }
    if !diagram_is_canonical(&diagram) {
        return Err(certificate_error("explicit diagram is not canonical"));
    }
    Ok(diagram)
}

fn decode_bar(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
) -> Result<Bar, CertificateError> {
    let dim = reader.bounded_usize("bar dimension", max_homology_dimension)?;
    let birth = f64::from_bits(reader.u64()?);
    let death = f64::from_bits(reader.u64()?);
    if !birth.is_finite() || birth < 0.0 || death.is_nan() || death <= birth {
        return Err(certificate_error(
            "explicit diagram contains an invalid bar",
        ));
    }
    Ok(Bar { dim, birth, death })
}

fn diagram_is_canonical(diagram: &Diagram) -> bool {
    diagram.bars.windows(2).all(|pair| {
        pair[0]
            .dim
            .cmp(&pair[1].dim)
            .then(pair[0].birth.total_cmp(&pair[1].birth))
            .then(pair[0].death.total_cmp(&pair[1].death))
            != std::cmp::Ordering::Greater
    })
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(output: &mut Vec<u8>, value: usize, label: &str) -> Result<(), CertificateError> {
    let value = u64::try_from(value)
        .map_err(|_| certificate_error(format!("{label} does not fit the wire integer")))?;
    put_u64(output, value);
    Ok(())
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
            .ok_or_else(|| certificate_error("explicit read position overflows"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| certificate_error("explicit certificate is truncated"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, CertificateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CertificateError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte explicit slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, CertificateError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte explicit slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, CertificateError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte explicit slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize, CertificateError> {
        usize::try_from(self.u64()?)
            .map_err(|_| certificate_error("explicit integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, maximum: usize) -> Result<usize, CertificateError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(certificate_error(format!(
                "{label} {value} exceeds its limit {maximum}"
            )));
        }
        Ok(value)
    }

    fn require_bytes(
        &self,
        count: usize,
        minimum_width: usize,
        label: &str,
    ) -> Result<(), CertificateError> {
        let required = count
            .checked_mul(minimum_width)
            .ok_or_else(|| certificate_error(format!("{label} minimum byte count overflows")))?;
        let remaining = self.remaining();
        if required > remaining {
            return Err(certificate_error(format!(
                "{label} requires at least {required} bytes, only {remaining} remain"
            )));
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), CertificateError> {
        if self.position != self.bytes.len() {
            return Err(certificate_error(
                "explicit certificate has trailing payload bytes",
            ));
        }
        Ok(())
    }
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
