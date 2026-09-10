use crate::classes::{BasisClassId, IntervalGroupId};
use crate::{ClassCorrespondence, CorrespondenceTerm, CorrespondenceVector};

use super::super::codec::{Reader, bounded_sum, put_u32, put_u64, put_usize};
use super::super::model::{
    CorrespondenceHeader, ProgramTraceDecodeLimits, ProgramTraceError, TraceTotals,
};

pub(crate) fn encode_correspondence(
    out: &mut Vec<u8>,
    correspondence: &ClassCorrespondence,
) -> std::result::Result<(), ProgramTraceError> {
    out.extend_from_slice(correspondence.old_space.as_bytes());
    out.extend_from_slice(correspondence.new_space.as_bytes());
    put_u64(out, correspondence.scale.to_bits());
    for (value, label) in [
        (correspondence.old_rank, "old rank"),
        (correspondence.new_rank, "new rank"),
        (correspondence.old_image_rank, "old image rank"),
        (correspondence.new_image_rank, "new image rank"),
        (correspondence.relation_rank, "relation rank"),
        (correspondence.basis.len(), "correspondence basis count"),
    ] {
        put_usize(out, value, label)?;
    }
    for vector in &correspondence.basis {
        put_usize(out, vector.old.len(), "old correspondence term count")?;
        put_usize(out, vector.new.len(), "new correspondence term count")?;
        encode_correspondence_terms(out, &vector.old);
        encode_correspondence_terms(out, &vector.new);
    }
    Ok(())
}

fn encode_correspondence_terms(out: &mut Vec<u8>, terms: &[CorrespondenceTerm]) {
    for term in terms {
        out.extend_from_slice(term.basis.as_bytes());
        put_u32(out, term.coefficient);
    }
}

pub(crate) fn decode_correspondence(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> std::result::Result<ClassCorrespondence, ProgramTraceError> {
    let header = decode_correspondence_header(reader)?;
    totals.correspondence_vectors = bounded_sum(
        totals.correspondence_vectors,
        header.basis_count,
        limits.max_correspondence_vectors,
        "correspondence vectors",
    )?;
    let basis = decode_correspondence_basis(reader, header.basis_count, limits, modulus, totals)?;
    Ok(ClassCorrespondence {
        old_space: header.old_space,
        new_space: header.new_space,
        scale: header.scale,
        old_rank: header.old_rank,
        new_rank: header.new_rank,
        old_image_rank: header.old_image_rank,
        new_image_rank: header.new_image_rank,
        relation_rank: header.relation_rank,
        basis,
    })
}

fn decode_correspondence_header(
    reader: &mut Reader<'_>,
) -> Result<CorrespondenceHeader, ProgramTraceError> {
    let old_space = IntervalGroupId::from_bytes(reader.array32()?);
    let new_space = IntervalGroupId::from_bytes(reader.array32()?);
    let scale = f64::from_bits(reader.u64()?);
    check_correspondence_scale(scale)?;
    let ranks = decode_correspondence_ranks(reader)?;
    let header = CorrespondenceHeader {
        old_space,
        new_space,
        scale,
        old_rank: ranks[0],
        new_rank: ranks[1],
        old_image_rank: ranks[2],
        new_image_rank: ranks[3],
        relation_rank: ranks[4],
        basis_count: ranks[5],
    };
    check_correspondence_ranks(&header)?;
    Ok(header)
}

fn check_correspondence_scale(scale: f64) -> Result<(), ProgramTraceError> {
    if !scale.is_finite() || scale < 0.0 || (scale == 0.0 && scale.to_bits() != 0) {
        return Err(ProgramTraceError::new(
            "correspondence scale must be finite and non-negative",
        ));
    }
    Ok(())
}

fn decode_correspondence_ranks(reader: &mut Reader<'_>) -> Result<[usize; 6], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

fn check_correspondence_ranks(header: &CorrespondenceHeader) -> Result<(), ProgramTraceError> {
    let inconsistent = header.old_rank == 0
        || header.new_rank == 0
        || header.old_image_rank > header.old_rank
        || header.new_image_rank > header.new_rank
        || header.relation_rank == 0
        || header.relation_rank > header.old_image_rank.min(header.new_image_rank)
        || header.relation_rank != header.basis_count;
    if inconsistent {
        return Err(ProgramTraceError::new(
            "correspondence ranks are inconsistent",
        ));
    }
    Ok(())
}

fn decode_correspondence_basis(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<CorrespondenceVector>, ProgramTraceError> {
    let bytes = count
        .checked_mul(88)
        .ok_or_else(|| ProgramTraceError::new("correspondence basis bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "correspondence basis exceeds the remaining bytes",
        ));
    }
    let mut basis = Vec::with_capacity(count);
    for _ in 0..count {
        basis.push(decode_correspondence_vector(
            reader, limits, modulus, totals,
        )?);
    }
    Ok(basis)
}

fn decode_correspondence_vector(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<CorrespondenceVector, ProgramTraceError> {
    let old_count = reader.usize()?;
    let new_count = reader.usize()?;
    check_correspondence_sides(old_count, new_count)?;
    add_correspondence_terms(totals, old_count, new_count, limits)?;
    Ok(CorrespondenceVector {
        old: decode_correspondence_terms(reader, old_count, modulus)?,
        new: decode_correspondence_terms(reader, new_count, modulus)?,
    })
}

fn check_correspondence_sides(old_count: usize, new_count: usize) -> Result<(), ProgramTraceError> {
    if old_count == 0 || new_count == 0 {
        return Err(ProgramTraceError::new(
            "a correspondence vector has an empty side",
        ));
    }
    Ok(())
}

fn add_correspondence_terms(
    totals: &mut TraceTotals,
    old_count: usize,
    new_count: usize,
    limits: ProgramTraceDecodeLimits,
) -> Result<(), ProgramTraceError> {
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        old_count,
        limits.max_correspondence_terms,
        "correspondence terms",
    )?;
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        new_count,
        limits.max_correspondence_terms,
        "correspondence terms",
    )?;
    Ok(())
}

fn decode_correspondence_terms(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
) -> std::result::Result<Vec<CorrespondenceTerm>, ProgramTraceError> {
    let bytes = count
        .checked_mul(36)
        .ok_or_else(|| ProgramTraceError::new("correspondence term bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "correspondence terms exceed the remaining bytes",
        ));
    }
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        let term = CorrespondenceTerm {
            basis: BasisClassId::from_bytes(reader.array32()?),
            coefficient: reader.u32()?,
        };
        if term.coefficient == 0 || term.coefficient >= modulus {
            return Err(ProgramTraceError::new(
                "correspondence coefficient is outside the field",
            ));
        }
        if terms
            .last()
            .is_some_and(|previous: &CorrespondenceTerm| previous.basis >= term.basis)
        {
            return Err(ProgramTraceError::new(
                "correspondence terms are not canonical",
            ));
        }
        terms.push(term);
    }
    Ok(terms)
}

#[cfg(test)]
mod tests {
    use super::super::super::model::TraceTotals;
    use super::*;

    #[test]
    fn basis_count_checks_remaining_bytes_before_allocation() {
        let mut reader = Reader::new(&[]);
        let mut totals = TraceTotals::default();
        let error = decode_correspondence_basis(
            &mut reader,
            10_000,
            ProgramTraceDecodeLimits::default(),
            3,
            &mut totals,
        )
        .unwrap_err();
        assert!(error.message().contains("correspondence basis"));
    }
}
