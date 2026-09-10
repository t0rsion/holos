use super::super::{
    CollapseCertificate, CollapseCompleteness, CollapseObjective, RemovalStep, SchedulePosition,
};
use super::model::{
    ArtifactCounts, ArtifactError, ArtifactHeader, ArtifactMetadata, ArtifactMetadataPrefix,
    ArtifactTail, CollapseArtifact, DecodeLimits,
};
use super::primitives::Reader;
use super::validation::{canonical_output, reconstruct_input, validate_edges};
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};
use crate::SparseDistanceMatrix;

pub(super) fn validate_artifact_size(
    bytes: &[u8],
    limits: DecodeLimits,
) -> Result<(), ArtifactError> {
    if bytes.len() > limits.max_bytes {
        Err(ArtifactError::new(format!(
            "{} bytes exceed the decoder limit {}",
            bytes.len(),
            limits.max_bytes
        )))
    } else {
        Ok(())
    }
}

pub(super) fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ArtifactError> {
    if reader.take(8)? != MAGIC {
        return Err(ArtifactError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(ArtifactError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(ArtifactError::new(format!(
            "unsupported scalar codec {codec}"
        )));
    }
    Ok(())
}

pub(super) fn decode_header(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<ArtifactHeader, ArtifactError> {
    let prefix = decode_metadata_prefix(reader)?;
    let tail = decode_header_tail(reader, limits)?;
    Ok(ArtifactHeader {
        metadata: ArtifactMetadata {
            algorithm_version: prefix.algorithm_version,
            objective: prefix.objective,
            completeness: prefix.completeness,
            requested_threshold: prefix.requested_threshold,
            terminal_level: prefix.terminal_level,
            work_limit: tail.work_limit,
            work_used: tail.work_used,
        },
        counts: tail.counts,
        input_digest: tail.input_digest,
        output_digest: tail.output_digest,
    })
}

fn decode_metadata_prefix(
    reader: &mut Reader<'_>,
) -> Result<ArtifactMetadataPrefix, ArtifactError> {
    Ok(ArtifactMetadataPrefix {
        algorithm_version: reader.u32()?,
        objective: decode_objective(reader.u8()?)?,
        completeness: decode_completeness(reader.u8()?)?,
        requested_threshold: reader.optional_f64()?,
        terminal_level: f64::from_bits(reader.u64()?),
    })
}

fn decode_objective(tag: u8) -> Result<Option<CollapseObjective>, ArtifactError> {
    match tag {
        0 => Ok(None),
        1 => Ok(Some(CollapseObjective::H1)),
        2 => Ok(Some(CollapseObjective::H2)),
        _ => Err(ArtifactError::new(format!("unknown objective tag {tag}"))),
    }
}

fn decode_completeness(tag: u8) -> Result<CollapseCompleteness, ArtifactError> {
    match tag {
        0 => Ok(CollapseCompleteness::CompleteFixedPoint),
        1 => Ok(CollapseCompleteness::BudgetLimited),
        _ => Err(ArtifactError::new(format!(
            "unknown completeness tag {tag}"
        ))),
    }
}

fn decode_header_tail(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<ArtifactTail, ArtifactError> {
    let (vertex_count, input_edges, output_edges) = decode_graph_counts(reader, limits)?;
    let work_limit = reader.optional_u64()?;
    let work_used = reader.u64()?;
    let input_digest = reader.array32()?;
    let output_digest = reader.array32()?;
    let steps = reader.bounded_usize("step count", limits.max_steps)?;
    let encoded_output = reader.bounded_usize("encoded output edge count", limits.max_edges)?;
    validate_record_counts(input_edges, output_edges, encoded_output, steps)?;
    Ok(ArtifactTail {
        counts: ArtifactCounts {
            vertex_count,
            input_edges,
            output_edges,
            steps,
        },
        work_limit,
        work_used,
        input_digest,
        output_digest,
    })
}

fn decode_graph_counts(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<(usize, usize, usize), ArtifactError> {
    Ok((
        reader.bounded_usize("vertex count", limits.max_vertices)?,
        reader.bounded_usize("input edge count", limits.max_edges)?,
        reader.bounded_usize("output edge count", limits.max_edges)?,
    ))
}

fn validate_record_counts(
    input: usize,
    output: usize,
    encoded_output: usize,
    steps: usize,
) -> Result<(), ArtifactError> {
    if encoded_output != output {
        return Err(ArtifactError::new(format!(
            "encoded output edge count {encoded_output} differs from header {output}"
        )));
    }
    if input != output.saturating_add(steps) {
        return Err(ArtifactError::new(format!(
            "input edge count {input} differs from output {output} plus {steps} steps"
        )));
    }
    Ok(())
}

pub(super) fn validate_minimum_records(
    reader: &Reader<'_>,
    counts: &ArtifactCounts,
) -> Result<(), ArtifactError> {
    let minimum = counts
        .output_edges
        .checked_mul(24)
        .and_then(|bytes| {
            counts
                .steps
                .checked_mul(41)
                .and_then(|steps| bytes.checked_add(steps))
        })
        .ok_or_else(|| ArtifactError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        Err(ArtifactError::new(format!(
            "record counts need at least {minimum} bytes, only {} remain",
            reader.remaining()
        )))
    } else {
        Ok(())
    }
}

pub(super) fn decode_output_edges(
    reader: &mut Reader<'_>,
    counts: &ArtifactCounts,
) -> Result<Vec<(usize, usize, f64)>, ArtifactError> {
    let mut output = Vec::with_capacity(counts.output_edges);
    for _ in 0..counts.output_edges {
        output.push((
            reader.usize()?,
            reader.usize()?,
            f64::from_bits(reader.u64()?),
        ));
    }
    validate_edges(counts.vertex_count, &output, "encoded output")?;
    Ok(output)
}

pub(super) fn decode_steps(
    reader: &mut Reader<'_>,
    counts: &ArtifactCounts,
    limits: DecodeLimits,
) -> Result<Vec<RemovalStep>, ArtifactError> {
    let mut steps = Vec::with_capacity(counts.steps);
    let mut witnesses = 0usize;
    for _ in 0..counts.steps {
        steps.push(decode_step(reader, &mut witnesses, limits)?);
    }
    Ok(steps)
}

fn decode_step(
    reader: &mut Reader<'_>,
    witness_total: &mut usize,
    limits: DecodeLimits,
) -> Result<RemovalStep, ArtifactError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    let value = f64::from_bits(reader.u64()?);
    let position = decode_schedule_position(reader.u8()?, reader.usize()?)?;
    let witness_count = reader.usize()?;
    record_witness_count(reader, witness_total, witness_count, limits)?;
    Ok(RemovalStep {
        u,
        v,
        value,
        position,
        witnesses: decode_witnesses(reader, witness_count)?,
    })
}

fn decode_schedule_position(tag: u8, number: usize) -> Result<SchedulePosition, ArtifactError> {
    match tag {
        1 => Ok(SchedulePosition::Pass(number)),
        2 => Ok(SchedulePosition::Round(number)),
        3 => Ok(SchedulePosition::Sequence(number)),
        _ => Err(ArtifactError::new(format!(
            "unknown schedule-position tag {tag}"
        ))),
    }
}

fn record_witness_count(
    reader: &Reader<'_>,
    total: &mut usize,
    count: usize,
    limits: DecodeLimits,
) -> Result<(), ArtifactError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ArtifactError::new("witness count overflows usize"))?;
    if *total > limits.max_witness_segments {
        return Err(ArtifactError::new(format!(
            "{} witness segments exceed the decoder limit {}",
            *total, limits.max_witness_segments
        )));
    }
    let bytes = count
        .checked_mul(16)
        .ok_or_else(|| ArtifactError::new("witness bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ArtifactError::new(format!(
            "{count} witness segments need {bytes} bytes, only {} remain",
            reader.remaining()
        )));
    }
    Ok(())
}

