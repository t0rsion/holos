//! Binary certificate encoding and decoding.

use crate::{Bar, Diagram};

use super::model::{
    CertificateError, CertificateLimits, CertificateResult, CertificateTerm, ChangeColumn,
    ReductionCertificate,
};
use super::verify::diagram_bits_equal;

const MAGIC: &[u8; 8] = b"HOLOSRED";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

#[derive(Clone, Copy)]
struct RecordCounts {
    edges: usize,
    triangles: usize,
    bars: usize,
}

struct DecodedHeader {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    graph_digest: [u8; 32],
    counts: RecordCounts,
}

fn check_envelope_size(bytes: &[u8], limits: CertificateLimits) -> CertificateResult<()> {
    if bytes.len() > limits.max_bytes {
        return Err(CertificateError::new(format!(
            "{} bytes exceed the limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    Ok(())
}

fn check_wire_preamble(reader: &mut Reader<'_>) -> CertificateResult<()> {
    if reader.take(8)? != MAGIC {
        return Err(CertificateError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(CertificateError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(CertificateError::new(format!(
            "unsupported scalar codec {codec}"
        )));
    }
    Ok(())
}

fn decode_record_counts(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> CertificateResult<RecordCounts> {
    Ok(RecordCounts {
        edges: reader.bounded_usize("edge column count", limits.max_edges)?,
        triangles: reader.bounded_usize("triangle column count", limits.max_triangles)?,
        bars: reader.bounded_usize("bar count", limits.max_bars)?,
    })
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: CertificateLimits,
) -> CertificateResult<DecodedHeader> {
    check_wire_preamble(reader)?;
    let modulus = reader.u32()?;
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let threshold = reader.optional_f64()?;
    let counts = decode_record_counts(reader, limits)?;
    let graph_digest = reader.array32()?;
    Ok(DecodedHeader {
        vertex_count,
        threshold,
        modulus,
        graph_digest,
        counts,
    })
}

fn check_minimum_record_bytes(reader: &Reader<'_>, counts: RecordCounts) -> CertificateResult<()> {
    let minimum = counts
        .edges
        .checked_add(counts.triangles)
        .and_then(|count| count.checked_mul(20))
        .and_then(|bytes| {
            counts
                .bars
                .checked_mul(24)
                .and_then(|bars| bytes.checked_add(bars))
        })
        .ok_or_else(|| CertificateError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(CertificateError::new(format!(
            "record counts need at least {minimum} bytes, only {} remain",
            reader.remaining()
        )));
    }
    Ok(())
}

fn decode_bars(reader: &mut Reader<'_>, count: usize) -> CertificateResult<Vec<Bar>> {
    let mut bars = Vec::with_capacity(count);
    for _ in 0..count {
        bars.push(Bar {
            dim: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    Ok(bars)
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> CertificateResult<()> {
    if reader.remaining() != 0 {
        return Err(CertificateError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}
impl ReductionCertificate {
    /// Encode the canonical `HOLOSRED` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, CertificateError> {
        self.check_envelope(CertificateLimits::default())?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, WIRE_VERSION);
        out.push(F64_BITS_CODEC);
        put_u32(&mut out, self.modulus);
        put_usize(&mut out, self.vertex_count, "vertex count")?;
        put_optional_f64(&mut out, self.threshold);
        put_usize(&mut out, self.edge_columns.len(), "edge column count")?;
        put_usize(
            &mut out,
            self.triangle_columns.len(),
            "triangle column count",
        )?;
        put_usize(&mut out, self.diagram.bars.len(), "bar count")?;
        out.extend_from_slice(&self.graph_digest);
        encode_columns(&mut out, &self.edge_columns)?;
        encode_columns(&mut out, &self.triangle_columns)?;
        for bar in &self.diagram.bars {
            put_usize(&mut out, bar.dim, "bar dimension")?;
            put_u64(&mut out, bar.birth.to_bits());
            put_u64(&mut out, bar.death.to_bits());
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded `HOLOSRED` envelope.
    pub fn decode(
        bytes: &[u8],
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        check_envelope_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        let header = decode_header(&mut reader, limits)?;
        check_minimum_record_bytes(&reader, header.counts)?;
        let mut total_terms = 0usize;
        let edge_columns = decode_columns(
            &mut reader,
            header.counts.edges,
            header.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        let triangle_columns = decode_columns(
            &mut reader,
            header.counts.triangles,
            header.modulus,
            limits.max_terms,
            &mut total_terms,
        )?;
        let bars = decode_bars(&mut reader, header.counts.bars)?;
        check_no_trailing_bytes(&reader)?;
        let certificate = Self {
            vertex_count: header.vertex_count,
            threshold: header.threshold,
            modulus: header.modulus,
            graph_digest: header.graph_digest,
            edge_columns,
            triangle_columns,
            diagram: Diagram { bars },
        };
        certificate.check_envelope(limits)?;
        Ok(certificate)
    }
}

pub(super) fn check_bars(diagram: &Diagram, max_bars: usize) -> CertificateResult<()> {
    if diagram.bars.len() > max_bars {
        return Err(CertificateError::new(format!(
            "{} bars exceed the limit {max_bars}",
            diagram.bars.len()
        )));
    }
    for (index, bar) in diagram.bars.iter().enumerate() {
        if !canonical_bar(bar) {
            return Err(CertificateError::new(format!(
                "bar {index} is not canonical"
            )));
        }
    }
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&canonical, diagram) {
        return Err(CertificateError::new("bars are not in canonical order"));
    }
    Ok(())
}

fn canonical_bar(bar: &Bar) -> bool {
    bar.dim <= 1 && canonical_birth(bar.birth) && canonical_death(bar.birth, bar.death)
}

fn canonical_birth(value: f64) -> bool {
    !value.is_nan() && value.is_finite() && value >= 0.0 && (value != 0.0 || value.to_bits() == 0)
}

fn canonical_death(birth: f64, death: f64) -> bool {
    !death.is_nan() && death >= 0.0 && death > birth && (death != 0.0 || death.to_bits() == 0)
}

pub(super) fn check_change_columns(
    label: &str,
    columns: &[ChangeColumn],
    modulus: u32,
    max_terms: usize,
    total_terms: &mut usize,
) -> std::result::Result<(), CertificateError> {
    for (column_index, column) in columns.iter().enumerate() {
        add_term_count(total_terms, column.terms.len(), max_terms)?;
        check_change_column(label, column_index, column, modulus)?;
    }
    Ok(())
}

fn add_term_count(total: &mut usize, count: usize, maximum: usize) -> CertificateResult<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("certificate term count overflows usize"))?;
    if *total > maximum {
        return Err(CertificateError::new(format!(
            "{} terms exceed the limit {maximum}",
            *total
        )));
    }
    Ok(())
}

pub(super) fn check_change_column(
    label: &str,
    column_index: usize,
    column: &ChangeColumn,
    modulus: u32,
) -> CertificateResult<()> {
    if column.terms.is_empty() {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} is empty"
        )));
    }
    let mut previous = None;
    for (term_index, term) in column.terms.iter().enumerate() {
        check_change_term(label, column_index, term_index, term, previous, modulus)?;
        previous = Some(term.index);
    }
    let unit = CertificateTerm {
        index: column_index,
        coefficient: 1,
    };
    if column.terms.last() != Some(&unit) {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} is not unit triangular"
        )));
    }
    Ok(())
}

fn check_change_term(
    label: &str,
    column_index: usize,
    term_index: usize,
    term: &CertificateTerm,
    previous: Option<usize>,
    modulus: u32,
) -> CertificateResult<()> {
    if term.index > column_index {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} term {term_index} points forward"
        )));
    }
    if previous.is_some_and(|previous| previous >= term.index) {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} terms are not strictly ordered"
        )));
    }
    if term.coefficient == 0 || term.coefficient >= modulus {
        return Err(CertificateError::new(format!(
            "{label} change column {column_index} has invalid coefficient {}",
            term.coefficient
        )));
    }
    Ok(())
}

