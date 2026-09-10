use sha2::{Digest, Sha256};

use crate::{CohomologySpaceId, Error, KineticEdgeKey, Result};

use super::model::{
    ProducerWork, ProofData, SearchData, SelectionData, SynthesisAction, SynthesisArtifact,
    SynthesisCoordinate, SynthesisHeader, SynthesisLimits, SynthesisState, SynthesisStatus,
    TopologicalSpecification, WorkLimits,
};
use super::proof_wire::{
    decode_optional_proof, decode_root_blockers, encode_optional_proof, encode_root_blockers,
};
use super::source_wire::{
    Reader, add_proof_terms, decode_edges, decode_indices, decode_source, decode_usizes,
    encode_edges, encode_optional_u64, encode_source, encode_usizes, put_usize,
};
use super::{F64_BITS_CODEC, FORMAT_MAX_ACTIONS, FORMAT_MAX_STATES, MAGIC, VERSION};

impl SynthesisArtifact {
    /// Encode canonical `HOLOSSYN` version 1 bytes.
    pub fn encode(&self, limits: SynthesisLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "synthesis artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSSYN` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: SynthesisLimits) -> Result<Self> {
        validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_prefix(&mut reader)?;
        let header = decode_header(&mut reader, limits)?;
        let states = decode_states(&mut reader, limits)?;
        let actions = decode_actions(&mut reader, states.len(), limits)?;
        let search = decode_search_data(&mut reader, actions.len())?;
        let proof = decode_proof_data(&mut reader, actions.len(), states.len(), limits)?;
        let digest = decode_trailer(&mut reader)?;
        let artifact = Self {
            specification: TopologicalSpecification {
                vertex_count: header.vertex_count,
                dimension: header.dimension,
                scale: header.scale,
                modulus: header.modulus,
                source: header.source,
                states,
            },
            actions,
            max_edits: search.max_edits,
            oracle_limit: search.limits.oracle,
            node_limit: search.limits.nodes,
            status: search.selection.status,
            selected: search.selection.selected,
            lower_bound_cost: search.selection.lower_bound_cost,
            upper_bound_cost: search.selection.upper_bound_cost,
            producer_oracle_calls: search.work.oracle_calls,
            producer_search_nodes: search.work.search_nodes,
            producer_cache_hits: search.work.cache_hits,
            root_blockers: proof.root_blockers,
            before_ranks: proof.before_ranks,
            after_ranks: proof.after_ranks,
            proof: proof.proof,
            proof_nodes: proof.nodes,
            proof_topology_checks: proof.topology_checks,
            digest,
        };
        validate_decoded_artifact(&artifact, bytes, limits)?;
        Ok(artifact)
    }

    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = self.encode_payload()?;
        let mut hash = Sha256::new();
        hash.update(b"holos-synthesis-artifact-v1");
        hash.update(payload);
        Ok(hash.finalize().into())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        encode_prefix(&mut output);
        encode_specification(&mut output, &self.specification)?;
        encode_actions(&mut output, &self.actions)?;
        encode_search_data(&mut output, self)?;
        encode_proof_data(&mut output, self)?;
        Ok(output)
    }
}

fn validate_artifact_size(bytes: &[u8], limits: SynthesisLimits) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "synthesis artifact exceeds its byte limit or is truncated".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<()> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(Error::InvalidInput("unsupported synthesis artifact".into()))
    } else {
        Ok(())
    }
}

fn decode_header(reader: &mut Reader<'_>, limits: SynthesisLimits) -> Result<SynthesisHeader> {
    Ok(SynthesisHeader {
        vertex_count: reader.bounded_usize("vertex count", limits.max_vertices)?,
        dimension: reader.bounded_usize("dimension", limits.cohomology.max_dimension)?,
        scale: f64::from_bits(reader.u64()?),
        modulus: reader.u32()?,
        source: decode_source(reader, limits)?,
    })
}

fn decode_states(reader: &mut Reader<'_>, limits: SynthesisLimits) -> Result<Vec<SynthesisState>> {
    let count = reader.bounded_usize("state count", limits.max_states.min(FORMAT_MAX_STATES))?;
    let mut states = Vec::with_capacity(count);
    let mut target_terms = 0usize;
    for _ in 0..count {
        states.push(decode_state(reader, &mut target_terms, limits)?);
    }
    Ok(states)
}

