use crate::{
    BipersistenceMap, BipersistenceNode, BipersistenceRectangle, BipersistenceRegion,
    BipersistenceTerm, ClassExtension, ClassExtensionKind, ClassExtensionRegion,
    CohomologyClassAtlas, Error, Result,
};

use super::super::model::{
    ArtifactCircularEntry, ArtifactCircularFamily, ArtifactEdge, BipersistenceArtifact,
};
use super::super::{
    BipersistenceArtifactLimits, BipersistenceRectangleClaim, BipersistenceRegionClaim,
};
use super::super::{F64_BITS_CODEC, MAGIC, VERSION};
use super::reader::Reader;

struct DecodedHeader {
    vertex_count: usize,
    edges: Vec<ArtifactEdge>,
    threshold_bits: u64,
    modulus: u32,
    scale_bits: Vec<u64>,
    minimum_degrees: Vec<usize>,
}

struct DecodedClaims {
    nodes: Vec<BipersistenceNode>,
    cover_maps: Vec<BipersistenceMap>,
    rectangles: Vec<BipersistenceRectangleClaim>,
    regions: Vec<BipersistenceRegionClaim>,
    class_atlases: Vec<CohomologyClassAtlas>,
    circular_families: Vec<ArtifactCircularFamily>,
}

pub(super) fn decode_artifact(
    bytes: &[u8],
    limits: BipersistenceArtifactLimits,
) -> Result<BipersistenceArtifact> {
    validate_byte_limit(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    validate_format(&mut reader)?;
    let header = decode_header(&mut reader, limits)?;
    let claims = decode_claims(&mut reader, limits)?;
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        return Err(Error::InvalidInput(
            "trailing bytes follow the bipersistence artifact".into(),
        ));
    }
    let artifact = BipersistenceArtifact {
        vertex_count: header.vertex_count,
        edges: header.edges,
        threshold_bits: header.threshold_bits,
        modulus: header.modulus,
        scale_bits: header.scale_bits,
        minimum_degrees: header.minimum_degrees,
        nodes: claims.nodes,
        cover_maps: claims.cover_maps,
        rectangles: claims.rectangles,
        regions: claims.regions,
        class_atlases: claims.class_atlases,
        circular_families: claims.circular_families,
        digest,
    };
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "bipersistence artifact encoding is not canonical".into(),
        ));
    }
    Ok(artifact)
}

fn validate_byte_limit(bytes: &[u8], limits: BipersistenceArtifactLimits) -> Result<()> {
    if bytes.len() < 32 || bytes.len() > limits.max_bytes {
        return Err(Error::InvalidInput(
            "bipersistence artifact is truncated or exceeds its byte limit".into(),
        ));
    }
    Ok(())
}

fn validate_format(reader: &mut Reader<'_>) -> Result<()> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(Error::InvalidInput(
            "unsupported bipersistence artifact".into(),
        ));
    }
    Ok(())
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<DecodedHeader> {
    let vertex_count =
        reader.bounded_usize("vertex count", limits.bifiltration.complex.max_vertices)?;
    let edge_count = reader.bounded_usize("source edge count", limits.max_source_edges)?;
    let edges = decode_edges(reader, edge_count)?;
    let threshold_bits = reader.u64()?;
    let modulus = reader.u32()?;
    let scale_bits = reader.u64s("scale count", limits.bifiltration.max_scales)?;
    let minimum_degrees = reader.usizes(
        "density-level count",
        limits.bifiltration.max_density_levels,
    )?;
    Ok(DecodedHeader {
        vertex_count,
        edges,
        threshold_bits,
        modulus,
        scale_bits,
        minimum_degrees,
    })
}

fn decode_edges(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ArtifactEdge>> {
    (0..count)
        .map(|_| {
            Ok(ArtifactEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                value_bits: reader.u64()?,
            })
        })
        .collect()
}

