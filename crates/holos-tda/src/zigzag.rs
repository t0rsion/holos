//! Exact interval decomposition of finite zigzag vector-space modules.
//!
//! A module is a type-A quiver over one declared prime field. Arrows may
//! point in either direction. The decomposition uses the generalized rank of
//! every contiguous submodule and Möbius inversion. Repeated interval
//! summands remain one space with a multiplicity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result};

/// Resource limits for exact zigzag decomposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ZigzagLimits {
    /// Largest accepted node count.
    pub max_nodes: usize,
    /// Largest sum of node dimensions.
    pub max_total_dimension: usize,
    /// Largest total nonzero map coefficient count.
    pub max_map_terms: usize,
    /// Largest charged generalized-rank work count.
    pub max_rank_work: usize,
}

impl Default for ZigzagLimits {
    fn default() -> Self {
        Self {
            max_nodes: 2_049,
            max_total_dimension: 100_000,
            max_map_terms: 20_000_000,
            max_rank_work: 100_000_000,
        }
    }
}

/// Direction of one arrow between adjacent module nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZigzagDirection {
    /// The map goes from the left node to the right node.
    Forward,
    /// The map goes from the right node to the left node.
    Backward,
}

/// One nonzero coefficient in a zigzag map column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZigzagTerm {
    /// Target basis position.
    pub target: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// One linear map between adjacent zigzag nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZigzagMap {
    direction: ZigzagDirection,
    columns: Vec<Vec<ZigzagTerm>>,
}

impl ZigzagMap {
    /// Construct a map from columns in source basis order.
    pub fn new(direction: ZigzagDirection, columns: Vec<Vec<ZigzagTerm>>) -> Self {
        Self { direction, columns }
    }

    /// Arrow direction.
    pub fn direction(&self) -> ZigzagDirection {
        self.direction
    }

    /// Map columns in source basis order.
    pub fn columns(&self) -> &[Vec<ZigzagTerm>] {
        &self.columns
    }
}

/// Content identifier of one finite zigzag module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZigzagModuleId([u8; 32]);

impl ZigzagModuleId {
    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ZigzagModuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

/// Content identifier of an interval-isotypic class space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZigzagIntervalId([u8; 32]);

impl ZigzagIntervalId {
    /// Raw identifier bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ZigzagIntervalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

/// One interval-isotypic space in a zigzag decomposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZigzagInterval {
    /// Content identifier of this interval and its complete source module.
    pub id: ZigzagIntervalId,
    /// First node covered by the interval.
    pub start: usize,
    /// Last node covered by the interval, inclusive.
    pub end: usize,
    /// Number of indistinguishable copies of this interval summand.
    pub multiplicity: usize,
}

/// Exact interval decomposition and generalized ranks of a finite zigzag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZigzagBarcode {
    /// Content identifier of the checked module.
    pub module: ZigzagModuleId,
    /// Node dimensions in zigzag order.
    pub dimensions: Vec<usize>,
    /// Generalized rank for every `[start, end]`, stored row-major.
    pub generalized_ranks: Vec<usize>,
    /// Nonzero interval multiplicities in lexicographic endpoint order.
    pub intervals: Vec<ZigzagInterval>,
}

impl ZigzagBarcode {
    /// Generalized rank on the inclusive subinterval `[start, end]`.
    pub fn rank(&self, start: usize, end: usize) -> Option<usize> {
        (start <= end && end < self.dimensions.len())
            .then(|| self.generalized_ranks[start * self.dimensions.len() + end])
    }
}

/// A checked finite zigzag vector-space module over a prime field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZigzagModule {
    id: ZigzagModuleId,
    modulus: u32,
    dimensions: Vec<usize>,
    maps: Vec<ZigzagMap>,
    limits: ZigzagLimits,
}

