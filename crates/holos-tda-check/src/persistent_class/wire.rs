use sha2::{Digest, Sha256};

use crate::{ProofBar, ProofError, ProofLimits, Reader, is_prime};

use super::model::{
    DecodedPersistentClass, PersistenceCycleTerm, PersistenceTriangleTerm, PersistentCocycleTerm,
    PersistentCriticalPair, PersistentSourceEdge,
};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSPC\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;

struct GraphHeader {
    modulus: u32,
    vertex_count: usize,
    source: Vec<PersistentSourceEdge>,
    threshold: Option<f64>,
}

struct ClassHeader {
    group_id: [u8; 32],
    class_id: [u8; 32],
    basis_index: usize,
    interval: ProofBar,
    scale: f64,
}

struct Witnesses {
    cocycle: Vec<PersistentCocycleTerm>,
    pair: PersistentCriticalPair,
    cycle: Vec<PersistenceCycleTerm>,
    bounding_chain: Vec<PersistenceTriangleTerm>,
}

pub(super) fn decode(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<DecodedPersistentClass, ProofError> {
    let payload_digest = verify_digest(bytes, limits)?;
    let payload = &bytes[..bytes.len() - 32];
    decode_payload(payload, payload_digest, limits)
}

fn decode_payload(
    payload: &[u8],
    payload_digest: [u8; 32],
    limits: ProofLimits,
) -> Result<DecodedPersistentClass, ProofError> {
    let mut reader = Reader::new(payload);
    let graph = decode_graph_header(&mut reader, limits)?;
    let class = decode_class_header(&mut reader)?;
    let witnesses = decode_witnesses(&mut reader, graph.vertex_count, graph.modulus, limits)?;
    reject_trailing(&reader)?;
    Ok(DecodedPersistentClass {
        vertex_count: graph.vertex_count,
        source: graph.source,
        threshold: graph.threshold,
        modulus: graph.modulus,
        group_id: class.group_id,
        class_id: class.class_id,
        basis_index: class.basis_index,
        interval: class.interval,
        scale: class.scale,
        cocycle: witnesses.cocycle,
        pair: witnesses.pair,
        cycle: witnesses.cycle,
        bounding_chain: witnesses.bounding_chain,
        payload_digest,
    })
}

fn decode_graph_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<GraphHeader, ProofError> {
    decode_prefix(reader)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let vertex_count =
        reader.bounded_usize("persistent-class vertex count", limits.max_vertices)?;
    validate_vertex_count(vertex_count)?;
    let source = decode_source(reader, vertex_count, limits)?;
    let threshold = decode_threshold(reader)?;
    Ok(GraphHeader {
        modulus,
        vertex_count,
        source,
        threshold,
    })
}

fn validate_vertex_count(vertex_count: usize) -> Result<(), ProofError> {
    if vertex_count == 0 {
        return Err(ProofError::new("persistent-class source has no vertices"));
    }
    Ok(())
}

fn decode_class_header(reader: &mut Reader<'_>) -> Result<ClassHeader, ProofError> {
    let group_id = reader.array32()?;
    let class_id = reader.array32()?;
    let basis_index = reader.usize()?;
    let interval = decode_interval(reader)?;
    let scale = decode_scale(reader, interval)?;
    Ok(ClassHeader {
        group_id,
        class_id,
        basis_index,
        interval,
        scale,
    })
}

fn decode_witnesses(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Witnesses, ProofError> {
    let mut term_total = 0usize;
    let cocycle = decode_cocycle(reader, vertex_count, modulus, limits, &mut term_total)?;
    let pair = decode_pair(reader, vertex_count)?;
    let cycle = decode_cycle(reader, vertex_count, modulus, limits, &mut term_total)?;
    let bounding_chain =
        decode_bounding_chain(reader, vertex_count, modulus, limits, &mut term_total)?;
    Ok(Witnesses {
        cocycle,
        pair,
        cycle,
        bounding_chain,
    })
}

fn decode_pair(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> Result<PersistentCriticalPair, ProofError> {
    let birth = decode_edge(reader, vertex_count, "persistent-class birth edge")?;
    let death = decode_death(reader, vertex_count)?;
    Ok(PersistentCriticalPair { birth, death })
}

fn reject_trailing(reader: &Reader<'_>) -> Result<(), ProofError> {
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "persistent-class artifact has trailing payload bytes",
        ));
    }
    Ok(())
}

fn verify_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() < 32 || bytes.len() > limits.max_bytes {
        return Err(ProofError::new(
            "persistent-class artifact is truncated or exceeds its byte limit",
        ));
    }
    let payload_len = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    let digest: [u8; 32] = bytes[payload_len..]
        .try_into()
        .expect("32-byte persistent-class digest");
    if digest != expected {
        return Err(ProofError::new(
            "persistent-class digest differs from its content",
        ));
    }
    Ok(digest)
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported persistent-class artifact"));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "persistent-class modulus is not a supported prime",
        ));
    }
    Ok(())
}

