use crate::proof::{ProofBar, ProofError};

use super::claim::{
    AtlasClaim, ClassClaim, CocycleTermClaim, CriticalPairClaim, ProvenanceClaim, SimplexClaim,
    SpaceClaim,
};
use super::model::ProgramProofLimits;
use super::reduction_wire::decode_reduction;
use super::wire::{
    ATLAS_WIRE_VERSION, ProgramTotals, Reader, bounded_sum, check_atlas_identity, check_bar,
    check_modulus, check_threshold, decode_bars, error, same_optional_f64,
};

const ATLAS_SPACE_HEADER_BYTES: usize = 64;
const CRITICAL_PAIR_MINIMUM_BYTES: usize = 33;
const CLASS_MINIMUM_BYTES_V1: usize = 56;
const CLASS_MINIMUM_BYTES_V2: usize = CLASS_MINIMUM_BYTES_V1 + 1;

pub(super) fn decode_atlas(
    bytes: &[u8],
    max_bytes: usize,
    program_modulus: u32,
    atom_vertices: usize,
    program_threshold: Option<f64>,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<AtlasClaim, ProofError> {
    if bytes.len() > max_bytes {
        return Err(error(format!(
            "{} atlas bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_atlas_header(
        &mut reader,
        atom_vertices,
        program_modulus,
        program_threshold,
        limits,
    )?;
    let diagram = decode_bars(&mut reader, header.bar_count)?;
    crate::proof::check_diagram(&diagram)?;
    let mut spaces = Vec::with_capacity(header.space_count);
    for _ in 0..header.space_count {
        spaces.push(decode_space(
            &mut reader,
            header.vertex_count,
            header.modulus,
            header.certificate_bytes,
            header.version == ATLAS_WIRE_VERSION,
            limits,
            totals,
        )?);
    }
    let nested = reader.take(header.certificate_bytes)?;
    let reduction = decode_reduction(nested, limits, totals)?;
    if reader.remaining() != 0 {
        return Err(error(format!(
            "{} trailing bytes after the atlas envelope",
            reader.remaining()
        )));
    }
    Ok(AtlasClaim {
        modulus: header.modulus,
        vertex_count: header.vertex_count,
        threshold: header.threshold,
        input_digest: header.input_digest,
        diagram,
        spaces,
        reduction,
    })
}

struct AtlasHeader {
    version: u16,
    modulus: u32,
    vertex_count: usize,
    threshold: Option<f64>,
    bar_count: usize,
    space_count: usize,
    certificate_bytes: usize,
    input_digest: [u8; 32],
}

fn decode_atlas_header(
    reader: &mut Reader<'_>,
    atom_vertices: usize,
    program_modulus: u32,
    program_threshold: Option<f64>,
    limits: ProgramProofLimits,
) -> Result<AtlasHeader, ProofError> {
    let header = read_atlas_header(reader, limits)?;
    check_atlas_header(
        &header,
        reader.remaining(),
        atom_vertices,
        program_modulus,
        program_threshold,
    )
}

fn read_atlas_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
) -> Result<AtlasHeader, ProofError> {
    let version = check_atlas_identity(reader)?;
    let modulus = reader.u32()?;
    let vertex_count = reader.bounded_usize("atlas vertex count", limits.max_vertices)?;
    let threshold = reader.optional_f64()?;
    let bar_count = reader.bounded_usize("atlas bar count", limits.max_bars)?;
    let space_count = reader.bounded_usize("atlas space count", limits.max_spaces)?;
    let certificate_bytes =
        reader.bounded_usize("nested reduction byte count", limits.max_certificate_bytes)?;
    let input_digest = reader.array32()?;
    Ok(AtlasHeader {
        version,
        modulus,
        vertex_count,
        threshold,
        bar_count,
        space_count,
        certificate_bytes,
        input_digest,
    })
}

fn check_atlas_header(
    header: &AtlasHeader,
    remaining: usize,
    atom_vertices: usize,
    program_modulus: u32,
    program_threshold: Option<f64>,
) -> Result<AtlasHeader, ProofError> {
    check_modulus(header.modulus)?;
    check_threshold(header.threshold)?;
    if header.vertex_count != atom_vertices {
        return Err(error("atlas vertex count differs from its atom"));
    }
    if header.modulus != program_modulus {
        return Err(error("atlas modulus differs from the program"));
    }
    if !same_optional_f64(header.threshold, program_threshold) {
        return Err(error("atlas threshold differs from the program"));
    }
    let minimum = header
        .bar_count
        .checked_mul(24)
        .and_then(|bars| {
            header
                .space_count
                .checked_mul(ATLAS_SPACE_HEADER_BYTES)
                .and_then(|spaces| bars.checked_add(spaces))
        })
        .and_then(|records| records.checked_add(header.certificate_bytes))
        .ok_or_else(|| error("atlas minimum record bytes overflow usize"))?;
    if minimum > remaining {
        return Err(error("atlas record counts exceed the remaining bytes"));
    }
    Ok(AtlasHeader {
        version: header.version,
        modulus: header.modulus,
        vertex_count: header.vertex_count,
        threshold: header.threshold,
        bar_count: header.bar_count,
        space_count: header.space_count,
        certificate_bytes: header.certificate_bytes,
        input_digest: header.input_digest,
    })
}

fn decode_space(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    certificate_bytes: usize,
    with_provenance: bool,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<SpaceClaim, ProofError> {
    let header = decode_space_header(reader, limits, totals)?;
    check_space_record_bounds(
        &header,
        reader.remaining(),
        certificate_bytes,
        with_provenance,
    )?;
    let mut critical_pairs = Vec::with_capacity(header.critical_count);
    for _ in 0..header.critical_count {
        critical_pairs.push(decode_critical_pair(reader, vertex_count)?);
    }
    let mut basis = Vec::with_capacity(header.basis_count);
    for _ in 0..header.basis_count {
        basis.push(decode_class(
            reader,
            vertex_count,
            modulus,
            certificate_bytes,
            with_provenance,
            limits,
            totals,
        )?);
    }
    Ok(SpaceClaim {
        id: header.id,
        interval: header.interval,
        critical_pairs,
        basis,
    })
}

struct SpaceHeader {
    id: [u8; 32],
    interval: ProofBar,
    basis_count: usize,
    critical_count: usize,
}

fn decode_space_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<SpaceHeader, ProofError> {
    let id = reader.array32()?;
    let interval = ProofBar {
        dimension: 1,
        birth: f64::from_bits(reader.u64()?),
        death: f64::from_bits(reader.u64()?),
    };
    check_bar(&interval)?;
    let basis_count = reader.usize()?;
    let critical_count = reader.usize()?;
    totals.basis = bounded_sum(totals.basis, basis_count, limits.max_basis, "basis classes")?;
    totals.critical_pairs = bounded_sum(
        totals.critical_pairs,
        critical_count,
        limits.max_critical_pairs,
        "critical pairs",
    )?;
    Ok(SpaceHeader {
        id,
        interval,
        basis_count,
        critical_count,
    })
}

fn check_space_record_bounds(
    header: &SpaceHeader,
    remaining: usize,
    certificate_bytes: usize,
    with_provenance: bool,
) -> Result<(), ProofError> {
    let critical_bytes = header
        .critical_count
        .checked_mul(CRITICAL_PAIR_MINIMUM_BYTES)
        .ok_or_else(|| error("critical-pair minimum bytes overflow usize"))?;
    let class_bytes = header
        .basis_count
        .checked_mul(if with_provenance {
            CLASS_MINIMUM_BYTES_V2
        } else {
            CLASS_MINIMUM_BYTES_V1
        })
        .ok_or_else(|| error("class minimum bytes overflow usize"))?;
    let required = critical_bytes
        .checked_add(class_bytes)
        .ok_or_else(|| error("space minimum bytes overflow usize"))?;
    if required > remaining.saturating_sub(certificate_bytes) {
        return Err(error("space records exceed the remaining atlas envelope"));
    }
    Ok(())
}

fn decode_class(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    certificate_bytes: usize,
    with_provenance: bool,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<ClassClaim, ProofError> {
    let header = decode_class_header(reader, limits, totals)?;
    check_class_term_bounds(&header, reader.remaining(), certificate_bytes)?;
    let terms = decode_class_terms(reader, header.term_count, vertex_count, modulus)?;
    let provenance = decode_class_provenance(reader, with_provenance)?;
    Ok(ClassClaim {
        id: header.id,
        basis_index: header.basis_index,
        scale: header.scale,
        terms,
        provenance,
    })
}

struct ClassHeader {
    id: [u8; 32],
    basis_index: usize,
    scale: f64,
    term_count: usize,
}

fn decode_class_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<ClassHeader, ProofError> {
    let id = reader.array32()?;
    let basis_index = reader.usize()?;
    let scale = f64::from_bits(reader.u64()?);
    let term_count = reader.usize()?;
    totals.cocycle_terms = bounded_sum(
        totals.cocycle_terms,
        term_count,
        limits.max_cocycle_terms,
        "cocycle terms",
    )?;
    Ok(ClassHeader {
        id,
        basis_index,
        scale,
        term_count,
    })
}

fn check_class_term_bounds(
    header: &ClassHeader,
    remaining: usize,
    certificate_bytes: usize,
) -> Result<(), ProofError> {
    let bytes = header
        .term_count
        .checked_mul(20)
        .ok_or_else(|| error("cocycle term bytes overflow usize"))?;
    if bytes > remaining.saturating_sub(certificate_bytes) {
        return Err(error("cocycle terms exceed the remaining atlas records"));
    }
    Ok(())
}

fn decode_class_terms(
    reader: &mut Reader<'_>,
    term_count: usize,
    vertex_count: usize,
    modulus: u32,
) -> Result<Vec<CocycleTermClaim>, ProofError> {
    let mut terms = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        let u = reader.usize()?;
        let v = reader.usize()?;
        let coefficient = reader.u32()?;
        if u >= v || v >= vertex_count || coefficient == 0 || coefficient >= modulus {
            return Err(error("cocycle term is outside the declared atlas"));
        }
        terms.push(CocycleTermClaim { u, v, coefficient });
    }
    Ok(terms)
}

fn decode_class_provenance(
    reader: &mut Reader<'_>,
    with_provenance: bool,
) -> Result<Option<ProvenanceClaim>, ProofError> {
    if !with_provenance {
        return Ok(None);
    }
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(decode_provenance(reader)?)),
        tag => Err(error(format!("unknown provenance tag {tag}"))),
    }
}

fn decode_provenance(reader: &mut Reader<'_>) -> Result<ProvenanceClaim, ProofError> {
    let source_graph_digest = reader.array32()?;
    let class_digest = reader.array32()?;
    let interval = ProofBar {
        dimension: reader.usize()?,
        birth: f64::from_bits(reader.u64()?),
        death: f64::from_bits(reader.u64()?),
    };
    let modulus = reader.u32()?;
    let scale = f64::from_bits(reader.u64()?);
    Ok(ProvenanceClaim {
        source_graph_digest,
        class_digest,
        interval,
        modulus,
        scale,
    })
}

fn decode_critical_pair(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> Result<CriticalPairClaim, ProofError> {
    let birth = decode_critical(reader, vertex_count)?;
    let death = match reader.u8()? {
        0 => None,
        1 => Some(decode_critical(reader, vertex_count)?),
        tag => return Err(error(format!("unknown critical-pair tag {tag}"))),
    };
    Ok(CriticalPairClaim { birth, death })
}

fn decode_critical(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> Result<SimplexClaim, ProofError> {
    let size = reader.usize()?;
    if size != 2 && size != 3 {
        return Err(error(format!(
            "critical simplex has unsupported size {size}"
        )));
    }
    let mut vertices = Vec::with_capacity(size);
    for _ in 0..size {
        let vertex = reader.usize()?;
        if vertex >= vertex_count {
            return Err(error("critical simplex vertex is outside the atlas"));
        }
        vertices.push(vertex);
    }
    Ok(SimplexClaim {
        vertices,
        value: f64::from_bits(reader.u64()?),
    })
}

#[cfg(test)]
mod tests {
    use super::super::wire::ProgramTotals;
    use super::*;

    fn put_u64(bytes: &mut Vec<u8>, value: u64) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn atlas_with_version(version: u16, basis: u64, critical: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"HOLOSATL");
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        put_u64(&mut bytes, 0);
        bytes.push(0);
        put_u64(&mut bytes, 0);
        put_u64(&mut bytes, 1);
        put_u64(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&[0; 32]);
        put_u64(&mut bytes, 0.0f64.to_bits());
        put_u64(&mut bytes, 1.0f64.to_bits());
        put_u64(&mut bytes, basis);
        put_u64(&mut bytes, critical);
        bytes
    }

    fn atlas_with_space_counts(basis: u64, critical: u64) -> Vec<u8> {
        atlas_with_version(ATLAS_WIRE_VERSION, basis, critical)
    }

    #[test]
    fn space_counts_are_checked_before_vector_allocation() {
        for (basis, critical) in [(100, 0), (0, 100)] {
            let bytes = atlas_with_space_counts(basis, critical);
            let mut totals = ProgramTotals::default();
            let error = match decode_atlas(
                &bytes,
                bytes.len(),
                2,
                0,
                None,
                ProgramProofLimits::default(),
                &mut totals,
            ) {
                Ok(_) => panic!("space count was accepted without records"),
                Err(error) => error,
            };
            assert_eq!(
                error.message(),
                "space records exceed the remaining atlas envelope"
            );
        }
    }

    #[test]
    fn atlas_version_one_is_rejected() {
        let bytes = atlas_with_version(1, 100, 0);
        let mut totals = ProgramTotals::default();
        let error = decode_atlas(
            &bytes,
            bytes.len(),
            2,
            0,
            None,
            ProgramProofLimits::default(),
            &mut totals,
        )
        .expect_err("the checker must reject the retired atlas version");
        assert_eq!(error.message(), "unsupported wire version 1");
    }
}
