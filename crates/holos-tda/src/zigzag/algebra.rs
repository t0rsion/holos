use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Vector(pub(super) BTreeMap<usize, u32>);

impl Vector {
    pub(super) fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    pub(super) fn leading(&self) -> Option<(usize, u32)> {
        self.0
            .first_key_value()
            .map(|(&position, &coefficient)| (position, coefficient))
    }

    pub(super) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&position, &coefficient) in &source.0 {
            let current = u64::from(self.0.get(&position).copied().unwrap_or(0));
            let next = (current + factor * u64::from(coefficient)) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next as u32);
            }
        }
    }

    pub(super) fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
}

pub(super) fn rank(rows: Vec<Vector>, modulus: u64) -> usize {
    rref(rows, modulus).len()
}

fn rref(rows: Vec<Vector>, modulus: u64) -> Vec<Vector> {
    let mut basis: Vec<Vector> = Vec::new();
    for mut row in rows {
        reduce(&mut row, &basis, modulus);
        let Some((pivot, coefficient)) = row.leading() else {
            continue;
        };
        row.scale(inverse_mod(u64::from(coefficient), modulus), modulus);
        for existing in &mut basis {
            if let Some(&value) = existing.0.get(&pivot) {
                existing.add_scaled(&row, modulus - u64::from(value), modulus);
            }
        }
        basis.push(row);
        basis.sort_by_key(|vector| vector.leading().map(|(position, _)| position));
    }
    basis
}

fn reduce(row: &mut Vector, basis: &[Vector], modulus: u64) {
    for vector in basis {
        let pivot = vector
            .leading()
            .expect("a reduced basis does not contain zero")
            .0;
        if let Some(&coefficient) = row.0.get(&pivot) {
            row.add_scaled(vector, modulus - u64::from(coefficient), modulus);
        }
    }
}

pub(super) fn nullspace(equations: Vec<Vector>, variables: usize, modulus: u64) -> Vec<Vector> {
    let equations = rref(equations, modulus);
    let pivots: BTreeSet<_> = equations
        .iter()
        .filter_map(|row| row.leading().map(|(position, _)| position))
        .collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|position| !pivots.contains(position)) {
        let mut vector = Vector::default();
        vector.insert(free, 1);
        for equation in &equations {
            let pivot = equation
                .leading()
                .expect("a reduced equation does not contain zero")
                .0;
            if let Some(&coefficient) = equation.0.get(&free) {
                vector.insert(pivot, (modulus - u64::from(coefficient)) as u32);
            }
        }
        basis.push(vector);
    }
    basis
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
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
