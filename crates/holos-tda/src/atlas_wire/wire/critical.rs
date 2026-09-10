use crate::{CriticalPair, CriticalSimplex};

use super::super::codec::{Reader, put_u64, put_usize};
use super::super::model::AtlasArtifactError;

pub(super) fn encode_critical_pairs(
    out: &mut Vec<u8>,
    pairs: &[CriticalPair],
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in pairs {
        encode_critical(out, &pair.birth)?;
        encode_optional_critical(out, pair.death.as_ref())?;
    }
    Ok(())
}

fn encode_optional_critical(
    out: &mut Vec<u8>,
    critical: Option<&CriticalSimplex>,
) -> std::result::Result<(), AtlasArtifactError> {
    match critical {
        None => out.push(0),
        Some(critical) => {
            out.push(1);
            encode_critical(out, critical)?;
        }
    }
    Ok(())
}

fn encode_critical(
    out: &mut Vec<u8>,
    simplex: &CriticalSimplex,
) -> std::result::Result<(), AtlasArtifactError> {
    put_usize(out, simplex.vertices.len(), "critical simplex size")?;
    for &vertex in &simplex.vertices {
        put_usize(out, vertex, "critical simplex vertex")?;
    }
    put_u64(out, simplex.value.to_bits());
    Ok(())
}

pub(super) fn decode_critical_pairs(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    count: usize,
) -> std::result::Result<Vec<CriticalPair>, AtlasArtifactError> {
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        pairs.push(CriticalPair {
            birth: decode_critical(reader, vertex_count)?,
            death: decode_optional_critical(reader, vertex_count)?,
        });
    }
    Ok(pairs)
}

fn decode_optional_critical(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> std::result::Result<Option<CriticalSimplex>, AtlasArtifactError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_critical(reader, vertex_count).map(Some),
        tag => Err(AtlasArtifactError::new(format!(
            "unknown optional-critical tag {tag}"
        ))),
    }
}

fn decode_critical(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> std::result::Result<CriticalSimplex, AtlasArtifactError> {
    let size = reader.usize()?;
    if size != 2 && size != 3 {
        return Err(AtlasArtifactError::new(format!(
            "critical simplex has unsupported size {size}"
        )));
    }
    let mut vertices = Vec::with_capacity(size);
    for _ in 0..size {
        let vertex = reader.usize()?;
        if vertex >= vertex_count {
            return Err(AtlasArtifactError::new(format!(
                "critical simplex vertex {vertex} is outside {vertex_count}"
            )));
        }
        vertices.push(vertex);
    }
    Ok(CriticalSimplex {
        vertices,
        value: f64::from_bits(reader.u64()?),
    })
}
