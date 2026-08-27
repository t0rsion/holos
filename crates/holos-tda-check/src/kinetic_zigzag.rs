use std::collections::{BTreeMap, BTreeSet};

use num_rational::BigRational;
use sha2::{Digest, Sha256};

use crate::cohomology::{Edge, MapTerm, Space};
use crate::{MODULUS_LIMIT, ProofError, ProofLimits, Reader, is_prime};

const MAGIC: &[u8; 8] = b"HOLOSZZ\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_NODES: usize = 2_049;
const FORMAT_MAX_RANK_WORK: usize = 100_000_000;

/// Result of independently replaying one kinetic zigzag artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedKineticZigzag {
    /// Target cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Affine edge trajectory count.
    pub edges: usize,
    /// Alternating open-cell and exact-event node count.
    pub nodes: usize,
    /// Exact restriction arrow count.
    pub arrows: usize,
    /// Nonzero interval-isotypic space count.
    pub intervals: usize,
    /// Sum of all interval multiplicities.
    pub interval_copies: usize,
}

/// Whether bytes start with the kinetic zigzag magic.
pub fn is_kinetic_zigzag(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify a `HOLOSZZ` artifact without linking to the producer crate.
///
/// The checker reconstructs every exact event complex, canonical cohomology
/// basis, restriction map, generalized rank, and interval multiplicity.
pub fn verify_kinetic_zigzag(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedKineticZigzag, ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "kinetic zigzag exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-kinetic-zigzag-v1");
    hash.update(&bytes[..payload_len]);
    let expected: [u8; 32] = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "kinetic zigzag digest differs from its content",
        ));
    }
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported kinetic zigzag artifact"));
    }
    let vertex_count = reader.bounded_usize("zigzag vertex count", limits.max_vertices)?;
    let edge_count = reader.bounded_usize("zigzag edge count", limits.max_edges)?;
    if edge_count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "kinetic zigzag edge count exceeds the remaining bytes",
        ));
    }
    let mut trajectories = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        trajectories.push(AffineEdge {
            edge: Edge {
                u: reader.usize()?,
                v: reader.usize()?,
            },
            intercept: f64::from_bits(reader.u64()?),
            velocity: f64::from_bits(reader.u64()?),
        });
    }
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let dimension = reader.bounded_usize("zigzag dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    let persistent_ties = reader.usize()?;
    validate_input(vertex_count, &trajectories, start, end, scale, modulus)?;
    let node_ranks = read_usizes(&mut reader, "zigzag node rank", limits.max_snapshots)?;
    if node_ranks.is_empty() || node_ranks.len() > FORMAT_MAX_NODES {
        return Err(ProofError::new(
            "kinetic zigzag node count exceeds the format limit",
        ));
    }
    let node_edges = read_usizes(&mut reader, "zigzag node edge count", limits.max_snapshots)?;
    let arrow_ranks = read_usizes(&mut reader, "zigzag arrow rank", limits.max_references)?;
    let maximum_ranks = node_ranks
        .len()
        .checked_mul(node_ranks.len())
        .ok_or_else(|| ProofError::new("kinetic zigzag rank count overflows"))?;
    let generalized_ranks = read_usizes(&mut reader, "zigzag generalized rank", maximum_ranks)?;
    let maximum_intervals = node_ranks
        .len()
        .checked_mul(node_ranks.len().saturating_add(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag interval count overflows"))?;
    let interval_count = reader.bounded_usize("zigzag interval count", maximum_intervals)?;
    let mut intervals = Vec::with_capacity(interval_count);
    for _ in 0..interval_count {
        intervals.push((reader.usize()?, reader.usize()?, reader.usize()?));
    }
    if reader.array32()? != expected || reader.remaining() != 0 {
        return Err(ProofError::new(
            "kinetic zigzag has a wrong digest or trailing bytes",
        ));
    }

    let schedule = exact_schedule(&trajectories, start, end, scale, limits)?;
    if schedule.persistent_ties != persistent_ties {
        return Err(ProofError::new(
            "kinetic zigzag persistent-tie count is wrong",
        ));
    }
    let graphs = event_graphs(&trajectories, start, end, scale, &schedule.events);
    if graphs.len() != node_ranks.len()
        || node_edges.len() != graphs.len()
        || arrow_ranks.len() + 1 != graphs.len()
        || generalized_ranks.len() != maximum_ranks
    {
        return Err(ProofError::new("kinetic zigzag claim shape is wrong"));
    }
    let spaces = graphs
        .iter()
        .map(|graph| Space::build(vertex_count, dimension, graph, modulus, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let checked_node_ranks = spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let checked_node_edges = graphs.iter().map(Vec::len).collect::<Vec<_>>();
    if node_ranks != checked_node_ranks || node_edges != checked_node_edges {
        return Err(ProofError::new(
            "kinetic zigzag node ranks or active-edge counts are wrong",
        ));
    }
    let mut maps = Vec::with_capacity(arrow_ranks.len());
    let mut checked_arrow_ranks = Vec::with_capacity(arrow_ranks.len());
    for event in 0..schedule.events.len() {
        let left = 2 * event;
        let middle = left + 1;
        let right = left + 2;
        let (left_columns, left_rank) = spaces[middle].restriction_to(&spaces[left], modulus)?;
        maps.push(Map {
            forward: false,
            columns: left_columns,
        });
        checked_arrow_ranks.push(left_rank);
        let (right_columns, right_rank) = spaces[middle].restriction_to(&spaces[right], modulus)?;
        maps.push(Map {
            forward: true,
            columns: right_columns,
        });
        checked_arrow_ranks.push(right_rank);
    }
    if arrow_ranks != checked_arrow_ranks {
        return Err(ProofError::new(
            "kinetic zigzag restriction ranks are wrong",
        ));
    }
    let (checked_ranks, checked_intervals) = decompose(&checked_node_ranks, &maps, modulus)?;
    if generalized_ranks != checked_ranks || intervals != checked_intervals {
        return Err(ProofError::new(
            "kinetic zigzag interval decomposition is wrong",
        ));
    }
    Ok(VerifiedKineticZigzag {
        dimension,
        modulus,
        edges: trajectories.len(),
        nodes: spaces.len(),
        arrows: maps.len(),
        intervals: intervals.len(),
        interval_copies: intervals.iter().map(|item| item.2).sum(),
    })
}

#[derive(Clone)]
struct AffineEdge {
    edge: Edge,
    intercept: f64,
    velocity: f64,
}

fn validate_input(
    vertex_count: usize,
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    modulus: u32,
) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end || !scale.is_finite() || scale < 0.0 {
        return Err(ProofError::new("kinetic zigzag interval is invalid"));
    }
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "kinetic zigzag modulus is not a supported prime",
        ));
    }
    let mut previous = None;
    for trajectory in edges {
        if trajectory.edge.u >= trajectory.edge.v
            || trajectory.edge.v >= vertex_count
            || !trajectory.intercept.is_finite()
            || !trajectory.velocity.is_finite()
            || previous.is_some_and(|edge| edge >= trajectory.edge)
        {
            return Err(ProofError::new(
                "kinetic zigzag edge trajectory is not canonical",
            ));
        }
        for time in [start, end] {
            let weight = trajectory.intercept + trajectory.velocity * time;
            if !weight.is_finite() || weight < 0.0 {
                return Err(ProofError::new(
                    "kinetic zigzag edge weight leaves its valid range",
                ));
            }
        }
        previous = Some(trajectory.edge);
    }
    Ok(())
}