impl ZigzagModule {
    /// Construct and validate one finite zigzag module.
    pub fn new(
        modulus: u32,
        dimensions: Vec<usize>,
        maps: Vec<ZigzagMap>,
        limits: ZigzagLimits,
    ) -> Result<Self> {
        validate_module_shape(modulus, &dimensions, &maps, limits)?;
        let total_dimension = total_dimension(&dimensions)?;
        if total_dimension > limits.max_total_dimension {
            return Err(Error::InvalidInput(format!(
                "zigzag total dimension exceeds the limit {}",
                limits.max_total_dimension
            )));
        }
        let mut terms = 0usize;
        for (position, map) in maps.iter().enumerate() {
            terms = terms
                .checked_add(validate_map(position, map, &dimensions, modulus)?)
                .ok_or_else(|| Error::InvalidInput("zigzag map term count overflows".into()))?;
        }
        if terms > limits.max_map_terms {
            return Err(Error::InvalidInput(format!(
                "zigzag map term count exceeds the limit {}",
                limits.max_map_terms
            )));
        }
        let id = module_id(modulus, &dimensions, &maps);
        Ok(Self {
            id,
            modulus,
            dimensions,
            maps,
            limits,
        })
    }

    /// Content identifier of this complete module.
    pub fn id(&self) -> ZigzagModuleId {
        self.id
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Node dimensions in zigzag order.
    pub fn dimensions(&self) -> &[usize] {
        &self.dimensions
    }

    /// Adjacent maps in zigzag order.
    pub fn maps(&self) -> &[ZigzagMap] {
        &self.maps
    }

    /// Decompose this type-A representation into interval summands.
    pub fn decompose(&self) -> Result<ZigzagBarcode> {
        let generalized_ranks = self.compute_generalized_ranks()?;
        let intervals =
            interval_multiplicities(self.id, &generalized_ranks, self.dimensions.len())?;
        Ok(ZigzagBarcode {
            module: self.id,
            dimensions: self.dimensions.clone(),
            generalized_ranks,
            intervals,
        })
    }

    fn compute_generalized_ranks(&self) -> Result<Vec<usize>> {
        let count = self.dimensions.len();
        let mut ranks = vec![0usize; count * count];
        let mut work = 0usize;
        for start in (0..count).rev() {
            for end in start..count {
                work = work
                    .checked_add(self.rank_work(start, end)?)
                    .ok_or_else(rank_work_overflow)?;
                if work > self.limits.max_rank_work {
                    return Err(Error::InvalidInput(format!(
                        "zigzag generalized-rank work exceeds the limit {}",
                        self.limits.max_rank_work
                    )));
                }
                ranks[start * count + end] = generalized_rank(self, start, end)?;
            }
        }
        Ok(ranks)
    }

    fn rank_work(&self, start: usize, end: usize) -> Result<usize> {
        let ambient = self.dimensions[start..=end].iter().sum::<usize>();
        let arrows = (start..end).try_fold(0usize, |sum, position| {
            let (source, target) =
                map_shape(&self.dimensions, position, self.maps[position].direction);
            sum.checked_add(source)
                .and_then(|value| value.checked_add(target))
                .ok_or_else(rank_work_overflow)
        })?;
        ambient.checked_add(arrows).ok_or_else(rank_work_overflow)
    }
}

fn validate_module_shape(
    modulus: u32,
    dimensions: &[usize],
    maps: &[ZigzagMap],
    limits: ZigzagLimits,
) -> Result<()> {
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(
            "zigzag modulus must be a supported prime".into(),
        ));
    }
    if dimensions.is_empty() || dimensions.len() > limits.max_nodes {
        return Err(Error::InvalidInput(format!(
            "zigzag node count must be in 1..={}",
            limits.max_nodes
        )));
    }
    if maps.len() + 1 != dimensions.len() {
        return Err(Error::InvalidInput(
            "zigzag requires one map between each adjacent node".into(),
        ));
    }
    Ok(())
}

fn total_dimension(dimensions: &[usize]) -> Result<usize> {
    dimensions.iter().try_fold(0usize, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| Error::InvalidInput("zigzag total dimension overflows".into()))
    })
}

fn validate_map(
    position: usize,
    map: &ZigzagMap,
    dimensions: &[usize],
    modulus: u32,
) -> Result<usize> {
    let (source, target) = map_shape(dimensions, position, map.direction);
    if map.columns.len() != source {
        return Err(Error::InvalidInput(format!(
            "zigzag map {position} has {} columns but its source dimension is {source}",
            map.columns.len()
        )));
    }
    let mut terms = 0usize;
    for column in &map.columns {
        validate_column(position, column, target, modulus)?;
        terms = terms
            .checked_add(column.len())
            .ok_or_else(|| Error::InvalidInput("zigzag map term count overflows".into()))?;
    }
    Ok(terms)
}

