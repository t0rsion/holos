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

/// Result of replaying one kinetic zigzag artifact.
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

/// Return true when bytes start with the kinetic zigzag magic.
pub fn is_kinetic_zigzag(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify a `HOLOSZZ` artifact.
///
/// The checker reconstructs every exact event complex, canonical cohomology
/// basis, restriction map, generalized rank, and interval multiplicity.
pub fn verify_kinetic_zigzag(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedKineticZigzag, ProofError> {
    let claim = decode_claim(bytes, limits)?;
    let schedule = verify_schedule(&claim, limits)?;
    let graphs = event_graphs(
        &claim.trajectories,
        claim.start,
        claim.end,
        claim.scale,
        &schedule.events,
    );
    verify_shape(&claim, graphs.len())?;
    let spaces = build_spaces(&claim, &graphs, limits)?;
    verify_nodes(&claim, &graphs, &spaces)?;
    let maps = build_maps(&spaces, schedule.events.len(), claim.modulus)?;
    verify_arrows(&claim, &maps)?;
    verify_decomposition(&claim, &spaces, &maps)?;
    Ok(zigzag_summary(&claim, &spaces, &maps))
}

struct KineticClaim {
    vertex_count: usize,
    trajectories: Vec<AffineEdge>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    persistent_ties: usize,
    node_ranks: Vec<usize>,
    node_edges: Vec<usize>,
    arrow_ranks: Vec<usize>,
    generalized_ranks: Vec<usize>,
    intervals: Vec<Interval>,
}

struct KineticHeader {
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    persistent_ties: usize,
}

struct KineticOutput {
    node_ranks: Vec<usize>,
    node_edges: Vec<usize>,
    arrow_ranks: Vec<usize>,
    generalized_ranks: Vec<usize>,
    intervals: Vec<Interval>,
}

fn decode_claim(bytes: &[u8], limits: ProofLimits) -> Result<KineticClaim, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let vertex_count = reader.bounded_usize("zigzag vertex count", limits.max_vertices)?;
    let trajectories = decode_trajectories(&mut reader, limits)?;
    let claim = decode_claim_body(&mut reader, vertex_count, trajectories, limits)?;
    decode_trailer(&mut reader, expected)?;
    Ok(claim)
}

fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "kinetic zigzag exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-kinetic-zigzag-v1");
    hash.update(&bytes[..payload_len]);
    let expected = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "kinetic zigzag digest differs from its content",
        ));
    }
    Ok(expected)
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(ProofError::new("unsupported kinetic zigzag artifact"))
    } else {
        Ok(())
    }
}