fn decode_source(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Vec<PersistentSourceEdge>, ProofError> {
    let count = reader.bounded_usize("persistent-class source edge count", limits.max_edges)?;
    reader.require_bytes(count, 24, "persistent-class source edges")?;
    decode_source_edges(reader, count, vertex_count)
}

fn decode_source_edges(
    reader: &mut Reader<'_>,
    count: usize,
    vertex_count: usize,
) -> Result<Vec<PersistentSourceEdge>, ProofError> {
    let mut source = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let edge = decode_source_edge(reader, vertex_count, previous)?;
        previous = Some((edge.u, edge.v));
        source.push(edge);
    }
    Ok(source)
}

fn decode_source_edge(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    previous: Option<(usize, usize)>,
) -> Result<PersistentSourceEdge, ProofError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    let value = f64::from_bits(reader.u64()?);
    validate_source_edge(u, v, value, vertex_count, previous)?;
    Ok(PersistentSourceEdge { u, v, value })
}

fn validate_source_edge(
    u: usize,
    v: usize,
    value: f64,
    vertex_count: usize,
    previous: Option<(usize, usize)>,
) -> Result<(), ProofError> {
    if u >= v {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    if v >= vertex_count {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    if !value.is_finite() {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    if value < 0.0 {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    if is_negative_zero(value) {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    if previous.is_some_and(|edge| edge >= (u, v)) {
        return Err(ProofError::new(
            "persistent-class source edges are not canonical",
        ));
    }
    Ok(())
}

fn decode_threshold(reader: &mut Reader<'_>) -> Result<Option<f64>, ProofError> {
    let threshold = match reader.u8()? {
        0 => None,
        1 => Some(f64::from_bits(reader.u64()?)),
        _ => {
            return Err(ProofError::new("persistent-class threshold tag is invalid"));
        }
    };
    validate_threshold(threshold)?;
    Ok(threshold)
}

fn validate_threshold(threshold: Option<f64>) -> Result<(), ProofError> {
    if threshold.is_some_and(|value| value.is_nan() || value < 0.0 || is_negative_zero(value)) {
        return Err(ProofError::new(
            "persistent-class threshold is not canonical",
        ));
    }
    Ok(())
}

fn decode_interval(reader: &mut Reader<'_>) -> Result<ProofBar, ProofError> {
    let interval = ProofBar {
        dimension: 1,
        birth: f64::from_bits(reader.u64()?),
        death: f64::from_bits(reader.u64()?),
    };
    validate_interval(interval)?;
    Ok(interval)
}

fn validate_interval(interval: ProofBar) -> Result<(), ProofError> {
    validate_interval_birth(interval.birth)?;
    validate_interval_death(interval.death)?;
    if interval.death.is_finite() && interval.death <= interval.birth {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    Ok(())
}

fn validate_interval_birth(birth: f64) -> Result<(), ProofError> {
    if !birth.is_finite() {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    if birth < 0.0 {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    if is_negative_zero(birth) {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    Ok(())
}

fn validate_interval_death(death: f64) -> Result<(), ProofError> {
    if death.is_nan() {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    if death < 0.0 {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    if is_negative_zero(death) {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    if death.is_infinite() && death.is_sign_negative() {
        return Err(ProofError::new(
            "persistent-class interval is not canonical",
        ));
    }
    Ok(())
}

fn decode_scale(reader: &mut Reader<'_>, interval: ProofBar) -> Result<f64, ProofError> {
    let scale = f64::from_bits(reader.u64()?);
    if !scale.is_finite()
        || scale < 0.0
        || is_negative_zero(scale)
        || scale < interval.birth
        || (interval.death.is_finite() && scale >= interval.death)
    {
        return Err(ProofError::new(
            "persistent-class representative scale is outside its interval",
        ));
    }
    Ok(scale)
}

fn decode_cocycle(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: ProofLimits,
    term_total: &mut usize,
) -> Result<Vec<PersistentCocycleTerm>, ProofError> {
    let count = decode_term_count(reader, limits, term_total, "persistent-class cocycle")?;
    if count == 0 {
        return Err(ProofError::new("persistent-class cocycle has no terms"));
    }
    reader.require_bytes(count, 20, "persistent-class cocycle terms")?;
    let mut terms = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let term = decode_field_edge_term(reader, vertex_count, modulus)?;
        if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
            return Err(ProofError::new(
                "persistent-class cocycle terms are not canonical",
            ));
        }
        previous = Some((term.u, term.v));
        terms.push(PersistentCocycleTerm {
            u: term.u,
            v: term.v,
            coefficient: term.coefficient,
        });
    }
    if terms[0].coefficient != 1 {
        return Err(ProofError::new(
            "persistent-class cocycle is not normalized",
        ));
    }
    Ok(terms)
}

fn decode_cycle(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: ProofLimits,
    term_total: &mut usize,
) -> Result<Vec<PersistenceCycleTerm>, ProofError> {
    let count = decode_term_count(reader, limits, term_total, "persistent-class cycle")?;
    if count == 0 {
        return Err(ProofError::new("persistent-class cycle has no terms"));
    }
    reader.require_bytes(count, 20, "persistent-class cycle terms")?;
    let mut terms = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let term = decode_field_edge_term(reader, vertex_count, modulus)?;
        if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
            return Err(ProofError::new(
                "persistent-class cycle terms are not canonical",
            ));
        }
        previous = Some((term.u, term.v));
        terms.push(PersistenceCycleTerm {
            u: term.u,
            v: term.v,
            coefficient: term.coefficient,
        });
    }
    Ok(terms)
}

fn decode_bounding_chain(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: ProofLimits,
    term_total: &mut usize,
) -> Result<Vec<PersistenceTriangleTerm>, ProofError> {
    let count = decode_chain_count(reader, limits, term_total)?;
    reader.require_bytes(count, 28, "persistent-class bounding-chain terms")?;
    decode_chain_terms(reader, count, vertex_count, modulus)
}

fn decode_chain_count(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    term_total: &mut usize,
) -> Result<usize, ProofError> {
    let count = reader.bounded_usize(
        "persistent-class bounding-chain term count",
        limits.max_triangles.min(limits.max_terms),
    )?;
    *term_total = term_total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("persistent-class term count overflows"))?;
    if *term_total > limits.max_terms {
        return Err(ProofError::new(
            "persistent-class terms exceed their total limit",
        ));
    }
    Ok(count)
}

fn decode_chain_terms(
    reader: &mut Reader<'_>,
    count: usize,
    vertex_count: usize,
    modulus: u32,
) -> Result<Vec<PersistenceTriangleTerm>, ProofError> {
    let mut terms = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let term = decode_chain_term(reader, vertex_count, modulus, previous)?;
        previous = Some((term.vertices[0], term.vertices[1], term.vertices[2]));
        terms.push(term);
    }
    Ok(terms)
}

fn decode_chain_term(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    previous: Option<(usize, usize, usize)>,
) -> Result<PersistenceTriangleTerm, ProofError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    let w = reader.usize()?;
    let coefficient = reader.u32()?;
    validate_chain_term(u, v, w, coefficient, vertex_count, modulus, previous)?;
    Ok(PersistenceTriangleTerm {
        vertices: [u, v, w],
        coefficient,
    })
}

fn validate_chain_term(
    u: usize,
    v: usize,
    w: usize,
    coefficient: u32,
    vertex_count: usize,
    modulus: u32,
    previous: Option<(usize, usize, usize)>,
) -> Result<(), ProofError> {
    if u >= v {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    if v >= w {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    if w >= vertex_count {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    if coefficient == 0 {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    if coefficient >= modulus {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    if previous.is_some_and(|triangle| triangle >= (u, v, w)) {
        return Err(ProofError::new(
            "persistent-class bounding-chain terms are not canonical",
        ));
    }
    Ok(())
}

fn decode_term_count(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    total: &mut usize,
    name: &str,
) -> Result<usize, ProofError> {
    let count = reader.bounded_usize(&format!("{name} term count"), limits.max_terms)?;
    *total = total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("persistent-class term count overflows"))?;
    if *total > limits.max_terms {
        return Err(ProofError::new(
            "persistent-class terms exceed their total limit",
        ));
    }
    Ok(count)
}

struct FieldEdgeTerm {
    u: usize,
    v: usize,
    coefficient: u32,
}

fn decode_field_edge_term(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
) -> Result<FieldEdgeTerm, ProofError> {
    let edge = decode_edge(reader, vertex_count, "persistent-class field edge")?;
    let coefficient = reader.u32()?;
    if coefficient == 0 || coefficient >= modulus {
        return Err(ProofError::new(
            "persistent-class field coefficient is invalid",
        ));
    }
    Ok(FieldEdgeTerm {
        u: edge[0],
        v: edge[1],
        coefficient,
    })
}

fn decode_edge(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    name: &str,
) -> Result<[usize; 2], ProofError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    if u >= v || v >= vertex_count {
        return Err(ProofError::new(format!("{name} is not canonical")));
    }
    Ok([u, v])
}

fn decode_death(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> Result<Option<[usize; 3]>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_death_triangle(reader, vertex_count).map(Some),
        _ => Err(ProofError::new("persistent-class death tag is invalid")),
    }
}

fn decode_death_triangle(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> Result<[usize; 3], ProofError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    let w = reader.usize()?;
    validate_death_triangle(u, v, w, vertex_count)?;
    Ok([u, v, w])
}

fn validate_death_triangle(
    u: usize,
    v: usize,
    w: usize,
    vertex_count: usize,
) -> Result<(), ProofError> {
    if u >= v || v >= w || w >= vertex_count {
        return Err(ProofError::new(
            "persistent-class death triangle is not canonical",
        ));
    }
    Ok(())
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}
