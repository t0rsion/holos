use std::collections::BTreeMap;

use crate::ProofBar;

pub(crate) struct DecodedExplicit {
    pub(crate) max_homology_dimension: usize,
    pub(crate) modulus: u32,
    pub(crate) labels: Vec<usize>,
    pub(crate) complex: Vec<Vec<Simplex>>,
    pub(crate) columns: Vec<Vec<ChangeColumn>>,
    pub(crate) bars: Vec<ProofBar>,
    pub(crate) change_terms: usize,
}

#[derive(Clone)]
pub(crate) struct Simplex {
    pub(crate) vertices: Vec<usize>,
    pub(crate) grade: f64,
}

pub(crate) struct ChangeColumn {
    pub(crate) terms: Vec<Term>,
}

pub(crate) struct Term {
    pub(crate) index: usize,
    pub(crate) coefficient: u32,
}

#[derive(Clone, Default)]
pub(crate) struct SparseColumn(pub(crate) BTreeMap<usize, u64>);

impl SparseColumn {
    pub(crate) fn insert(&mut self, index: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(index, coefficient);
        }
    }

    pub(crate) fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&index, &coefficient)| (index, coefficient))
    }

    pub(crate) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&index, &coefficient) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * coefficient) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }
}
