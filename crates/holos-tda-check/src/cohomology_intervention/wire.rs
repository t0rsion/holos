use sha2::{Digest, Sha256};

use crate::cohomology::Edge;
use crate::{MODULUS_LIMIT, ProofError, ProofLimits, Reader, is_prime};

use super::model::{Candidate, Claim, Scenario, VerifiedCohomologyInterventionStatus};
use super::{
    F64_BITS_CODEC, FORMAT_MAX_CANDIDATES, FORMAT_MAX_ORACLE_CALLS, FORMAT_MAX_PROOF_TERMS,
    FORMAT_MAX_SCENARIOS, FORMAT_MAX_SEARCH_NODES, MAGIC, VERSION,
};

struct Header {
    vertex_count: usize,
    dimension: usize,
    modulus: u32,
}

struct WorkLimits {
    max_edits: usize,
    oracle: usize,
    nodes: usize,
}

struct SearchClaimData {
    status: VerifiedCohomologyInterventionStatus,
    edits: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

struct ProofClaimData {
    root_blockers: Vec<Vec<usize>>,
    root_blocker_bound: u64,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
}

pub(super) fn decode_claim(bytes: &[u8], limits: ProofLimits) -> Result<Claim, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let header = decode_header(&mut reader, limits)?;
    let scenarios = decode_scenarios(&mut reader, header.vertex_count, limits)?;
    let candidates = decode_candidates(&mut reader, header.vertex_count, &scenarios, limits)?;
    let claim = decode_search_claim(&mut reader, header, scenarios, candidates, limits)?;
    decode_trailer(&mut reader, expected)?;
    Ok(claim)
}

fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "cohomology intervention exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-intervention-v2");
    hash.update(&bytes[..payload_len]);
    let expected = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "cohomology intervention digest differs from its content",
        ));
    }
    Ok(expected)
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(ProofError::new(
            "unsupported cohomology intervention artifact",
        ))
    } else {
        Ok(())
    }
}

fn decode_header(reader: &mut Reader<'_>, limits: ProofLimits) -> Result<Header, ProofError> {
    let vertex_count = reader.bounded_usize("intervention vertex count", limits.max_vertices)?;
    let dimension = reader.bounded_usize("intervention dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    validate_field(scale, modulus)?;
    Ok(Header {
        vertex_count,
        dimension,
        modulus,
    })
}

fn validate_field(scale: f64, modulus: u32) -> Result<(), ProofError> {
    if !scale.is_finite() || scale < 0.0 {
        return Err(ProofError::new("cohomology intervention scale is invalid"));
    }
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "cohomology intervention modulus is not a supported prime",
        ));
    }
    Ok(())
}

fn decode_scenarios(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Scenario>, ProofError> {
    let count = reader.bounded_usize(
        "intervention scenario count",
        limits.max_snapshots.min(FORMAT_MAX_SCENARIOS),
    )?;
    if count == 0 {
        return Err(ProofError::new("cohomology intervention has no scenario"));
    }
    let mut scenarios = Vec::with_capacity(count);
    let mut total_edges = 0usize;
    for _ in 0..count {
        let scenario = decode_scenario(reader, vertex_count, limits)?;
        total_edges = add_edge_count(total_edges, scenario.edges.len(), limits)?;
        scenarios.push(scenario);
    }
    Ok(scenarios)
}

fn decode_scenario(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Scenario, ProofError> {
    Ok(Scenario {
        edges: decode_edges(reader, vertex_count, limits.max_edges)?,
        target_basis: reader.usize()?,
    })
}

fn add_edge_count(total: usize, add: usize, limits: ProofLimits) -> Result<usize, ProofError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new("intervention edge count overflows"))?;
    if total > limits.max_edges {
        Err(ProofError::new(
            "intervention scenario edges exceed their total limit",
        ))
    } else {
        Ok(total)
    }
}

fn decode_candidates(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    scenarios: &[Scenario],
    limits: ProofLimits,
) -> Result<Vec<Candidate>, ProofError> {
    let count = reader.bounded_usize(
        "intervention candidate count",
        limits.max_references.min(FORMAT_MAX_CANDIDATES),
    )?;
    if count > reader.remaining() / 24 {
        return Err(ProofError::new(
            "intervention candidates exceed the remaining bytes",
        ));
    }
    let candidates = (0..count)
        .map(|_| decode_candidate(reader))
        .collect::<Result<Vec<_>, _>>()?;
    validate_candidates(&candidates, vertex_count, scenarios)?;
    Ok(candidates)
}

fn decode_candidate(reader: &mut Reader<'_>) -> Result<Candidate, ProofError> {
    Ok(Candidate {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
    })
}

