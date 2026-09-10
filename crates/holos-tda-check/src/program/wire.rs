use crate::is_prime;
use crate::proof::{ProofBar, ProofError, check_diagram};

use super::atlas_wire::decode_atlas;
use super::claim::{ProgramAtomClaim, ProgramClaim};
use super::model::ProgramProofLimits;

pub(super) const PROGRAM_MAGIC: &[u8; 8] = b"HOLOSPRG";
pub(super) const ATLAS_MAGIC: &[u8; 8] = b"HOLOSATL";
pub(super) const WIRE_VERSION: u16 = 1;
pub(super) const ATLAS_WIRE_VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;

pub(super) fn decode_program(
    bytes: &[u8],
    limits: ProgramProofLimits,
) -> Result<ProgramClaim, ProofError> {
    if bytes.len() > limits.max_bytes {
        return Err(error(format!(
            "{} bytes exceed the decoder limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_program_header(&mut reader, limits)?;
    check_minimum_bytes(&reader, header.bar_count, header.atom_count)?;
    let diagram = decode_bars(&mut reader, header.bar_count)?;
    check_diagram(&diagram)?;
    let mut totals = ProgramTotals::default();
    let atoms = decode_atoms(&mut reader, &header, limits, &mut totals)?;
    if reader.remaining() != 0 {
        return Err(error(format!(
            "{} trailing bytes after the program envelope",
            reader.remaining()
        )));
    }
    Ok(ProgramClaim {
        modulus: header.modulus,
        vertex_count: header.vertex_count,
        threshold: header.threshold,
        input_digest: header.input_digest,
        diagram,
        atoms,
    })
}

struct ProgramHeader {
    modulus: u32,
    vertex_count: usize,
    threshold: Option<f64>,
    bar_count: usize,
    atom_count: usize,
    input_digest: [u8; 32],
}

fn decode_program_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
) -> Result<ProgramHeader, ProofError> {
    let header = read_program_header(reader, limits)?;
    check_modulus(header.modulus)?;
    check_threshold(header.threshold)?;
    Ok(header)
}

fn read_program_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
) -> Result<ProgramHeader, ProofError> {
    check_identity(reader, PROGRAM_MAGIC)?;
    let modulus = reader.u32()?;
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let threshold = reader.optional_f64()?;
    let bar_count = reader.bounded_usize("bar count", limits.max_bars)?;
    let atom_count = reader.bounded_usize("atom count", limits.max_atoms)?;
    let input_digest = reader.array32()?;
    Ok(ProgramHeader {
        modulus,
        vertex_count,
        threshold,
        bar_count,
        atom_count,
        input_digest,
    })
}

fn decode_atoms(
    reader: &mut Reader<'_>,
    header: &ProgramHeader,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<Vec<ProgramAtomClaim>, ProofError> {
    let mut atoms = Vec::with_capacity(header.atom_count);
    for _ in 0..header.atom_count {
        atoms.push(decode_atom(
            reader,
            header.vertex_count,
            header.modulus,
            header.threshold,
            limits,
            totals,
        )?);
    }
    Ok(atoms)
}

#[derive(Default)]
pub(super) struct ProgramTotals {
    pub(super) vertices: usize,
    pub(super) edges: usize,
    pub(super) atlas_bytes: usize,
    pub(super) basis: usize,
    pub(super) critical_pairs: usize,
    pub(super) cocycle_terms: usize,
    pub(super) reduction_terms: usize,
}

fn decode_atom(
    reader: &mut Reader<'_>,
    program_vertices: usize,
    modulus: u32,
    threshold: Option<f64>,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<ProgramAtomClaim, ProofError> {
    let header = decode_atom_header(reader, limits, totals)?;
    check_atom_record_bounds(&header, reader.remaining())?;
    let vertices = decode_atom_vertices(reader, header.vertex_count, program_vertices, header.id)?;
    let edges = decode_atom_edges(reader, header.edge_count)?;
    let nested = reader.take(header.atlas_bytes)?;
    let atlas_limit = limits.max_nested_atlas_bytes.min(header.atlas_bytes);
    let atlas = decode_atlas(
        nested,
        atlas_limit,
        modulus,
        header.vertex_count,
        threshold,
        limits,
        totals,
    )?;
    Ok(ProgramAtomClaim {
        id: header.id,
        vertices,
        edges,
        atlas,
    })
}

struct AtomHeader {
    id: usize,
    vertex_count: usize,
    edge_count: usize,
    atlas_bytes: usize,
}

fn decode_atom_header(
    reader: &mut Reader<'_>,
    limits: ProgramProofLimits,
    totals: &mut ProgramTotals,
) -> Result<AtomHeader, ProofError> {
    let id = reader.usize()?;
    let vertex_count = reader.usize()?;
    let edge_count = reader.usize()?;
    let atlas_bytes = reader.usize()?;
    totals.vertices = bounded_sum(
        totals.vertices,
        vertex_count,
        limits.max_atom_vertices,
        "atom vertices",
    )?;
    totals.edges = bounded_sum(
        totals.edges,
        edge_count,
        limits.max_atom_edges,
        "atom edges",
    )?;
    totals.atlas_bytes = bounded_sum(
        totals.atlas_bytes,
        atlas_bytes,
        limits.max_atlas_bytes,
        "nested atlas bytes",
    )?;
    Ok(AtomHeader {
        id,
        vertex_count,
        edge_count,
        atlas_bytes,
    })
}

fn check_atom_record_bounds(header: &AtomHeader, remaining: usize) -> Result<(), ProofError> {
    let minimum = header
        .vertex_count
        .checked_mul(8)
        .and_then(|bytes| {
            header
                .edge_count
                .checked_mul(16)
                .and_then(|edges| bytes.checked_add(edges))
        })
        .and_then(|bytes| bytes.checked_add(header.atlas_bytes))
        .ok_or_else(|| error("atom record bytes overflow usize"))?;
    if minimum > remaining {
        return Err(error("atom record exceeds the remaining bytes"));
    }
    Ok(())
}

fn decode_atom_vertices(
    reader: &mut Reader<'_>,
    count: usize,
    program_vertices: usize,
    id: usize,
) -> Result<Vec<usize>, ProofError> {
    let vertices = (0..count)
        .map(|_| reader.usize())
        .collect::<Result<Vec<_>, _>>()?;
    if vertices.iter().any(|&vertex| vertex >= program_vertices) {
        return Err(error(format!(
            "atom {id} contains a vertex outside the program graph"
        )));
    }
    Ok(vertices)
}

fn decode_atom_edges(reader: &mut Reader<'_>, count: usize) -> Result<Vec<[usize; 2]>, ProofError> {
    (0..count)
        .map(|_| Ok([reader.usize()?, reader.usize()?]))
        .collect::<Result<Vec<_>, ProofError>>()
}

pub(super) fn decode_bars(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    (0..count)
        .map(|_| {
            let bar = ProofBar {
                dimension: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            };
            check_bar(&bar)?;
            Ok(bar)
        })
        .collect()
}

pub(super) fn check_bar(bar: &ProofBar) -> Result<(), ProofError> {
    if bar.dimension > 1 || invalid_birth(bar.birth) || invalid_death(bar.death) {
        return Err(error("diagram contains an invalid bar"));
    }
    if bar.death <= bar.birth {
        return Err(error("diagram contains an invalid bar"));
    }
    Ok(())
}

fn invalid_birth(value: f64) -> bool {
    !value.is_finite() || value < 0.0 || is_negative_zero(value)
}

fn invalid_death(value: f64) -> bool {
    value.is_nan()
        || value < 0.0
        || is_negative_zero(value)
        || (value.is_infinite() && value.is_sign_negative())
}

pub(super) fn check_identity(reader: &mut Reader<'_>, magic: &[u8; 8]) -> Result<(), ProofError> {
    if reader.take(8)? != magic {
        return Err(error("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(error(format!("unsupported wire version {version}")));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(error(format!("unsupported scalar codec {codec}")));
    }
    Ok(())
}

pub(super) fn check_atlas_identity(reader: &mut Reader<'_>) -> Result<u16, ProofError> {
    if reader.take(8)? != ATLAS_MAGIC {
        return Err(error("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != ATLAS_WIRE_VERSION {
        return Err(error(format!("unsupported wire version {version}")));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(error(format!("unsupported scalar codec {codec}")));
    }
    Ok(version)
}

pub(super) fn check_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
        return Err(error(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )));
    }
    Ok(())
}

pub(super) fn check_threshold(threshold: Option<f64>) -> Result<(), ProofError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 || is_negative_zero(value) {
        return Err(error(format!(
            "threshold must be non-negative, got {value}"
        )));
    }
    Ok(())
}

pub(super) fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

pub(super) fn same_optional_f64(left: Option<f64>, right: Option<f64>) -> bool {
    left.map(f64::to_bits) == right.map(f64::to_bits)
}

fn check_minimum_bytes(
    reader: &Reader<'_>,
    bar_count: usize,
    atom_count: usize,
) -> Result<(), ProofError> {
    let minimum = bar_count
        .checked_mul(24)
        .and_then(|bars| {
            atom_count
                .checked_mul(32)
                .and_then(|atoms| bars.checked_add(atoms))
        })
        .ok_or_else(|| error("program minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(error("program record counts exceed the remaining bytes"));
    }
    Ok(())
}

pub(super) fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| error(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(error(format!("{next} {label} exceed the limit {limit}")));
    }
    Ok(next)
}

pub(super) fn error(message: impl Into<String>) -> ProofError {
    ProofError::new(message)
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

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| error("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(error("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    pub(super) fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    pub(super) fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    pub(super) fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?).map_err(|_| error("wire integer does not fit usize"))
    }

    pub(super) fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > limit {
            return Err(error(format!("{label} {value} exceeds the limit {limit}")));
        }
        Ok(value)
    }

    pub(super) fn optional_f64(&mut self) -> Result<Option<f64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(error(format!("unknown optional-float tag {tag}"))),
        }
    }

    pub(super) fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}
