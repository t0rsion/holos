use super::Result;
use super::model::{
    EdgeWeightEdit, InterventionArtifact, InterventionDecodeLimits, InterventionError,
    InterventionStatus,
};
use crate::{CertificateLimits, EdgeKey, IntervalGroupId, ProgramTraceArtifact};

const MAGIC: &[u8; 8] = b"HOLOSINT";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

impl InterventionArtifact {
    /// Encode the canonical `HOLOSINT` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, InterventionError> {
        self.check_shape()?;
        let trace = encode_trace(&self.trace)?;
        let mut out = Vec::new();
        encode_intervention_header(&mut out, self, trace.len())?;
        encode_edits(&mut out, &self.edits)?;
        out.extend_from_slice(&trace);
        Ok(out)
    }

    /// Decode and structurally validate a bounded intervention envelope.
    pub fn decode(
        bytes: &[u8],
        limits: InterventionDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, InterventionError> {
        check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_intervention_header(&mut reader, limits)?;
        check_record_bytes(&reader, header.edit_count, header.trace_bytes)?;
        let edits = decode_edits(&mut reader, header.edit_count)?;
        let trace = decode_trace(&mut reader, header.trace_bytes, limits, certificate_limits)?;
        check_no_trailing_bytes(&reader)?;
        let artifact = Self {
            target: header.target,
            target_scale: header.target_scale,
            status: header.status,
            lower_bound: header.lower_bound,
            upper_bound: header.upper_bound,
            edits,
            trace,
        };
        artifact.check_shape()?;
        Ok(artifact)
    }
}

struct InterventionHeader {
    target: IntervalGroupId,
    target_scale: f64,
    status: InterventionStatus,
    lower_bound: f64,
    upper_bound: f64,
    edit_count: usize,
    trace_bytes: usize,
}

fn encode_trace(trace: &ProgramTraceArtifact) -> Result<Vec<u8>, InterventionError> {
    trace
        .encode()
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn encode_intervention_header(
    out: &mut Vec<u8>,
    artifact: &InterventionArtifact,
    trace_bytes: usize,
) -> Result<(), InterventionError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    out.extend_from_slice(artifact.target.as_bytes());
    put_u64(out, artifact.target_scale.to_bits());
    out.push(status_tag(artifact.status));
    put_u64(out, artifact.lower_bound.to_bits());
    put_u64(out, artifact.upper_bound.to_bits());
    put_usize(out, artifact.edits.len(), "edge-edit count")?;
    put_usize(out, trace_bytes, "trace byte count")?;
    Ok(())
}

fn encode_edits(out: &mut Vec<u8>, edits: &[EdgeWeightEdit]) -> Result<(), InterventionError> {
    for edit in edits {
        put_usize(out, edit.edge.u, "edge endpoint")?;
        put_usize(out, edit.edge.v, "edge endpoint")?;
        put_u64(out, edit.before.to_bits());
        put_u64(out, edit.after.to_bits());
    }
    Ok(())
}

fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), InterventionError> {
    if bytes.len() > max_bytes {
        return Err(InterventionError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

fn decode_intervention_header(
    reader: &mut Reader<'_>,
    limits: InterventionDecodeLimits,
) -> Result<InterventionHeader, InterventionError> {
    check_intervention_identity(reader)?;
    let target = IntervalGroupId::from_bytes(reader.array32()?);
    let (target_scale, status, lower_bound, upper_bound) = decode_claim(reader)?;
    let (edit_count, trace_bytes) = decode_intervention_counts(reader, limits)?;
    Ok(InterventionHeader {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound,
        edit_count,
        trace_bytes,
    })
}

fn check_intervention_identity(reader: &mut Reader<'_>) -> Result<(), InterventionError> {
    if reader.take(8)? != MAGIC {
        return Err(InterventionError::new("wrong magic bytes"));
    }
    if reader.u16()? != WIRE_VERSION {
        return Err(InterventionError::new("unsupported wire version"));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(InterventionError::new("unsupported scalar codec"));
    }
    Ok(())
}

fn decode_claim(
    reader: &mut Reader<'_>,
) -> Result<(f64, InterventionStatus, f64, f64), InterventionError> {
    Ok((
        f64::from_bits(reader.u64()?),
        decode_status(reader.u8()?)?,
        f64::from_bits(reader.u64()?),
        f64::from_bits(reader.u64()?),
    ))
}

fn decode_intervention_counts(
    reader: &mut Reader<'_>,
    limits: InterventionDecodeLimits,
) -> Result<(usize, usize), InterventionError> {
    Ok((
        reader.bounded_usize("edge-edit count", limits.max_edits)?,
        reader.bounded_usize("trace byte count", limits.max_trace_bytes)?,
    ))
}

fn check_record_bytes(
    reader: &Reader<'_>,
    edit_count: usize,
    trace_bytes: usize,
) -> Result<(), InterventionError> {
    let fixed = edit_count
        .checked_mul(32)
        .and_then(|edits| edits.checked_add(trace_bytes))
        .ok_or_else(|| InterventionError::new("record bytes overflow usize"))?;
    if fixed > reader.remaining() {
        return Err(InterventionError::new("record exceeds the remaining bytes"));
    }
    Ok(())
}

fn decode_edits(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<EdgeWeightEdit>, InterventionError> {
    let mut edits = Vec::with_capacity(count);
    for _ in 0..count {
        edits.push(EdgeWeightEdit {
            edge: EdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            },
            before: f64::from_bits(reader.u64()?),
            after: f64::from_bits(reader.u64()?),
        });
    }
    Ok(edits)
}

fn decode_trace(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: InterventionDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<ProgramTraceArtifact, InterventionError> {
    ProgramTraceArtifact::decode(reader.take(byte_count)?, limits.trace, certificate_limits)
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), InterventionError> {
    if reader.remaining() != 0 {
        return Err(InterventionError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}
fn status_tag(status: InterventionStatus) -> u8 {
    match status {
        InterventionStatus::Optimal => 0,
        InterventionStatus::BoundedGap => 1,
        InterventionStatus::BudgetLimited => 2,
    }
}

fn decode_status(tag: u8) -> std::result::Result<InterventionStatus, InterventionError> {
    match tag {
        0 => Ok(InterventionStatus::Optimal),
        1 => Ok(InterventionStatus::BoundedGap),
        2 => Ok(InterventionStatus::BudgetLimited),
        _ => Err(InterventionError::new(format!(
            "unknown intervention-status tag {tag}"
        ))),
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), InterventionError> {
    let value = u64::try_from(value)
        .map_err(|_| InterventionError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], InterventionError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| InterventionError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(InterventionError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, InterventionError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, InterventionError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, InterventionError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, InterventionError> {
        usize::try_from(self.u64()?)
            .map_err(|_| InterventionError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, InterventionError> {
        let value = self.usize()?;
        if value > limit {
            return Err(InterventionError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], InterventionError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
