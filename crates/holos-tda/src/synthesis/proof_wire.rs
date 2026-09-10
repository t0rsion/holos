use crate::{Error, Result};

use super::model::{BoundKind, ProofNode, SynthesisLimits};
use super::source_wire::{Reader, add_proof_terms, decode_indices, encode_usizes, put_usize};
use super::{FORMAT_MAX_PROOF_NODES, FORMAT_MAX_PROOF_TERMS};

pub(super) fn encode_root_blockers(output: &mut Vec<u8>, blockers: &[Vec<usize>]) -> Result<()> {
    put_usize(output, blockers.len())?;
    for blocker in blockers {
        encode_usizes(output, blocker)?;
    }
    Ok(())
}

pub(super) fn encode_optional_proof(output: &mut Vec<u8>, proof: Option<&ProofNode>) -> Result<()> {
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
pub(super) fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_proof_terms(terms, blocker.len(), limits.max_terms, "synthesis proof")?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

pub(super) fn decode_optional_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Option<ProofNode>> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_proof(reader, action_count, 0, nodes, terms, limits).map(Some),
        _ => Err(Error::InvalidInput(
            "synthesis proof-presence flag is invalid".into(),
        )),
    }
}

pub(super) fn encode_proof(output: &mut Vec<u8>, proof: &ProofNode) -> Result<()> {
    match proof {
        ProofNode::Cost => output.push(1),
        ProofNode::SurvivingMaximum => output.push(2),
        ProofNode::SurvivingEditLimit => output.push(3),
        ProofNode::BlockerBound { kind, blockers } => encode_bound_node(output, *kind, blockers)?,
        ProofNode::Branch { blocker, children } => encode_branch_node(output, blocker, children)?,
    }
    Ok(())
}

pub(super) fn encode_bound_node(
    output: &mut Vec<u8>,
    kind: BoundKind,
    blockers: &[Vec<usize>],
) -> Result<()> {
    output.push(4);
    output.push(match kind {
        BoundKind::Cost => 1,
        BoundKind::Edits => 2,
    });
    encode_root_blockers(output, blockers)
}

pub(super) fn encode_branch_node(
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

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    record_decoded_node(nodes, depth, limits)?;
    match reader.u8()? {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingEditLimit),
        4 => decode_bound_node(reader, action_count, terms, limits),
        5 => decode_branch_node(reader, action_count, depth, nodes, terms, limits),
        _ => Err(Error::InvalidInput(
            "synthesis proof node kind is invalid".into(),
        )),
    }
}

pub(super) fn record_decoded_node(
    nodes: &mut usize,
    depth: usize,
    limits: SynthesisLimits,
) -> Result<()> {
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
    if *nodes > limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES) || depth > limits.max_proof_depth
    {
        Err(Error::InvalidInput(
            "synthesis proof tree exceeds its node or depth limit".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_bound_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    Ok(ProofNode::BlockerBound {
        kind: decode_bound_kind(reader)?,
        blockers: decode_proof_blockers(reader, action_count, terms, limits)?,
    })
}

pub(super) fn decode_bound_kind(reader: &mut Reader<'_>) -> Result<BoundKind> {
    match reader.u8()? {
        1 => Ok(BoundKind::Cost),
        2 => Ok(BoundKind::Edits),
        _ => Err(Error::InvalidInput(
            "synthesis proof bound kind is invalid".into(),
        )),
    }
}

pub(super) fn decode_proof_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("proof blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_proof_terms(
            terms,
            blocker.len(),
            limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
            "synthesis proof",
        )?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_branch_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    let blocker = decode_indices(reader, action_count, limits.max_terms)?;
    add_proof_terms(
        terms,
        blocker.len(),
        limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
        "synthesis proof",
    )?;
    let count = reader.bounded_usize("proof child count", action_count)?;
    if count != blocker.len() {
        return Err(Error::InvalidInput(
            "synthesis branch child count differs from its blocker".into(),
        ));
    }
    let children =
        decode_proof_children(reader, action_count, count, depth + 1, nodes, terms, limits)?;
    Ok(ProofNode::Branch { blocker, children })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_proof_children(
    reader: &mut Reader<'_>,
    action_count: usize,
    count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<ProofNode>> {
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        children.push(decode_proof(
            reader,
            action_count,
            depth,
            nodes,
            terms,
            limits,
        )?);
    }
    Ok(children)
}
