use crate::{MODULUS_LIMIT, ProofError, ProofLimits, checked_threshold, is_prime};

use super::wire::{F64_BITS_CODEC, VERSION};

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(
        bytes: &'a [u8],
        limits: ProofLimits,
        magic: &[u8; 8],
    ) -> Result<Self, ProofError> {
        if bytes.len() > limits.max_bytes {
            return Err(ProofError::new(format!(
                "{} bytes exceed the limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Self { bytes, position: 0 };
        if reader.take(8)? != magic {
            return Err(ProofError::new("wrong index-proof magic bytes"));
        }
        Ok(reader)
    }

    pub(super) fn header(
        &mut self,
        limits: ProofLimits,
    ) -> Result<(usize, u32, Option<f64>, usize), ProofError> {
        self.format_header()?;
        self.parameter_header(limits)
    }

    fn format_header(&mut self) -> Result<(), ProofError> {
        if self.u16()? != VERSION {
            return Err(ProofError::new("unsupported index-proof version"));
        }
        if self.u8()? != F64_BITS_CODEC {
            return Err(ProofError::new("unsupported index-proof scalar codec"));
        }
        Ok(())
    }

    fn parameter_header(
        &mut self,
        limits: ProofLimits,
    ) -> Result<(usize, u32, Option<f64>, usize), ProofError> {
        let max_dim = self.bounded_usize("index homology dimension", limits.max_dimension)?;
        let modulus = self.u32()?;
        if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
            return Err(ProofError::new(format!(
                "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
            )));
        }
        let threshold = self.optional_f64()?;
        checked_threshold(threshold)?;
        let vertex_count = self.bounded_usize("index vertex count", limits.max_vertices)?;
        if vertex_count == 0 {
            return Err(ProofError::new("index proof has no vertices"));
        }
        Ok((max_dim, modulus, threshold, vertex_count))
    }

    pub(super) fn finish(&self) -> Result<(), ProofError> {
        if self.position != self.bytes.len() {
            return Err(ProofError::new(format!(
                "{} trailing bytes after the index proof",
                self.bytes.len() - self.position
            )));
        }
        Ok(())
    }

    pub(super) fn require_remaining(&self, bytes: usize, label: &str) -> Result<(), ProofError> {
        if bytes > self.remaining() {
            return Err(ProofError::new(format!(
                "{label} require {bytes} bytes, but only {} remain",
                self.remaining()
            )));
        }
        Ok(())
    }

    pub(super) fn require_records(
        &self,
        count: usize,
        record_bytes: usize,
        label: &str,
    ) -> Result<(), ProofError> {
        let bytes = count
            .checked_mul(record_bytes)
            .ok_or_else(|| ProofError::new(format!("{label} byte count overflows usize")))?;
        self.require_remaining(bytes, label)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("index-proof position overflow"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("truncated index proof"));
        }
        let output = &self.bytes[self.position..end];
        self.position = end;
        Ok(output)
    }

    pub(super) fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, ProofError> {
        let mut bytes = [0; 2];
        bytes.copy_from_slice(self.take(2)?);
        Ok(u16::from_be_bytes(bytes))
    }

    pub(super) fn u32(&mut self) -> Result<u32, ProofError> {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(bytes))
    }

    pub(super) fn u64(&mut self) -> Result<u64, ProofError> {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(bytes))
    }

    pub(super) fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProofError::new("index-proof integer does not fit usize"))
    }

    pub(super) fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProofError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        let mut bytes = [0; 32];
        bytes.copy_from_slice(self.take(32)?);
        Ok(bytes)
    }

    fn optional_f64(&mut self) -> Result<Option<f64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            _ => Err(ProofError::new("invalid optional threshold tag")),
        }
    }
}
