use crate::{
    BipersistenceMap, BipersistenceNode, BipersistenceTerm, ClassExtensionKind,
    CohomologyClassAtlas, Result,
};

use super::super::model::{ArtifactCircularFamily, BipersistenceArtifact};
use super::super::{BipersistenceRectangleClaim, BipersistenceRegionClaim};
use super::super::{F64_BITS_CODEC, MAGIC, VERSION};

pub(super) fn encode_payload(artifact: &BipersistenceArtifact) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    encode_header(&mut output, artifact)?;
    encode_claims(&mut output, artifact)?;
    Ok(output)
}

fn encode_header(output: &mut Vec<u8>, artifact: &BipersistenceArtifact) -> Result<()> {
    output.extend_from_slice(MAGIC);
    put_u16(output, VERSION);
    output.push(F64_BITS_CODEC);
    encode_source(output, artifact)?;
    put_u64(output, artifact.threshold_bits);
    put_u32(output, artifact.modulus);
    put_u64s(output, &artifact.scale_bits)?;
    put_usizes(output, &artifact.minimum_degrees)?;
    Ok(())
}

fn encode_source(output: &mut Vec<u8>, artifact: &BipersistenceArtifact) -> Result<()> {
    put_usize(output, artifact.vertex_count)?;
    put_usize(output, artifact.edges.len())?;
    for edge in &artifact.edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
        put_u64(output, edge.value_bits);
    }
    Ok(())
}

fn encode_claims(output: &mut Vec<u8>, artifact: &BipersistenceArtifact) -> Result<()> {
    encode_nodes(output, &artifact.nodes)?;
    encode_maps(output, &artifact.cover_maps)?;
    encode_rectangles(output, &artifact.rectangles)?;
    encode_regions(output, &artifact.regions)?;
    encode_atlases(output, &artifact.class_atlases)?;
    encode_circular_families(output, &artifact.circular_families)
}

fn encode_grade(output: &mut Vec<u8>, grade: crate::Bigrade) -> Result<()> {
    put_usize(output, grade.scale())?;
    put_usize(output, grade.density())
}

fn encode_nodes(output: &mut Vec<u8>, nodes: &[BipersistenceNode]) -> Result<()> {
    put_usize(output, nodes.len())?;
    for node in nodes {
        encode_grade(output, node.grade)?;
        output.extend_from_slice(node.space.as_bytes());
        put_usize(output, node.rank)?;
    }
    Ok(())
}

fn encode_terms(output: &mut Vec<u8>, terms: &[BipersistenceTerm]) -> Result<()> {
    put_usize(output, terms.len())?;
    for term in terms {
        put_usize(output, term.basis_index)?;
        put_u32(output, term.coefficient);
    }
    Ok(())
}

fn encode_maps(output: &mut Vec<u8>, maps: &[BipersistenceMap]) -> Result<()> {
    put_usize(output, maps.len())?;
    for map in maps {
        encode_grade(output, map.lower_grade)?;
        encode_grade(output, map.upper_grade)?;
        output.extend_from_slice(map.source_space.as_bytes());
        output.extend_from_slice(map.target_space.as_bytes());
        put_usize(output, map.rank)?;
        put_usize(output, map.columns.len())?;
        for column in &map.columns {
            put_usize(output, column.source_basis_index)?;
            encode_terms(output, &column.image)?;
        }
    }
    Ok(())
}

fn encode_rectangles(output: &mut Vec<u8>, claims: &[BipersistenceRectangleClaim]) -> Result<()> {
    put_usize(output, claims.len())?;
    for claim in claims {
        encode_grade(output, claim.rectangle.lower)?;
        encode_grade(output, claim.rectangle.upper)?;
        put_usize(output, claim.rank)?;
    }
    Ok(())
}

fn encode_regions(output: &mut Vec<u8>, claims: &[BipersistenceRegionClaim]) -> Result<()> {
    put_usize(output, claims.len())?;
    for claim in claims {
        put_usize(output, claim.region.grades().len())?;
        for &grade in claim.region.grades() {
            encode_grade(output, grade)?;
        }
        put_usize(output, claim.rank)?;
    }
    Ok(())
}

fn encode_atlases(output: &mut Vec<u8>, atlases: &[CohomologyClassAtlas]) -> Result<()> {
    put_usize(output, atlases.len())?;
    for atlas in atlases {
        encode_atlas(output, atlas)?;
    }
    Ok(())
}

fn encode_atlas(output: &mut Vec<u8>, atlas: &CohomologyClassAtlas) -> Result<()> {
    encode_grade(output, atlas.base_grade)?;
    encode_terms(output, &atlas.base_class)?;
    put_usize(output, atlas.extensions.len())?;
    for extension in &atlas.extensions {
        encode_extension(output, extension)?;
    }
    put_usize(output, atlas.regions.len())?;
    for region in &atlas.regions {
        encode_region(output, region)?;
    }
    Ok(())
}

fn encode_extension(output: &mut Vec<u8>, extension: &crate::ClassExtension) -> Result<()> {
    encode_grade(output, extension.grade)?;
    output.push(encode_kind(extension.kind));
    encode_terms(output, &extension.class)?;
    put_usize(output, extension.ambiguity.len())?;
    for row in &extension.ambiguity {
        encode_terms(output, row)?;
    }
    Ok(())
}

fn encode_region(output: &mut Vec<u8>, region: &crate::ClassExtensionRegion) -> Result<()> {
    put_usize(output, region.region_index)?;
    output.push(encode_kind(region.kind));
    put_usize(output, region.ambiguity_rank)?;
    put_usize(output, region.grades.len())?;
    for grade in &region.grades {
        encode_grade(output, *grade)?;
    }
    Ok(())
}

fn encode_circular_families(
    output: &mut Vec<u8>,
    families: &[ArtifactCircularFamily],
) -> Result<()> {
    put_usize(output, families.len())?;
    for family in families {
        encode_circular_family(output, family)?;
    }
    Ok(())
}

fn encode_circular_family(output: &mut Vec<u8>, family: &ArtifactCircularFamily) -> Result<()> {
    encode_grade(output, family.base_grade)?;
    encode_terms(output, &family.base_class)?;
    put_u64(output, family.tolerance_bits);
    put_usize(output, family.max_iterations)?;
    put_usize(output, family.entries.len())?;
    for entry in &family.entries {
        encode_circular_entry(output, entry)?;
    }
    Ok(())
}

fn encode_circular_entry(
    output: &mut Vec<u8>,
    entry: &super::super::model::ArtifactCircularEntry,
) -> Result<()> {
    encode_grade(output, entry.grade)?;
    output.push(encode_kind(entry.extension));
    match &entry.coordinate {
        None => output.push(0),
        Some(bytes) => {
            output.push(1);
            put_usize(output, bytes.len())?;
            output.extend_from_slice(bytes);
        }
    }
    Ok(())
}

fn encode_kind(kind: ClassExtensionKind) -> u8 {
    match kind {
        ClassExtensionKind::Unique => 1,
        ClassExtensionKind::Ambiguous => 2,
        ClassExtensionKind::NoExtension => 3,
    }
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    put_u64(
        output,
        u64::try_from(value)
            .map_err(|_| crate::Error::InvalidInput("bipersistence integer exceeds u64".into()))?,
    );
    Ok(())
}

fn put_u64s(output: &mut Vec<u8>, values: &[u64]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_u64(output, *value);
    }
    Ok(())
}

fn put_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}
