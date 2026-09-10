use crate::EdgeKey;

use super::model::TrajectoryError;

pub(super) fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), TrajectoryError> {
    let value = u64::try_from(value)
        .map_err(|_| TrajectoryError::new(format!("{label} does not fit the wire format")))?;
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

    pub(super) fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], TrajectoryError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| TrajectoryError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(TrajectoryError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> std::result::Result<u8, TrajectoryError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> std::result::Result<u16, TrajectoryError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(super) fn u64(&mut self) -> std::result::Result<u64, TrajectoryError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(super) fn usize(&mut self) -> std::result::Result<usize, TrajectoryError> {
        usize::try_from(self.u64()?)
            .map_err(|_| TrajectoryError::new("wire integer does not fit usize"))
    }

    pub(super) fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, TrajectoryError> {
        let value = self.usize()?;
        if value > limit {
            return Err(TrajectoryError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    pub(super) fn optional_edge(
        &mut self,
    ) -> std::result::Result<Option<EdgeKey>, TrajectoryError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let u = self.usize()?;
                let v = self.usize()?;
                if u >= v {
                    return Err(TrajectoryError::new(
                        "event edge is not in canonical endpoint order",
                    ));
                }
                Ok(Some(EdgeKey { u, v }))
            }
            tag => Err(TrajectoryError::new(format!(
                "unknown optional-edge tag {tag}"
            ))),
        }
    }

    pub(super) fn optional_f64(&mut self) -> std::result::Result<Option<f64>, TrajectoryError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let value = f64::from_bits(self.u64()?);
                if !value.is_finite() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
                    return Err(TrajectoryError::new(
                        "event scalar is not a canonical number",
                    ));
                }
                Ok(Some(value))
            }
            tag => Err(TrajectoryError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }
}
