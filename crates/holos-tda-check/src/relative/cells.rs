use super::super::{ProofColumn, ProofError, ProofLimits, ProofTerm, Reader, check_column};
use super::digest::dimension_limit;
use super::model::{Cell, Term};

pub(super) fn decode_cells(
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

pub(super) fn decode_cell_dimension(
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

pub(super) fn decode_cell(
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

pub(super) fn decode_column_dimension(
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

pub(super) fn decode_column(
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

pub(super) fn decode_key(
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