fn validate_column(
    position: usize,
    column: &[ZigzagTerm],
    target: usize,
    modulus: u32,
) -> Result<()> {
    let mut previous = None;
    for term in column {
        if term.target >= target
            || term.coefficient == 0
            || term.coefficient >= modulus
            || previous.is_some_and(|value| value >= term.target)
        {
            return Err(Error::InvalidInput(format!(
                "zigzag map {position} has a noncanonical term"
            )));
        }
        previous = Some(term.target);
    }
    Ok(())
}

fn rank_work_overflow() -> Error {
    Error::InvalidInput("zigzag rank work overflows".into())
}

fn interval_multiplicities(
    module: ZigzagModuleId,
    ranks: &[usize],
    count: usize,
) -> Result<Vec<ZigzagInterval>> {
    let mut intervals = Vec::new();
    for start in 0..count {
        for end in start..count {
            if let Some(interval) = interval_multiplicity(module, ranks, count, start, end)? {
                intervals.push(interval);
            }
        }
    }
    Ok(intervals)
}

fn interval_multiplicity(
    module: ZigzagModuleId,
    ranks: &[usize],
    count: usize,
    start: usize,
    end: usize,
) -> Result<Option<ZigzagInterval>> {
    let rank = |left: usize, right: usize| -> i128 { ranks[left * count + right] as i128 };
    let mut multiplicity = rank(start, end);
    if start > 0 {
        multiplicity -= rank(start - 1, end);
    }
    if end + 1 < count {
        multiplicity -= rank(start, end + 1);
    }
    if start > 0 && end + 1 < count {
        multiplicity += rank(start - 1, end + 1);
    }
    if multiplicity < 0 {
        return Err(Error::InvalidInput(
            "zigzag generalized ranks violate interval decomposability".into(),
        ));
    }
    if multiplicity == 0 {
        return Ok(None);
    }
    let multiplicity = usize::try_from(multiplicity)
        .map_err(|_| Error::InvalidInput("zigzag interval multiplicity overflows".into()))?;
    Ok(Some(ZigzagInterval {
        id: interval_id(module, start, end),
        start,
        end,
        multiplicity,
    }))
}

fn map_shape(dimensions: &[usize], position: usize, direction: ZigzagDirection) -> (usize, usize) {
    match direction {
        ZigzagDirection::Forward => (dimensions[position], dimensions[position + 1]),
        ZigzagDirection::Backward => (dimensions[position + 1], dimensions[position]),
    }
}

fn generalized_rank(module: &ZigzagModule, start: usize, end: usize) -> Result<usize> {
    if start == end {
        return Ok(module.dimensions[start]);
    }
    let modulus = module.modulus as u64;
    let offsets = node_offsets(&module.dimensions, start, end);
    let ambient = offsets.last().copied().unwrap_or(0) + module.dimensions[end];
    let (mut relations, equations) = compatibility_system(module, start, end, &offsets);
    let relation_rank = rank(relations.clone(), modulus);
    let limit = nullspace(equations, ambient, modulus);
    relations.extend(first_node_images(limit, module.dimensions[start]));
    Ok(rank(relations, modulus) - relation_rank)
}

fn node_offsets(dimensions: &[usize], start: usize, end: usize) -> Vec<usize> {
    let mut offsets = vec![0usize; end - start + 1];
    for position in 1..offsets.len() {
        offsets[position] = offsets[position - 1] + dimensions[start + position - 1];
    }
    offsets
}

fn compatibility_system(
    module: &ZigzagModule,
    start: usize,
    end: usize,
    offsets: &[usize],
) -> (Vec<Vector>, Vec<Vector>) {
    let mut relations = Vec::new();
    let mut equations = Vec::new();
    for position in start..end {
        let map = &module.maps[position];
        let left_offset = offsets[position - start];
        let right_offset = offsets[position + 1 - start];
        let shape = oriented_offsets(module, position, left_offset, right_offset);
        relations.extend(map_relations(map, shape.0, shape.1, module.modulus));
        equations.extend(map_equations(
            map,
            shape.0,
            shape.1,
            shape.2,
            module.modulus,
        ));
    }
    (relations, equations)
}