fn encode_columns(
    out: &mut Vec<u8>,
    columns: &[ChangeColumn],
) -> std::result::Result<(), CertificateError> {
    for column in columns {
        put_usize(out, column.terms.len(), "change-column term count")?;
        for term in &column.terms {
            put_usize(out, term.index, "change-column source index")?;
            put_u32(out, term.coefficient);
        }
    }
    Ok(())
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    max_terms: usize,
    total_terms: &mut usize,
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    let mut columns = Vec::with_capacity(count);
    for column_index in 0..count {
        columns.push(decode_column(reader, column_index, max_terms, total_terms)?);
    }
    let mut checked = 0;
    check_change_columns("decoded", &columns, modulus, max_terms, &mut checked)?;
    Ok(columns)
}

fn decode_column(
    reader: &mut Reader<'_>,
    column_index: usize,
    max_terms: usize,
    total_terms: &mut usize,
) -> CertificateResult<ChangeColumn> {
    let term_count = reader.usize()?;
    add_term_count(total_terms, term_count, max_terms)?;
    let bytes = term_count
        .checked_mul(12)
        .ok_or_else(|| CertificateError::new("term bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(CertificateError::new(format!(
            "column {column_index} terms exceed the remaining bytes"
        )));
    }
    let mut terms = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        terms.push(decode_term(reader)?);
    }
    Ok(ChangeColumn { terms })
}

fn decode_term(reader: &mut Reader<'_>) -> CertificateResult<CertificateTerm> {
    Ok(CertificateTerm {
        index: reader.usize()?,
        coefficient: reader.u32()?,
    })
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), CertificateError> {
    let value = u64::try_from(value)
        .map_err(|_| CertificateError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
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

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], CertificateError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| CertificateError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(CertificateError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, CertificateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, CertificateError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> std::result::Result<u32, CertificateError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, CertificateError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, CertificateError> {
        usize::try_from(self.u64()?)
            .map_err(|_| CertificateError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, CertificateError> {
        let value = self.usize()?;
        if value > limit {
            return Err(CertificateError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_f64(&mut self) -> std::result::Result<Option<f64>, CertificateError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(CertificateError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], CertificateError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