fn decode_trajectories(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<AffineEdge>, ProofError> {
    let count = reader.bounded_usize("zigzag edge count", limits.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "kinetic zigzag edge count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| decode_trajectory(reader)).collect()
}

fn decode_trajectory(reader: &mut Reader<'_>) -> Result<AffineEdge, ProofError> {
    Ok(AffineEdge {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}

fn decode_claim_body(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    trajectories: Vec<AffineEdge>,
    limits: ProofLimits,
) -> Result<KineticClaim, ProofError> {
    let header = decode_kinetic_header(reader, vertex_count, &trajectories, limits)?;
    let output = decode_kinetic_output(reader, limits)?;
    Ok(KineticClaim {
        vertex_count,
        trajectories,
        start: header.start,
        end: header.end,
        dimension: header.dimension,
        scale: header.scale,
        modulus: header.modulus,
        persistent_ties: header.persistent_ties,
        node_ranks: output.node_ranks,
        node_edges: output.node_edges,
        arrow_ranks: output.arrow_ranks,
        generalized_ranks: output.generalized_ranks,
        intervals: output.intervals,
    })
}

fn decode_kinetic_header(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    trajectories: &[AffineEdge],
    limits: ProofLimits,
) -> Result<KineticHeader, ProofError> {
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let dimension = reader.bounded_usize("zigzag dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    let persistent_ties = reader.usize()?;
    validate_input(vertex_count, trajectories, start, end, scale, modulus)?;
    Ok(KineticHeader {
        start,
        end,
        dimension,
        scale,
        modulus,
        persistent_ties,
    })
}

fn decode_kinetic_output(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<KineticOutput, ProofError> {
    let node_ranks = decode_node_ranks(reader, limits)?;
    let node_edges = read_usizes(reader, "zigzag node edge count", limits.max_snapshots)?;
    let arrow_ranks = read_usizes(reader, "zigzag arrow rank", limits.max_references)?;
    let maximum_ranks = square_count(node_ranks.len())?;
    let generalized_ranks = read_usizes(reader, "zigzag generalized rank", maximum_ranks)?;
    let intervals = decode_intervals(reader, node_ranks.len())?;
    Ok(KineticOutput {
        node_ranks,
        node_edges,
        arrow_ranks,
        generalized_ranks,
        intervals,
    })
}

fn decode_node_ranks(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<usize>, ProofError> {
    let ranks = read_usizes(reader, "zigzag node rank", limits.max_snapshots)?;
    if ranks.is_empty() || ranks.len() > FORMAT_MAX_NODES {
        Err(ProofError::new(
            "kinetic zigzag node count exceeds the format limit",
        ))
    } else {
        Ok(ranks)
    }
}

fn square_count(count: usize) -> Result<usize, ProofError> {
    count
        .checked_mul(count)
        .ok_or_else(|| ProofError::new("kinetic zigzag rank count overflows"))
}

fn decode_intervals(
    reader: &mut Reader<'_>,
    node_count: usize,
) -> Result<Vec<Interval>, ProofError> {
    let maximum = node_count
        .checked_mul(node_count.saturating_add(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag interval count overflows"))?;
    let count = reader.bounded_usize("zigzag interval count", maximum)?;
    (0..count)
        .map(|_| Ok((reader.usize()?, reader.usize()?, reader.usize()?)))
        .collect()
}

fn decode_trailer(reader: &mut Reader<'_>, expected: [u8; 32]) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        Err(ProofError::new(
            "kinetic zigzag has a wrong digest or trailing bytes",
        ))
    } else {
        Ok(())
    }
}

fn verify_schedule(claim: &KineticClaim, limits: ProofLimits) -> Result<ExactSchedule, ProofError> {
    let schedule = exact_schedule(
        &claim.trajectories,
        claim.start,
        claim.end,
        claim.scale,
        limits,
    )?;
    if schedule.persistent_ties != claim.persistent_ties {
        Err(ProofError::new(
            "kinetic zigzag persistent-tie count is wrong",
        ))
    } else {
        Ok(schedule)
    }
}

fn verify_shape(claim: &KineticClaim, graph_count: usize) -> Result<(), ProofError> {
    let maximum_ranks = square_count(claim.node_ranks.len())?;
    if graph_count != claim.node_ranks.len()
        || claim.node_edges.len() != graph_count
        || claim.arrow_ranks.len() + 1 != graph_count
        || claim.generalized_ranks.len() != maximum_ranks
    {
        Err(ProofError::new("kinetic zigzag claim shape is wrong"))
    } else {
        Ok(())
    }
}

fn build_spaces(
    claim: &KineticClaim,
    graphs: &[Vec<Edge>],
    limits: ProofLimits,
) -> Result<Vec<Space>, ProofError> {
    graphs
        .iter()
        .map(|graph| {
            Space::build(
                claim.vertex_count,
                claim.dimension,
                graph,
                claim.modulus,
                limits,
            )
        })
        .collect()
}

fn verify_nodes(
    claim: &KineticClaim,
    graphs: &[Vec<Edge>],
    spaces: &[Space],
) -> Result<(), ProofError> {
    let ranks = spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let edges = graphs.iter().map(Vec::len).collect::<Vec<_>>();
    if claim.node_ranks != ranks || claim.node_edges != edges {
        Err(ProofError::new(
            "kinetic zigzag node ranks or active-edge counts are wrong",
        ))
    } else {
        Ok(())
    }
}

fn build_maps(spaces: &[Space], event_count: usize, modulus: u32) -> Result<Vec<Map>, ProofError> {
    let mut maps = Vec::with_capacity(event_count * 2);
    for event in 0..event_count {
        append_event_maps(&mut maps, spaces, event, modulus)?;
    }
    Ok(maps)
}

fn append_event_maps(
    maps: &mut Vec<Map>,
    spaces: &[Space],
    event: usize,
    modulus: u32,
) -> Result<(), ProofError> {
    let left = 2 * event;
    let middle = left + 1;
    let right = left + 2;
    let (left_columns, left_rank) = spaces[middle].restriction_to(&spaces[left], modulus)?;
    maps.push(Map {
        forward: false,
        columns: left_columns,
        rank: left_rank,
    });
    let (right_columns, right_rank) = spaces[middle].restriction_to(&spaces[right], modulus)?;
    maps.push(Map {
        forward: true,
        columns: right_columns,
        rank: right_rank,
    });
    Ok(())
}

fn verify_arrows(claim: &KineticClaim, maps: &[Map]) -> Result<(), ProofError> {
    let ranks = maps.iter().map(|map| map.rank).collect::<Vec<_>>();
    if claim.arrow_ranks != ranks {
        Err(ProofError::new(
            "kinetic zigzag restriction ranks are wrong",
        ))
    } else {
        Ok(())
    }
}

fn verify_decomposition(
    claim: &KineticClaim,
    spaces: &[Space],
    maps: &[Map],
) -> Result<(), ProofError> {
    let ranks = spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let (checked_ranks, checked_intervals) = decompose(&ranks, maps, claim.modulus)?;
    if claim.generalized_ranks != checked_ranks || claim.intervals != checked_intervals {
        Err(ProofError::new(
            "kinetic zigzag interval decomposition is wrong",
        ))
    } else {
        Ok(())
    }
}

fn zigzag_summary(claim: &KineticClaim, spaces: &[Space], maps: &[Map]) -> VerifiedKineticZigzag {
    VerifiedKineticZigzag {
        dimension: claim.dimension,
        modulus: claim.modulus,
        edges: claim.trajectories.len(),
        nodes: spaces.len(),
        arrows: maps.len(),
        intervals: claim.intervals.len(),
        interval_copies: claim.intervals.iter().map(|item| item.2).sum(),
    }
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
    validate_interval(start, end, scale)?;
    validate_modulus(modulus)?;
    let mut previous = None;
    for trajectory in edges {
        validate_trajectory(trajectory, previous, vertex_count, start, end)?;
        previous = Some(trajectory.edge);
    }
    Ok(())
}

fn validate_interval(start: f64, end: f64, scale: f64) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end || !scale.is_finite() || scale < 0.0 {
        Err(ProofError::new("kinetic zigzag interval is invalid"))
    } else {
        Ok(())
    }
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(
            "kinetic zigzag modulus is not a supported prime",
        ))
    } else {
        Ok(())
    }
}

fn validate_trajectory(
    trajectory: &AffineEdge,
    previous: Option<Edge>,
    vertex_count: usize,
    start: f64,
    end: f64,
) -> Result<(), ProofError> {
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
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

fn validate_trajectory_weight(trajectory: &AffineEdge, time: f64) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "kinetic zigzag edge weight leaves its valid range",
        ))
    } else {
        Ok(())
    }
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
    validate_pair_count(edges.len(), limits)?;
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let coefficients = edges
        .iter()
        .map(|edge| (rational(edge.intercept), rational(edge.velocity)))
        .collect::<Vec<_>>();
    let (mut events, persistent_ties) = pair_events(&coefficients, &start, &end);
    threshold_events(&mut events, &coefficients, &start, &end, &scale);
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

fn validate_pair_count(count: usize, limits: ProofLimits) -> Result<(), ProofError> {
    let pairs = count
        .checked_mul(count.saturating_sub(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag pair count overflows"))?;
    if pairs > limits.max_references {
        Err(ProofError::new(
            "kinetic zigzag pair count exceeds its limit",
        ))
    } else {
        Ok(())
    }
}

fn pair_events(
    coefficients: &[(BigRational, BigRational)],
    start: &BigRational,
    end: &BigRational,
) -> (BTreeSet<BigRational>, usize) {
    let mut events = BTreeSet::new();
    let mut persistent_ties = 0usize;
    for left in 0..coefficients.len() {
        for right in left + 1..coefficients.len() {
            let numerator = &coefficients[right].0 - &coefficients[left].0;
            let denominator = &coefficients[left].1 - &coefficients[right].1;
            if denominator == BigRational::from_integer(0.into()) {
                if numerator == BigRational::from_integer(0.into()) {
                    persistent_ties += 1;
                }
            } else {
                let time = numerator / denominator;
                if start < &time && &time < end {
                    events.insert(time);
                }
            }
        }
    }
    (events, persistent_ties)
}

fn threshold_events(
    events: &mut BTreeSet<BigRational>,
    coefficients: &[(BigRational, BigRational)],
    start: &BigRational,
    end: &BigRational,
    scale: &BigRational,
) {
    for (intercept, velocity) in coefficients {
        if velocity != &BigRational::from_integer(0.into()) {
            let time = (scale - intercept) / velocity;
            if start < &time && &time < end {
                events.insert(time);
            }
        }
    }
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
    rank: usize,
}

type Interval = (usize, usize, usize);

fn decompose(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
) -> Result<(Vec<usize>, Vec<Interval>), ProofError> {
    let ranks = compute_generalized_ranks(dimensions, maps, modulus)?;
    let intervals = interval_decomposition(&ranks, dimensions.len())?;
    Ok((ranks, intervals))
}

fn compute_generalized_ranks(
    dimensions: &[usize],
    maps: &[Map],
    modulus: u32,
) -> Result<Vec<usize>, ProofError> {
    let count = dimensions.len();
    let mut ranks = vec![0usize; count * count];
    let mut work = 0usize;
    for start in (0..count).rev() {
        for end in start..count {
            work = add_rank_work(work, dimensions, maps, start, end)?;
            ranks[start * count + end] = generalized_rank(dimensions, maps, modulus, start, end);
        }
    }
    Ok(ranks)
}

fn add_rank_work(
    work: usize,
    dimensions: &[usize],
    maps: &[Map],
    start: usize,
    end: usize,
) -> Result<usize, ProofError> {
    let ambient = dimensions[start..=end]
        .iter()
        .try_fold(0usize, |sum, dimension| sum.checked_add(*dimension));
    let arrows = (start..end).try_fold(0usize, |sum, position| {
        let source = maps[position].columns.len();
        let target = if maps[position].forward {
            dimensions[position + 1]
        } else {
            dimensions[position]
        };
        sum.checked_add(source)?.checked_add(target)
    });
    let next = ambient
        .and_then(|value| arrows.and_then(|arrow_work| value.checked_add(arrow_work)))
        .and_then(|value| work.checked_add(value))
        .ok_or_else(|| ProofError::new("kinetic zigzag rank work overflows"))?;
    if next > FORMAT_MAX_RANK_WORK {
        Err(ProofError::new(
            "kinetic zigzag rank work exceeds the format limit",
        ))
    } else {
        Ok(next)
    }
}

fn interval_decomposition(ranks: &[usize], count: usize) -> Result<Vec<Interval>, ProofError> {
    let mut intervals = Vec::new();
    for start in 0..count {
        for end in start..count {
            let multiplicity = interval_multiplicity(ranks, count, start, end);
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
    Ok(intervals)
}

fn interval_multiplicity(ranks: &[usize], count: usize, start: usize, end: usize) -> i128 {
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
    multiplicity
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
        append_map_constraints(
            &mut relations,
            &mut equations,
            dimensions,
            maps,
            &offsets,
            start,
            position,
            modulus,
        );
    }
    let relation_rank = rref(relations.clone(), modulus).len();
    let limit = nullspace(equations, ambient, modulus);
    append_limit_images(&mut relations, limit, dimensions[start]);
    rref(relations, modulus).len() - relation_rank
}

#[allow(clippy::too_many_arguments)]
fn append_map_constraints(
    relations: &mut Vec<Vector>,
    equations: &mut Vec<Vector>,
    dimensions: &[usize],
    maps: &[Map],
    offsets: &[usize],
    start: usize,
    position: usize,
    modulus: u64,
) {
    let map = &maps[position];
    let left = offsets[position - start];
    let right = offsets[position + 1 - start];
    let (source, target, target_dimension) = if map.forward {
        (left, right, dimensions[position + 1])
    } else {
        (right, left, dimensions[position])
    };
    for (column, terms) in map.columns.iter().enumerate() {
        relations.push(map_relation(source, target, column, terms, modulus));
    }
    for target_position in 0..target_dimension {
        equations.push(map_equation(
            source,
            target,
            target_position,
            &map.columns,
            modulus,
        ));
    }
}

fn map_relation(
    source: usize,
    target: usize,
    column: usize,
    terms: &[MapTerm],
    modulus: u64,
) -> Vector {
    let mut relation = Vector::default();
    relation.insert(source + column, 1);
    for term in terms {
        relation.insert(
            target + term.target,
            (modulus - u64::from(term.coefficient)) as u32,
        );
    }
    relation
}

fn map_equation(
    source: usize,
    target: usize,
    target_position: usize,
    columns: &[Vec<MapTerm>],
    modulus: u64,
) -> Vector {
    let mut equation = Vector::default();
    equation.insert(target + target_position, 1);
    for (column, terms) in columns.iter().enumerate() {
        if let Ok(term) = terms.binary_search_by_key(&target_position, |item| item.target) {
            equation.insert(
                source + column,
                (modulus - u64::from(terms[term].coefficient)) as u32,
            );
        }
    }
    equation
}

fn append_limit_images(relations: &mut Vec<Vector>, limit: Vec<Vector>, dimension: usize) {
    for section in limit {
        let mut image = Vector::default();
        for (&position, &coefficient) in section.0.range(..dimension) {
            image.insert(position, coefficient);
        }
        if !image.is_zero() {
            relations.push(image);
        }
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
