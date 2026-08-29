//! Proof-carrying persistence for explicit scalar filtered complexes.
//!
//! The caller supplies every simplex and face as a
//! [`FilteredSimplicialComplex`]. The certificate records a
//! filtration-compatible unit-triangular basis change in each boundary
//! dimension. The diagram is derived from checked pivots.

use sha2::{Digest, Sha256};

use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::filtration::{FilteredSimplex, FilteredSimplicialComplex, ScalarGrade};
use crate::graded_certificate::{GradedComplex, check_all, reduce_all_dimensions};
use crate::{Bar, Diagram};

const MAGIC: &[u8; 8] = b"HOLOSEXP";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// A dimension-generic `D V = R` certificate for an explicit filtration.
#[derive(Debug, Clone)]
pub struct ExplicitReductionCertificate {
    complex: FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    modulus: u32,
    columns: Vec<Vec<ChangeColumn>>,
    diagram: Diagram,
    digest: [u8; 32],
}

impl ExplicitReductionCertificate {
    /// Build and check an explicit reduction certificate.
    ///
    /// The complex must contain simplex groups through dimension
    /// `max_homology_dimension + 1`. An empty group records that no cofaces
    /// occur in that dimension.
    pub fn build(
        complex: &FilteredSimplicialComplex<ScalarGrade>,
        max_homology_dimension: usize,
        modulus: u32,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        validate_parameters(complex, max_homology_dimension, modulus, limits)?;
        let complex = truncate_complex(complex, max_homology_dimension)?;
        let ordered = GradedComplex::from_filtered(&complex, max_homology_dimension, limits)?;
        let columns = reduce_all_dimensions(&ordered, max_homology_dimension, modulus, limits)?;
        let checked = check_all(&ordered, modulus, &columns, limits)?;
        let mut certificate = Self {
            complex,
            max_homology_dimension,
            modulus,
            columns,
            diagram: checked.diagram,
            digest: [0; 32],
        };
        certificate.digest = certificate.compute_digest()?;
        certificate.verify(limits)?;
        Ok(certificate)
    }

    /// Explicit complex bound to the proof.
    pub fn complex(&self) -> &FilteredSimplicialComplex<ScalarGrade> {
        &self.complex
    }

