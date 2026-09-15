use sha2::{Digest, Sha256};

use super::claims::*;
use crate::{MODULUS_LIMIT, ProofError, Reader, is_prime};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSBP\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

struct ClaimHeader {
    vertex_count: usize,
    edges: Vec<WeightedEdge>,
    threshold_bits: u64,
    modulus: u32,
    scale_bits: Vec<u64>,
    minimum_degrees: Vec<usize>,
}

struct ClaimSections {
    nodes: Vec<NodeClaim>,
    cover_maps: Vec<MapClaim>,
    rectangles: Vec<RectangleClaim>,
    rank_regions: Vec<RankRegionClaim>,
    class_atlases: Vec<AtlasClaim>,
    circular_families: Vec<CircularFamilyClaim>,
}

pub(super) fn decode_claim(
    bytes: &[u8],
    limits: BipersistenceProofLimits,
) -> Result<Claim, ProofError> {
    verify_digest(bytes, limits.proof.max_bytes)?;
    let payload = &bytes[..bytes.len() - 32];
    let mut reader = Reader::new(payload);
    let header = decode_header(&mut reader, limits)?;
    let sections = decode_sections(&mut reader, limits)?;
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "bipersistence artifact has trailing payload bytes",
        ));
    }
    Ok(Claim {
        vertex_count: header.vertex_count,
        edges: header.edges,
        threshold_bits: header.threshold_bits,
        modulus: header.modulus,
        scale_bits: header.scale_bits,
        minimum_degrees: header.minimum_degrees,
        nodes: sections.nodes,
        cover_maps: sections.cover_maps,
        rectangles: sections.rectangles,
        rank_regions: sections.rank_regions,
        class_atlases: sections.class_atlases,
        circular_families: sections.circular_families,
    })
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<ClaimHeader, ProofError> {
    decode_prefix(reader)?;
    let vertex_count =
        reader.bounded_usize("bipersistence vertex count", limits.proof.max_vertices)?;
    let edges = decode_edges(reader, vertex_count, limits.proof.max_edges)?;
    let threshold_bits = reader.u64()?;
    validate_threshold(threshold_bits)?;
    let modulus = decode_modulus(reader)?;
    let scale_bits = decode_u64s(reader, "bipersistence scale count", limits.max_scales)?;
    let minimum_degrees = decode_usizes(
        reader,
        "bipersistence density-level count",
        limits.max_density_levels,
    )?;
    Ok(ClaimHeader {
        vertex_count,
        edges,
        threshold_bits,
        modulus,
        scale_bits,
        minimum_degrees,
    })
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported bipersistence artifact"));
    }
    Ok(())
}

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<WeightedEdge>, ProofError> {
    let count = reader.bounded_usize("bipersistence source edge count", maximum)?;
    let edges = (0..count)
        .map(|_| {
            Ok(WeightedEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                value_bits: reader.u64()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    validate_edges(vertex_count, &edges)?;
    Ok(edges)
}

fn validate_threshold(threshold_bits: u64) -> Result<(), ProofError> {
    let threshold = f64::from_bits(threshold_bits);
    if !threshold.is_finite() || threshold < 0.0 || threshold == 0.0 && threshold_bits != 0 {
        return Err(ProofError::new("bipersistence threshold is not canonical"));
    }
    Ok(())
}

fn decode_modulus(reader: &mut Reader<'_>) -> Result<u32, ProofError> {
    let modulus = reader.u32()?;
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "bipersistence modulus is not a supported prime",
        ));
    }
    Ok(modulus)
}

