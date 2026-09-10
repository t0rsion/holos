use crate::{Bar, EdgeKey};

use super::model::{ProgramArtifact, ProgramArtifactError, ProgramAtomArtifact};
use super::primitives::{put_optional_f64, put_u16, put_u32, put_u64, put_usize};
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(super) fn encode_atlases(
    atoms: &[ProgramAtomArtifact],
) -> Result<Vec<Vec<u8>>, ProgramArtifactError> {
    let mut encoded = Vec::with_capacity(atoms.len());
    for atom in atoms {
        encoded.push(
            atom.atlas
                .encode()
                .map_err(|error| ProgramArtifactError::new(error.to_string()))?,
        );
    }
    Ok(encoded)
}

pub(super) fn encode_program_header(
    out: &mut Vec<u8>,
    artifact: &ProgramArtifact,
) -> Result<(), ProgramArtifactError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    put_u32(out, artifact.modulus);
    put_usize(out, artifact.vertex_count, "vertex count")?;
    put_optional_f64(out, artifact.threshold);
    put_usize(out, artifact.diagram.bars.len(), "bar count")?;
    put_usize(out, artifact.atoms.len(), "atom count")?;
    out.extend_from_slice(&artifact.input_digest);
    Ok(())
}

pub(super) fn encode_bars(out: &mut Vec<u8>, bars: &[Bar]) -> Result<(), ProgramArtifactError> {
    for bar in bars {
        put_usize(out, bar.dim, "bar dimension")?;
        put_u64(out, bar.birth.to_bits());
        put_u64(out, bar.death.to_bits());
    }
    Ok(())
}

pub(super) fn encode_atom(
    out: &mut Vec<u8>,
    atom: &ProgramAtomArtifact,
    atlas: &[u8],
) -> Result<(), ProgramArtifactError> {
    encode_atom_header(out, atom, atlas.len())?;
    encode_atom_vertices(out, &atom.vertices)?;
    encode_atom_edges(out, &atom.edges)?;
    out.extend_from_slice(atlas);
    Ok(())
}

fn encode_atom_header(
    out: &mut Vec<u8>,
    atom: &ProgramAtomArtifact,
    atlas_bytes: usize,
) -> Result<(), ProgramArtifactError> {
    put_usize(out, atom.id, "atom identifier")?;
    put_usize(out, atom.vertices.len(), "atom vertex count")?;
    put_usize(out, atom.edges.len(), "atom edge count")?;
    put_usize(out, atlas_bytes, "nested atlas byte count")?;
    Ok(())
}

fn encode_atom_vertices(out: &mut Vec<u8>, vertices: &[usize]) -> Result<(), ProgramArtifactError> {
    for &vertex in vertices {
        put_usize(out, vertex, "atom vertex")?;
    }
    Ok(())
}

fn encode_atom_edges(out: &mut Vec<u8>, edges: &[EdgeKey]) -> Result<(), ProgramArtifactError> {
    for edge in edges {
        put_usize(out, edge.u, "atom edge endpoint")?;
        put_usize(out, edge.v, "atom edge endpoint")?;
    }
    Ok(())
}