fn decode_state(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<SynthesisState> {
    Ok(SynthesisState {
        scenario: reader.u64()?,
        step: reader.u64()?,
        active_edges: decode_edges(reader, limits.max_edges_per_state)?,
        target_space: CohomologySpaceId::from_bytes(reader.array32()?),
        target: decode_target(reader, target_terms, limits)?,
        max_surviving_rank: reader.usize()?,
    })
}

fn decode_target(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<SynthesisCoordinate>>> {
    let count = reader.bounded_usize("target row count", limits.max_terms)?;
    let mut target = Vec::with_capacity(count);
    for _ in 0..count {
        target.push(decode_target_row(reader, target_terms, limits)?);
    }
    Ok(target)
}

fn decode_target_row(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<SynthesisCoordinate>> {
    let count = reader.bounded_usize("target row term count", limits.max_terms)?;
    add_proof_terms(target_terms, count, limits.max_terms, "synthesis target")?;
    if count > reader.remaining() / 12 {
        return Err(Error::InvalidInput(
            "synthesis target terms exceed their limit or remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(SynthesisCoordinate {
                basis: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_actions(
    reader: &mut Reader<'_>,
    state_count: usize,
    limits: SynthesisLimits,
) -> Result<Vec<SynthesisAction>> {
    let count = reader.bounded_usize("action count", limits.max_actions.min(FORMAT_MAX_ACTIONS))?;
    let mut actions = Vec::with_capacity(count);
    for _ in 0..count {
        actions.push(decode_action(reader, state_count)?);
    }
    Ok(actions)
}

fn decode_action(reader: &mut Reader<'_>, state_count: usize) -> Result<SynthesisAction> {
    Ok(SynthesisAction {
        edge: KineticEdgeKey {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
        states: decode_indices(reader, state_count, state_count)?,
    })
}

fn decode_search_data(reader: &mut Reader<'_>, action_count: usize) -> Result<SearchData> {
    let max_edits = reader.usize()?;
    Ok(SearchData {
        max_edits,
        limits: decode_work_limits(reader)?,
        selection: decode_selection_data(reader, action_count)?,
        work: decode_producer_work(reader)?,
    })
}

fn decode_work_limits(reader: &mut Reader<'_>) -> Result<WorkLimits> {
    Ok(WorkLimits {
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    })
}

fn decode_selection_data(reader: &mut Reader<'_>, action_count: usize) -> Result<SelectionData> {
    Ok(SelectionData {
        status: SynthesisStatus::from_code(reader.u8()?)?,
        selected: decode_indices(reader, action_count, action_count)?,
        lower_bound_cost: reader.optional_u64()?,
        upper_bound_cost: reader.optional_u64()?,
    })
}

fn decode_producer_work(reader: &mut Reader<'_>) -> Result<ProducerWork> {
    Ok(ProducerWork {
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

fn decode_proof_data(
    reader: &mut Reader<'_>,
    action_count: usize,
    state_count: usize,
    limits: SynthesisLimits,
) -> Result<ProofData> {
    let mut terms = 0usize;
    let root_blockers = decode_root_blockers(reader, action_count, &mut terms, limits)?;
    let before_ranks = decode_usizes(reader, state_count)?;
    let after_ranks = decode_usizes(reader, state_count)?;
    let mut nodes = 0usize;
    let proof = decode_optional_proof(reader, action_count, &mut nodes, &mut terms, limits)?;
    let claimed_nodes = reader.usize()?;
    let topology_checks = reader.usize()?;
    if claimed_nodes != nodes {
        return Err(Error::InvalidInput(
            "synthesis proof count or digest differs from its content".into(),
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

fn decode_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the synthesis artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

fn validate_decoded_artifact(
    artifact: &SynthesisArtifact,
    bytes: &[u8],
    limits: SynthesisLimits,
) -> Result<()> {
    if artifact.compute_digest()? != artifact.digest {
        return Err(Error::InvalidInput(
            "synthesis proof count or digest differs from its content".into(),
        ));
    }
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "synthesis artifact encoding is not canonical".into(),
        ));
    }
    Ok(())
}

fn encode_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
}

fn encode_specification(
    output: &mut Vec<u8>,
    specification: &TopologicalSpecification,
) -> Result<()> {
    put_usize(output, specification.vertex_count)?;
    put_usize(output, specification.dimension)?;
    output.extend_from_slice(&specification.scale.to_bits().to_be_bytes());
    output.extend_from_slice(&specification.modulus.to_be_bytes());
    encode_source(output, &specification.source)?;
    put_usize(output, specification.states.len())?;
    for state in &specification.states {
        encode_state(output, state)?;
    }
    Ok(())
}

fn encode_state(output: &mut Vec<u8>, state: &SynthesisState) -> Result<()> {
    output.extend_from_slice(&state.scenario.to_be_bytes());
    output.extend_from_slice(&state.step.to_be_bytes());
    encode_edges(output, &state.active_edges)?;
    output.extend_from_slice(state.target_space.as_bytes());
    encode_target(output, &state.target)?;
    put_usize(output, state.max_surviving_rank)
}

fn encode_target(output: &mut Vec<u8>, target: &[Vec<SynthesisCoordinate>]) -> Result<()> {
    put_usize(output, target.len())?;
    for row in target {
        encode_target_row(output, row)?;
    }
    Ok(())
}

fn encode_target_row(output: &mut Vec<u8>, row: &[SynthesisCoordinate]) -> Result<()> {
    put_usize(output, row.len())?;
    for term in row {
        put_usize(output, term.basis)?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn encode_actions(output: &mut Vec<u8>, actions: &[SynthesisAction]) -> Result<()> {
    put_usize(output, actions.len())?;
    for action in actions {
        encode_action(output, action)?;
    }
    Ok(())
}

fn encode_action(output: &mut Vec<u8>, action: &SynthesisAction) -> Result<()> {
    put_usize(output, action.edge.u)?;
    put_usize(output, action.edge.v)?;
    output.extend_from_slice(&action.cost.to_be_bytes());
    encode_usizes(output, &action.states)
}

fn encode_search_data(output: &mut Vec<u8>, artifact: &SynthesisArtifact) -> Result<()> {
    put_usize(output, artifact.max_edits)?;
    put_usize(output, artifact.oracle_limit)?;
    put_usize(output, artifact.node_limit)?;
    output.push(artifact.status.code());
    encode_usizes(output, &artifact.selected)?;
    encode_optional_u64(output, artifact.lower_bound_cost);
    encode_optional_u64(output, artifact.upper_bound_cost);
    put_usize(output, artifact.producer_oracle_calls)?;
    put_usize(output, artifact.producer_search_nodes)?;
    put_usize(output, artifact.producer_cache_hits)
}

fn encode_proof_data(output: &mut Vec<u8>, artifact: &SynthesisArtifact) -> Result<()> {
    encode_root_blockers(output, &artifact.root_blockers)?;
    encode_usizes(output, &artifact.before_ranks)?;
    encode_usizes(output, &artifact.after_ranks)?;
    encode_optional_proof(output, artifact.proof.as_ref())?;
    put_usize(output, artifact.proof_nodes)?;
    put_usize(output, artifact.proof_topology_checks)
}