fn decode_sections(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<ClaimSections, ProofError> {
    Ok(ClaimSections {
        nodes: decode_nodes(reader, limits.proof.max_nodes)?,
        cover_maps: decode_maps(reader, limits)?,
        rectangles: decode_rectangles(reader, limits.max_rectangles)?,
        rank_regions: decode_rank_regions(reader, limits)?,
        class_atlases: decode_atlases(reader, limits)?,
        circular_families: decode_circular_families(reader, limits)?,
    })
}

fn verify_digest(bytes: &[u8], maximum: usize) -> Result<(), ProofError> {
    if bytes.len() < 32 || bytes.len() > maximum {
        return Err(ProofError::new(
            "bipersistence artifact is truncated or exceeds its byte limit",
        ));
    }
    let split = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-bipersistence-v2");
    hash.update(&bytes[..split]);
    let expected: [u8; 32] = hash.finalize().into();
    if bytes[split..] != expected {
        return Err(ProofError::new(
            "bipersistence artifact digest differs from its content",
        ));
    }
    Ok(())
}

fn validate_edges(vertex_count: usize, edges: &[WeightedEdge]) -> Result<(), ProofError> {
    let mut previous = None;
    for edge in edges {
        let value = f64::from_bits(edge.value_bits);
        if edge.u >= edge.v
            || edge.v >= vertex_count
            || !value.is_finite()
            || value < 0.0
            || value == 0.0 && edge.value_bits != 0
            || previous.is_some_and(|key| key >= (edge.u, edge.v))
        {
            return Err(ProofError::new(
                "bipersistence source edges are not canonical",
            ));
        }
        previous = Some((edge.u, edge.v));
    }
    Ok(())
}

fn decode_grade(reader: &mut Reader<'_>) -> Result<Grade, ProofError> {
    Ok(Grade {
        scale: reader.usize()?,
        density: reader.usize()?,
    })
}

fn decode_nodes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<NodeClaim>, ProofError> {
    let count = reader.bounded_usize("bipersistence node count", maximum)?;
    (0..count)
        .map(|_| {
            Ok(NodeClaim {
                grade: decode_grade(reader)?,
                space: reader.array32()?,
                rank: reader.usize()?,
            })
        })
        .collect()
}

fn decode_terms(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<Term>, ProofError> {
    let count = reader.bounded_usize("bipersistence coordinate term count", maximum)?;
    (0..count)
        .map(|_| {
            Ok(Term {
                basis: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_maps(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Vec<MapClaim>, ProofError> {
    let count =
        reader.bounded_usize("bipersistence cover-map count", limits.proof.max_references)?;
    let mut total_terms = 0usize;
    (0..count)
        .map(|_| {
            let lower = decode_grade(reader)?;
            let upper = decode_grade(reader)?;
            let source_space = reader.array32()?;
            let target_space = reader.array32()?;
            let rank = reader.usize()?;
            let column_count =
                reader.bounded_usize("bipersistence map column count", limits.proof.max_terms)?;
            let columns = (0..column_count)
                .map(|_| {
                    let source = reader.usize()?;
                    let image = decode_terms(reader, limits.proof.max_terms - total_terms)?;
                    total_terms = total_terms
                        .checked_add(image.len())
                        .ok_or_else(|| ProofError::new("bipersistence map term count overflows"))?;
                    Ok(Column { source, image })
                })
                .collect::<Result<Vec<_>, ProofError>>()?;
            Ok(MapClaim {
                lower,
                upper,
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
) -> Result<Vec<RectangleClaim>, ProofError> {
    let count = reader.bounded_usize("bipersistence rectangle count", maximum)?;
    let rectangles = (0..count)
        .map(|_| {
            Ok(RectangleClaim {
                lower: decode_grade(reader)?,
                upper: decode_grade(reader)?,
                rank: reader.usize()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    if rectangles
        .windows(2)
        .any(|pair| (pair[0].lower, pair[0].upper) >= (pair[1].lower, pair[1].upper))
    {
        return Err(ProofError::new(
            "bipersistence rectangles are not in canonical order",
        ));
    }
    Ok(rectangles)
}

fn decode_rank_regions(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Vec<RankRegionClaim>, ProofError> {
    let count = reader.bounded_usize("bipersistence region count", limits.max_regions)?;
    let regions = (0..count)
        .map(|_| {
            let grade_count =
                reader.bounded_usize("bipersistence region grade count", limits.proof.max_nodes)?;
            let grades = (0..grade_count)
                .map(|_| decode_grade(reader))
                .collect::<Result<Vec<_>, ProofError>>()?;
            if grades.is_empty() || grades.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(ProofError::new(
                    "bipersistence region grades are not canonical",
                ));
            }
            Ok(RankRegionClaim {
                grades,
                rank: reader.usize()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    if regions
        .windows(2)
        .any(|pair| pair[0].grades >= pair[1].grades)
    {
        return Err(ProofError::new(
            "bipersistence regions are not in canonical order",
        ));
    }
    Ok(regions)
}

fn decode_atlases(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Vec<AtlasClaim>, ProofError> {
    let count =
        reader.bounded_usize("bipersistence class-atlas count", limits.max_class_atlases)?;
    let atlases = (0..count)
        .map(|_| {
            let base_grade = decode_grade(reader)?;
            let base_class = decode_terms(reader, limits.proof.max_terms)?;
            let extension_count =
                reader.bounded_usize("bipersistence extension count", limits.proof.max_nodes)?;
            let extensions = (0..extension_count)
                .map(|_| decode_extension(reader, limits))
                .collect::<Result<Vec<_>, ProofError>>()?;
            let region_count =
                reader.bounded_usize("bipersistence region count", limits.proof.max_nodes)?;
            let regions = (0..region_count)
                .map(|_| decode_region(reader, limits))
                .collect::<Result<Vec<_>, ProofError>>()?;
            Ok(AtlasClaim {
                base_grade,
                base_class,
                extensions,
                regions,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    if atlases.windows(2).any(|pair| {
        (pair[0].base_grade, &pair[0].base_class) >= (pair[1].base_grade, &pair[1].base_class)
    }) {
        return Err(ProofError::new(
            "bipersistence class atlases are not in canonical order",
        ));
    }
    Ok(atlases)
}

fn decode_circular_families(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Vec<CircularFamilyClaim>, ProofError> {
    let count = reader.bounded_usize(
        "bipersistence circular-family count",
        limits.max_circular_families,
    )?;
    let families = (0..count)
        .map(|_| decode_circular_family(reader, limits))
        .collect::<Result<Vec<_>, ProofError>>()?;
    if families.windows(2).any(|pair| {
        (pair[0].base_grade, &pair[0].base_class) >= (pair[1].base_grade, &pair[1].base_class)
    }) {
        return Err(ProofError::new(
            "bipersistence circular families are not in canonical order",
        ));
    }
    Ok(families)
}

fn decode_circular_family(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<CircularFamilyClaim, ProofError> {
    let base_grade = decode_grade(reader)?;
    let base_class = decode_terms(reader, limits.proof.max_terms)?;
    let tolerance_bits = reader.u64()?;
    let tolerance = f64::from_bits(tolerance_bits);
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > limits.circular.max_tolerance {
        return Err(ProofError::new(
            "bipersistence circular tolerance exceeds its limit",
        ));
    }
    let max_iterations = reader.bounded_usize(
        "bipersistence circular iteration count",
        limits.max_circular_iterations,
    )?;
    if max_iterations == 0 {
        return Err(ProofError::new(
            "bipersistence circular iteration count is zero",
        ));
    }
    let entries = decode_circular_entries(reader, limits)?;
    Ok(CircularFamilyClaim {
        base_grade,
        base_class,
        tolerance_bits,
        max_iterations,
        entries,
    })
}

fn decode_circular_entries(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Vec<CircularEntryClaim>, ProofError> {
    let count = reader.bounded_usize(
        "bipersistence circular-family entry count",
        limits.proof.max_nodes,
    )?;
    (0..count)
        .map(|_| decode_circular_entry(reader, limits))
        .collect()
}

fn decode_circular_entry(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<CircularEntryClaim, ProofError> {
    Ok(CircularEntryClaim {
        grade: decode_grade(reader)?,
        extension: decode_kind(reader.u8()?)?,
        status: decode_status(reader, limits)?,
    })
}

fn decode_status(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<CircularFamilyStatus, ProofError> {
    match reader.u8()? {
        0 => Ok(CircularFamilyStatus::NotAttempted),
        1 => Ok(CircularFamilyStatus::LiftFailed),
        2 => Ok(CircularFamilyStatus::SolveFailed),
        3 => {
            let count = reader.bounded_usize(
                "nested circular coordinate byte count",
                limits.circular.proof.max_bytes,
            )?;
            if count == 0 {
                return Err(ProofError::new(
                    "successful circular status has no coordinate",
                ));
            }
            Ok(CircularFamilyStatus::Success(reader.take(count)?.to_vec()))
        }
        _ => Err(ProofError::new(
            "bipersistence circular-family status is invalid",
        )),
    }
}

fn decode_extension(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Extension, ProofError> {
    let grade = decode_grade(reader)?;
    let kind = decode_kind(reader.u8()?)?;
    let class = decode_terms(reader, limits.proof.max_terms)?;
    let row_count =
        reader.bounded_usize("bipersistence ambiguity row count", limits.proof.max_terms)?;
    let ambiguity = (0..row_count)
        .map(|_| decode_terms(reader, limits.proof.max_terms))
        .collect::<Result<Vec<_>, ProofError>>()?;
    Ok(Extension {
        grade,
        kind,
        class,
        ambiguity,
    })
}

fn decode_region(
    reader: &mut Reader<'_>,
    limits: BipersistenceProofLimits,
) -> Result<Region, ProofError> {
    let index = reader.usize()?;
    let kind = decode_kind(reader.u8()?)?;
    let ambiguity_rank = reader.usize()?;
    let grade_count =
        reader.bounded_usize("bipersistence region grade count", limits.proof.max_nodes)?;
    let grades = (0..grade_count)
        .map(|_| decode_grade(reader))
        .collect::<Result<Vec<_>, ProofError>>()?;
    Ok(Region {
        index,
        kind,
        ambiguity_rank,
        grades,
    })
}

fn decode_kind(value: u8) -> Result<ExtensionKind, ProofError> {
    match value {
        1 => Ok(ExtensionKind::Unique),
        2 => Ok(ExtensionKind::Ambiguous),
        3 => Ok(ExtensionKind::NoExtension),
        _ => Err(ProofError::new("bipersistence extension kind is invalid")),
    }
}

fn decode_u64s(
    reader: &mut Reader<'_>,
    label: &str,
    maximum: usize,
) -> Result<Vec<u64>, ProofError> {
    let count = reader.bounded_usize(label, maximum)?;
    (0..count).map(|_| reader.u64()).collect()
}

fn decode_usizes(
    reader: &mut Reader<'_>,
    label: &str,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize(label, maximum)?;
    (0..count).map(|_| reader.usize()).collect()
}
