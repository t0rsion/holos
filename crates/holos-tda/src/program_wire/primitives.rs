use super::model::ProgramArtifactError;

pub(super) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), ProgramArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| ProgramArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

pub(super) fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(super) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(super) fn take(
        &mut self,
        count: usize,
    ) -> std::result::Result<&'a [u8], ProgramArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProgramArtifactError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProgramArtifactError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> std::result::Result<u8, ProgramArtifactError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> std::result::Result<u16, ProgramArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(super) fn u32(&mut self) -> std::result::Result<u32, ProgramArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(super) fn u64(&mut self) -> std::result::Result<u64, ProgramArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(super) fn usize(&mut self) -> std::result::Result<usize, ProgramArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProgramArtifactError::new("wire integer does not fit usize"))
    }

    pub(super) fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, ProgramArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProgramArtifactError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(super) fn optional_f64(
        &mut self,
    ) -> std::result::Result<Option<f64>, ProgramArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(ProgramArtifactError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    pub(super) fn array32(&mut self) -> std::result::Result<[u8; 32], ProgramArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
