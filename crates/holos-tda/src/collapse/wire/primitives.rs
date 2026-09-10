use sha2::{Digest, Sha256};

use super::model::ArtifactError;

pub(crate) fn graph_digest(vertices: usize, edges: &[(usize, usize, f64)]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-collapse-graph-v1");
    hash.update((vertices as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for &(u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_usize(out: &mut Vec<u8>, value: usize, label: &str) -> Result<(), ArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| ArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

pub(super) fn put_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value);
        }
    }
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

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], ArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ArtifactError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(ArtifactError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> Result<u8, ArtifactError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, ArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(super) fn u32(&mut self) -> Result<u32, ArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(super) fn u64(&mut self) -> Result<u64, ArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(super) fn usize(&mut self) -> Result<usize, ArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ArtifactError::new("wire integer does not fit usize"))
    }

    pub(super) fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> Result<usize, ArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ArtifactError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(super) fn optional_u64(&mut self) -> Result<Option<u64>, ArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            tag => Err(ArtifactError::new(format!(
                "unknown optional-integer tag {tag}"
            ))),
        }
    }

    pub(super) fn optional_f64(&mut self) -> Result<Option<f64>, ArtifactError> {
        Ok(self.optional_u64()?.map(f64::from_bits))
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32], ArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
