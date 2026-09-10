use crate::monotone_proof::{BoundKind, ProofNode, ProofWork};
use crate::{CoverageSynthesisLimits, Error, Result};

use super::super::super::model::{DecodedCoverageProof, EvaluationClaim};
use super::super::super::model::{FORMAT_MAX_PROOF_NODES, FORMAT_MAX_PROOF_TERMS};
use super::reader::{Reader, decode_indices};

pub(in crate::coverage_synthesis::wire) fn decode_coverage_proof_data(
    reader: &mut Reader<'_>,
    action_count: usize,
    limits: CoverageSynthesisLimits,
) -> Result<DecodedCoverageProof> {
    let mut decoded_work = ProofWork::default();
    let root_blockers = decode_root_blockers(reader, action_count, &mut decoded_work, limits)?;
    let before = decode_evaluation(reader)?;
    let after = decode_evaluation(reader)?;
    let proof = decode_optional_proof(reader, action_count, &mut decoded_work, limits)?;
    let work = ProofWork {
        nodes: reader.usize()?,
        checks: reader.usize()?,
        terms: reader.usize()?,
    };
    if proof.is_some() && decoded_work.nodes != work.nodes {
        return Err(Error::InvalidInput(
            "coverage decoded proof node count differs from its claim".into(),
        ));
    }
    Ok(DecodedCoverageProof {
        root_blockers,
        before,
        after,
        proof,
        work,
    })
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("root blocker count", limits.max_proof_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_proof_terms)?;
        add_proof_terms(work, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn decode_optional_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<Option<ProofNode>> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_proof(reader, action_count, 0, work, limits).map(Some),
        _ => Err(Error::InvalidInput(
            "coverage proof-presence flag is invalid".into(),
        )),
    }
}

fn decode_evaluation(reader: &mut Reader<'_>) -> Result<EvaluationClaim> {
    let criterion_holds = match reader.u8()? {
        0 => false,
        1 => true,
        _ => {
            return Err(Error::InvalidInput(
                "coverage evaluation Boolean is invalid".into(),
            ));
        }
    };
    Ok(EvaluationClaim {
        criterion_holds,
        checks: reader.usize()?,
        minimum_witness_triangles: reader.optional_usize()?,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<ProofNode> {
    record_proof_node(work, depth, limits)?;
    let kind = reader.u8()?;
    decode_proof_kind(kind, reader, action_count, depth, work, limits)
}

fn decode_proof_kind(
    kind: u8,
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<ProofNode> {
    match kind {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingSelectionLimit),
        4 => Ok(ProofNode::BlockerBound {
            kind: decode_bound_kind(reader)?,
            blockers: decode_proof_blockers(reader, action_count, work, limits)?,
        }),
        5 => decode_branch_node(reader, action_count, depth, work, limits),
        _ => Err(Error::InvalidInput(
            "coverage proof node kind is invalid".into(),
        )),
    }
}

fn record_proof_node(
    work: &mut ProofWork,
    depth: usize,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    work.nodes = work
        .nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("coverage proof node count overflows".into()))?;
    if work.nodes > limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
        || depth > limits.max_proof_depth
    {
        Err(Error::InvalidInput(
            "coverage proof exceeds its node or depth limit".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_bound_kind(reader: &mut Reader<'_>) -> Result<BoundKind> {
    match reader.u8()? {
        1 => Ok(BoundKind::Cost),
        2 => Ok(BoundKind::Selections),
        _ => Err(Error::InvalidInput(
            "coverage proof bound kind is invalid".into(),
        )),
    }
}

fn decode_proof_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("proof blocker count", limits.max_proof_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_proof_terms)?;
        add_proof_terms(work, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn decode_branch_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<ProofNode> {
    let blocker = decode_indices(reader, action_count, limits.max_proof_terms)?;
    add_proof_terms(work, blocker.len(), limits)?;
    let count = reader.bounded_usize("proof child count", action_count)?;
    if count != blocker.len() {
        return Err(Error::InvalidInput(
            "coverage branch child count differs from its blocker".into(),
        ));
    }
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        children.push(decode_proof(reader, action_count, depth + 1, work, limits)?);
    }
    Ok(ProofNode::Branch { blocker, children })
}

fn add_proof_terms(
    work: &mut ProofWork,
    count: usize,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    work.terms = work
        .terms
        .checked_add(count)
        .ok_or_else(|| Error::InvalidInput("coverage proof term count overflows".into()))?;
    if work.terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS) {
        Err(Error::InvalidInput(
            "coverage proof terms exceed their limit".into(),
        ))
    } else {
        Ok(())
    }
}
