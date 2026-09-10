use std::collections::{BTreeMap, BTreeSet};

use crate::inverse_mod;

use super::super::ProofError;
use super::digest::cell_order;
use super::model::{Cell, DimensionMap, Step, Term};
use super::reduction::is_protected;

pub(super) fn replay(
    cells: Vec<Vec<Cell>>,
    steps: &[Step],
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    let mut maps = cells_to_maps(cells)?;
    for step in steps {
        apply_step(&mut maps, step, protected, modulus)?;
    }
    Ok(maps_to_cells(maps))
}

pub(super) fn apply_step(
    cells: &mut [DimensionMap],
    step: &Step,
    protected: &BTreeSet<usize>,
    modulus: u32,
) -> Result<(), ProofError> {
    let dimension = cancellation_dimension(cells.len(), step)?;
    verify_unprotected(step, protected)?;
    let upper = prepare_cancellation(cells, step, dimension, modulus)?;
    eliminate_lower(cells, step, dimension, &upper, modulus);
    remove_upper_from_cofaces(cells, step, dimension);
    cells[dimension].remove(&step.upper);
    cells[dimension - 1].remove(&step.lower);
    Ok(())
}

pub(super) fn cancellation_dimension(
    cell_dimensions: usize,
    step: &Step,
) -> Result<usize, ProofError> {
    let dimension = step
        .upper
        .len()
        .checked_sub(1)
        .ok_or_else(|| ProofError::new("relative cancellation upper cell is empty"))?;
    if dimension == 0 || step.lower.len() != dimension || dimension >= cell_dimensions {
        Err(ProofError::new(
            "relative cancellation dimensions are incompatible",
        ))
    } else {
        Ok(dimension)
    }
}

pub(super) fn verify_unprotected(
    step: &Step,
    protected: &BTreeSet<usize>,
) -> Result<(), ProofError> {
    if is_protected(&step.upper, protected) || is_protected(&step.lower, protected) {
        Err(ProofError::new(
            "relative cancellation removes a protected cell",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn prepare_cancellation(
    cells: &[DimensionMap],
    step: &Step,
    dimension: usize,
    modulus: u32,
) -> Result<Cell, ProofError> {
    let upper = cells[dimension]
        .get(&step.upper)
        .cloned()
        .ok_or_else(|| ProofError::new("relative cancellation upper cell is absent"))?;
    let lower = cells[dimension - 1]
        .get(&step.lower)
        .ok_or_else(|| ProofError::new("relative cancellation lower cell is absent"))?;
    if upper.value.to_bits() != lower.value.to_bits() {
        return Err(ProofError::new(
            "relative cancellation crosses a filtration value",
        ));
    }
    let coefficient = boundary_coefficient(&upper.boundary, &step.lower)
        .ok_or_else(|| ProofError::new("relative cancellation cells are not incident"))?;
    if coefficient != step.coefficient || coefficient == 0 || coefficient >= modulus {
        return Err(ProofError::new(
            "relative cancellation coefficient is wrong",
        ));
    }
    Ok(upper)
}

pub(super) fn eliminate_lower(
    cells: &mut [DimensionMap],
    step: &Step,
    dimension: usize,
    upper: &Cell,
    modulus: u32,
) {
    let inverse = inverse_mod(u64::from(step.coefficient), u64::from(modulus)) as u32;
    let keys: Vec<_> = cells[dimension].keys().cloned().collect();
    for key in keys {
        if key == step.upper {
            continue;
        }
        let cell = cells[dimension].get_mut(&key).unwrap();
        let Some(value) = boundary_coefficient(&cell.boundary, &step.lower) else {
            continue;
        };
        let factor = ((u64::from(modulus)
            - u64::from(value) * u64::from(inverse) % u64::from(modulus))
            % u64::from(modulus)) as u32;
        add_boundary_scaled(&mut cell.boundary, &upper.boundary, factor, modulus);
    }
}

pub(super) fn remove_upper_from_cofaces(cells: &mut [DimensionMap], step: &Step, dimension: usize) {
    if dimension + 1 < cells.len() {
        for cell in cells[dimension + 1].values_mut() {
            remove_boundary_term(&mut cell.boundary, &step.upper);
        }
    }
}

pub(super) fn cells_to_maps(cells: Vec<Vec<Cell>>) -> Result<Vec<DimensionMap>, ProofError> {
    cells
        .into_iter()
        .map(|dimension| {
            let expected = dimension.len();
            let map: DimensionMap = dimension
                .into_iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect();
            if map.len() == expected {
                Ok(map)
            } else {
                Err(ProofError::new("relative interface repeats a cell"))
            }
        })
        .collect()
}

pub(super) fn maps_to_cells(maps: Vec<DimensionMap>) -> Vec<Vec<Cell>> {
    maps.into_iter()
        .map(|dimension| {
            let mut values: Vec<_> = dimension.into_values().collect();
            values.sort_by(cell_order);
            values
        })
        .collect()
}

pub(super) fn boundary_coefficient(boundary: &[Term], key: &[usize]) -> Option<u32> {
    boundary
        .binary_search_by(|term| term.cell.as_slice().cmp(key))
        .ok()
        .map(|position| boundary[position].coefficient)
}

pub(super) fn remove_boundary_term(boundary: &mut Vec<Term>, key: &[usize]) {
    if let Ok(position) = boundary.binary_search_by(|term| term.cell.as_slice().cmp(key)) {
        boundary.remove(position);
    }
}

pub(super) fn add_boundary_scaled(
    target: &mut Vec<Term>,
    source: &[Term],
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
        .map(|(cell, coefficient)| Term { cell, coefficient })
        .collect();
}
