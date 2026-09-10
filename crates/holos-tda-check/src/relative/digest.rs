use sha2::{Digest, Sha256};

use super::super::{ProofBar, ProofColumn, ProofLimits};
use super::model::{Cell, VerifiedCertificate};

impl VerifiedCertificate {
    pub(crate) fn source_digest(&self) -> [u8; 32] {
        source_digest(
            self.max_dim,
            self.modulus,
            &self.protected_vertices,
            &self.input,
        )
    }
}

pub(super) fn dimension_limit(dimension: usize, limits: ProofLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

pub(super) fn count_cells(cells: &[Vec<Cell>]) -> usize {
    cells.iter().map(Vec::len).sum()
}

pub(super) fn cell_order(left: &Cell, right: &Cell) -> std::cmp::Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
}

pub(super) fn certificate_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<Cell>],
    columns: &[Vec<ProofColumn>],
    diagram: &[ProofBar],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-relative-interface-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, protected);
    for dimension in cells {
        hash.update((dimension.len() as u64).to_be_bytes());
        for cell in dimension {
            digest_usizes(&mut hash, &cell.vertices);
            hash.update(cell.value.to_bits().to_be_bytes());
            hash.update((cell.boundary.len() as u64).to_be_bytes());
            for term in &cell.boundary {
                digest_usizes(&mut hash, &term.cell);
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    for dimension in columns {
        hash.update((dimension.len() as u64).to_be_bytes());
        for column in dimension {
            hash.update((column.terms.len() as u64).to_be_bytes());
            for term in &column.terms {
                hash.update((term.index as u64).to_be_bytes());
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    hash.update((diagram.len() as u64).to_be_bytes());
    for bar in diagram {
        hash.update((bar.dimension as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn source_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<Cell>],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-relative-interface-source-v1");
    hash.update((max_dim as u64).to_be_bytes());
    hash.update(modulus.to_be_bytes());
    digest_usizes(&mut hash, protected);
    for dimension in cells {
        hash.update((dimension.len() as u64).to_be_bytes());
        for cell in dimension {
            digest_usizes(&mut hash, &cell.vertices);
            hash.update(cell.value.to_bits().to_be_bytes());
            hash.update((cell.boundary.len() as u64).to_be_bytes());
            for term in &cell.boundary {
                digest_usizes(&mut hash, &term.cell);
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    hash.finalize().into()
}

pub(super) fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
    }
}
