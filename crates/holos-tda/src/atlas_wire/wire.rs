use crate::{
    Bar, BasisClassId, CertificateLimits, Cocycle, CocycleTerm, Diagram, IntervalGroupId,
    PersistentClass, PersistentClassProvenance, PersistentClassSpace, ReductionCertificate,
};

use super::codec::{Reader, put_optional_f64, put_u16, put_u32, put_u64, put_usize};
use super::model::{
    AtlasArtifact, AtlasArtifactError, AtlasDecodeLimits, AtlasHeader, AtlasTotals, SpaceHeader,
};
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

mod critical;

use critical::{decode_critical_pairs, encode_critical_pairs};

pub(crate) fn encode_atlas_header(
    out: &mut Vec<u8>,
    artifact: &AtlasArtifact,
    certificate_bytes: usize,
) -> std::result::Result<(), AtlasArtifactError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    put_u32(out, artifact.modulus);
    put_usize(out, artifact.vertex_count, "vertex count")?;
    put_optional_f64(out, artifact.threshold);
    put_usize(out, artifact.explained.diagram.bars.len(), "bar count")?;
    put_usize(out, artifact.explained.spaces.len(), "space count")?;
    put_usize(out, certificate_bytes, "certificate byte count")?;
    out.extend_from_slice(&artifact.input_digest);
    Ok(())
}

pub(crate) fn encode_bars(
    out: &mut Vec<u8>,
    diagram: &Diagram,
) -> std::result::Result<(), AtlasArtifactError> {
    for bar in &diagram.bars {
        put_usize(out, bar.dim, "bar dimension")?;
        put_u64(out, bar.birth.to_bits());
        put_u64(out, bar.death.to_bits());
    }
    Ok(())
}

pub(crate) fn encode_spaces(
    out: &mut Vec<u8>,
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    for space in spaces {
        encode_space(out, space)?;
    }
    Ok(())
}

pub(crate) fn encode_space(
    out: &mut Vec<u8>,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    out.extend_from_slice(space.id.as_bytes());
    put_u64(out, space.interval.birth.to_bits());
    put_u64(out, space.interval.death.to_bits());
    put_usize(out, space.basis.len(), "basis count")?;
    put_usize(out, space.critical_pairs.len(), "critical-pair count")?;
    encode_critical_pairs(out, &space.critical_pairs)?;
    encode_basis(out, &space.basis)
}

pub(crate) fn encode_basis(
    out: &mut Vec<u8>,
    basis: &[PersistentClass],
) -> std::result::Result<(), AtlasArtifactError> {
    for class in basis {
        encode_class(out, class)?;
    }
    Ok(())
}

pub(crate) fn encode_class(
    out: &mut Vec<u8>,
    class: &PersistentClass,
) -> std::result::Result<(), AtlasArtifactError> {
    out.extend_from_slice(class.id.as_bytes());
    put_usize(out, class.basis_index, "basis index")?;
    put_u64(out, class.cocycle.scale.to_bits());
    put_usize(out, class.cocycle.terms.len(), "cocycle term count")?;
    for term in &class.cocycle.terms {
        put_usize(out, term.u, "term endpoint")?;
        put_usize(out, term.v, "term endpoint")?;
        put_u32(out, term.coefficient);
    }
    encode_provenance(out, class.provenance.as_ref())?;
    Ok(())
}

fn encode_provenance(
    out: &mut Vec<u8>,
    provenance: Option<&PersistentClassProvenance>,
) -> std::result::Result<(), AtlasArtifactError> {
    match provenance {
        None => out.push(0),
        Some(provenance) => {
            out.push(1);
            out.extend_from_slice(provenance.source_graph_digest());
            out.extend_from_slice(provenance.class_digest());
            put_usize(
                out,
                provenance.interval().dim,
                "provenance interval dimension",
            )?;
            put_u64(out, provenance.interval().birth.to_bits());
            put_u64(out, provenance.interval().death.to_bits());
            put_u32(out, provenance.modulus());
            put_u64(out, provenance.scale().to_bits());
        }
    }
    Ok(())
}

