use sha2::{Digest, Sha256};

use crate::{Diagram, SparseDistanceMatrix};

pub(super) fn graph_digest(input: &SparseDistanceMatrix, threshold: f64) -> [u8; 32] {
    let edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((input.len() as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for (u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn diagrams_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}