fn oriented_offsets(
    module: &ZigzagModule,
    position: usize,
    left_offset: usize,
    right_offset: usize,
) -> (usize, usize, usize) {
    match module.maps[position].direction {
        ZigzagDirection::Forward => (left_offset, right_offset, module.dimensions[position + 1]),
        ZigzagDirection::Backward => (right_offset, left_offset, module.dimensions[position]),
    }
}

fn map_relations(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    modulus: u32,
) -> Vec<Vector> {
    map.columns
        .iter()
        .enumerate()
        .map(|(column, terms)| {
            let mut relation = Vector::default();
            relation.insert(source_offset + column, 1);
            for term in terms {
                relation.insert(
                    target_offset + term.target,
                    (u64::from(modulus) - u64::from(term.coefficient)) as u32,
                );
            }
            relation
        })
        .collect()
}

fn map_equations(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    target_dimension: usize,
    modulus: u32,
) -> Vec<Vector> {
    (0..target_dimension)
        .map(|target| map_equation(map, source_offset, target_offset, target, modulus))
        .collect()
}

fn map_equation(
    map: &ZigzagMap,
    source_offset: usize,
    target_offset: usize,
    target: usize,
    modulus: u32,
) -> Vector {
    let mut equation = Vector::default();
    equation.insert(target_offset + target, 1);
    for (column, terms) in map.columns.iter().enumerate() {
        if let Ok(term_position) = terms.binary_search_by_key(&target, |term| term.target) {
            let coefficient = terms[term_position].coefficient;
            equation.insert(
                source_offset + column,
                (u64::from(modulus) - u64::from(coefficient)) as u32,
            );
        }
    }
    equation
}

