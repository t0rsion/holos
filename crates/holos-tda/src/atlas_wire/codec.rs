use crate::{Bar, Cocycle, CriticalPair, CriticalSimplex, Diagram};

use super::model::AtlasArtifactError;

pub(crate) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

pub(crate) fn cocycle_lists_bits_equal(a: &[Cocycle], b: &[Cocycle]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.modulus == b.modulus && a.scale.to_bits() == b.scale.to_bits() && a.terms == b.terms
        })
}

pub(crate) fn critical_pair_order(a: &CriticalPair, b: &CriticalPair) -> std::cmp::Ordering {
    a.birth.vertices.cmp(&b.birth.vertices).then_with(|| {
        a.death
            .as_ref()
            .map(|simplex| &simplex.vertices)
            .cmp(&b.death.as_ref().map(|simplex| &simplex.vertices))
    })
}

pub(crate) fn critical_pair_record_order(
    a: &(Bar, CriticalPair),
    b: &(Bar, CriticalPair),
) -> std::cmp::Ordering {
    a.0.birth
        .total_cmp(&b.0.birth)
        .then(a.0.death.total_cmp(&b.0.death))
        .then_with(|| critical_pair_order(&a.1, &b.1))
}

pub(crate) fn critical_pair_records_bits_equal(
    a: &[(Bar, CriticalPair)],
    b: &[(Bar, CriticalPair)],
) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|((a_bar, a_pair), (b_bar, b_pair))| {
            a_bar.birth.to_bits() == b_bar.birth.to_bits()
                && a_bar.death.to_bits() == b_bar.death.to_bits()
                && critical_simplex_bits_equal(&a_pair.birth, &b_pair.birth)
                && match (&a_pair.death, &b_pair.death) {
                    (None, None) => true,
                    (Some(a), Some(b)) => critical_simplex_bits_equal(a, b),
                    _ => false,
                }
        })
}

pub(crate) fn critical_simplex_bits_equal(a: &CriticalSimplex, b: &CriticalSimplex) -> bool {
    a.vertices == b.vertices && a.value.to_bits() == b.value.to_bits()
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
) -> std::result::Result<(), AtlasArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| AtlasArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

pub(crate) fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
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
    ) -> std::result::Result<&'a [u8], AtlasArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| AtlasArtifactError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(AtlasArtifactError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> std::result::Result<u8, AtlasArtifactError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> std::result::Result<u16, AtlasArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(crate) fn u32(&mut self) -> std::result::Result<u32, AtlasArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(crate) fn u64(&mut self) -> std::result::Result<u64, AtlasArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(crate) fn usize(&mut self) -> std::result::Result<usize, AtlasArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| AtlasArtifactError::new("wire integer does not fit usize"))
    }

    pub(crate) fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, AtlasArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(AtlasArtifactError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(crate) fn optional_f64(&mut self) -> std::result::Result<Option<f64>, AtlasArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(AtlasArtifactError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    pub(crate) fn array32(&mut self) -> std::result::Result<[u8; 32], AtlasArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
