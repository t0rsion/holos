use crate::{Error, KineticEdgeKey, Result};

use super::model::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus, FORMAT_MAX_CANDIDATES,
    FORMAT_MAX_PROOF_TERMS, FORMAT_MAX_SCENARIOS, InterventionHeader, InterventionProducerWork,
    InterventionProofData, InterventionSearchData, InterventionSelection, InterventionWorkLimits,
};

const MAGIC: &[u8; 8] = b"HOLOSCI\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

pub(super) fn validate_artifact_size(
    bytes: &[u8],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "cohomology intervention exceeds its byte limit or is truncated".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_prefix(reader: &mut Reader<'_>) -> Result<()> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(Error::InvalidInput(
            "unsupported cohomology intervention artifact".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_header(reader: &mut Reader<'_>) -> Result<InterventionHeader> {
    Ok(InterventionHeader {
        vertex_count: reader.usize()?,
        dimension: reader.usize()?,
        scale: f64::from_bits(reader.u64()?),
        modulus: reader.u32()?,
    })
}

pub(super) fn decode_scenarios(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<CohomologyInterventionScenario>> {
    let count = reader.bounded_usize(
        "scenario count",
        limits.max_scenarios.min(FORMAT_MAX_SCENARIOS),
    )?;
    let mut scenarios = Vec::with_capacity(count);
    for _ in 0..count {
        scenarios.push(decode_scenario(reader, limits)?);
    }
    Ok(scenarios)
}

pub(super) fn decode_scenario(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<CohomologyInterventionScenario> {
    Ok(CohomologyInterventionScenario {
        active_edges: decode_edges(reader, limits.max_edges_per_scenario)?,
        target_basis: reader.usize()?,
    })
}

pub(super) fn decode_candidates(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<CohomologyInterventionCandidate>> {
    let count = reader.bounded_usize(
        "candidate count",
        limits.max_candidates.min(FORMAT_MAX_CANDIDATES),
    )?;
    if count > reader.remaining() / 24 {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate count exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| decode_candidate(reader)).collect()
}

pub(super) fn decode_candidate(reader: &mut Reader<'_>) -> Result<CohomologyInterventionCandidate> {
    Ok(CohomologyInterventionCandidate {
        edge: KineticEdgeKey {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
    })
}

pub(super) fn decode_search_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
) -> Result<InterventionSearchData> {
    let max_edits = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, candidate_count)?;
    let work = decode_producer_work(reader)?;
    Ok(InterventionSearchData {
        max_edits,
        oracle_limit: limits.oracle,
        node_limit: limits.nodes,
        status: selection.status,
        edit_indices: selection.edit_indices,
        lower_bound_cost: selection.lower_bound_cost,
        upper_bound_cost: selection.upper_bound_cost,
        oracle_calls: work.oracle_calls,
        search_nodes: work.search_nodes,
        cache_hits: work.cache_hits,
    })
}

pub(super) fn decode_work_limits(reader: &mut Reader<'_>) -> Result<InterventionWorkLimits> {
    Ok(InterventionWorkLimits {
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    })
}

pub(super) fn decode_selection(
    reader: &mut Reader<'_>,
    candidate_count: usize,
) -> Result<InterventionSelection> {
    Ok(InterventionSelection {
        status: CohomologyInterventionStatus::from_code(reader.u8()?)?,
        edit_indices: decode_indices(reader, candidate_count, candidate_count)?,
        lower_bound_cost: reader.optional_u64()?,
        upper_bound_cost: reader.optional_u64()?,
    })
}

pub(super) fn decode_producer_work(reader: &mut Reader<'_>) -> Result<InterventionProducerWork> {
    Ok(InterventionProducerWork {
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

pub(super) fn decode_proof_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    scenario_count: usize,
    limits: CohomologyInterventionLimits,
) -> Result<InterventionProofData> {
    Ok(InterventionProofData {
        root_blockers: decode_root_blockers(reader, candidate_count, limits)?,
        root_blocker_bound: reader.u64()?,
        before_ranks: decode_usizes(reader, scenario_count)?,
        after_ranks: decode_usizes(reader, scenario_count)?,
    })
}

pub(super) fn decode_root_blockers(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<Vec<usize>>> {
    let maximum = limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS);
    let count = reader.bounded_usize("root blocker count", maximum)?;
    let mut blockers = Vec::with_capacity(count);
    let mut terms = 0usize;
    for _ in 0..count {
        let blocker = decode_indices(reader, candidate_count, maximum)?;
        terms = add_proof_terms(terms, blocker.len(), maximum)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

pub(super) fn add_proof_terms(total: usize, add: usize, maximum: usize) -> Result<usize> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| Error::InvalidInput("intervention proof term count overflows".into()))?;
    if total > maximum {
        Err(Error::InvalidInput(
            "cohomology intervention proof terms exceed their limit".into(),
        ))
    } else {
        Ok(total)
    }
}

pub(super) fn decode_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the cohomology intervention artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

pub(super) fn encode_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
}

pub(super) fn encode_header(
    output: &mut Vec<u8>,
    artifact: &CohomologyInterventionArtifact,
) -> Result<()> {
    put_usize(output, artifact.vertex_count)?;
    put_usize(output, artifact.dimension)?;
    output.extend_from_slice(&artifact.scale.to_bits().to_be_bytes());
    output.extend_from_slice(&artifact.modulus.to_be_bytes());
    Ok(())
}

pub(super) fn encode_scenarios(
    output: &mut Vec<u8>,
    scenarios: &[CohomologyInterventionScenario],
) -> Result<()> {
    put_usize(output, scenarios.len())?;
    for scenario in scenarios {
        encode_edges(output, &scenario.active_edges)?;
        put_usize(output, scenario.target_basis)?;
    }
    Ok(())
}

pub(super) fn encode_candidates(
    output: &mut Vec<u8>,
    candidates: &[CohomologyInterventionCandidate],
) -> Result<()> {
    put_usize(output, candidates.len())?;
    for candidate in candidates {
        put_usize(output, candidate.edge.u)?;
        put_usize(output, candidate.edge.v)?;
        output.extend_from_slice(&candidate.cost.to_be_bytes());
    }
    Ok(())
}

pub(super) fn encode_search_data(
    output: &mut Vec<u8>,
    artifact: &CohomologyInterventionArtifact,
) -> Result<()> {
    put_usize(output, artifact.max_edits)?;
    put_usize(output, artifact.oracle_limit)?;
    put_usize(output, artifact.node_limit)?;
    output.push(artifact.status.code());
    encode_indices(output, &edit_indices(artifact))?;
    encode_optional_u64(output, artifact.lower_bound_cost);
    encode_optional_u64(output, artifact.upper_bound_cost);
    put_usize(output, artifact.oracle_calls)?;
    put_usize(output, artifact.search_nodes)?;
    put_usize(output, artifact.cache_hits)
}

pub(super) fn edit_indices(artifact: &CohomologyInterventionArtifact) -> Vec<usize> {
    artifact
        .edits
        .iter()
        .map(|edit| {
            artifact
                .candidates
                .binary_search(edit)
                .expect("validated edit")
        })
        .collect()
}

pub(super) fn encode_proof_data(
    output: &mut Vec<u8>,
    artifact: &CohomologyInterventionArtifact,
) -> Result<()> {
    put_usize(output, artifact.root_blockers.len())?;
    for blocker in &artifact.root_blockers {
        encode_indices(output, blocker)?;
    }
    output.extend_from_slice(&artifact.root_blocker_bound.to_be_bytes());
    encode_usizes(output, &artifact.before_ranks)?;
    encode_usizes(output, &artifact.after_ranks)
}

pub(super) fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdgeKey]) -> Result<()> {
    put_usize(output, edges.len())?;
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
    }
    Ok(())
}

pub(super) fn decode_edges(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<KineticEdgeKey>> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(Error::InvalidInput(
            "cohomology intervention edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(KineticEdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            })
        })
        .collect()
}

pub(super) fn encode_indices(output: &mut Vec<u8>, indices: &[usize]) -> Result<()> {
    encode_usizes(output, indices)
}

pub(super) fn decode_indices(
    reader: &mut Reader<'_>,
    candidates: usize,
    maximum: usize,
) -> Result<Vec<usize>> {
    let indices = decode_usizes(reader, maximum)?;
    if indices.iter().any(|position| *position >= candidates)
        || indices.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate indices are not canonical".into(),
        ));
    }
    Ok(indices)
}

pub(super) fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

pub(super) fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(Error::InvalidInput(
            "cohomology intervention integer list exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

pub(super) fn encode_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
        None => output.push(0),
    }
}

pub(super) fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| Error::InvalidInput("artifact integer does not fit u64".into()))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| Error::InvalidInput("artifact position overflows".into()))?;
        if end > self.bytes.len() {
            return Err(Error::InvalidInput(
                "cohomology intervention artifact is truncated".into(),
            ));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(Error::InvalidInput(format!(
                "cohomology intervention {name} exceeds its limit"
            )));
        }
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| Error::InvalidInput("artifact integer does not fit usize".into()))
    }
    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Error::InvalidInput(
                "cohomology intervention optional integer flag is invalid".into(),
            )),
        }
    }
    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
