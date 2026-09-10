use super::super::{
    CollapseCertificate, CollapseCompleteness, CollapseObjective, RemovalStep, SchedulePosition,
};
use super::model::{ArtifactError, CollapseArtifact};
use super::primitives::{put_optional_f64, put_optional_u64, put_u16, put_u32, put_u64, put_usize};
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(super) fn encode_header(
    out: &mut Vec<u8>,
    artifact: &CollapseArtifact,
    output_count: usize,
) -> Result<(), ArtifactError> {
    encode_prefix(out);
    encode_metadata_prefix(out, &artifact.certificate);
    encode_graph_counts(out, &artifact.certificate)?;
    put_optional_u64(out, artifact.certificate.work_limit());
    put_u64(out, artifact.certificate.work_used());
    out.extend_from_slice(&artifact.input_digest);
    out.extend_from_slice(&artifact.output_digest);
    put_usize(out, artifact.certificate.steps().len(), "step count")?;
    put_usize(out, output_count, "output edge count")
}

fn encode_prefix(out: &mut Vec<u8>) {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
}

fn encode_metadata_prefix(out: &mut Vec<u8>, certificate: &CollapseCertificate) {
    put_u32(out, certificate.algorithm_version());
    out.push(objective_tag(certificate.objective()));
    out.push(completeness_tag(certificate.completeness()));
    put_optional_f64(out, certificate.requested_threshold());
    put_u64(out, certificate.terminal_level().to_bits());
}

fn objective_tag(objective: Option<CollapseObjective>) -> u8 {
    match objective {
        None => 0,
        Some(CollapseObjective::H1) => 1,
        Some(CollapseObjective::H2) => 2,
    }
}

fn completeness_tag(completeness: CollapseCompleteness) -> u8 {
    match completeness {
        CollapseCompleteness::CompleteFixedPoint => 0,
        CollapseCompleteness::BudgetLimited => 1,
    }
}

fn encode_graph_counts(
    out: &mut Vec<u8>,
    certificate: &CollapseCertificate,
) -> Result<(), ArtifactError> {
    put_usize(out, certificate.vertex_count(), "vertex count")?;
    put_usize(out, certificate.input_edge_count(), "input edge count")?;
    put_usize(out, certificate.output_edge_count(), "output edge count")
}

pub(super) fn encode_output_edges(
    out: &mut Vec<u8>,
    edges: &[(usize, usize, f64)],
) -> Result<(), ArtifactError> {
    for &(u, v, value) in edges {
        put_usize(out, u, "edge endpoint")?;
        put_usize(out, v, "edge endpoint")?;
        put_u64(out, value.to_bits());
    }
    Ok(())
}

pub(super) fn encode_steps(out: &mut Vec<u8>, steps: &[RemovalStep]) -> Result<(), ArtifactError> {
    for step in steps {
        encode_step(out, step)?;
    }
    Ok(())
}

fn encode_step(out: &mut Vec<u8>, step: &RemovalStep) -> Result<(), ArtifactError> {
    let (u, v) = step.edge();
    put_usize(out, u, "step endpoint")?;
    put_usize(out, v, "step endpoint")?;
    put_u64(out, step.value().to_bits());
    let (kind, number) = encode_schedule_position(step.position());
    out.push(kind);
    put_usize(out, number, "schedule position")?;
    encode_witnesses(out, step.witnesses())
}

fn encode_schedule_position(position: SchedulePosition) -> (u8, usize) {
    match position {
        SchedulePosition::Pass(number) => (1, number),
        SchedulePosition::Round(number) => (2, number),
        SchedulePosition::Sequence(number) => (3, number),
    }
}

fn encode_witnesses(out: &mut Vec<u8>, witnesses: &[(f64, usize)]) -> Result<(), ArtifactError> {
    put_usize(out, witnesses.len(), "witness count")?;
    for &(start, apex) in witnesses {
        put_u64(out, start.to_bits());
        put_usize(out, apex, "witness apex")?;
    }
    Ok(())
}
