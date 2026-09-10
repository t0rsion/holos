use crate::{
    ContinuationKind, Diagram, EdgeKey, ProgramArtifactError, ProgramEventKind, ProgramUpdateMode,
};

use super::model::ProgramTraceError;

pub(crate) fn mode_tag(mode: ProgramUpdateMode) -> u8 {
    match mode {
        ProgramUpdateMode::Reused => 0,
        ProgramUpdateMode::Repaired => 1,
        ProgramUpdateMode::Recompiled => 2,
    }
}

pub(crate) fn decode_mode(tag: u8) -> std::result::Result<ProgramUpdateMode, ProgramTraceError> {
    match tag {
        0 => Ok(ProgramUpdateMode::Reused),
        1 => Ok(ProgramUpdateMode::Repaired),
        2 => Ok(ProgramUpdateMode::Recompiled),
        _ => Err(ProgramTraceError::new(format!(
            "unknown update-mode tag {tag}"
        ))),
    }
}

pub(crate) fn event_kind_tag(kind: ProgramEventKind) -> u8 {
    match kind {
        ProgramEventKind::VertexSetChanged => 0,
        ProgramEventKind::EdgeSetChanged => 1,
        ProgramEventKind::ThresholdCrossing => 2,
        ProgramEventKind::GuardFailed => 3,
        ProgramEventKind::AtomRebuilt => 4,
        ProgramEventKind::ReductionSuffixRepaired => 5,
        ProgramEventKind::SeparatorContractChanged => 6,
    }
}

pub(crate) fn decode_event_kind(
    tag: u8,
) -> std::result::Result<ProgramEventKind, ProgramTraceError> {
    match tag {
        0 => Ok(ProgramEventKind::VertexSetChanged),
        1 => Ok(ProgramEventKind::EdgeSetChanged),
        2 => Ok(ProgramEventKind::ThresholdCrossing),
        3 => Ok(ProgramEventKind::GuardFailed),
        4 => Ok(ProgramEventKind::AtomRebuilt),
        5 => Ok(ProgramEventKind::ReductionSuffixRepaired),
        6 => Ok(ProgramEventKind::SeparatorContractChanged),
        _ => Err(ProgramTraceError::new(format!(
            "unknown event-kind tag {tag}"
        ))),
    }
}

pub(crate) fn continuation_kind_tag(kind: ContinuationKind) -> u8 {
    match kind {
        ContinuationKind::Isomorphism => 0,
        ContinuationKind::Split => 1,
        ContinuationKind::Merge => 2,
        ContinuationKind::Mixing => 3,
        ContinuationKind::Birth => 4,
        ContinuationKind::Death => 5,
        ContinuationKind::Ambiguous => 6,
    }
}

pub(crate) fn decode_continuation_kind(
    tag: u8,
) -> std::result::Result<ContinuationKind, ProgramTraceError> {
    match tag {
        0 => Ok(ContinuationKind::Isomorphism),
        1 => Ok(ContinuationKind::Split),
        2 => Ok(ContinuationKind::Merge),
        3 => Ok(ContinuationKind::Mixing),
        4 => Ok(ContinuationKind::Birth),
        5 => Ok(ContinuationKind::Death),
        6 => Ok(ContinuationKind::Ambiguous),
        _ => Err(ProgramTraceError::new(format!(
            "unknown continuation-kind tag {tag}"
        ))),
    }
}

pub(crate) fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> std::result::Result<usize, ProgramTraceError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProgramTraceError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProgramTraceError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

pub(crate) fn program_artifact_error(error: ProgramArtifactError) -> ProgramTraceError {
    ProgramTraceError::new(error.to_string())
}

pub(crate) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

pub(crate) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), ProgramTraceError> {
    let value = u64::try_from(value)
        .map_err(|_| ProgramTraceError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

pub(crate) fn put_optional_usize(
    out: &mut Vec<u8>,
    value: Option<usize>,
    label: &str,
) -> std::result::Result<(), ProgramTraceError> {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_usize(out, value, label)?;
        }
    }
    Ok(())
}

pub(crate) fn put_optional_edge(
    out: &mut Vec<u8>,
    edge: Option<EdgeKey>,
) -> std::result::Result<(), ProgramTraceError> {
    match edge {
        None => out.push(0),
        Some(edge) => {
            out.push(1);
            put_usize(out, edge.u, "event edge endpoint")?;
            put_usize(out, edge.v, "event edge endpoint")?;
        }
    }
    Ok(())
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(crate) fn take(
        &mut self,
        count: usize,
    ) -> std::result::Result<&'a [u8], ProgramTraceError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProgramTraceError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProgramTraceError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> std::result::Result<u8, ProgramTraceError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> std::result::Result<u16, ProgramTraceError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(crate) fn u32(&mut self) -> std::result::Result<u32, ProgramTraceError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(crate) fn u64(&mut self) -> std::result::Result<u64, ProgramTraceError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(crate) fn usize(&mut self) -> std::result::Result<usize, ProgramTraceError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProgramTraceError::new("wire integer does not fit usize"))
    }

    pub(crate) fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, ProgramTraceError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProgramTraceError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(crate) fn optional_usize(
        &mut self,
    ) -> std::result::Result<Option<usize>, ProgramTraceError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            tag => Err(ProgramTraceError::new(format!(
                "unknown optional-integer tag {tag}"
            ))),
        }
    }

    pub(crate) fn array32(&mut self) -> std::result::Result<[u8; 32], ProgramTraceError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
