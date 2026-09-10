use crate::ProofError;

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

    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("coverage artifact position overflows"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("coverage artifact is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(crate) fn bounded_usize(
        &mut self,
        name: &str,
        maximum: usize,
    ) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(ProofError::new(format!(
                "coverage {name} exceeds its limit"
            )));
        }
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProofError::new("coverage integer does not fit usize"))
    }

    pub(crate) fn optional_u64(&mut self) -> Result<Option<u64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(ProofError::new("coverage optional integer flag is invalid")),
        }
    }

    pub(crate) fn optional_usize(&mut self) -> Result<Option<usize>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            _ => Err(ProofError::new("coverage optional integer flag is invalid")),
        }
    }

    pub(crate) fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