pub(crate) fn validate_atlas_size(
    bytes: &[u8],
    limits: AtlasDecodeLimits,
) -> std::result::Result<(), AtlasArtifactError> {
    if bytes.len() > limits.max_bytes {
        Err(AtlasArtifactError::new(format!(
            "{} bytes exceed the decoder limit {}",
            bytes.len(),
            limits.max_bytes
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn decode_atlas_prefix(
    reader: &mut Reader<'_>,
) -> std::result::Result<(), AtlasArtifactError> {
    if reader.take(8)? != MAGIC {
        return Err(AtlasArtifactError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(AtlasArtifactError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(AtlasArtifactError::new(format!(
            "unsupported scalar codec {codec}"
        )));
    }
    Ok(())
}

pub(crate) fn decode_atlas_header(
    reader: &mut Reader<'_>,
    limits: AtlasDecodeLimits,
) -> std::result::Result<AtlasHeader, AtlasArtifactError> {
    Ok(AtlasHeader {
        modulus: reader.u32()?,
        vertex_count: reader.bounded_usize("vertex count", limits.max_vertices)?,
        threshold: reader.optional_f64()?,
        bars: reader.bounded_usize("bar count", limits.max_bars)?,
        spaces: reader.bounded_usize("space count", limits.max_spaces)?,
        certificate_bytes: reader
            .bounded_usize("certificate byte count", limits.max_certificate_bytes)?,
        input_digest: reader.array32()?,
    })
}

pub(crate) fn validate_minimum_records(
    reader: &Reader<'_>,
    header: &AtlasHeader,
) -> std::result::Result<(), AtlasArtifactError> {
    let minimum = header
        .bars
        .checked_mul(24)
        .and_then(|bars| {
            header
                .spaces
                .checked_mul(64)
                .and_then(|spaces| bars.checked_add(spaces))
        })
        .and_then(|records| records.checked_add(header.certificate_bytes))
        .ok_or_else(|| AtlasArtifactError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        Err(AtlasArtifactError::new(format!(
            "record counts need at least {minimum} bytes, only {} remain",
            reader.remaining()
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn decode_bars(
    reader: &mut Reader<'_>,
    count: usize,
) -> std::result::Result<Vec<Bar>, AtlasArtifactError> {
    (0..count)
        .map(|_| {
            Ok(Bar {
                dim: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}

pub(crate) fn decode_spaces(
    reader: &mut Reader<'_>,
    header: &AtlasHeader,
    limits: AtlasDecodeLimits,
) -> std::result::Result<Vec<PersistentClassSpace>, AtlasArtifactError> {
    let mut totals = AtlasTotals::default();
    let mut spaces = Vec::with_capacity(header.spaces);
    for _ in 0..header.spaces {
        spaces.push(decode_space(reader, header, &mut totals, limits)?);
    }
    Ok(spaces)
}

pub(crate) fn decode_space(
    reader: &mut Reader<'_>,
    atlas: &AtlasHeader,
    totals: &mut AtlasTotals,
    limits: AtlasDecodeLimits,
) -> std::result::Result<PersistentClassSpace, AtlasArtifactError> {
    let header = decode_space_header(reader, totals, limits)?;
    let critical_pairs = decode_critical_pairs(reader, atlas.vertex_count, header.critical_pairs)?;
    let basis = decode_basis(reader, atlas, &header, totals, limits)?;
    Ok(PersistentClassSpace {
        id: header.id,
        interval: header.interval,
        basis,
        critical_pairs,
    })
}

pub(crate) fn decode_space_header(
    reader: &mut Reader<'_>,
    totals: &mut AtlasTotals,
    limits: AtlasDecodeLimits,
) -> std::result::Result<SpaceHeader, AtlasArtifactError> {
    let id = IntervalGroupId::from_bytes(reader.array32()?);
    let interval = Bar {
        dim: 1,
        birth: f64::from_bits(reader.u64()?),
        death: f64::from_bits(reader.u64()?),
    };
    let basis = reader.usize()?;
    totals.basis = add_atlas_total(totals.basis, basis, limits.max_basis, "basis classes")?;
    let critical_pairs = reader.usize()?;
    totals.critical_pairs = add_atlas_total(
        totals.critical_pairs,
        critical_pairs,
        limits.max_critical_pairs,
        "critical pairs",
    )?;
    Ok(SpaceHeader {
        id,
        interval,
        basis,
        critical_pairs,
    })
}

pub(crate) fn add_atlas_total(
    total: usize,
    add: usize,
    maximum: usize,
    label: &str,
) -> std::result::Result<usize, AtlasArtifactError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| AtlasArtifactError::new(format!("{label} count overflows usize")))?;
    if total > maximum {
        Err(AtlasArtifactError::new(format!(
            "{total} {label} exceed the limit {maximum}"
        )))
    } else {
        Ok(total)
    }
}

pub(crate) fn decode_basis(
    reader: &mut Reader<'_>,
    atlas: &AtlasHeader,
    space: &SpaceHeader,
    totals: &mut AtlasTotals,
    limits: AtlasDecodeLimits,
) -> std::result::Result<Vec<PersistentClass>, AtlasArtifactError> {
    let mut basis = Vec::with_capacity(space.basis);
    for _ in 0..space.basis {
        basis.push(decode_class(reader, atlas, space, totals, limits)?);
    }
    Ok(basis)
}

pub(crate) fn decode_class(
    reader: &mut Reader<'_>,
    atlas: &AtlasHeader,
    space: &SpaceHeader,
    totals: &mut AtlasTotals,
    limits: AtlasDecodeLimits,
) -> std::result::Result<PersistentClass, AtlasArtifactError> {
    let id = BasisClassId::from_bytes(reader.array32()?);
    let basis_index = reader.usize()?;
    let scale = f64::from_bits(reader.u64()?);
    let count = reader.usize()?;
    totals.terms = add_atlas_total(totals.terms, count, limits.max_terms, "cocycle terms")?;
    validate_term_bytes(reader, count, atlas.certificate_bytes)?;
    let terms = decode_terms(reader, count)?;
    let provenance = decode_provenance(reader)?;
    Ok(PersistentClass {
        id,
        group_id: space.id,
        basis_index,
        interval: space.interval,
        cocycle: Cocycle {
            modulus: atlas.modulus,
            scale,
            terms,
        },
        provenance,
    })
}

fn decode_provenance(
    reader: &mut Reader<'_>,
) -> std::result::Result<Option<PersistentClassProvenance>, AtlasArtifactError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_provenance_body(reader).map(Some),
        tag => Err(AtlasArtifactError::new(format!(
            "unknown provenance tag {tag}"
        ))),
    }
}

fn decode_provenance_body(
    reader: &mut Reader<'_>,
) -> std::result::Result<PersistentClassProvenance, AtlasArtifactError> {
    let source_graph_digest = reader.array32()?;
    let class_digest = reader.array32()?;
    let interval = Bar {
        dim: reader.usize()?,
        birth: f64::from_bits(reader.u64()?),
        death: f64::from_bits(reader.u64()?),
    };
    let modulus = reader.u32()?;
    let scale = f64::from_bits(reader.u64()?);
    Ok(PersistentClassProvenance::from_parts(
        source_graph_digest,
        class_digest,
        interval,
        modulus,
        scale,
    ))
}

pub(crate) fn validate_term_bytes(
    reader: &Reader<'_>,
    count: usize,
    certificate_bytes: usize,
) -> std::result::Result<(), AtlasArtifactError> {
    let bytes = count
        .checked_mul(20)
        .ok_or_else(|| AtlasArtifactError::new("cocycle term bytes overflow usize"))?;
    if bytes > reader.remaining().saturating_sub(certificate_bytes) {
        Err(AtlasArtifactError::new(
            "cocycle terms exceed the remaining record bytes",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn decode_terms(
    reader: &mut Reader<'_>,
    count: usize,
) -> std::result::Result<Vec<CocycleTerm>, AtlasArtifactError> {
    (0..count)
        .map(|_| {
            Ok(CocycleTerm {
                u: reader.usize()?,
                v: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

pub(crate) fn decode_nested_certificate(
    reader: &mut Reader<'_>,
    header: &AtlasHeader,
    limits: &mut CertificateLimits,
) -> std::result::Result<ReductionCertificate, AtlasArtifactError> {
    let nested = reader.take(header.certificate_bytes)?;
    limits.max_bytes = limits.max_bytes.min(header.certificate_bytes);
    ReductionCertificate::decode(nested, *limits)
        .map_err(|error| AtlasArtifactError::new(error.to_string()))
}

pub(crate) fn finish_atlas_decode(
    reader: &Reader<'_>,
) -> std::result::Result<(), AtlasArtifactError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(AtlasArtifactError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )))
    }
}
