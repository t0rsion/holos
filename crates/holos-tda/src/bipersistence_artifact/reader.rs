use crate::{Error, Result};

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(super) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| Error::InvalidInput("truncated bipersistence artifact".into()))?;
        let output = &self.bytes[self.position..end];
        self.position = end;
        Ok(output)
    }

    pub(super) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(super) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| Error::InvalidInput("bipersistence integer exceeds usize".into()))
    }

    pub(super) fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            Err(Error::InvalidInput(format!(
                "bipersistence {name} exceeds its limit {maximum}"
            )))
        } else {
            Ok(value)
        }
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }

    pub(super) fn u64s(&mut self, name: &str, maximum: usize) -> Result<Vec<u64>> {
        let count = self.bounded_usize(name, maximum)?;
        (0..count).map(|_| self.u64()).collect()
    }

    pub(super) fn usizes(&mut self, name: &str, maximum: usize) -> Result<Vec<usize>> {
        let count = self.bounded_usize(name, maximum)?;
        (0..count).map(|_| self.usize()).collect()
    }
}
