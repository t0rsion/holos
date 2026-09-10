use crate::proof::{ProofColumn, ProofError, ProofTerm, check_column, check_diagram};

use super::claim::ReductionClaim;
use super::model::ProgramProofLimits;
use super::wire::{
    ProgramTotals, Reader, check_identity, check_modulus, check_threshold, decode_bars, error,
};

pub(super) fn decode_reduction(
    bytes: &[u8],
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<ReductionClaim, ProofError> {
    if bytes.len() > limits.max_certificate_bytes {
        return Err(error(format!(
            "{} reduction bytes exceed the decoder limit {}",
            bytes.len(),
            limits.max_certificate_bytes
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_reduction_header(&mut reader, limits)?;
    let edge_columns = decode_columns(
        &mut reader,
        header.edge_count,
        header.modulus,
        limits.max_terms,
        &mut totals.reduction_terms,
        "edge",
    )?;
    let triangle_columns = decode_columns(
        &mut reader,
        header.triangle_count,
        header.modulus,
        limits.max_terms,
        &mut totals.reduction_terms,
        "triangle",
    )?;
    let diagram = decode_bars(&mut reader, header.bar_count)?;
    check_diagram(&diagram)?;
    if reader.remaining() != 0 {
        return Err(error(format!(
            "{} trailing bytes after the reduction envelope",
            reader.remaining()
        )));
    }
    Ok(ReductionClaim {
        modulus: header.modulus,
        vertex_count: header.vertex_count,
        threshold: header.threshold,
        graph_digest: header.graph_digest,
        edge_columns,
        triangle_columns,
        diagram,
    })
}

struct ReductionHeader {
    modulus: u32,
    vertex_count: usize,
    threshold: Option<f64>,
    edge_count: usize,
    triangle_count: usize,
    bar_count: usize,
    graph_digest: [u8; 32],
}

fn decode_reduction_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
) -> Result<ReductionHeader, ProofError> {
    let header = read_reduction_header(reader, limits)?;
    check_modulus(header.modulus)?;
    check_threshold(header.threshold)?;
    let minimum = header
        .edge_count
        .checked_add(header.triangle_count)
        .and_then(|count| count.checked_mul(20))
        .and_then(|columns| {
            header
                .bar_count
                .checked_mul(24)
                .and_then(|bars| columns.checked_add(bars))
        })
        .ok_or_else(|| error("reduction minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(error("reduction record counts exceed the remaining bytes"));
    }
    Ok(header)
}

fn read_reduction_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
) -> Result<ReductionHeader, ProofError> {
    check_identity(reader, b"HOLOSRED")?;
    let modulus = reader.u32()?;
    let vertex_count = reader.bounded_usize("reduction vertex count", limits.max_vertices)?;
    let threshold = reader.optional_f64()?;
    let edge_count = reader.bounded_usize("edge column count", limits.max_edges)?;
    let triangle_count = reader.bounded_usize("triangle column count", limits.max_triangles)?;
    let bar_count = reader.bounded_usize("reduction bar count", limits.max_bars)?;
    let graph_digest = reader.array32()?;
    Ok(ReductionHeader {
        modulus,
        vertex_count,
        threshold,
        edge_count,
        triangle_count,
        bar_count,
        graph_digest,
    })
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
    label: &str,
) -> Result<Vec<ProofColumn>, ProofError> {
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        let term_count = reader.usize()?;
        *total_terms =
            super::wire::bounded_sum(*total_terms, term_count, term_limit, "reduction terms")?;
        let bytes = term_count
            .checked_mul(12)
            .ok_or_else(|| error("reduction term bytes overflow usize"))?;
        if bytes > reader.remaining() {
            return Err(error(format!(
                "{label} column {target} terms exceed the envelope"
            )));
        }
        let mut terms = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            terms.push(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            });
        }
        check_column(target, &terms, modulus)?;
        columns.push(ProofColumn { terms });
    }
    Ok(columns)
}