fn first_node_images(limit: Vec<Vector>, first_dimension: usize) -> impl Iterator<Item = Vector> {
    limit.into_iter().filter_map(move |section| {
        let mut image = Vector::default();
        for (&position, &coefficient) in section.0.range(..first_dimension) {
            image.insert(position, coefficient);
        }
        (!image.is_zero()).then_some(image)
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Vector(BTreeMap<usize, u32>);

impl Vector {
    fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    fn leading(&self) -> Option<(usize, u32)> {
        self.0
            .first_key_value()
            .map(|(&position, &coefficient)| (position, coefficient))
    }

    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
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

    fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
}

fn rank(rows: Vec<Vector>, modulus: u64) -> usize {
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

fn nullspace(equations: Vec<Vector>, variables: usize, modulus: u64) -> Vec<Vector> {
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

fn module_id(modulus: u32, dimensions: &[usize], maps: &[ZigzagMap]) -> ZigzagModuleId {
    let mut hash = Sha256::new();
    hash.update(b"holos-zigzag-module-v1");
    hash.update(modulus.to_be_bytes());
    hash.update((dimensions.len() as u64).to_be_bytes());
    for dimension in dimensions {
        hash.update((*dimension as u64).to_be_bytes());
    }
    for map in maps {
        hash.update([match map.direction {
            ZigzagDirection::Forward => 1,
            ZigzagDirection::Backward => 2,
        }]);
        hash.update((map.columns.len() as u64).to_be_bytes());
        for column in &map.columns {
            hash.update((column.len() as u64).to_be_bytes());
            for term in column {
                hash.update((term.target as u64).to_be_bytes());
                hash.update(term.coefficient.to_be_bytes());
            }
        }
    }
    ZigzagModuleId(hash.finalize().into())
}

fn interval_id(module: ZigzagModuleId, start: usize, end: usize) -> ZigzagIntervalId {
    let mut hash = Sha256::new();
    hash.update(b"holos-zigzag-interval-v1");
    hash.update(module.as_bytes());
    hash.update((start as u64).to_be_bytes());
    hash.update((end as u64).to_be_bytes());
    ZigzagIntervalId(hash.finalize().into())
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(terms: &[(usize, u32)]) -> Vec<ZigzagTerm> {
        terms
            .iter()
            .map(|&(target, coefficient)| ZigzagTerm {
                target,
                coefficient,
            })
            .collect()
    }

    #[test]
    fn identity_chain_is_one_complete_interval() {
        let module = ZigzagModule::new(
            3,
            vec![1, 1, 1],
            vec![
                ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)])]),
                ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)])]),
            ],
            ZigzagLimits::default(),
        )
        .unwrap();
        let barcode = module.decompose().unwrap();
        assert_eq!(barcode.rank(0, 2), Some(1));
        assert_eq!(barcode.intervals.len(), 1);
        assert_eq!(barcode.intervals[0].start, 0);
        assert_eq!(barcode.intervals[0].end, 2);
        assert_eq!(barcode.intervals[0].multiplicity, 1);
    }

    #[test]
    fn zero_map_splits_two_point_intervals() {
        let module = ZigzagModule::new(
            5,
            vec![1, 1],
            vec![ZigzagMap::new(ZigzagDirection::Forward, vec![vec![]])],
            ZigzagLimits::default(),
        )
        .unwrap();
        let intervals = module.decompose().unwrap().intervals;
        assert_eq!(
            intervals
                .iter()
                .map(|interval| (interval.start, interval.end, interval.multiplicity))
                .collect::<Vec<_>>(),
            vec![(0, 0, 1), (1, 1, 1)]
        );
    }

    #[test]
    fn fork_distinguishes_shared_and_independent_event_classes() {
        let shared = ZigzagModule::new(
            3,
            vec![1, 2, 1],
            vec![
                ZigzagMap::new(ZigzagDirection::Backward, vec![column(&[(0, 1)]), vec![]]),
                ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)]), vec![]]),
            ],
            ZigzagLimits::default(),
        )
        .unwrap()
        .decompose()
        .unwrap();
        assert!(
            shared
                .intervals
                .iter()
                .any(|item| item.start == 0 && item.end == 2)
        );

        let independent = ZigzagModule::new(
            3,
            vec![1, 2, 1],
            vec![
                ZigzagMap::new(ZigzagDirection::Backward, vec![column(&[(0, 1)]), vec![]]),
                ZigzagMap::new(ZigzagDirection::Forward, vec![vec![], column(&[(0, 1)])]),
            ],
            ZigzagLimits::default(),
        )
        .unwrap()
        .decompose()
        .unwrap();
        assert_eq!(independent.rank(0, 2), Some(0));
        assert_eq!(
            independent
                .intervals
                .iter()
                .map(|item| (item.start, item.end, item.multiplicity))
                .collect::<Vec<_>>(),
            vec![(0, 1, 1), (1, 2, 1)]
        );
    }

    #[test]
    fn every_orientation_recovers_direct_sum_multiplicities() {
        let expected = vec![(0, 2, 2), (0, 4, 1), (1, 3, 1), (2, 2, 1), (4, 4, 2)];
        let copies = expected
            .iter()
            .flat_map(|&(start, end, multiplicity)| std::iter::repeat_n((start, end), multiplicity))
            .collect::<Vec<_>>();
        let bases = (0..5)
            .map(|node| {
                copies
                    .iter()
                    .enumerate()
                    .filter_map(|(copy, &(start, end))| {
                        (start <= node && node <= end).then_some(copy)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let dimensions = bases.iter().map(Vec::len).collect::<Vec<_>>();

        for modulus in [2, 3, 5] {
            for directions in 0..16 {
                let maps = (0..4)
                    .map(|position| {
                        let direction = if directions & (1 << position) == 0 {
                            ZigzagDirection::Forward
                        } else {
                            ZigzagDirection::Backward
                        };
                        let (source, target) = match direction {
                            ZigzagDirection::Forward => (&bases[position], &bases[position + 1]),
                            ZigzagDirection::Backward => (&bases[position + 1], &bases[position]),
                        };
                        let columns = source
                            .iter()
                            .map(|copy| {
                                target
                                    .iter()
                                    .position(|candidate| candidate == copy)
                                    .map_or_else(Vec::new, |target| column(&[(target, 1)]))
                            })
                            .collect();
                        ZigzagMap::new(direction, columns)
                    })
                    .collect();
                let actual =
                    ZigzagModule::new(modulus, dimensions.clone(), maps, ZigzagLimits::default())
                        .unwrap()
                        .decompose()
                        .unwrap()
                        .intervals
                        .iter()
                        .map(|interval| (interval.start, interval.end, interval.multiplicity))
                        .collect::<Vec<_>>();
                assert_eq!(actual, expected, "modulus {modulus}, mask {directions}");
            }
        }
    }
}
