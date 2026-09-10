use super::super::{MODULUS_LIMIT, ProofBar, ProofError, ProofLimits, Reader, is_prime};
use super::cells::{decode_cells, decode_columns, decode_key};
use super::digest::count_cells;
use super::model::{CertificateHeader, Step, VerifiedCertificate};
use super::verify::verify_certificate;
use super::{F64_BITS_CODEC, MAGIC, VERSION};

pub(crate) fn decode_verified(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCertificate, ProofError> {
    let certificate = decode_certificate(bytes, limits)?;
    verify_certificate(&certificate, limits)?;
    Ok(certificate)
}

pub(super) fn decode_certificate(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCertificate, ProofError> {
    if bytes.len() > limits.max_bytes {
        return Err(ProofError::new(format!(
            "{} bytes exceed the limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_certificate_header(&mut reader, limits)?;
    let input = decode_cells(&mut reader, header.max_dim, header.modulus, limits)?;
    let input_count = count_cells(&input);
    let steps = decode_steps(&mut reader, header.max_dim, input_count, limits)?;
    let core = decode_cells(&mut reader, header.max_dim, header.modulus, limits)?;
    let columns = decode_columns(&mut reader, header.max_dim, header.modulus, limits)?;
    let diagram = decode_diagram(&mut reader, header.max_dim, limits)?;
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "trailing bytes after the relative-interface certificate",
        ));
    }

    Ok(VerifiedCertificate {
        digest,
        max_dim: header.max_dim,
        modulus: header.modulus,
        protected_vertices: header.protected_vertices,
        input,
        steps,
        core,
        columns,
        diagram,
    })
}

pub(super) fn decode_certificate_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<CertificateHeader, ProofError> {
    decode_certificate_prefix(reader)?;
    let max_dim = reader.bounded_usize("relative dimension", limits.max_dimension)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let protected_vertices = decode_protected_vertices(reader, limits)?;
    Ok(CertificateHeader {
        max_dim,
        modulus,
        protected_vertices,
    })
}

pub(super) fn decode_certificate_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC {
        return Err(ProofError::new("wrong relative-interface magic bytes"));
    }
    if reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new(
            "unsupported relative-interface wire version or scalar codec",
        ));
    }
    Ok(())
}

pub(super) fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(
            "relative-interface modulus is not a supported prime",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn decode_protected_vertices(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("protected vertex count", limits.max_vertices)?;
    let vertices = (0..count)
        .map(|_| reader.usize())
        .collect::<Result<Vec<_>, _>>()?;
    if vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        Err(ProofError::new(
            "relative-interface protected vertices are not canonical",
        ))
    } else {
        Ok(vertices)
    }
}

pub(super) fn decode_steps(
    reader: &mut Reader<'_>,
    max_dim: usize,
    input_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Step>, ProofError> {
    let count = reader.bounded_usize("relative cancellation count", input_count / 2)?;
    (0..count)
        .map(|_| {
            Ok(Step {
                upper: decode_key(reader, max_dim + 2, limits.max_vertices)?,
                lower: decode_key(reader, max_dim + 1, limits.max_vertices)?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

pub(super) fn decode_diagram(
    reader: &mut Reader<'_>,
    max_dim: usize,
    limits: ProofLimits,
) -> Result<Vec<ProofBar>, ProofError> {
    let count = reader.bounded_usize("relative bar count", limits.max_bars)?;
    (0..count)
        .map(|_| {
            Ok(ProofBar {
                dimension: reader.bounded_usize("bar dimension", max_dim)?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}
