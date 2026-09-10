use std::collections::BTreeMap;

use crate::certificate::{CertificateError, CertificateLimits};

use super::model::{InterfaceCell, InterfaceChainTerm};
use super::reduction::SparseColumn;
use super::validation::{cell_order, enforce_cell_limits};

pub(super) fn boundary_matrices(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
) -> Result<Vec<Vec<SparseColumn>>, CertificateError> {
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
                        if term.coefficient == 0 || term.coefficient >= modulus {
                            return Err(CertificateError::new(
                                "relative boundary coefficient is outside the field",
                            ));
                        }
                        let row = rows[dimension - 1].get(&term.cell).ok_or_else(|| {
                            CertificateError::new(format!(
                                "relative boundary of {:?} references absent cell {:?}",
                                cell.vertices, term.cell
                            ))
                        })?;
                        column.insert(*row, term.coefficient as u64);
                    }
                    Ok(column)
                })
                .collect()
        })
        .collect()
}

pub(super) fn check_chain_complex(
    cells: &[Vec<InterfaceCell>],
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    check_cell_dimension_count(cells.len(), max_dim)?;
    enforce_cell_limits(cells, limits)?;
    let maps = cell_reference_maps(cells);
    for (dimension, dimension_cells) in cells.iter().enumerate() {
        check_cell_dimension(dimension_cells, dimension, &maps, modulus)?;
    }
    check_boundary_squared(cells, &maps, modulus)?;
    Ok(())
}

fn check_cell_dimension_count(count: usize, max_dim: usize) -> Result<(), CertificateError> {
    if count != max_dim + 2 {
        return Err(CertificateError::new(
            "relative interface has the wrong dimension count",
        ));
    }
    Ok(())
}

fn cell_reference_maps(cells: &[Vec<InterfaceCell>]) -> Vec<BTreeMap<Vec<usize>, &InterfaceCell>> {
    cells
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect()
        })
        .collect()
}

fn check_cell_dimension(
    cells: &[InterfaceCell],
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut previous = None;
    for cell in cells {
        check_interface_cell(cell, dimension, previous, maps, modulus)?;
        previous = Some(cell);
    }
    if maps[dimension].len() != cells.len() {
        return Err(CertificateError::new("relative interface repeats a cell"));
    }
    Ok(())
}

fn check_interface_cell(
    cell: &InterfaceCell,
    dimension: usize,
    previous: Option<&InterfaceCell>,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    check_interface_cell_shape(cell, dimension, previous)?;
    check_boundary_terms(cell, dimension, maps, modulus)
}

fn check_interface_cell_shape(
    cell: &InterfaceCell,
    dimension: usize,
    previous: Option<&InterfaceCell>,
) -> Result<(), CertificateError> {
    let invalid = cell.vertices.len() != dimension + 1
        || cell.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || !cell.value.is_finite()
        || cell.value < 0.0
        || previous.is_some_and(|prior| cell_order(prior, cell).is_gt());
    if invalid {
        return Err(CertificateError::new(
            "relative interface cell order or value is invalid",
        ));
    }
    Ok(())
}

fn check_boundary_terms(
    cell: &InterfaceCell,
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut previous = None;
    for term in &cell.boundary {
        let face = find_boundary_face(cell, term, dimension, maps)?;
        check_boundary_term(term, face, cell.value, previous, modulus)?;
        previous = Some(term.cell.as_slice());
    }
    Ok(())
}

fn find_boundary_face<'a>(
    cell: &InterfaceCell,
    term: &InterfaceChainTerm,
    dimension: usize,
    maps: &'a [BTreeMap<Vec<usize>, &InterfaceCell>],
) -> Result<&'a InterfaceCell, CertificateError> {
    maps.get(dimension.wrapping_sub(1))
        .and_then(|rows| rows.get(&term.cell))
        .copied()
        .ok_or_else(|| {
            CertificateError::new(format!(
                "relative boundary of {:?} references absent cell {:?}",
                cell.vertices, term.cell
            ))
        })
}

fn check_boundary_term(
    term: &InterfaceChainTerm,
    face: &InterfaceCell,
    value: f64,
    previous: Option<&[usize]>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let invalid = term.coefficient == 0
        || term.coefficient >= modulus
        || face.value > value
        || previous.is_some_and(|prior| prior >= term.cell.as_slice());
    if invalid {
        return Err(CertificateError::new(
            "relative boundary term is not canonical or filtered",
        ));
    }
    Ok(())
}

fn check_boundary_squared(
    cells: &[Vec<InterfaceCell>],
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    for (dimension, dimension_cells) in cells.iter().enumerate().skip(2) {
        for cell in dimension_cells {
            check_cell_boundary_squared(cell, dimension, maps, modulus)?;
        }
    }
    Ok(())
}

fn check_cell_boundary_squared(
    cell: &InterfaceCell,
    dimension: usize,
    maps: &[BTreeMap<Vec<usize>, &InterfaceCell>],
    modulus: u32,
) -> Result<(), CertificateError> {
    let mut square = BTreeMap::<Vec<usize>, u64>::new();
    for term in &cell.boundary {
        let face = maps[dimension - 1][&term.cell];
        add_squared_boundary(&mut square, term.coefficient, &face.boundary, modulus);
    }
    if !square.is_empty() {
        return Err(CertificateError::new(
            "relative interface boundary does not square to zero",
        ));
    }
    Ok(())
}

fn add_squared_boundary(
    square: &mut BTreeMap<Vec<usize>, u64>,
    coefficient: u32,
    boundary: &[InterfaceChainTerm],
    modulus: u32,
) {
    for lower in boundary {
        let value = (square.get(&lower.cell).copied().unwrap_or(0)
            + coefficient as u64 * lower.coefficient as u64)
            % modulus as u64;
        if value == 0 {
            square.remove(&lower.cell);
        } else {
            square.insert(lower.cell.clone(), value);
        }
    }
}
