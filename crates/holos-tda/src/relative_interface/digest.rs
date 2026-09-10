use sha2::{Digest, Sha256};

use crate::Diagram;
use crate::certificate::ChangeColumn;

use super::model::InterfaceCell;

pub(super) fn certificate_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<InterfaceCell>],
    columns: &[Vec<ChangeColumn>],
    diagram: &Diagram,
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
    hash.update((diagram.bars.len() as u64).to_be_bytes());
    for bar in &diagram.bars {
        hash.update((bar.dim as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn source_digest(
    max_dim: usize,
    modulus: u32,
    protected: &[usize],
    cells: &[Vec<InterfaceCell>],
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

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
    }
}

pub(super) fn diagrams_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

pub(super) fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}
