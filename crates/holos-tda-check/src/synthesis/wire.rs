use sha2::{Digest, Sha256};

use crate::cohomology::{Edge, MapTerm};
use crate::{ProofError, ProofLimits, is_prime};

use super::model::{
    Action, Claim, DecodedSynthesis, F64_BITS_CODEC, FORMAT_MAX_ACTIONS, FORMAT_MAX_ORACLE_CALLS,
    FORMAT_MAX_PROOF_TERMS, FORMAT_MAX_SEARCH_NODES, FORMAT_MAX_STATES, MAGIC, MODULUS_LIMIT,
    ProofData, SearchData, Selection, State, SynthesisHeader, VERSION, VerifiedSynthesisStatus,
    WorkLimits,
};
use super::proof::decode_optional_proof;
pub(super) use super::wire_reader::Reader;
use super::wire_source::decode_source;

pub(super) fn decode_synthesis(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<DecodedSynthesis, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let header = decode_header(&mut reader, limits)?;
    let states = decode_states(&mut reader, header.vertex_count, limits)?;
    let actions = decode_actions(&mut reader, header.vertex_count, states.len(), limits)?;
    let search = decode_search(&mut reader, actions.len())?;
    let proof = decode_proof_data(&mut reader, actions.len(), states.len(), limits)?;
    decode_trailer(&mut reader, expected)?;
    let claim = Claim {
        vertex_count: header.vertex_count,
        dimension: header.dimension,
        scale: header.scale,
        modulus: header.modulus,
        source: header.source,
        states,
        actions,
        max_edits: search.max_edits,
        status: search.selection.status,
        selected: search.selection.selected,
        lower_bound: search.selection.lower_bound,
        upper_bound: search.selection.upper_bound,
        root_blockers: proof.root_blockers,
        before_ranks: proof.before_ranks,
        after_ranks: proof.after_ranks,
        proof: proof.proof,
    };
    Ok(DecodedSynthesis {
        claim,
        producer_oracle_calls: search.producer_oracle_calls,
        producer_search_nodes: search.producer_search_nodes,
        proof_nodes: proof.nodes,
        proof_topology_checks: proof.topology_checks,
    })
}

pub(super) fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "synthesis artifact exceeds its byte limit or is truncated",
        ));
    }
    let mut hash = Sha256::new();
    hash.update(b"holos-synthesis-artifact-v1");
    hash.update(&bytes[..bytes.len() - 32]);
    Ok(hash.finalize().into())
}

pub(super) fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported synthesis artifact"));
    }
    Ok(())
}