struct ExactSchedule {
    events: Vec<BigRational>,
    persistent_ties: usize,
}

fn exact_schedule(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    limits: ProofLimits,
) -> Result<ExactSchedule, ProofError> {
    let pairs = edges
        .len()
        .checked_mul(edges.len().saturating_sub(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag pair count overflows"))?;
    if pairs > limits.max_references {
        return Err(ProofError::new(
            "kinetic zigzag pair count exceeds its limit",
        ));
    }
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let coefficients = edges
        .iter()
        .map(|edge| (rational(edge.intercept), rational(edge.velocity)))
        .collect::<Vec<_>>();
    let mut events = BTreeSet::new();
    let mut persistent_ties = 0usize;
    for left in 0..edges.len() {
        for right in left + 1..edges.len() {
            let numerator = &coefficients[right].0 - &coefficients[left].0;
            let denominator = &coefficients[left].1 - &coefficients[right].1;
            if denominator == BigRational::from_integer(0.into()) {
                if numerator == BigRational::from_integer(0.into()) {
                    persistent_ties += 1;
                }
            } else {
                let time = numerator / denominator;
                if start < time && time < end {
                    events.insert(time);
                }
            }
        }
    }
    for (intercept, velocity) in &coefficients {
        if velocity != &BigRational::from_integer(0.into()) {
            let time = (&scale - intercept) / velocity;
            if start < time && time < end {
                events.insert(time);
            }
        }
    }
    if events.len() > limits.max_snapshots {
        return Err(ProofError::new(
            "kinetic zigzag event count exceeds its limit",
        ));
    }
    Ok(ExactSchedule {
        events: events.into_iter().collect(),
        persistent_ties,
    })
}

fn event_graphs(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    events: &[BigRational],
) -> Vec<Vec<Edge>> {
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let mut graphs = Vec::with_capacity(events.len() * 2 + 1);
    for position in 0..=events.len() {
        let left = if position == 0 {
            &start
        } else {
            &events[position - 1]
        };
        let right = events.get(position).unwrap_or(&end);
        graphs.push(active_edges(edges, &midpoint(left, right), &scale));
        if let Some(time) = events.get(position) {
            graphs.push(active_edges(edges, time, &scale));
        }
    }
    graphs
}

fn active_edges(edges: &[AffineEdge], time: &BigRational, scale: &BigRational) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| rational(edge.intercept) + rational(edge.velocity) * time <= *scale)
        .map(|edge| edge.edge)
        .collect()
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

struct Map {
    forward: bool,
    columns: Vec<Vec<MapTerm>>,
}

type Interval = (usize, usize, usize);

fn decompose(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
) -> Result<(Vec<usize>, Vec<Interval>), ProofError> {
    let count = dimensions.len();
    let mut ranks = vec![0usize; count * count];
    let mut work = 0usize;
    for start in (0..count).rev() {
        for end in start..count {
            let ambient = dimensions[start..=end]
                .iter()
                .try_fold(0usize, |sum, dimension| sum.checked_add(*dimension));
            let arrow_work = (start..end).try_fold(0usize, |sum, position| {
                let source = maps[position].columns.len();
                let target = if maps[position].forward {
                    dimensions[position + 1]
                } else {
                    dimensions[position]
                };
                sum.checked_add(source)?.checked_add(target)
            });
            work = ambient
                .and_then(|value| arrow_work.and_then(|arrows| value.checked_add(arrows)))
                .and_then(|value| work.checked_add(value))
                .ok_or_else(|| ProofError::new("kinetic zigzag rank work overflows"))?;
            if work > FORMAT_MAX_RANK_WORK {
                return Err(ProofError::new(
                    "kinetic zigzag rank work exceeds the format limit",
                ));
            }
            ranks[start * count + end] = generalized_rank(dimensions, maps, modulus, start, end);
        }
    }
    let mut intervals = Vec::new();
    for start in 0..count {
        for end in start..count {
            let rank = |left: usize, right: usize| ranks[left * count + right] as i128;
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
                return Err(ProofError::new(
                    "kinetic zigzag generalized ranks are not interval decomposable",
                ));
            }
            if multiplicity > 0 {
                intervals.push((start, end, multiplicity as usize));
            }
        }
    }
    Ok((ranks, intervals))
}

