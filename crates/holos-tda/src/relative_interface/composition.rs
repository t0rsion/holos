use std::collections::BTreeMap;

use crate::certificate::CertificateError;

use super::model::{InterfaceCell, RelativeInterfaceCertificate};
use super::validation::cell_order;

pub(super) fn check_composition_children<'a>(
    children: &[&'a RelativeInterfaceCertificate],
) -> Result<&'a RelativeInterfaceCertificate, CertificateError> {
    let first = children
        .first()
        .copied()
        .ok_or_else(|| CertificateError::new("relative composition requires a child"))?;
    for child in children {
        if child.max_dim != first.max_dim || child.modulus != first.modulus {
            return Err(CertificateError::new(
                "relative composition requires one dimension and coefficient field",
            ));
        }
    }
    Ok(first)
}

pub(super) fn merge_child_cells(
    children: &[&RelativeInterfaceCertificate],
    max_dim: usize,
) -> Result<Vec<Vec<InterfaceCell>>, CertificateError> {
    let mut dimensions = vec![DimensionMap::new(); max_dim + 2];
    for child in children {
        merge_one_child(&mut dimensions, &child.core_cells)?;
    }
    Ok(maps_to_cells(dimensions))
}

fn merge_one_child(
    dimensions: &mut [DimensionMap],
    child: &[Vec<InterfaceCell>],
) -> Result<(), CertificateError> {
    for (dimension, cells) in child.iter().enumerate() {
        for cell in cells {
            merge_interface_cell(&mut dimensions[dimension], cell)?;
        }
    }
    Ok(())
}

fn merge_interface_cell(
    cells: &mut DimensionMap,
    cell: &InterfaceCell,
) -> Result<(), CertificateError> {
    if let Some(existing) = cells.get(&cell.vertices) {
        if existing != cell {
            return Err(CertificateError::new(
                "identified interface cells have different filtered boundaries",
            ));
        }
        return Ok(());
    }
    cells.insert(cell.vertices.clone(), cell.clone());
    Ok(())
}

pub(super) type DimensionMap = BTreeMap<Vec<usize>, InterfaceCell>;

pub(super) fn cells_to_maps(
    cells: Vec<Vec<InterfaceCell>>,
) -> Result<Vec<DimensionMap>, CertificateError> {
    cells
        .into_iter()
        .map(|dimension| {
            let expected = dimension.len();
            let map: DimensionMap = dimension
                .into_iter()
                .map(|cell| (cell.vertices.clone(), cell))
                .collect();
            if map.len() != expected {
                Err(CertificateError::new("relative interface repeats a cell"))
            } else {
                Ok(map)
            }
        })
        .collect()
}

pub(super) fn maps_to_cells(maps: Vec<DimensionMap>) -> Vec<Vec<InterfaceCell>> {
    maps.into_iter()
        .map(|dimension| {
            let mut cells: Vec<_> = dimension.into_values().collect();
            cells.sort_by(cell_order);
            cells
        })
        .collect()
}