fn decode_claims(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<DecodedClaims> {
    Ok(DecodedClaims {
        nodes: decode_nodes(reader, limits.module.max_nodes)?,
        cover_maps: decode_maps(reader, limits)?,
        rectangles: decode_rectangles(reader, limits.max_rectangles)?,
        regions: decode_regions(reader, limits.max_regions, limits.module.max_nodes)?,
        class_atlases: decode_atlases(reader, limits)?,
        circular_families: decode_circular_families(reader, limits)?,
    })
}

fn decode_regions(
    reader: &mut Reader<'_>,
    maximum: usize,
    maximum_grades: usize,
) -> Result<Vec<BipersistenceRegionClaim>> {
    let count = reader.bounded_usize("region count", maximum)?;
    (0..count)
        .map(|_| {
            let grade_count = reader.bounded_usize("region grade count", maximum_grades)?;
            let grades = (0..grade_count)
                .map(|_| decode_grade(reader))
                .collect::<Result<Vec<_>>>()?;
            Ok(BipersistenceRegionClaim {
                region: BipersistenceRegion::new(grades)?,
                rank: reader.usize()?,
            })
        })
        .collect()
}

fn decode_grade(reader: &mut Reader<'_>) -> Result<crate::Bigrade> {
    Ok(crate::Bigrade::new(reader.usize()?, reader.usize()?))
}

fn decode_nodes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<BipersistenceNode>> {
    let count = reader.bounded_usize("node count", maximum)?;
    (0..count)
        .map(|_| {
            Ok(BipersistenceNode {
                grade: decode_grade(reader)?,
                space: crate::CohomologySpaceId::from_bytes(reader.array32()?),
                rank: reader.usize()?,
            })
        })
        .collect()
}

fn decode_terms(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<BipersistenceTerm>> {
    let count = reader.bounded_usize("coordinate term count", maximum)?;
    (0..count)
        .map(|_| {
            Ok(BipersistenceTerm {
                basis_index: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_maps(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<Vec<BipersistenceMap>> {
    let count = reader.bounded_usize("cover-map count", limits.module.max_cover_maps)?;
    let mut total_terms = 0usize;
    (0..count)
        .map(|_| {
            let lower_grade = decode_grade(reader)?;
            let upper_grade = decode_grade(reader)?;
            let source_space = crate::CohomologySpaceId::from_bytes(reader.array32()?);
            let target_space = crate::CohomologySpaceId::from_bytes(reader.array32()?);
            let rank = reader.usize()?;
            let column_count =
                reader.bounded_usize("map column count", limits.module.max_total_rank)?;
            let columns = (0..column_count)
                .map(|_| {
                    let source_basis_index = reader.usize()?;
                    let image = decode_terms(
                        reader,
                        limits.module.max_map_terms.saturating_sub(total_terms),
                    )?;
                    total_terms = total_terms.checked_add(image.len()).ok_or_else(|| {
                        Error::InvalidInput("bipersistence map term count overflows".into())
                    })?;
                    Ok(crate::BipersistenceMapColumn {
                        source_basis_index,
                        image,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(BipersistenceMap {
                lower_grade,
                upper_grade,
                source_space,
                target_space,
                rank,
                columns,
            })
        })
        .collect()
}

fn decode_rectangles(
    reader: &mut Reader<'_>,
    maximum: usize,
) -> Result<Vec<BipersistenceRectangleClaim>> {
    let count = reader.bounded_usize("rectangle count", maximum)?;
    (0..count)
        .map(|_| {
            Ok(BipersistenceRectangleClaim {
                rectangle: BipersistenceRectangle::new(
                    decode_grade(reader)?,
                    decode_grade(reader)?,
                )?,
                rank: reader.usize()?,
            })
        })
        .collect()
}

fn decode_atlases(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<Vec<CohomologyClassAtlas>> {
    let count = reader.bounded_usize("class-atlas count", limits.max_class_atlases)?;
    (0..count)
        .map(|_| {
            let base_grade = decode_grade(reader)?;
            let base_class = decode_terms(reader, limits.module.max_total_rank)?;
            let extension_count =
                reader.bounded_usize("class extension count", limits.module.max_nodes)?;
            let extensions = (0..extension_count)
                .map(|_| decode_extension(reader, limits))
                .collect::<Result<Vec<_>>>()?;
            let region_count =
                reader.bounded_usize("class region count", limits.module.max_nodes)?;
            let regions = (0..region_count)
                .map(|_| decode_region(reader, limits))
                .collect::<Result<Vec<_>>>()?;
            Ok(CohomologyClassAtlas {
                base_grade,
                base_class,
                extensions,
                regions,
            })
        })
        .collect()
}

fn decode_circular_families(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<Vec<ArtifactCircularFamily>> {
    let count = reader.bounded_usize("circular-family count", limits.max_circular_families)?;
    (0..count)
        .map(|_| {
            let base_grade = decode_grade(reader)?;
            let base_class = decode_terms(reader, limits.module.max_total_rank)?;
            let tolerance_bits = reader.u64()?;
            let max_iterations = reader.usize()?;
            let entry_count =
                reader.bounded_usize("circular-family entry count", limits.module.max_nodes)?;
            let entries = (0..entry_count)
                .map(|_| {
                    let grade = decode_grade(reader)?;
                    let extension = decode_kind(reader.u8()?)?;
                    let coordinate = match reader.u8()? {
                        0 => None,
                        1 => {
                            let count = reader.bounded_usize(
                                "nested circular coordinate byte count",
                                limits.max_coordinate_bytes,
                            )?;
                            Some(reader.take(count)?.to_vec())
                        }
                        _ => {
                            return Err(Error::InvalidInput(
                                "invalid nested circular coordinate flag".into(),
                            ));
                        }
                    };
                    Ok(ArtifactCircularEntry {
                        grade,
                        extension,
                        coordinate,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(ArtifactCircularFamily {
                base_grade,
                base_class,
                tolerance_bits,
                max_iterations,
                entries,
            })
        })
        .collect()
}

fn decode_extension(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<ClassExtension> {
    let grade = decode_grade(reader)?;
    let kind = decode_kind(reader.u8()?)?;
    let class = decode_terms(reader, limits.module.max_total_rank)?;
    let row_count = reader.bounded_usize("ambiguity row count", limits.module.max_total_rank)?;
    let ambiguity = (0..row_count)
        .map(|_| decode_terms(reader, limits.module.max_total_rank))
        .collect::<Result<Vec<_>>>()?;
    Ok(ClassExtension {
        grade,
        kind,
        class,
        ambiguity,
    })
}

fn decode_region(
    reader: &mut Reader<'_>,
    limits: BipersistenceArtifactLimits,
) -> Result<ClassExtensionRegion> {
    let region_index = reader.usize()?;
    let kind = decode_kind(reader.u8()?)?;
    let ambiguity_rank = reader.usize()?;
    let count = reader.bounded_usize("region grade count", limits.module.max_nodes)?;
    let grades = (0..count)
        .map(|_| decode_grade(reader))
        .collect::<Result<Vec<_>>>()?;
    Ok(ClassExtensionRegion {
        region_index,
        kind,
        ambiguity_rank,
        grades,
    })
}

fn decode_kind(value: u8) -> Result<ClassExtensionKind> {
    match value {
        1 => Ok(ClassExtensionKind::Unique),
        2 => Ok(ClassExtensionKind::Ambiguous),
        3 => Ok(ClassExtensionKind::NoExtension),
        _ => Err(Error::InvalidInput(
            "invalid bipersistence class-extension kind".into(),
        )),
    }
}
