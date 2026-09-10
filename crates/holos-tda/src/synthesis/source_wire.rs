use crate::{Error, KineticEdge, KineticEdgeKey, Result};

use super::model::{SynthesisLimits, SynthesisSource};

pub(super) fn add_proof_terms(
    total: &mut usize,
    count: usize,
    maximum: usize,
    subject: &str,
) -> Result<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| Error::InvalidInput(format!("{subject} term count overflows")))?;
    if *total > maximum {
        Err(Error::InvalidInput(format!(
            "{subject} terms exceed their limit"
        )))
    } else {
        Ok(())
    }
}

pub(super) fn encode_source(output: &mut Vec<u8>, source: &SynthesisSource) -> Result<()> {
    match source {
        SynthesisSource::Finite => output.push(0),
        SynthesisSource::Affine { .. } => encode_affine_source(output, source)?,
    }
    Ok(())
}

pub(super) fn encode_affine_source(output: &mut Vec<u8>, source: &SynthesisSource) -> Result<()> {
    let SynthesisSource::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    } = source
    else {
        return Ok(());
    };
    output.push(1);
    output.extend_from_slice(&scenario.to_be_bytes());
    output.extend_from_slice(&start.to_bits().to_be_bytes());
    output.extend_from_slice(&end.to_bits().to_be_bytes());
    put_usize(output, *maximum_rank)?;
    put_usize(output, edges.len())?;
    for edge in edges {
        encode_affine_edge(output, edge)?;
    }
    Ok(())
}

pub(super) fn encode_affine_edge(output: &mut Vec<u8>, edge: &KineticEdge) -> Result<()> {
    put_usize(output, edge.u)?;
    put_usize(output, edge.v)?;
    output.extend_from_slice(&edge.intercept.to_bits().to_be_bytes());
    output.extend_from_slice(&edge.velocity.to_bits().to_be_bytes());
    Ok(())
}

pub(super) fn decode_source(
    reader: &mut Reader<'_>,
    limits: SynthesisLimits,
) -> Result<SynthesisSource> {
    match reader.u8()? {
        0 => Ok(SynthesisSource::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(Error::InvalidInput(
            "synthesis source kind is invalid".into(),
        )),
    }
}

pub(super) fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: SynthesisLimits,
) -> Result<SynthesisSource> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let maximum_rank = reader.usize()?;
    let edges = decode_affine_edges(reader, limits)?;
    Ok(SynthesisSource::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    })
}

pub(super) fn decode_affine_edges(
    reader: &mut Reader<'_>,
    limits: SynthesisLimits,
) -> Result<Vec<KineticEdge>> {
    let count = reader.bounded_usize("affine edge count", limits.kinetic.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput(
            "synthesis affine edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| decode_affine_edge(reader)).collect()
}

pub(super) fn decode_affine_edge(reader: &mut Reader<'_>) -> Result<KineticEdge> {
    Ok(KineticEdge {
        u: reader.usize()?,
        v: reader.usize()?,
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}

pub(super) fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdgeKey]) -> Result<()> {
    put_usize(output, edges.len())?;
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
    }
    Ok(())
}

pub(super) fn decode_edges(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<KineticEdgeKey>> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(Error::InvalidInput(
            "synthesis edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(KineticEdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            })
        })
        .collect()
}

pub(super) fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

pub(super) fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(Error::InvalidInput(
            "synthesis integer list exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

pub(super) fn decode_indices(
    reader: &mut Reader<'_>,
    exclusive_maximum: usize,
    maximum_count: usize,
) -> Result<Vec<usize>> {
    let values = decode_usizes(reader, maximum_count)?;
    if values.iter().any(|value| *value >= exclusive_maximum)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "synthesis index list is not canonical".into(),
        ));
    }
    Ok(values)
}

pub(super) fn encode_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
        None => output.push(0),
    }
}

pub(super) fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| Error::InvalidInput("synthesis integer does not fit u64".into()))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
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

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| Error::InvalidInput("synthesis artifact position overflows".into()))?;
        if end > self.bytes.len() {
            return Err(Error::InvalidInput(
                "synthesis artifact is truncated".into(),
            ));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(super) fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(Error::InvalidInput(format!(
                "synthesis {name} exceeds its limit"
            )));
        }
        Ok(value)
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
            .map_err(|_| Error::InvalidInput("synthesis integer does not fit usize".into()))
    }

    pub(super) fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Error::InvalidInput(
                "synthesis optional integer flag is invalid".into(),
            )),
        }
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