pub(super) fn decode_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<SynthesisHeader, ProofError> {
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let dimension = reader.bounded_usize("dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    validate_field(scale, modulus)?;
    Ok(SynthesisHeader {
        vertex_count,
        dimension,
        scale,
        modulus,
        source: decode_source(reader, limits)?,
    })
}

pub(super) fn validate_field(scale: f64, modulus: u32) -> Result<(), ProofError> {
    if !scale.is_finite()
        || scale < 0.0
        || !is_prime(u64::from(modulus))
        || u64::from(modulus) >= MODULUS_LIMIT
    {
        Err(ProofError::new(
            "synthesis scale or coefficient field is invalid",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_states(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Vec<State>, ProofError> {
    let count = reader.bounded_usize("state count", limits.max_snapshots.min(FORMAT_MAX_STATES))?;
    let mut states = Vec::with_capacity(count);
    let mut edge_count = 0usize;
    let mut target_terms = 0usize;
    let mut prior = None;
    for _ in 0..count {
        let state = decode_state(reader, vertex_count, &mut target_terms, limits)?;
        validate_state_order(prior, &state)?;
        prior = Some((state.scenario, state.step));
        edge_count = add_edge_count(edge_count, state.edges.len(), limits)?;
        states.push(state);
    }
    Ok(states)
}

pub(super) fn decode_state(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<State, ProofError> {
    let scenario = reader.u64()?;
    let step = reader.u64()?;
    let edges = decode_edges(reader, vertex_count, limits.max_edges)?;
    let target_space = reader.array32()?;
    let target = decode_target(reader, target_terms, limits)?;
    Ok(State {
        scenario,
        step,
        edges,
        target_space,
        target,
        max_surviving_rank: reader.usize()?,
    })
}

pub(super) fn decode_target(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<MapTerm>>, ProofError> {
    let count = reader.bounded_usize("target row count", limits.max_terms)?;
    let mut target = Vec::with_capacity(count);
    for _ in 0..count {
        target.push(decode_target_row(reader, target_terms, limits)?);
    }
    Ok(target)
}

pub(super) fn decode_target_row(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<MapTerm>, ProofError> {
    let count = reader.bounded_usize("target row term count", limits.max_terms)?;
    *target_terms = target_terms
        .checked_add(count)
        .ok_or_else(|| ProofError::new("synthesis target term count overflows"))?;
    if *target_terms > limits.max_terms || count > reader.remaining() / 12 {
        return Err(ProofError::new(
            "synthesis target terms exceed their limit or remaining bytes",
        ));
    }
    (0..count)
        .map(|_| {
            Ok(MapTerm {
                target: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

pub(super) fn validate_state_order(
    prior: Option<(u64, u64)>,
    state: &State,
) -> Result<(), ProofError> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        Err(ProofError::new(
            "synthesis states are not in canonical scenario and step order",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn add_edge_count(
    total: usize,
    add: usize,
    limits: ProofLimits,
) -> Result<usize, ProofError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new("synthesis edge count overflows"))?;
    if total > limits.max_edges {
        Err(ProofError::new(
            "synthesis state edges exceed their total limit",
        ))
    } else {
        Ok(total)
    }
}

pub(super) fn decode_actions(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    state_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Action>, ProofError> {
    let count = reader.bounded_usize(
        "action count",
        limits.max_references.min(FORMAT_MAX_ACTIONS),
    )?;
    let mut actions = Vec::with_capacity(count);
    for _ in 0..count {
        actions.push(decode_action(reader, state_count)?);
    }
    validate_actions(&actions, vertex_count)?;
    Ok(actions)
}

pub(super) fn decode_action(
    reader: &mut Reader<'_>,
    state_count: usize,
) -> Result<Action, ProofError> {
    Ok(Action {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
        states: decode_indices(reader, state_count, state_count)?,
    })
}

pub(super) fn validate_actions(actions: &[Action], vertex_count: usize) -> Result<(), ProofError> {
    let unordered = actions.windows(2).any(|pair| pair[0] >= pair[1]);
    let invalid = actions.iter().any(|action| {
        action.edge.u >= action.edge.v
            || action.edge.v >= vertex_count
            || action.cost == 0
            || action.states.is_empty()
    });
    if unordered || invalid {
        Err(ProofError::new("synthesis action list is not canonical"))
    } else {
        Ok(())
    }
}

pub(super) fn decode_search(
    reader: &mut Reader<'_>,
    action_count: usize,
) -> Result<SearchData, ProofError> {
    let max_edits = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, action_count, max_edits)?;
    let producer_oracle_calls = reader.usize()?;
    let producer_search_nodes = reader.usize()?;
    let _producer_cache_hits = reader.usize()?;
    if producer_oracle_calls > limits.oracle || producer_search_nodes > limits.nodes {
        return Err(ProofError::new(
            "synthesis producer work exceeds its declared limit",
        ));
    }
    Ok(SearchData {
        max_edits,
        selection,
        producer_oracle_calls,
        producer_search_nodes,
    })
}

pub(super) fn decode_work_limits(reader: &mut Reader<'_>) -> Result<WorkLimits, ProofError> {
    let oracle = reader.usize()?;
    let nodes = reader.usize()?;
    if oracle == 0
        || oracle > FORMAT_MAX_ORACLE_CALLS
        || nodes == 0
        || nodes > FORMAT_MAX_SEARCH_NODES
    {
        Err(ProofError::new(
            "synthesis edit or producer work limit is invalid",
        ))
    } else {
        Ok(WorkLimits { oracle, nodes })
    }
}

pub(super) fn decode_selection(
    reader: &mut Reader<'_>,
    action_count: usize,
    max_edits: usize,
) -> Result<Selection, ProofError> {
    let status = VerifiedSynthesisStatus::from_code(reader.u8()?)?;
    let selected = decode_indices(reader, action_count, action_count)?;
    if selected.len() > max_edits {
        return Err(ProofError::new(
            "synthesis selection exceeds the edit limit",
        ));
    }
    Ok(Selection {
        status,
        selected,
        lower_bound: reader.optional_u64()?,
        upper_bound: reader.optional_u64()?,
    })
}

pub(super) fn decode_proof_data(
    reader: &mut Reader<'_>,
    action_count: usize,
    state_count: usize,
    limits: ProofLimits,
) -> Result<ProofData, ProofError> {
    let mut terms = 0usize;
    let root_blockers = decode_root_blockers(reader, action_count, &mut terms, limits)?;
    let before_ranks = decode_usizes(reader, state_count)?;
    let after_ranks = decode_usizes(reader, state_count)?;
    let mut nodes = 0usize;
    let proof = decode_optional_proof(reader, action_count, &mut nodes, &mut terms, limits)?;
    let claimed_nodes = reader.usize()?;
    let topology_checks = reader.usize()?;
    if claimed_nodes != nodes {
        return Err(ProofError::new(
            "synthesis proof node count differs from its tree",
        ));
    }
    Ok(ProofData {
        root_blockers,
        before_ranks,
        after_ranks,
        proof,
        nodes,
        topology_checks,
    })
}

pub(super) fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_terms(terms, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

pub(super) fn add_terms(
    total: &mut usize,
    count: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("synthesis proof term count overflows"))?;
    if *total > limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
        return Err(ProofError::new("synthesis proof terms exceed their limit"));
    }
    Ok(())
}

pub(super) fn decode_trailer(
    reader: &mut Reader<'_>,
    expected: [u8; 32],
) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        Err(ProofError::new(
            "synthesis artifact has a wrong digest or trailing bytes",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<Edge>, ProofError> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(ProofError::new(
            "synthesis edge count exceeds the remaining bytes",
        ));
    }
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        edges.push(Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        });
    }
    if edges
        .iter()
        .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "synthesis active edge list is not canonical",
        ));
    }
    Ok(edges)
}

pub(super) fn decode_usizes(
    reader: &mut Reader<'_>,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "synthesis integer list exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

pub(super) fn decode_indices(
    reader: &mut Reader<'_>,
    exclusive_maximum: usize,
    maximum_count: usize,
) -> Result<Vec<usize>, ProofError> {
    let values = decode_usizes(reader, maximum_count)?;
    if values.iter().any(|value| *value >= exclusive_maximum)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new("synthesis index list is not canonical"));
    }
    Ok(values)
}
