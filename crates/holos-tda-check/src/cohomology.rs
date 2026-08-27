use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::{ProofError, ProofLimits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Edge {
    pub(crate) u: usize,
    pub(crate) v: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MapTerm {
    pub(crate) target: usize,
    pub(crate) coefficient: u32,
}

pub(crate) struct Space {
    simplices: Vec<Vec<usize>>,
    coboundaries: Vec<Vector>,
    basis: Vec<Vector>,
}

fn adjacency(vertex_count: usize, edges: &[Edge]) -> Vec<BTreeSet<usize>> {
    let mut adjacency = vec![BTreeSet::new(); vertex_count];
    for edge in edges {
        adjacency[edge.u].insert(edge.v);
        adjacency[edge.v].insert(edge.u);
    }
    adjacency
}

fn flag_dimensions(
    vertex_count: usize,
    dimension: usize,
    adjacency: &[BTreeSet<usize>],
    limits: ProofLimits,
) -> Result<Vec<Vec<Vec<usize>>>, ProofError> {
    let vertices = (0..vertex_count)
        .map(|vertex| vec![vertex])
        .collect::<Vec<Vec<usize>>>();
    let mut dimensions = vec![vertices];
    for current in 1..=dimension + 1 {
        let next = flag_cofacets(
            vertex_count,
            current,
            &dimensions[current - 1],
            adjacency,
            simplex_limit(current, limits),
        )?;
        dimensions.push(next);
    }
    Ok(dimensions)
}

fn flag_cofacets(
    vertex_count: usize,
    dimension: usize,
    simplices: &[Vec<usize>],
    adjacency: &[BTreeSet<usize>],
    maximum: usize,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let mut next = Vec::new();
    for simplex in simplices {
        for vertex in simplex.last().copied().unwrap_or(0) + 1..vertex_count {
            if simplex
                .iter()
                .all(|member| adjacency[*member].contains(&vertex))
            {
                let mut cofacet = simplex.clone();
                cofacet.push(vertex);
                next.push(cofacet);
                if next.len() > maximum {
                    return Err(ProofError::new(format!(
                        "dimension {dimension} zigzag simplex count exceeds {maximum}"
                    )));
                }
            }
        }
    }
    Ok(next)
}

fn simplex_limit(dimension: usize, limits: ProofLimits) -> usize {
    match dimension {
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn validate_incidence_count(
    dimensions: &[Vec<Vec<usize>>],
    dimension: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    let incidences = dimensions[dimension]
        .len()
        .checked_mul(dimension + 1)
        .and_then(|count| {
            dimensions[dimension + 1]
                .len()
                .checked_mul(dimension + 2)
                .and_then(|next| count.checked_add(next))
        })
        .ok_or_else(|| ProofError::new("zigzag incidence count overflows"))?;
    if incidences > limits.max_terms {
        Err(ProofError::new("zigzag incidence count exceeds its limit"))
    } else {
        Ok(())
    }
}

fn coboundary_space(
    dimensions: &[Vec<Vec<usize>>],
    dimension: usize,
    q_simplices: &[Vec<usize>],
    modulus: u32,
) -> Result<Vec<Vector>, ProofError> {
    let rows = if dimension == 0 {
        Vec::new()
    } else {
        image_rows(&dimensions[dimension - 1], q_simplices, u64::from(modulus))?
    };
    Ok(rref(rows, u64::from(modulus)))
}

fn quotient_basis(cocycles: Vec<Vector>, coboundaries: &[Vector], modulus: u32) -> Vec<Vector> {
    let mut quotient = Vec::new();
    for mut cocycle in cocycles {
        reduce(&mut cocycle, coboundaries, u64::from(modulus));
        if !cocycle.is_zero() {
            quotient.push(cocycle);
        }
    }
    quotient
}

impl Space {
    pub(crate) fn build(
        vertex_count: usize,
        dimension: usize,
        edges: &[Edge],
        modulus: u32,
        limits: ProofLimits,
    ) -> Result<Self, ProofError> {
        let adjacency = adjacency(vertex_count, edges);
        let dimensions = flag_dimensions(vertex_count, dimension, &adjacency, limits)?;
        validate_incidence_count(&dimensions, dimension, limits)?;
        let q_simplices = &dimensions[dimension];
        let positions: BTreeMap<_, _> = q_simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect();
        let equations = dimensions[dimension + 1]
            .iter()
            .map(|simplex| boundary_row(simplex, &positions, modulus as u64))
            .collect::<Result<Vec<_>, _>>()?;
        let cocycles = nullspace(equations, q_simplices.len(), modulus as u64);
        let coboundaries = coboundary_space(&dimensions, dimension, q_simplices, modulus)?;
        let quotient = quotient_basis(cocycles, &coboundaries, modulus);
        Ok(Self {
            simplices: q_simplices.clone(),
            coboundaries,
            basis: rref(quotient, modulus as u64),
        })
    }

    pub(crate) fn rank(&self) -> usize {
        self.basis.len()
    }

    pub(crate) fn id(
        &self,
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        edges: &[Edge],
    ) -> [u8; 32] {
        let mut graph_hash = Sha256::new();
        graph_hash.update(b"holos-active-flag-graph-v1");
        graph_hash.update((vertex_count as u64).to_be_bytes());
        graph_hash.update(scale.to_bits().to_be_bytes());
        for edge in edges {
            graph_hash.update((edge.u as u64).to_be_bytes());
            graph_hash.update((edge.v as u64).to_be_bytes());
        }
        let graph_digest: [u8; 32] = graph_hash.finalize().into();
        let mut hash = Sha256::new();
        hash.update(b"holos-cohomology-space-v1");
        hash.update((vertex_count as u64).to_be_bytes());
        hash.update((dimension as u64).to_be_bytes());
        hash.update(scale.to_bits().to_be_bytes());
        hash.update(modulus.to_be_bytes());
        hash.update(graph_digest);
        hash.update((self.basis.len() as u64).to_be_bytes());
        for vector in &self.basis {
            hash.update((vector.0.len() as u64).to_be_bytes());
            for (&position, &coefficient) in &vector.0 {
                hash.update((self.simplices[position].len() as u64).to_be_bytes());
                for vertex in &self.simplices[position] {
                    hash.update((*vertex as u64).to_be_bytes());
                }
                hash.update(coefficient.to_be_bytes());
            }
        }
        hash.finalize().into()
    }

    pub(crate) fn canonical_subspace(
        &self,
        rows: &[Vec<MapTerm>],
        modulus: u32,
    ) -> Result<Vec<Vec<MapTerm>>, ProofError> {
        let vectors = rows
            .iter()
            .map(|row| {
                if row.windows(2).any(|pair| pair[0].target >= pair[1].target)
                    || row.iter().any(|term| {
                        term.target >= self.rank()
                            || term.coefficient == 0
                            || term.coefficient >= modulus
                    })
                {
                    return Err(ProofError::new(
                        "synthesis target coordinates are not canonical",
                    ));
                }
                let mut vector = Vector::default();
                for term in row {
                    vector.insert(term.target, term.coefficient);
                }
                Ok(vector)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rref(vectors, modulus as u64)
            .into_iter()
            .map(|row| {
                row.0
                    .into_iter()
                    .map(|(target, coefficient)| MapTerm {
                        target,
                        coefficient,
                    })
                    .collect()
            })
            .collect())
    }

    pub(crate) fn subspace_intersection_rank_from(
        &self,
        source: &Self,
        subspace: &[Vec<MapTerm>],
        modulus: u32,
    ) -> Result<usize, ProofError> {
        let (columns, _) = source.restriction_to(self, modulus)?;
        let image = columns
            .into_iter()
            .map(|column| {
                let mut row = Vector::default();
                for term in column {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let target = subspace
            .iter()
            .map(|terms| {
                let mut row = Vector::default();
                for term in terms {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect::<Vec<_>>();
        let modulus = modulus as u64;
        let image_rank = rref(image.clone(), modulus).len();
        let target_rank = target.len();
        let union_rank = rref(image.into_iter().chain(target).collect(), modulus).len();
        image_rank
            .checked_add(target_rank)
            .and_then(|sum| sum.checked_sub(union_rank))
            .ok_or_else(|| ProofError::new("synthesis intersection rank is invalid"))
    }

    pub(crate) fn restriction_to(
        &self,
        target: &Self,
        modulus: u32,
    ) -> Result<(Vec<Vec<MapTerm>>, usize), ProofError> {
        let positions: BTreeMap<_, _> = target
            .simplices
            .iter()
            .cloned()
            .enumerate()
            .map(|(position, simplex)| (simplex, position))
            .collect();
        let mut coordinate_rows = Vec::with_capacity(self.basis.len());
        let mut columns = Vec::with_capacity(self.basis.len());
        for vector in &self.basis {
            let mut restricted = Vector::default();
            for (&position, &coefficient) in &vector.0 {
                if let Some(&target_position) = positions.get(&self.simplices[position]) {
                    restricted.insert(target_position, coefficient);
                }
            }
            reduce(&mut restricted, &target.coboundaries, modulus as u64);
            let coordinates = coordinates(&restricted, &target.basis, modulus as u64)?;
            columns.push(
                coordinates
                    .0
                    .iter()
                    .map(|(&target, &coefficient)| MapTerm {
                        target,
                        coefficient,
                    })
                    .collect(),
            );
            coordinate_rows.push(coordinates);
        }
        let rank = rref(coordinate_rows, modulus as u64).len();
        Ok((columns, rank))
    }

    pub(crate) fn target_in_image_from(
        &self,
        source: &Self,
        target_basis: usize,
        modulus: u32,
    ) -> Result<bool, ProofError> {
        if target_basis >= self.basis.len() {
            return Err(ProofError::new(
                "cohomology intervention target basis is out of range",
            ));
        }
        let (columns, _) = source.restriction_to(self, modulus)?;
        let rows = columns
            .into_iter()
            .map(|column| {
                let mut row = Vector::default();
                for term in column {
                    row.insert(term.target, term.coefficient);
                }
                row
            })
            .collect();
        let mut target = Vector::default();
        target.insert(target_basis, 1);
        reduce(&mut target, &rref(rows, modulus as u64), modulus as u64);
        Ok(target.is_zero())
    }
}

fn coordinates(vector: &Vector, basis: &[Vector], modulus: u64) -> Result<Vector, ProofError> {
    let mut residual = vector.clone();
    let mut output = Vector::default();
    for (position, basis_vector) in basis.iter().enumerate() {
        let pivot = basis_vector.leading().unwrap().0;
        if let Some(&coefficient) = residual.0.get(&pivot) {
            output.insert(position, coefficient);
            residual.add_scaled(basis_vector, modulus - u64::from(coefficient), modulus);
        }
    }
    if !residual.is_zero() {
        return Err(ProofError::new(
            "zigzag restriction is outside the target cohomology basis",
        ));
    }
    Ok(output)
}

fn boundary_row(
    simplex: &[usize],
    positions: &BTreeMap<Vec<usize>, usize>,
    modulus: u64,
) -> Result<Vector, ProofError> {
    let mut row = Vector::default();
    for removed in 0..simplex.len() {
        let mut face = simplex.to_vec();
        face.remove(removed);
        let position = positions
            .get(&face)
            .copied()
            .ok_or_else(|| ProofError::new("zigzag coboundary omits a face"))?;
        row.insert(position, sign(removed, modulus));
    }
    Ok(row)
}

fn image_rows(
    faces: &[Vec<usize>],
    simplices: &[Vec<usize>],
    modulus: u64,
) -> Result<Vec<Vector>, ProofError> {
    let positions: BTreeMap<_, _> = faces
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    let mut rows = vec![Vector::default(); faces.len()];
    for (simplex_position, simplex) in simplices.iter().enumerate() {
        for removed in 0..simplex.len() {
            let mut face = simplex.clone();
            face.remove(removed);
            let position = positions
                .get(&face)
                .copied()
                .ok_or_else(|| ProofError::new("zigzag boundary omits a face"))?;
            rows[position].insert(simplex_position, sign(removed, modulus));
        }
    }
    Ok(rows)
}

fn sign(position: usize, modulus: u64) -> u32 {
    if position % 2 == 0 {
        1
    } else {
        (modulus - 1) as u32
    }
}

#[derive(Clone, Default)]
struct Vector(BTreeMap<usize, u32>);

impl Vector {
    fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    fn leading(&self) -> Option<(usize, u32)> {
        self.0.first_key_value().map(|(&key, &value)| (key, value))
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
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

    fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
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
            if let Some(&coefficient) = existing.0.get(&pivot) {
                existing.add_scaled(&row, modulus - u64::from(coefficient), modulus);
            }
        }
        basis.push(row);
        basis.sort_by_key(|row| row.leading().map(|(position, _)| position));
    }
    basis
}

fn reduce(row: &mut Vector, basis: &[Vector], modulus: u64) {
    for existing in basis {
        let pivot = existing.leading().unwrap().0;
        if let Some(&coefficient) = row.0.get(&pivot) {
            row.add_scaled(existing, modulus - u64::from(coefficient), modulus);
        }
    }
}

fn nullspace(equations: Vec<Vector>, variables: usize, modulus: u64) -> Vec<Vector> {
    let equations = rref(equations, modulus);
    let pivots: BTreeSet<_> = equations
        .iter()
        .map(|row| row.leading().unwrap().0)
        .collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|position| !pivots.contains(position)) {
        let mut vector = Vector::default();
        vector.insert(free, 1);
        for equation in &equations {
            if let Some(&coefficient) = equation.0.get(&free) {
                vector.insert(
                    equation.leading().unwrap().0,
                    (modulus - u64::from(coefficient)) as u32,
                );
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
