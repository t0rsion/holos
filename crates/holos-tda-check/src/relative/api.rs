use std::collections::BTreeMap;

use super::super::{ProofError, ProofLimits};
use super::decode::decode_verified;
use super::digest::{cell_order, count_cells};
use super::model::{Cell, DimensionMap, VerifiedCertificate, VerifiedRelativeInterface};

/// Verify one bounded `HOLOSRI` certificate.
pub fn verify_relative_interface(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedRelativeInterface, ProofError> {
    let certificate = decode_verified(bytes, limits)?;
    Ok(relative_summary(&certificate))
}

/// Verify that a parent input is the keyed union of checked child cores.
///
/// The parent certificate separately proves every cancellation and its
/// final reduction.
pub fn verify_relative_composition(
    parent: &[u8],
    children: &[&[u8]],
    expected_protected_vertices: &[usize],
    limits: ProofLimits,
) -> Result<VerifiedRelativeInterface, ProofError> {
    require_children(children)?;
    let parent = decode_verified(parent, limits)?;
    verify_parent_protected(&parent, expected_protected_vertices)?;
    let mut union = vec![BTreeMap::<Vec<usize>, Cell>::new(); parent.max_dim + 2];
    for bytes in children {
        let child = decode_verified(bytes, limits)?;
        merge_child(&parent, &child, &mut union)?;
    }
    let expected = union_cells(union);
    if expected != parent.input {
        return Err(ProofError::new(
            "relative parent input differs from the keyed child-core union",
        ));
    }
    Ok(relative_summary(&parent))
}

pub(super) fn relative_summary(certificate: &VerifiedCertificate) -> VerifiedRelativeInterface {
    VerifiedRelativeInterface {
        digest: certificate.digest,
        max_dim: certificate.max_dim,
        input_cells: count_cells(&certificate.input),
        cancellations: certificate.steps.len(),
        core_cells: count_cells(&certificate.core),
        reduction_columns: certificate.columns.iter().map(Vec::len).sum(),
        bars: certificate.diagram.len(),
    }
}

pub(super) fn require_children(children: &[&[u8]]) -> Result<(), ProofError> {
    if children.is_empty() {
        Err(ProofError::new(
            "relative composition requires a child certificate",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn verify_parent_protected(
    parent: &VerifiedCertificate,
    expected: &[usize],
) -> Result<(), ProofError> {
    if expected.windows(2).any(|pair| pair[0] >= pair[1]) || parent.protected_vertices != expected {
        Err(ProofError::new(
            "relative parent has the wrong protected vertex set",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn merge_child(
    parent: &VerifiedCertificate,
    child: &VerifiedCertificate,
    union: &mut [DimensionMap],
) -> Result<(), ProofError> {
    if child.max_dim != parent.max_dim || child.modulus != parent.modulus {
        return Err(ProofError::new(
            "relative composition changes dimension or coefficient field",
        ));
    }
    for (dimension, cells) in child.core.iter().enumerate() {
        for cell in cells {
            merge_child_cell(&mut union[dimension], cell)?;
        }
    }
    Ok(())
}

pub(super) fn merge_child_cell(
    dimension: &mut DimensionMap,
    cell: &Cell,
) -> Result<(), ProofError> {
    match dimension.get(&cell.vertices) {
        Some(existing) if existing != cell => Err(ProofError::new(
            "relative composition identifies conflicting cells",
        )),
        Some(_) => Ok(()),
        None => {
            dimension.insert(cell.vertices.clone(), cell.clone());
            Ok(())
        }
    }
}

pub(super) fn union_cells(union: Vec<DimensionMap>) -> Vec<Vec<Cell>> {
    union
        .into_iter()
        .map(|dimension| {
            let mut cells = dimension.into_values().collect::<Vec<_>>();
            cells.sort_by(cell_order);
            cells
        })
        .collect()
}
