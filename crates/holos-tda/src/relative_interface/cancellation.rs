use std::collections::{BTreeMap, BTreeSet};

use crate::certificate::CertificateError;

use super::composition::{DimensionMap, cells_to_maps, maps_to_cells};
use super::digest::inverse_mod;
use super::model::{InterfaceCancellation, InterfaceCell, InterfaceChainTerm};
use super::validation::{cell_order, is_protected};

pub(super) fn simplex_boundary(vertices: &[usize], modulus: u32) -> Vec<InterfaceChainTerm> {
    let mut boundary = (0..vertices.len())
        .map(|removed| {
            let mut face = vertices.to_vec();
            face.remove(removed);
            InterfaceChainTerm {
                cell: face,
                coefficient: if removed % 2 == 0 { 1 } else { modulus - 1 },
            }
        })
        .collect::<Vec<_>>();
    boundary.sort();
    boundary
}

pub(super) fn cancel_relative(
    cells: Vec<Vec<InterfaceCell>>,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(Vec<Vec<InterfaceCell>>, Vec<InterfaceCancellation>), CertificateError> {
    let mut maps = cells_to_maps(cells)?;
    let mut steps = Vec::new();
    while let Some(step) = next_cancellation(&maps, protected) {
        apply_cancellation(&mut maps, &step, protected, modulus)?;
        steps.push(step);
    }
    Ok((maps_to_cells(maps), steps))
}

pub(super) fn replay_cancellations(
    cells: Vec<Vec<InterfaceCell>>,
    steps: &[InterfaceCancellation],
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    let mut maps = cells_to_maps(cells)?;
    for step in steps {
        apply_cancellation(&mut maps, step, protected, modulus)?;
    }
    Ok(maps_to_cells(maps))
}

fn next_cancellation(
    cells: &[DimensionMap],
    protected: &BTreeSet<usize>,
) -> Option<InterfaceCancellation> {
    for dimension in 1..cells.len() {
        let mut upper_cells: Vec<_> = cells[dimension].values().collect();
        upper_cells.sort_by(|left, right| cell_order(left, right));
        for upper in upper_cells {
            if is_protected(&upper.vertices, protected) {
                continue;
            }
            for term in &upper.boundary {
                let lower = cells[dimension - 1].get(&term.cell)?;
                if lower.value.to_bits() == upper.value.to_bits()
                    && !is_protected(&lower.vertices, protected)
                {
                    return Some(InterfaceCancellation {
                        upper: upper.vertices.clone(),
                        lower: lower.vertices.clone(),
                        coefficient: term.coefficient,
                    });
                }
            }
        }
    }
    None
}

fn apply_cancellation(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let pair = prepare_cancellation(cells, step, protected, modulus)?;
    eliminate_lower_boundary(cells, step, &pair, modulus);
    remove_upper_cofaces(cells, step, pair.dimension);
    cells[pair.dimension].remove(&step.upper);
    cells[pair.dimension - 1].remove(&step.lower);
    Ok(())
}

struct CancellationPair {
    dimension: usize,
    upper: InterfaceCell,
    inverse: u32,
}

fn prepare_cancellation(
    cells: &[DimensionMap],
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<CancellationPair, CertificateError> {
    let dimension = checked_cancellation_dimension(cells, step)?;
    check_unprotected_cancellation(step, protected)?;
    let upper = cells[dimension]
        .get(&step.upper)
        .cloned()
        .ok_or_else(|| CertificateError::new("relative cancellation upper cell is absent"))?;
    let lower = cells[dimension - 1]
        .get(&step.lower)
        .ok_or_else(|| CertificateError::new("relative cancellation lower cell is absent"))?;
    check_cancellation_incidence(&upper, lower, step, modulus)?;
    Ok(CancellationPair {
        dimension,
        upper,
        inverse: inverse_mod(step.coefficient as u64, modulus as u64) as u32,
    })
}

fn checked_cancellation_dimension(
    cells: &[DimensionMap],
    step: &InterfaceCancellation,
) -> Result<usize, CertificateError> {
    let dimension =
        step.upper.len().checked_sub(1).ok_or_else(|| {
            CertificateError::new("relative cancellation has an empty upper cell")
        })?;
    if dimension == 0 || step.lower.len() != dimension || dimension >= cells.len() {
        return Err(CertificateError::new(
            "relative cancellation cells have incompatible dimensions",
        ));
    }
    Ok(dimension)
}

fn check_unprotected_cancellation(
    step: &InterfaceCancellation,
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    if is_protected(&step.upper, protected) || is_protected(&step.lower, protected) {
        return Err(CertificateError::new(
            "relative cancellation removes a protected separator cell",
        ));
    }
    Ok(())
}

fn check_cancellation_incidence(
    upper: &InterfaceCell,
    lower: &InterfaceCell,
    step: &InterfaceCancellation,
    modulus: u32,
) -> Result<(), CertificateError> {
    if upper.value.to_bits() != lower.value.to_bits() {
        return Err(CertificateError::new(
            "relative cancellation crosses a filtration value",
        ));
    }
    let coefficient = boundary_coefficient(&upper.boundary, &step.lower)
        .ok_or_else(|| CertificateError::new("relative cancellation cells are not incident"))?;
    if coefficient != step.coefficient || coefficient == 0 || coefficient >= modulus {
        return Err(CertificateError::new(
            "relative cancellation has the wrong incidence coefficient",
        ));
    }
    Ok(())
}

fn eliminate_lower_boundary(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    pair: &CancellationPair,
    modulus: u32,
) {
    let keys: Vec<_> = cells[pair.dimension].keys().cloned().collect();
    for key in keys {
        if key == step.upper {
            continue;
        }
        let cell = cells[pair.dimension].get_mut(&key).unwrap();
        let Some(value) = boundary_coefficient(&cell.boundary, &step.lower) else {
            continue;
        };
        let factor = ((modulus as u64 - value as u64 * pair.inverse as u64 % modulus as u64)
            % modulus as u64) as u32;
        add_boundary_scaled(&mut cell.boundary, &pair.upper.boundary, factor, modulus);
    }
}

fn remove_upper_cofaces(
    cells: &mut [DimensionMap],
    step: &InterfaceCancellation,
    dimension: usize,
) {
    if dimension + 1 < cells.len() {
        for cell in cells[dimension + 1].values_mut() {
            remove_boundary_term(&mut cell.boundary, &step.upper);
        }
    }
}

fn boundary_coefficient(boundary: &[InterfaceChainTerm], key: &[usize]) -> Option<u32> {
    boundary
        .binary_search_by(|term| term.cell.as_slice().cmp(key))
        .ok()
        .map(|position| boundary[position].coefficient)
}

fn remove_boundary_term(boundary: &mut Vec<InterfaceChainTerm>, key: &[usize]) {
    if let Ok(position) = boundary.binary_search_by(|term| term.cell.as_slice().cmp(key)) {
        boundary.remove(position);
    }
}

fn add_boundary_scaled(
    target: &mut Vec<InterfaceChainTerm>,
    source: &[InterfaceChainTerm],
    factor: u32,
    modulus: u32,
) {
    let mut values: BTreeMap<_, _> = target
        .iter()
        .map(|term| (term.cell.clone(), term.coefficient))
        .collect();
    for term in source {
        let value = (values.get(&term.cell).copied().unwrap_or(0) as u64
            + factor as u64 * term.coefficient as u64)
            % modulus as u64;
        if value == 0 {
            values.remove(&term.cell);
        } else {
            values.insert(term.cell.clone(), value as u32);
        }
    }
    *target = values
        .into_iter()
        .map(|(cell, coefficient)| InterfaceChainTerm { cell, coefficient })
        .collect();
}
