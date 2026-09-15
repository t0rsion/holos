use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Bar, Diagram};

use super::super::model::{
    InterfaceCancellation, InterfaceCell, InterfaceChainTerm, RelativeInterfaceCertificate,
    RelativeInterfaceWork,
};
use super::super::reduction::reduce_core;
use super::super::validation::count_cells;
use super::encode::check_encoded_size;
use super::{F64_BITS_CODEC, MAGIC, VERSION};

const WIRE_USIZE_BYTES: usize = 8;
const CANCELLATION_BYTES: usize = 2 * WIRE_USIZE_BYTES + 4;
const CELL_BYTES: usize = 3 * WIRE_USIZE_BYTES;
const BOUNDARY_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;
const CHANGE_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;
const BAR_BYTES: usize = 3 * WIRE_USIZE_BYTES;

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

pub(super) fn decode(
    bytes: &[u8],
    limits: CertificateLimits,
) -> Result<RelativeInterfaceCertificate, CertificateError> {
    check_encoded_size(bytes.len(), limits.max_bytes)?;
    let mut reader = Reader::new(bytes);
    let header = decode_interface_header(&mut reader, limits)?;
    let body = decode_interface_body(&mut reader, &header, limits)?;
    check_no_trailing_bytes(&reader)?;
    finish_decoded_interface(header, body, limits)
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
    reader.require_bytes(count, WIRE_USIZE_BYTES, "protected vertices")?;
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
    let upper_len = max_dim
        .checked_add(2)
        .ok_or_else(|| CertificateError::new("relative cancellation key length overflows"))?;
    let lower_len = max_dim
        .checked_add(1)
        .ok_or_else(|| CertificateError::new("relative cancellation key length overflows"))?;
    reader.require_bytes(count, CANCELLATION_BYTES, "cancellations")?;
    let mut cancellations = Vec::with_capacity(count);
    for _ in 0..count {
        cancellations.push(InterfaceCancellation {
            upper: decode_key(reader, upper_len, limits.max_vertices)?,
            lower: decode_key(reader, lower_len, limits.max_vertices)?,
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
    reader.require_bytes(count, BAR_BYTES, "diagram bars")?;
    let mut diagram = Diagram::default();
    diagram.bars.reserve(count);
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

fn decode_cells(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    let dimension_count = max_dim
        .checked_add(2)
        .ok_or_else(|| CertificateError::new("relative cell dimension count overflows"))?;
    check_decoded_dimension_count(reader, "cell", dimension_count)?;
    reader.require_bytes(dimension_count, WIRE_USIZE_BYTES, "cell dimension headers")?;
    let mut cells = Vec::with_capacity(dimension_count);
    let mut total_terms = 0usize;
    for dimension in 0..dimension_count {
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
    reader.require_bytes(count, CELL_BYTES, "cells")?;
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
    let expected_vertices = dimension
        .checked_add(1)
        .ok_or_else(|| CertificateError::new("relative cell vertex count overflows"))?;
    let vertices = decode_key(reader, expected_vertices, limits.max_vertices)?;
    check_decoded_cell_dimension(vertices.len(), expected_vertices)?;
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
    expected_vertices: usize,
) -> Result<(), CertificateError> {
    if vertex_count != expected_vertices {
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
    reader.require_bytes(count, BOUNDARY_TERM_BYTES, "boundary terms")?;
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
    let dimension_count = max_dim
        .checked_add(1)
        .ok_or_else(|| CertificateError::new("relative reduction dimension count overflows"))?;
    check_decoded_dimension_count(reader, "reduction", dimension_count)?;
    reader.require_bytes(
        dimension_count,
        WIRE_USIZE_BYTES,
        "reduction dimension headers",
    )?;
    let mut columns = Vec::with_capacity(dimension_count);
    let mut total_terms = 0usize;
    for dimension in 1..=dimension_count {
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
    reader.require_bytes(count, WIRE_USIZE_BYTES, "reduction column headers")?;
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
    reader.require_bytes(count, CHANGE_TERM_BYTES, "change terms")?;
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
    reader.require_bytes(count, WIRE_USIZE_BYTES, "cell key vertices")?;
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

    fn require_bytes(
        &self,
        count: usize,
        minimum_width: usize,
        label: &str,
    ) -> Result<(), CertificateError> {
        let required = count.checked_mul(minimum_width).ok_or_else(|| {
            CertificateError::new(format!("{label} minimum byte count overflows"))
        })?;
        let remaining = self.remaining();
        if required > remaining {
            return Err(CertificateError::new(format!(
                "{label} requires at least {required} bytes, only {remaining} remain"
            )));
        }
        Ok(())
    }

    fn array32(&mut self) -> Result<[u8; 32], CertificateError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_count_requires_minimum_encoded_bytes() {
        let bytes = 1u64.to_be_bytes();
        let mut reader = Reader::new(&bytes);
        let error =
            decode_protected_vertices(&mut reader, CertificateLimits::default()).unwrap_err();
        assert!(
            error
                .message()
                .contains("protected vertices requires at least 8 bytes")
        );
    }
}