    /// Highest homology dimension.
    pub fn max_homology_dimension(&self) -> usize {
        self.max_homology_dimension
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Unit-triangular change columns by boundary dimension.
    pub fn columns(&self) -> &[Vec<ChangeColumn>] {
        &self.columns
    }

    /// Diagram derived from the checked reductions.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// Content digest of the payload.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Verify the filtration, every `D V = R` relation, unique pivots, and the diagram.
    pub fn verify(&self, limits: CertificateLimits) -> Result<(), CertificateError> {
        validate_parameters(
            &self.complex,
            self.max_homology_dimension,
            self.modulus,
            limits,
        )?;
        if self.columns.len() != self.max_homology_dimension + 1 {
            return Err(certificate_error(
                "boundary-dimension count differs from the requested range",
            ));
        }
        let ordered =
            GradedComplex::from_filtered(&self.complex, self.max_homology_dimension, limits)?;
        let checked = check_all(&ordered, self.modulus, &self.columns, limits)?;
        if !diagrams_equal(&checked.diagram, &self.diagram) {
            return Err(certificate_error(
                "diagram differs from the checked explicit reductions",
            ));
        }
        if self.compute_digest()? != self.digest {
            return Err(certificate_error(
                "explicit certificate digest does not match",
            ));
        }
        Ok(())
    }

    /// Encode canonical `HOLOSEXP` version 1 bytes.
    pub fn encode(&self, limits: CertificateLimits) -> Result<Vec<u8>, CertificateError> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(certificate_error(
                "explicit certificate exceeds its byte limit",
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSEXP` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: CertificateLimits) -> Result<Self, CertificateError> {
        let (payload, digest) = decode_digest(bytes, limits.max_bytes)?;
        let decoded = decode_payload(payload, digest, limits)?;
        decoded.verify(limits)?;
        Ok(decoded)
    }

    fn encode_payload(&self) -> Result<Vec<u8>, CertificateError> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        put_usize(
            &mut output,
            self.max_homology_dimension,
            "homology dimension",
        )?;
        put_u32(&mut output, self.modulus);
        encode_complex(&mut output, &self.complex)?;
        encode_columns(&mut output, &self.columns)?;
        encode_diagram(&mut output, &self.diagram)?;
        Ok(output)
    }

    fn compute_digest(&self) -> Result<[u8; 32], CertificateError> {
        Ok(Sha256::digest(self.encode_payload()?).into())
    }
}

fn decode_payload(
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

fn validate_parameters(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if max_homology_dimension > limits.max_dimension
        || complex.max_dimension() < max_homology_dimension + 1
    {
        return Err(certificate_error(
            "explicit complex does not cover the requested homology dimensions",
        ));
    }
    if !supported_prime(modulus) {
        return Err(certificate_error(
            "explicit certificate modulus must be a prime below 32768",
        ));
    }
    validate_complex_counts(complex, max_homology_dimension, limits)
}

fn validate_complex_counts(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    for dimension in 0..=max_homology_dimension + 1 {
        let count = complex.simplices()[dimension].len();
        let maximum = simplex_limit(dimension, limits);
        if count > maximum {
            return Err(certificate_error(format!(
                "explicit simplex count in dimension {dimension} exceeds its limit"
            )));
        }
    }
    Ok(())
}

fn truncate_complex(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
) -> Result<FilteredSimplicialComplex<ScalarGrade>, CertificateError> {
    FilteredSimplicialComplex::new(
        complex.vertex_labels().to_vec(),
        complex.simplices()[..=max_homology_dimension + 1].to_vec(),
    )
    .map_err(|error| certificate_error(error.to_string()))
}

fn supported_prime(value: u32) -> bool {
    if !(2..32_768).contains(&value) {
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

fn simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
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

fn decode_digest(bytes: &[u8], maximum: usize) -> Result<(&[u8], [u8; 32]), CertificateError> {
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

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), CertificateError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(certificate_error(
            "explicit certificate envelope version is unsupported",
        ));
    }
    Ok(())
}

fn decode_complex(
    reader: &mut Reader<'_>,
    max_homology_dimension: usize,
    limits: CertificateLimits,
) -> Result<FilteredSimplicialComplex<ScalarGrade>, CertificateError> {
    let labels = decode_labels(reader, limits.max_vertices)?;
    let dimension_count = reader.bounded_usize(
        "simplex dimension count",
        limits.max_dimension.saturating_add(2),
    )?;
    if dimension_count != max_homology_dimension + 2 {
        return Err(certificate_error(
            "explicit simplex dimension count is not canonical",
        ));
    }
    let mut simplices = Vec::with_capacity(dimension_count);
    for dimension in 0..dimension_count {
        simplices.push(decode_simplex_dimension(reader, dimension, limits)?);
    }
    FilteredSimplicialComplex::new(labels, simplices)
        .map_err(|error| certificate_error(error.to_string()))
}

fn decode_labels(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, CertificateError> {
    let count = reader.bounded_usize("vertex label count", maximum)?;
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
    let vertex_count = reader.bounded_usize("simplex vertex count", dimension + 1)?;
    if vertex_count != dimension + 1 {
        return Err(certificate_error(
            "explicit simplex has the wrong vertex count",
        ));
    }
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
    let count = reader.bounded_usize(
        "boundary dimension count",
        max_homology_dimension.saturating_add(1),
    )?;
    if count != max_homology_dimension + 1 {
        return Err(certificate_error(
            "explicit boundary dimension count is not canonical",
        ));
    }
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

    fn finish(&self) -> Result<(), CertificateError> {
        if self.position != self.bytes.len() {
            return Err(certificate_error(
                "explicit certificate has trailing payload bytes",
            ));
        }
        Ok(())
    }
}

fn certificate_error(message: impl Into<String>) -> CertificateError {
    CertificateError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FlagComplexParams, RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

    fn cycle_complex() -> FilteredSimplicialComplex<ScalarGrade> {
        let zero = ScalarGrade::new(0.0).unwrap();
        let one = ScalarGrade::new(1.0).unwrap();
        FilteredSimplicialComplex::new(
            vec![0, 1, 2, 3],
            vec![
                (0..4)
                    .map(|vertex| FilteredSimplex::new(vec![vertex], zero))
                    .collect(),
                vec![
                    FilteredSimplex::new(vec![0, 1], one),
                    FilteredSimplex::new(vec![0, 3], one),
                    FilteredSimplex::new(vec![1, 2], one),
                    FilteredSimplex::new(vec![2, 3], one),
                ],
                Vec::new(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn explicit_cycle_has_one_essential_h1_class() {
        let certificate = ExplicitReductionCertificate::build(
            &cycle_complex(),
            1,
            3,
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(certificate.diagram().in_dim(1).count(), 1);
        assert!(
            certificate
                .diagram()
                .in_dim(1)
                .next()
                .unwrap()
                .is_essential()
        );
        certificate.verify(CertificateLimits::default()).unwrap();
    }

    #[test]
    fn explicit_flag_certificate_matches_the_implicit_engine() {
        let graph = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap();
        let complex = FilteredSimplicialComplex::from_flag_graph(
            &graph,
            &[0, 1, 2, 3],
            FlagComplexParams {
                max_dimension: 2,
                threshold: None,
                limits: crate::ComplexLimits::default(),
            },
        )
        .unwrap();
        let explicit =
            ExplicitReductionCertificate::build(&complex, 1, 5, CertificateLimits::default())
                .unwrap();
        let implicit =
            rips_persistence_sparse(&graph, &RipsParams::new(1).with_modulus(5)).unwrap();
        assert!(diagrams_equal(explicit.diagram(), &implicit));
    }

    #[test]
    fn explicit_artifact_round_trips_and_rejects_mutation() {
        let certificate = ExplicitReductionCertificate::build(
            &cycle_complex(),
            1,
            2,
            CertificateLimits::default(),
        )
        .unwrap();
        let mut bytes = certificate.encode(CertificateLimits::default()).unwrap();
        let decoded =
            ExplicitReductionCertificate::decode(&bytes, CertificateLimits::default()).unwrap();
        assert!(diagrams_equal(certificate.diagram(), decoded.diagram()));
        let checked = holos_tda_check::verify_explicit_persistence(
            &bytes,
            holos_tda_check::ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(checked.max_homology_dimension, 1);
        assert_eq!(checked.modulus, 2);
        assert_eq!(checked.bars.len(), certificate.diagram().bars.len());
        bytes[20] ^= 1;
        assert!(
            ExplicitReductionCertificate::decode(&bytes, CertificateLimits::default()).is_err()
        );
    }
}
