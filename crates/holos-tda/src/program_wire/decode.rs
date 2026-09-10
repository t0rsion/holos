use crate::{AtlasArtifact, Bar, CertificateLimits, EdgeKey};

use super::model::{ProgramArtifactError, ProgramAtomArtifact, ProgramDecodeLimits};
use super::primitives::Reader;
use super::verification::bounded_sum;
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(super) struct ProgramHeader {
    pub(super) modulus: u32,
    pub(super) vertex_count: usize,
    pub(super) threshold: Option<f64>,
    pub(super) bar_count: usize,
    pub(super) atom_count: usize,
    pub(super) input_digest: [u8; 32],
}

struct AtomHeader {
    id: usize,
    vertex_count: usize,
    edge_count: usize,
    atlas_bytes: usize,
}

#[derive(Default)]
struct DecodeTotals {
    vertices: usize,
    edges: usize,
    atlas_bytes: usize,
}

impl DecodeTotals {
    fn add(
        &mut self,
        header: &AtomHeader,
        limits: ProgramDecodeLimits,
    ) -> Result<(), ProgramArtifactError> {
        self.vertices = bounded_sum(
            self.vertices,
            header.vertex_count,
            limits.max_atom_vertices,
            "atom vertices",
        )?;
        self.edges = bounded_sum(
            self.edges,
            header.edge_count,
            limits.max_atom_edges,
            "atom edges",
        )?;
        self.atlas_bytes = bounded_sum(
            self.atlas_bytes,
            header.atlas_bytes,
            limits.max_atlas_bytes,
            "nested atlas bytes",
        )?;
        Ok(())
    }
}

pub(super) fn check_envelope_size(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<(), ProgramArtifactError> {
    if bytes.len() > max_bytes {
        return Err(ProgramArtifactError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

pub(super) fn decode_program_header(
    reader: &mut Reader<'_>,
    limits: ProgramDecodeLimits,
) -> Result<ProgramHeader, ProgramArtifactError> {
    check_program_identity(reader)?;
    Ok(ProgramHeader {
        modulus: reader.u32()?,
        vertex_count: reader.bounded_usize("vertex count", limits.max_vertices)?,
        threshold: reader.optional_f64()?,
        bar_count: reader.bounded_usize("bar count", limits.max_bars)?,
        atom_count: reader.bounded_usize("atom count", limits.max_atoms)?,
        input_digest: reader.array32()?,
    })
}

fn check_program_identity(reader: &mut Reader<'_>) -> Result<(), ProgramArtifactError> {
    if reader.take(8)? != MAGIC {
        return Err(ProgramArtifactError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(ProgramArtifactError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(ProgramArtifactError::new("unsupported scalar codec"));
    }
    Ok(())
}

pub(super) fn check_minimum_record_bytes(
    reader: &Reader<'_>,
    bar_count: usize,
    atom_count: usize,
) -> Result<(), ProgramArtifactError> {
    let bar_bytes = bar_count
        .checked_mul(24)
        .ok_or_else(|| ProgramArtifactError::new("minimum record bytes overflow usize"))?;
    let atom_bytes = atom_count
        .checked_mul(32)
        .ok_or_else(|| ProgramArtifactError::new("minimum record bytes overflow usize"))?;
    let minimum = bar_bytes
        .checked_add(atom_bytes)
        .ok_or_else(|| ProgramArtifactError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(ProgramArtifactError::new(
            "record counts exceed the remaining bytes",
        ));
    }
    Ok(())
}

pub(super) fn decode_bars(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<Bar>, ProgramArtifactError> {
    let mut bars = Vec::with_capacity(count);
    for _ in 0..count {
        bars.push(Bar {
            dim: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    Ok(bars)
}

pub(super) fn decode_atoms(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<Vec<ProgramAtomArtifact>, ProgramArtifactError> {
    let mut atoms = Vec::with_capacity(count);
    let mut totals = DecodeTotals::default();
    for _ in 0..count {
        atoms.push(decode_atom(
            reader,
            limits,
            certificate_limits,
            &mut totals,
        )?);
    }
    Ok(atoms)
}

fn decode_atom(
    reader: &mut Reader<'_>,
    limits: ProgramDecodeLimits,
    certificate_limits: CertificateLimits,
    totals: &mut DecodeTotals,
) -> Result<ProgramAtomArtifact, ProgramArtifactError> {
    let header = decode_atom_header(reader)?;
    totals.add(&header, limits)?;
    check_atom_record_bytes(reader, &header)?;
    let vertices = decode_atom_vertices(reader, header.vertex_count)?;
    let edges = decode_atom_edges(reader, header.edge_count)?;
    let atlas = decode_atlas(reader, header.atlas_bytes, limits, certificate_limits)?;
    Ok(ProgramAtomArtifact {
        id: header.id,
        vertices,
        edges,
        atlas,
    })
}

fn decode_atom_header(reader: &mut Reader<'_>) -> Result<AtomHeader, ProgramArtifactError> {
    Ok(AtomHeader {
        id: reader.usize()?,
        vertex_count: reader.usize()?,
        edge_count: reader.usize()?,
        atlas_bytes: reader.usize()?,
    })
}

fn check_atom_record_bytes(
    reader: &Reader<'_>,
    header: &AtomHeader,
) -> Result<(), ProgramArtifactError> {
    let vertex_bytes = header
        .vertex_count
        .checked_mul(8)
        .ok_or_else(|| ProgramArtifactError::new("atom record bytes overflow usize"))?;
    let edge_bytes = header
        .edge_count
        .checked_mul(16)
        .ok_or_else(|| ProgramArtifactError::new("atom record bytes overflow usize"))?;
    let fixed = vertex_bytes
        .checked_add(edge_bytes)
        .and_then(|bytes| bytes.checked_add(header.atlas_bytes))
        .ok_or_else(|| ProgramArtifactError::new("atom record bytes overflow usize"))?;
    if fixed > reader.remaining() {
        return Err(ProgramArtifactError::new(
            "atom record exceeds the remaining bytes",
        ));
    }
    Ok(())
}

fn decode_atom_vertices(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<usize>, ProgramArtifactError> {
    let mut vertices = Vec::with_capacity(count);
    for _ in 0..count {
        vertices.push(reader.usize()?);
    }
    Ok(vertices)
}

fn decode_atom_edges(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<EdgeKey>, ProgramArtifactError> {
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        edges.push(EdgeKey {
            u: reader.usize()?,
            v: reader.usize()?,
        });
    }
    Ok(edges)
}

fn decode_atlas(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: ProgramDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<AtlasArtifact, ProgramArtifactError> {
    AtlasArtifact::decode(reader.take(byte_count)?, limits.atlas, certificate_limits)
        .map_err(|error| ProgramArtifactError::new(error.to_string()))
}

pub(super) fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), ProgramArtifactError> {
    if reader.remaining() != 0 {
        return Err(ProgramArtifactError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}