fn decode_witnesses(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<(f64, usize)>, ArtifactError> {
    (0..count)
        .map(|_| Ok((f64::from_bits(reader.u64()?), reader.usize()?)))
        .collect()
}

pub(super) fn finish_decode(reader: &Reader<'_>) -> Result<(), ArtifactError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(ArtifactError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )))
    }
}

pub(super) fn assemble_artifact(
    header: ArtifactHeader,
    output: Vec<(usize, usize, f64)>,
    steps: Vec<RemovalStep>,
) -> Result<CollapseArtifact, ArtifactError> {
    let matrix = SparseDistanceMatrix::from_triplets(header.counts.vertex_count, &output)
        .map_err(|error| ArtifactError::new(error.to_string()))?;
    let certificate = decoded_certificate(&header, steps);
    validate_decoded_bindings(&matrix, &certificate, &header)?;
    Ok(CollapseArtifact {
        matrix,
        certificate,
        input_digest: header.input_digest,
        output_digest: header.output_digest,
    })
}

fn decoded_certificate(header: &ArtifactHeader, steps: Vec<RemovalStep>) -> CollapseCertificate {
    CollapseCertificate {
        algorithm_version: header.metadata.algorithm_version,
        objective: header.metadata.objective,
        completeness: header.metadata.completeness,
        work_limit: header.metadata.work_limit,
        work_used: header.metadata.work_used,
        vertex_count: header.counts.vertex_count,
        requested_threshold: header.metadata.requested_threshold,
        terminal_level: header.metadata.terminal_level,
        input_edge_count: header.counts.input_edges,
        output_edge_count: header.counts.output_edges,
        steps,
    }
}

fn validate_decoded_bindings(
    matrix: &SparseDistanceMatrix,
    certificate: &CollapseCertificate,
    header: &ArtifactHeader,
) -> Result<(), ArtifactError> {
    let output = canonical_output(matrix)?;
    let input = reconstruct_input(&output, certificate)?;
    if super::primitives::graph_digest(header.counts.vertex_count, &input) != header.input_digest {
        return Err(ArtifactError::new("input graph binding does not match"));
    }
    if super::primitives::graph_digest(header.counts.vertex_count, &output) != header.output_digest
    {
        return Err(ArtifactError::new("output graph binding does not match"));
    }
    Ok(())
}
