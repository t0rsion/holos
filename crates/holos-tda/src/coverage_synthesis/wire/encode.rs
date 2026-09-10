//! Canonical coverage artifact encoding.

use crate::monotone_proof::{BoundKind, ProofNode};
use crate::{CoverageSource, Error, KineticEdge, KineticEdgeKey, Result};

use super::super::model::{
    CoverageAction, CoverageSpecification, CoverageState, CoverageSynthesisArtifact,
    EvaluationClaim,
};
use super::{F64_BITS_CODEC, MAGIC, VERSION};

pub(super) fn encode_coverage_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
}

pub(super) fn encode_coverage_specification(
    output: &mut Vec<u8>,
    specification: &CoverageSpecification,
) -> Result<()> {
    encode_coverage_header(output, specification)?;
    put_usize(output, specification.states.len())?;
    for state in &specification.states {
        encode_coverage_state(output, state)?;
    }
    Ok(())
}

fn encode_coverage_header(
    output: &mut Vec<u8>,
    specification: &CoverageSpecification,
) -> Result<()> {
    put_usize(output, specification.vertex_count)?;
    output.extend_from_slice(
        &specification
            .model
            .broadcast_radius()
            .to_bits()
            .to_be_bytes(),
    );
    output.extend_from_slice(&specification.model.sensing_radius().to_bits().to_be_bytes());
    output.extend_from_slice(&specification.modulus.to_be_bytes());
    encode_usizes(output, specification.fence.vertices())?;
    encode_usizes(output, &specification.failable_vertices)?;
    put_usize(output, specification.failure_budget)?;
    encode_source(output, &specification.source)
}

fn encode_coverage_state(output: &mut Vec<u8>, state: &CoverageState) -> Result<()> {
    output.extend_from_slice(&state.scenario.to_be_bytes());
    output.extend_from_slice(&state.step.to_be_bytes());
    encode_usizes(output, &state.base_vertices)?;
    encode_edges(output, &state.possible_edges)
}

pub(super) fn encode_coverage_actions(
    output: &mut Vec<u8>,
    actions: &[CoverageAction],
) -> Result<()> {
    put_usize(output, actions.len())?;
    for action in actions {
        encode_coverage_action(output, action)?;
    }
    Ok(())
}

fn encode_coverage_action(output: &mut Vec<u8>, action: &CoverageAction) -> Result<()> {
    put_usize(output, action.vertex)?;
    output.extend_from_slice(&action.cost.to_be_bytes());
    encode_usizes(output, &action.states)
}

pub(super) fn encode_coverage_search(
    output: &mut Vec<u8>,
    artifact: &CoverageSynthesisArtifact,
) -> Result<()> {
    put_usize(output, artifact.max_activations)?;
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

pub(super) fn encode_coverage_proof_data(
    output: &mut Vec<u8>,
    artifact: &CoverageSynthesisArtifact,
) -> Result<()> {
    encode_coverage_root_blockers(output, &artifact.root_blockers)?;
    encode_evaluation(output, artifact.before)?;
    encode_evaluation(output, artifact.after)?;
    encode_optional_coverage_proof(output, artifact.proof.as_ref())?;
    put_usize(output, artifact.proof_work.nodes)?;
    put_usize(output, artifact.proof_work.checks)?;
    put_usize(output, artifact.proof_work.terms)
}

fn encode_coverage_root_blockers(output: &mut Vec<u8>, blockers: &[Vec<usize>]) -> Result<()> {
    put_usize(output, blockers.len())?;
    for blocker in blockers {
        encode_usizes(output, blocker)?;
    }
    Ok(())
}

fn encode_optional_coverage_proof(output: &mut Vec<u8>, proof: Option<&ProofNode>) -> Result<()> {
    match proof {
        Some(proof) => {
            output.push(1);
            encode_proof(output, proof)
        }
        None => {
            output.push(0);
            Ok(())
        }
    }
}

fn encode_source(output: &mut Vec<u8>, source: &CoverageSource) -> Result<()> {
    match source {
        CoverageSource::Finite => output.push(0),
        CoverageSource::Affine { .. } => encode_affine_source(output, source)?,
    }
    Ok(())
}

fn encode_affine_source(output: &mut Vec<u8>, source: &CoverageSource) -> Result<()> {
    let CoverageSource::Affine {
        scenario,
        edges,
        start,
        end,
    } = source
    else {
        return Ok(());
    };
    output.push(1);
    output.extend_from_slice(&scenario.to_be_bytes());
    output.extend_from_slice(&start.to_bits().to_be_bytes());
    output.extend_from_slice(&end.to_bits().to_be_bytes());
    put_usize(output, edges.len())?;
    for edge in edges {
        encode_affine_edge(output, edge)?;
    }
    Ok(())
}

fn encode_affine_edge(output: &mut Vec<u8>, edge: &KineticEdge) -> Result<()> {
    put_usize(output, edge.u)?;
    put_usize(output, edge.v)?;
    output.extend_from_slice(&edge.intercept.to_bits().to_be_bytes());
    output.extend_from_slice(&edge.velocity.to_bits().to_be_bytes());
    Ok(())
}

fn encode_evaluation(output: &mut Vec<u8>, claim: EvaluationClaim) -> Result<()> {
    output.push(u8::from(claim.criterion_holds));
    put_usize(output, claim.checks)?;
    encode_optional_usize(output, claim.minimum_witness_triangles)?;
    Ok(())
}

fn encode_proof(output: &mut Vec<u8>, proof: &ProofNode) -> Result<()> {
    match proof {
        ProofNode::Cost => output.push(1),
        ProofNode::SurvivingMaximum => output.push(2),
        ProofNode::SurvivingSelectionLimit => output.push(3),
        ProofNode::BlockerBound { kind, blockers } => encode_bound_node(output, *kind, blockers)?,
        ProofNode::Branch { blocker, children } => encode_branch_node(output, blocker, children)?,
    }
    Ok(())
}

fn encode_bound_node(output: &mut Vec<u8>, kind: BoundKind, blockers: &[Vec<usize>]) -> Result<()> {
    output.push(4);
    output.push(match kind {
        BoundKind::Cost => 1,
        BoundKind::Selections => 2,
    });
    encode_coverage_root_blockers(output, blockers)
}

fn encode_branch_node(
    output: &mut Vec<u8>,
    blocker: &[usize],
    children: &[ProofNode],
) -> Result<()> {
    output.push(5);
    encode_usizes(output, blocker)?;
    put_usize(output, children.len())?;
    for child in children {
        encode_proof(output, child)?;
    }
    Ok(())
}

fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdgeKey]) -> Result<()> {
    put_usize(output, edges.len())?;
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
    }
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

fn encode_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
        None => output.push(0),
    }
}

fn encode_optional_usize(output: &mut Vec<u8>, value: Option<usize>) -> Result<()> {
    match value {
        Some(value) => {
            output.push(1);
            put_usize(output, value)?;
        }
        None => output.push(0),
    }
    Ok(())
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| Error::InvalidInput("coverage integer does not fit u64".into()))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}