fn generalized_rank(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
    start: usize,
    end: usize,
) -> usize {
    if start == end {
        return dimensions[start];
    }
    let modulus = modulus as u64;
    let mut offsets = vec![0usize; end - start + 1];
    for position in 1..offsets.len() {
        offsets[position] = offsets[position - 1] + dimensions[start + position - 1];
    }
    let ambient = offsets.last().copied().unwrap_or(0) + dimensions[end];
    let mut relations = Vec::new();
    let mut equations = Vec::new();
    for position in start..end {
        let map = &maps[position];
        let left = offsets[position - start];
        let right = offsets[position + 1 - start];
        let (source, target, target_dimension) = if map.forward {
            (left, right, dimensions[position + 1])
        } else {
            (right, left, dimensions[position])
        };
        for (column, terms) in map.columns.iter().enumerate() {
            let mut relation = Vector::default();
            relation.insert(source + column, 1);
            for term in terms {
                relation.insert(
                    target + term.target,
                    (modulus - u64::from(term.coefficient)) as u32,
                );
            }
            relations.push(relation);
        }
        for target_position in 0..target_dimension {
            let mut equation = Vector::default();
            equation.insert(target + target_position, 1);
            for (column, terms) in map.columns.iter().enumerate() {
                if let Ok(term) = terms.binary_search_by_key(&target_position, |item| item.target) {
                    equation.insert(
                        source + column,
                        (modulus - u64::from(terms[term].coefficient)) as u32,
                    );
                }
            }
            equations.push(equation);
        }
    }
    let relation_rank = rref(relations.clone(), modulus).len();
    let limit = nullspace(equations, ambient, modulus);
    for section in limit {
        let mut image = Vector::default();
        for (&position, &coefficient) in section.0.range(..dimensions[start]) {
            image.insert(position, coefficient);
        }
        if !image.is_zero() {
            relations.push(image);
        }
    }
    rref(relations, modulus).len() - relation_rank
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
    let pivots = equations
        .iter()
        .map(|row| row.leading().unwrap().0)
        .collect::<BTreeSet<_>>();
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

fn read_usizes(
    reader: &mut Reader<'_>,
    name: &str,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize(name, maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(format!(
            "{name} count exceeds the remaining bytes"
        )));
    }
    (0..count).map(|_| reader.usize()).collect()
}
