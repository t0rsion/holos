//! Exact interval decomposition of finite zigzag vector-space modules.
//!
//! A module is a type-A quiver over one declared prime field. Arrows may
//! point in either direction. The decomposition uses the generalized rank of
//! every contiguous submodule and Möbius inversion. Repeated interval
//! summands remain one space with a multiplicity.

mod algebra;
mod ranks;
use ranks::generalized_rank;
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
    /// Largest generalized-rank work count.
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
    /// Content identifier of this interval and its source module.
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

    /// Content identifier of this module.
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
mod tests;