fn validate_candidates(
    candidates: &[Candidate],
    vertex_count: usize,
    scenarios: &[Scenario],
) -> Result<(), ProofError> {
    let invalid = candidates.iter().any(|candidate| {
        candidate.edge.u >= candidate.edge.v
            || candidate.edge.v >= vertex_count
            || candidate.cost == 0
    });
    let unordered = candidates
        .windows(2)
        .any(|pair| pair[0].edge >= pair[1].edge);
    if invalid || unordered {
        return Err(ProofError::new(
            "cohomology intervention candidate list is not canonical",
        ));
    }
    if scenarios.iter().any(|scenario| {
        candidates
            .iter()
            .any(|candidate| scenario.edges.binary_search(&candidate.edge).is_ok())
    }) {
        return Err(ProofError::new(
            "cohomology intervention candidate is active in a scenario",
        ));
    }
    Ok(())
}

fn decode_search_claim(
    reader: &mut Reader<'_>,
    header: Header,
    scenarios: Vec<Scenario>,
    candidates: Vec<Candidate>,
    limits: ProofLimits,
) -> Result<Claim, ProofError> {
    let work = decode_work_limits(reader, candidates.len(), limits)?;
    let search = decode_search_data(reader, candidates.len())?;
    let proof = decode_proof_claim_data(reader, candidates.len(), scenarios.len(), limits)?;
    Ok(Claim {
        vertex_count: header.vertex_count,
        dimension: header.dimension,
        modulus: header.modulus,
        scenarios,
        candidates,
        max_edits: work.max_edits,
        oracle_limit: work.oracle,
        node_limit: work.nodes,
        status: search.status,
        edits: search.edits,
        lower_bound: search.lower_bound,
        upper_bound: search.upper_bound,
        oracle_calls: search.oracle_calls,
        search_nodes: search.search_nodes,
        cache_hits: search.cache_hits,
        root_blockers: proof.root_blockers,
        root_blocker_bound: proof.root_blocker_bound,
        before_ranks: proof.before_ranks,
        after_ranks: proof.after_ranks,
    })
}

fn decode_search_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
) -> Result<SearchClaimData, ProofError> {
    Ok(SearchClaimData {
        status: VerifiedCohomologyInterventionStatus::from_code(reader.u8()?)?,
        edits: decode_indices(reader, candidate_count, candidate_count)?,
        lower_bound: optional_u64(reader)?,
        upper_bound: optional_u64(reader)?,
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

fn decode_proof_claim_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    scenario_count: usize,
    limits: ProofLimits,
) -> Result<ProofClaimData, ProofError> {
    Ok(ProofClaimData {
        root_blockers: decode_root_blockers(reader, candidate_count, limits)?,
        root_blocker_bound: reader.u64()?,
        before_ranks: decode_usizes(reader, scenario_count)?,
        after_ranks: decode_usizes(reader, scenario_count)?,
    })
}

fn decode_work_limits(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    limits: ProofLimits,
) -> Result<WorkLimits, ProofError> {
    let work = WorkLimits {
        max_edits: reader.usize()?,
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    };
    if work.max_edits > candidate_count
        || work.oracle == 0
        || work.oracle > limits.max_snapshots.min(FORMAT_MAX_ORACLE_CALLS)
        || work.nodes == 0
        || work.nodes > limits.max_nodes.min(FORMAT_MAX_SEARCH_NODES)
    {
        Err(ProofError::new(
            "cohomology intervention search limits are invalid",
        ))
    } else {
        Ok(work)
    }
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let maximum = limits.max_terms.min(FORMAT_MAX_PROOF_TERMS);
    let count = reader.bounded_usize("intervention blocker count", maximum)?;
    let mut blockers = Vec::with_capacity(count);
    let mut terms = 0usize;
    for _ in 0..count {
        let blocker = decode_indices(reader, candidate_count, maximum)?;
        terms = add_proof_terms(terms, blocker.len(), maximum)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn add_proof_terms(total: usize, add: usize, maximum: usize) -> Result<usize, ProofError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new("intervention proof term count overflows"))?;
    if total > maximum {
        Err(ProofError::new(
            "intervention proof terms exceed their limit",
        ))
    } else {
        Ok(total)
    }
}

fn decode_trailer(reader: &mut Reader<'_>, expected: [u8; 32]) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        Err(ProofError::new(
            "cohomology intervention has a wrong digest or trailing bytes",
        ))
    } else {
        Ok(())
    }
}

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<Edge>, ProofError> {
    let count = reader.bounded_usize("intervention edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(ProofError::new(
            "intervention edge count exceeds the remaining bytes",
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
            "cohomology intervention edge list is not canonical",
        ));
    }
    Ok(edges)
}

fn decode_indices(
    reader: &mut Reader<'_>,
    candidates: usize,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let values = decode_usizes(reader, maximum)?;
    if values.iter().any(|value| *value >= candidates)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "intervention candidate indices are not canonical",
        ));
    }
    Ok(values)
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("intervention integer count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "intervention integer count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn optional_u64(reader: &mut Reader<'_>) -> Result<Option<u64>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.u64()?)),
        _ => Err(ProofError::new(
            "intervention optional integer tag is invalid",
        )),
    }
}
