use super::super::{ProofColumn, ProofError, ProofLimits, ProofTerm, Reader, check_column};
use super::digest::dimension_limit;
use super::model::{Cell, Term};

const WIRE_USIZE_BYTES: usize = 8;
const CELL_BYTES: usize = 3 * WIRE_USIZE_BYTES;
const BOUNDARY_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;
const CHANGE_TERM_BYTES: usize = WIRE_USIZE_BYTES + 4;

pub(super) fn decode_cells(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    let dimension_count = max_dim
        .checked_add(2)
        .ok_or_else(|| ProofError::new("relative cell dimension count overflows"))?;
    if reader.bounded_usize("cell dimension count", dimension_count)? != dimension_count {
        return Err(ProofError::new(
            "relative-interface cell dimension count is wrong",
        ));
    }
    reader.require_bytes(dimension_count, WIRE_USIZE_BYTES, "cell dimension headers")?;
    let mut cells = Vec::with_capacity(dimension_count);
    let mut total_terms = 0usize;
    for dimension in 0..dimension_count {
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

pub(super) fn decode_cell_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Cell>, ProofError> {
    let limit = dimension_limit(dimension, limits);
    let count = reader.bounded_usize("dimension cell count", limit)?;
    reader.require_bytes(count, CELL_BYTES, "cells")?;
    (0..count)
        .map(|_| decode_cell(reader, dimension, modulus, total_terms, limit, limits))
        .collect()
}

pub(super) fn decode_cell(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    term_limit: usize,
    limits: ProofLimits,
) -> Result<Cell, ProofError> {
    let expected_vertices = dimension
        .checked_add(1)
        .ok_or_else(|| ProofError::new("relative cell vertex count overflows"))?;
    let vertices = decode_key(reader, expected_vertices, limits.max_vertices)?;
    if vertices.len() != expected_vertices {
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

pub(super) fn decode_boundary(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    term_limit: usize,
    limits: ProofLimits,
) -> Result<Vec<Term>, ProofError> {
    let count = reader.bounded_usize("cell boundary term count", term_limit)?;
    add_term_count(total_terms, count, limits.max_terms, "relative boundary")?;
    reader.require_bytes(count, BOUNDARY_TERM_BYTES, "boundary terms")?;
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

pub(super) fn add_term_count(
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

pub(super) fn decode_columns(
    reader: &mut Reader<'_>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<ProofColumn>>, ProofError> {
    let dimension_count = max_dim
        .checked_add(1)
        .ok_or_else(|| ProofError::new("relative reduction dimension count overflows"))?;
    if reader.bounded_usize("reduction dimension count", dimension_count)? != dimension_count {
        return Err(ProofError::new(
            "relative reduction dimension count is wrong",
        ));
    }
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
            &mut total_terms,
            limits,
        )?);
    }
    Ok(columns)
}

pub(super) fn decode_column_dimension(
    reader: &mut Reader<'_>,
    dimension: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<ProofColumn>, ProofError> {
    let count =
        reader.bounded_usize("reduction column count", dimension_limit(dimension, limits))?;
    reader.require_bytes(count, WIRE_USIZE_BYTES, "reduction column headers")?;
    (0..count)
        .map(|target| decode_column(reader, target, modulus, total_terms, limits))
        .collect()
}

pub(super) fn decode_column(
    reader: &mut Reader<'_>,
    target: usize,
    modulus: u32,
    total_terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofColumn, ProofError> {
    let count = reader.bounded_usize("change term count", limits.max_terms)?;
    add_term_count(total_terms, count, limits.max_terms, "change")?;
    reader.require_bytes(count, CHANGE_TERM_BYTES, "change terms")?;
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

pub(super) fn decode_key(
    reader: &mut Reader<'_>,
    maximum_len: usize,
    maximum_vertex: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("cell key length", maximum_len)?;
    reader.require_bytes(count, WIRE_USIZE_BYTES, "cell key vertices")?;
    let mut key = Vec::with_capacity(count);
    for _ in 0..count {
        key.push(reader.bounded_usize("cell vertex", maximum_vertex)?);
    }
    if key.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ProofError::new("cell key is not canonical"));
    }
    Ok(key)
}
