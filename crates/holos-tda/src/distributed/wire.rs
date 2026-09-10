use sha2::{Digest, Sha256};

use super::model::{ArtifactId, DistributedInterfaceError};

pub(super) const MANIFEST_MAGIC: &[u8; 8] = b"HOLOSDM\0";
pub(super) const PROGRESS_MAGIC: &[u8; 8] = b"HOLOSDW\0";
pub(super) const VERSION: u16 = 1;

pub(super) fn encode_ids(
    output: &mut Vec<u8>,
    values: &[ArtifactId],
) -> Result<(), DistributedInterfaceError> {
    put_usize(output, values.len())?;
    for value in values {
        output.extend_from_slice(value.as_bytes());
    }
    Ok(())
}

pub(super) fn decode_ids(
    reader: &mut Reader<'_>,
) -> Result<Vec<ArtifactId>, DistributedInterfaceError> {
    let count = reader.usize()?;
    let maximum = reader.remaining() / 32;
    if count > maximum {
        return Err(DistributedInterfaceError::new(
            "manifest identifier count exceeds the remaining bytes",
        ));
    }
    (0..count)
        .map(|_| reader.array32().map(ArtifactId))
        .collect()
}

pub(super) fn encode_usizes(
    output: &mut Vec<u8>,
    values: &[usize],
) -> Result<(), DistributedInterfaceError> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

pub(super) fn decode_usizes(
    reader: &mut Reader<'_>,
) -> Result<Vec<usize>, DistributedInterfaceError> {
    let count = reader.usize()?;
    let maximum = reader.remaining() / 8;
    if count > maximum {
        return Err(DistributedInterfaceError::new(
            "manifest integer count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

pub(super) fn put_usize(
    output: &mut Vec<u8>,
    value: usize,
) -> Result<(), DistributedInterfaceError> {
    let value = u64::try_from(value)
        .map_err(|_| DistributedInterfaceError::new("integer does not fit the wire format"))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

pub(super) fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for value in values {
        hash.update((*value as u64).to_be_bytes());
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

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], DistributedInterfaceError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| DistributedInterfaceError::new("manifest position overflows"))?;
        if end > self.bytes.len() {
            return Err(DistributedInterfaceError::new("manifest is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(super) fn u16(&mut self) -> Result<u16, DistributedInterfaceError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(super) fn u32(&mut self) -> Result<u32, DistributedInterfaceError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn u64(&mut self) -> Result<u64, DistributedInterfaceError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn usize(&mut self) -> Result<usize, DistributedInterfaceError> {
        usize::try_from(self.u64()?)
            .map_err(|_| DistributedInterfaceError::new("manifest integer does not fit usize"))
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32], DistributedInterfaceError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
