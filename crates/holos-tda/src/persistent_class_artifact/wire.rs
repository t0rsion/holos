use sha2::{Digest, Sha256};

use crate::certificate::{CertificateError, CertificateLimits};
use crate::field::is_prime;
use crate::{CriticalPair, PersistentClass, SparseDistanceMatrix};

use super::{
    F64_BITS_CODEC, MAGIC, PersistenceCycleTerm, PersistenceTriangleTerm, PersistentClassArtifact,
    VERSION,
};

/// Encode the artifact payload followed by its SHA-256 digest.
impl PersistentClassArtifact {
    /// Encode canonical `HOLOSPC` version 1 bytes.
    ///
    /// Resource limits are checked for every call. Semantic fields were
    /// validated when the private artifact was built.
    pub fn encode(&self, limits: CertificateLimits) -> Result<Vec<u8>, CertificateError> {
        validate_encode_limits(self, limits)?;
        let payload_len = encoded_payload_len(self)?;
        validate_encoded_length(payload_len, limits)?;
        let payload = encode_payload(self, payload_len)?;
        debug_assert_eq!(payload.len(), payload_len);
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        let mut output = payload;
        output.extend_from_slice(&digest);
        Ok(output)
    }
}

fn validate_encode_limits(
    artifact: &PersistentClassArtifact,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    validate_artifact_limits(&artifact.source, limits)?;
    validate_total_terms(artifact, limits)?;
    validate_chain_limit(artifact, limits)?;
    super::build::count_triangles(
        &artifact.source,
        artifact.threshold.unwrap_or(f64::INFINITY),
        limits.max_triangles,
    )?;
    Ok(())
}

fn validate_encoded_length(
    payload_len: usize,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    let total = payload_len
        .checked_add(32)
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))?;
    if total > limits.max_bytes {
        return Err(certificate_error(
            "persistent class artifact exceeds its byte limit",
        ));
    }
    Ok(())
}

pub(super) fn encode_payload(
    artifact: &PersistentClassArtifact,
    capacity: usize,
) -> Result<Vec<u8>, CertificateError> {
    let mut output = Vec::with_capacity(capacity);
    encode_header(&mut output, artifact)?;
    encode_source(&mut output, artifact)?;
    encode_threshold(&mut output, artifact.threshold);
    encode_class_header(&mut output, artifact)?;
    encode_cocycle(&mut output, &artifact.class)?;
    encode_pair(&mut output, &artifact.critical_pair)?;
    encode_cycle(&mut output, &artifact.cycle)?;
    encode_chain(&mut output, &artifact.bounding_chain)?;
    Ok(output)
}

fn encode_header(
    output: &mut Vec<u8>,
    artifact: &PersistentClassArtifact,
) -> Result<(), CertificateError> {
    output.extend_from_slice(MAGIC);
    put_u16(output, VERSION);
    output.push(F64_BITS_CODEC);
    put_u32(output, artifact.class.cocycle.modulus);
    put_u64(output, to_u64(artifact.source.len(), "vertex count")?);
    put_u64(
        output,
        to_u64(artifact.source.num_edges(), "source edge count")?,
    );
    Ok(())
}

fn encode_source(
    output: &mut Vec<u8>,
    artifact: &PersistentClassArtifact,
) -> Result<(), CertificateError> {
    for (u, v, value) in artifact.source.edges() {
        put_u64(output, to_u64(u, "source edge endpoint")?);
        put_u64(output, to_u64(v, "source edge endpoint")?);
        put_u64(output, value.to_bits());
    }
    Ok(())
}

fn encode_class_header(
    output: &mut Vec<u8>,
    artifact: &PersistentClassArtifact,
) -> Result<(), CertificateError> {
    output.extend_from_slice(artifact.class.group_id.as_bytes());
    output.extend_from_slice(artifact.class.id.as_bytes());
    put_u64(output, to_u64(artifact.class.basis_index, "basis index")?);
    put_u64(output, artifact.class.interval.birth.to_bits());
    put_u64(output, artifact.class.interval.death.to_bits());
    put_u64(output, artifact.class.cocycle.scale.to_bits());
    Ok(())
}

fn encoded_payload_len(artifact: &PersistentClassArtifact) -> Result<usize, CertificateError> {
    let mut length = 8 + 2 + 1 + 4 + 8 + 8;
    add_payload_component(&mut length, source_bytes(artifact))?;
    length = checked_add(length, threshold_bytes(artifact.threshold))?;
    add_payload_component(&mut length, class_bytes(artifact))?;
    length = checked_add(length, pair_bytes(artifact))?;
    add_payload_component(&mut length, cycle_bytes(artifact))?;
    add_payload_component(&mut length, chain_bytes(artifact))?;
    Ok(length)
}

fn add_payload_component(
    length: &mut usize,
    component: Result<usize, CertificateError>,
) -> Result<(), CertificateError> {
    let component = component?;
    *length = checked_add(*length, component)?;
    Ok(())
}

fn source_bytes(artifact: &PersistentClassArtifact) -> Result<usize, CertificateError> {
    checked_mul(artifact.source.num_edges(), 24)
}

fn threshold_bytes(threshold: Option<f64>) -> usize {
    threshold.map_or(1, |_| 9)
}

fn class_bytes(artifact: &PersistentClassArtifact) -> Result<usize, CertificateError> {
    let mut length = 32 + 32 + 8 + 8 + 8 + 8;
    length = checked_add(
        length,
        collection_with_count_bytes(artifact.class.cocycle.terms.len(), 20)?,
    )?;
    Ok(length)
}

fn pair_bytes(artifact: &PersistentClassArtifact) -> usize {
    16 + 1 + artifact.critical_pair.death.as_ref().map_or(0, |_| 24)
}

fn cycle_bytes(artifact: &PersistentClassArtifact) -> Result<usize, CertificateError> {
    collection_with_count_bytes(artifact.cycle.len(), 20)
}

fn chain_bytes(artifact: &PersistentClassArtifact) -> Result<usize, CertificateError> {
    collection_with_count_bytes(artifact.bounding_chain.len(), 28)
}

fn collection_with_count_bytes(count: usize, width: usize) -> Result<usize, CertificateError> {
    checked_add(8, checked_mul(count, width)?)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, CertificateError> {
    left.checked_mul(right)
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))
}

fn checked_add(left: usize, right: usize) -> Result<usize, CertificateError> {
    left.checked_add(right)
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))
}

fn encode_threshold(output: &mut Vec<u8>, threshold: Option<f64>) {
    match threshold {
        None => output.push(0),
        Some(value) => {
            output.push(1);
            put_u64(output, value.to_bits());
        }
    }
}

fn encode_cocycle(output: &mut Vec<u8>, class: &PersistentClass) -> Result<(), CertificateError> {
    put_u64(
        output,
        to_u64(class.cocycle.terms.len(), "cocycle term count")?,
    );
    for term in &class.cocycle.terms {
        put_u64(output, to_u64(term.u, "cocycle endpoint")?);
        put_u64(output, to_u64(term.v, "cocycle endpoint")?);
        put_u32(output, term.coefficient);
    }
    Ok(())
}

fn encode_pair(output: &mut Vec<u8>, pair: &CriticalPair) -> Result<(), CertificateError> {
    let [u, v] = [pair.birth.vertices[0], pair.birth.vertices[1]];
    put_u64(output, to_u64(u, "birth endpoint")?);
    put_u64(output, to_u64(v, "birth endpoint")?);
    match &pair.death {
        None => output.push(0),
        Some(death) => {
            let [u, v, w] = [death.vertices[0], death.vertices[1], death.vertices[2]];
            output.push(1);
            put_u64(output, to_u64(u, "death vertex")?);
            put_u64(output, to_u64(v, "death vertex")?);
            put_u64(output, to_u64(w, "death vertex")?);
        }
    }
    Ok(())
}

fn encode_cycle(
    output: &mut Vec<u8>,
    cycle: &[PersistenceCycleTerm],
) -> Result<(), CertificateError> {
    put_u64(output, to_u64(cycle.len(), "cycle term count")?);
    for term in cycle {
        put_u64(output, to_u64(term.u, "cycle endpoint")?);
        put_u64(output, to_u64(term.v, "cycle endpoint")?);
        put_u32(output, term.coefficient);
    }
    Ok(())
}

fn encode_chain(
    output: &mut Vec<u8>,
    chain: &[PersistenceTriangleTerm],
) -> Result<(), CertificateError> {
    put_u64(output, to_u64(chain.len(), "bounding-chain term count")?);
    for term in chain {
        for &vertex in &term.vertices {
            put_u64(output, to_u64(vertex, "bounding-chain vertex")?);
        }
        put_u32(output, term.coefficient);
    }
    Ok(())
}

pub(super) fn validate_for_build(
    artifact: &PersistentClassArtifact,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    validate_artifact_limits(&artifact.source, limits)?;
    validate_total_terms(artifact, limits)?;
    validate_chain_limit(artifact, limits)?;
    validate_semantics(artifact)
}

fn validate_semantics(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    validate_threshold_option(artifact.threshold)?;
    validate_source(&artifact.source)?;
    validate_class(artifact)?;
    artifact
        .class
        .validate_provenance(&artifact.source)
        .map_err(|error| certificate_error(error.to_string()))?;
    validate_pair(artifact)?;
    validate_cycle(artifact)?;
    validate_chain(artifact)
}

fn validate_artifact_limits(
    source: &SparseDistanceMatrix,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if source.len() > limits.max_vertices {
        return Err(certificate_error(format!(
            "{} source vertices exceed the limit {}",
            source.len(),
            limits.max_vertices
        )));
    }
    if source.num_edges() > limits.max_edges {
        return Err(certificate_error(format!(
            "{} source edges exceed the limit {}",
            source.num_edges(),
            limits.max_edges
        )));
    }
    if limits.max_dimension < 1 {
        return Err(certificate_error(
            "persistent H1 artifacts require a dimension limit of at least 1",
        ));
    }
    if limits.max_bars == 0 {
        return Err(certificate_error("persistent class bar exceeds its limit"));
    }
    Ok(())
}

fn validate_total_terms(
    artifact: &PersistentClassArtifact,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    let cocycle = artifact.class.cocycle.terms.len();
    let cycle = artifact.cycle.len();
    let chain = artifact.bounding_chain.len();
    let total_terms = cocycle
        .checked_add(cycle)
        .and_then(|count| count.checked_add(chain))
        .ok_or_else(|| certificate_error("persistent class term count overflows"))?;
    check_total_terms(total_terms, limits.max_terms, "persistent class")
}

fn validate_chain_limit(
    artifact: &PersistentClassArtifact,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if artifact.bounding_chain.len() > limits.max_triangles {
        return Err(certificate_error(format!(
            "{} bounding-chain triangles exceed the limit {}",
            artifact.bounding_chain.len(),
            limits.max_triangles
        )));
    }
    Ok(())
}

fn validate_threshold_option(threshold: Option<f64>) -> Result<(), CertificateError> {
    if let Some(value) = threshold {
        validate_threshold(value)?;
    }
    Ok(())
}

fn validate_threshold(value: f64) -> Result<(), CertificateError> {
    if value.is_nan() || value < 0.0 || is_negative_zero(value) {
        return Err(certificate_error(
            "persistent class threshold must be non-negative and not negative zero",
        ));
    }
    Ok(())
}

fn validate_source(source: &SparseDistanceMatrix) -> Result<(), CertificateError> {
    let mut previous = None;
    for (u, v, value) in source.edges() {
        if previous.is_some_and(|edge| edge >= (u, v)) {
            return Err(certificate_error(
                "source edges are not in strict endpoint order",
            ));
        }
        if u >= v || v >= source.len() || !valid_weight(value) {
            return Err(certificate_error("source edge is invalid"));
        }
        previous = Some((u, v));
    }
    Ok(())
}

fn validate_class(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    let class = &artifact.class;
    validate_class_modulus(class.cocycle.modulus)?;
    validate_class_interval(class)?;
    validate_class_threshold(artifact)?;
    validate_cocycle(artifact)
}

fn validate_class_modulus(modulus: u32) -> Result<(), CertificateError> {
    if modulus == 0 {
        return Err(certificate_error(
            "persistent class modulus is not supported",
        ));
    }
    if modulus >= 32_768 {
        return Err(certificate_error(
            "persistent class modulus is not supported",
        ));
    }
    if !is_prime(u64::from(modulus)) {
        return Err(certificate_error(
            "persistent class modulus is not supported",
        ));
    }
    Ok(())
}

fn validate_class_interval(class: &PersistentClass) -> Result<(), CertificateError> {
    validate_interval_values(class.interval)?;
    validate_class_scale(class)
}

fn validate_interval_values(interval: crate::Bar) -> Result<(), CertificateError> {
    if interval.dim != 1 {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    if !valid_finite_nonnegative(interval.birth) {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    if !valid_death(interval.death) {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    if interval.death.is_finite() && interval.death <= interval.birth {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    Ok(())
}

fn validate_class_scale(class: &PersistentClass) -> Result<(), CertificateError> {
    let scale = class.cocycle.scale;
    if !valid_finite_nonnegative(scale) {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    if scale < class.interval.birth {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    if class.interval.death.is_finite() && scale >= class.interval.death {
        return Err(certificate_error(
            "persistent class interval or scale is invalid",
        ));
    }
    Ok(())
}

fn validate_class_threshold(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    let Some(threshold) = artifact.threshold else {
        return Ok(());
    };
    if artifact.class.interval.birth > threshold {
        return Err(certificate_error(
            "persistent class lies beyond its declared threshold",
        ));
    }
    if artifact.class.cocycle.scale > threshold {
        return Err(certificate_error(
            "persistent class lies beyond its declared threshold",
        ));
    }
    if artifact.class.interval.death.is_finite() && artifact.class.interval.death > threshold {
        return Err(certificate_error(
            "persistent class lies beyond its declared threshold",
        ));
    }
    Ok(())
}

fn validate_cocycle(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    let cocycle = &artifact.class.cocycle;
    validate_cocycle_header(cocycle)?;
    let mut previous = None;
    for term in &cocycle.terms {
        validate_cocycle_term(artifact, term, previous)?;
        previous = Some((term.u, term.v));
    }
    Ok(())
}

fn validate_cocycle_header(cocycle: &crate::Cocycle) -> Result<(), CertificateError> {
    if cocycle.terms.is_empty() {
        return Err(certificate_error(
            "persistent class cocycle must start with coefficient one",
        ));
    }
    if cocycle.terms[0].coefficient != 1 {
        return Err(certificate_error(
            "persistent class cocycle must start with coefficient one",
        ));
    }
    Ok(())
}

fn validate_cocycle_term(
    artifact: &PersistentClassArtifact,
    term: &crate::CocycleTerm,
    previous: Option<(usize, usize)>,
) -> Result<(), CertificateError> {
    if term.u >= term.v {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    if term.v >= artifact.source.len() {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    if term.coefficient == 0 {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    if term.coefficient >= artifact.class.cocycle.modulus {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    if artifact.source.get(term.u, term.v) > artifact.class.cocycle.scale {
        return Err(certificate_error(
            "persistent class cocycle term is invalid",
        ));
    }
    Ok(())
}

fn validate_pair(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    validate_pair_birth(artifact)?;
    validate_pair_death(artifact)
}

fn validate_pair_birth(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    let pair = &artifact.critical_pair;
    let [birth_u, birth_v] = edge_vertices(&pair.birth, "birth edge")?;
    if birth_v >= artifact.source.len() {
        return Err(certificate_error(
            "critical birth edge is outside the source",
        ));
    }
    if artifact.source.get(birth_u, birth_v).to_bits() != pair.birth.value.to_bits() {
        return Err(certificate_error(
            "critical birth edge does not match the class",
        ));
    }
    if pair.birth.value.to_bits() != artifact.class.interval.birth.to_bits() {
        return Err(certificate_error(
            "critical birth edge does not match the class",
        ));
    }
    Ok(())
}

fn validate_pair_death(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    match (&artifact.critical_pair.death, artifact.class.interval.death) {
        (None, death) if death.is_infinite() => Ok(()),
        (Some(simplex), death) if death.is_finite() => {
            validate_finite_pair_death(artifact, simplex, death)
        }
        _ => Err(certificate_error(
            "critical pair essential status differs from the class",
        )),
    }
}

fn validate_finite_pair_death(
    artifact: &PersistentClassArtifact,
    simplex: &crate::CriticalSimplex,
    death: f64,
) -> Result<(), CertificateError> {
    let [u, v, w] = triangle_vertices(simplex, "death triangle")?;
    if w >= artifact.source.len() {
        return Err(certificate_error(
            "critical death triangle is outside the source",
        ));
    }
    let value = artifact
        .source
        .get(u, v)
        .max(artifact.source.get(u, w))
        .max(artifact.source.get(v, w));
    if value.to_bits() != simplex.value.to_bits() {
        return Err(certificate_error(
            "critical death triangle does not match the class",
        ));
    }
    if value.to_bits() != death.to_bits() {
        return Err(certificate_error(
            "critical death triangle does not match the class",
        ));
    }
    Ok(())
}

fn validate_cycle(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    let cycle = &artifact.cycle;
    if cycle.is_empty() {
        return Err(certificate_error("persistent class cycle has no terms"));
    }
    let mut previous = None;
    let scale = artifact.critical_pair.birth.value;
    for term in cycle {
        validate_cycle_term(artifact, term, previous, scale)?;
        previous = Some((term.u, term.v));
    }
    Ok(())
}

fn validate_cycle_term(
    artifact: &PersistentClassArtifact,
    term: &PersistenceCycleTerm,
    previous: Option<(usize, usize)>,
    scale: f64,
) -> Result<(), CertificateError> {
    if term.u >= term.v {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    if term.v >= artifact.source.len() {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    if term.coefficient == 0 {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    if term.coefficient >= artifact.class.cocycle.modulus {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    if artifact.source.get(term.u, term.v) > scale {
        return Err(certificate_error("persistent class cycle term is invalid"));
    }
    Ok(())
}

fn validate_chain(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    if artifact.critical_pair.death.is_none() {
        return validate_essential_chain(artifact);
    }
    validate_finite_chain(artifact)
}

fn validate_essential_chain(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    if !artifact.bounding_chain.is_empty() {
        return Err(certificate_error(
            "essential persistent class cannot carry a bounding chain",
        ));
    }
    Ok(())
}

fn validate_finite_chain(artifact: &PersistentClassArtifact) -> Result<(), CertificateError> {
    if artifact.bounding_chain.is_empty() {
        return Err(certificate_error(
            "finite persistent class needs a bounding chain",
        ));
    }
    let death = artifact
        .critical_pair
        .death
        .as_ref()
        .expect("finite critical pair has a death");
    let death_scale = death.value;
    let mut previous = None;
    for term in &artifact.bounding_chain {
        validate_chain_term(artifact, term, previous, death_scale)?;
        previous = Some(term.vertices);
    }
    Ok(())
}

fn validate_chain_term(
    artifact: &PersistentClassArtifact,
    term: &PersistenceTriangleTerm,
    previous: Option<[usize; 3]>,
    death_scale: f64,
) -> Result<(), CertificateError> {
    validate_chain_term_shape(artifact, term, previous)?;
    validate_chain_term_support(artifact, term, death_scale)
}

fn validate_chain_term_shape(
    artifact: &PersistentClassArtifact,
    term: &PersistenceTriangleTerm,
    previous: Option<[usize; 3]>,
) -> Result<(), CertificateError> {
    let [u, v, w] = term.vertices;
    if u >= v {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if v >= w {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if w >= artifact.source.len() {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if term.coefficient == 0 {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if term.coefficient >= artifact.class.cocycle.modulus {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if previous.is_some_and(|triangle| triangle >= term.vertices) {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    Ok(())
}

fn validate_chain_term_support(
    artifact: &PersistentClassArtifact,
    term: &PersistenceTriangleTerm,
    death_scale: f64,
) -> Result<(), CertificateError> {
    let [u, v, w] = term.vertices;
    if artifact.source.get(u, v) > death_scale {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if artifact.source.get(u, w) > death_scale {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    if artifact.source.get(v, w) > death_scale {
        return Err(certificate_error(
            "persistent class bounding-chain term is invalid",
        ));
    }
    Ok(())
}

fn edge_vertices(
    simplex: &crate::CriticalSimplex,
    name: &str,
) -> Result<[usize; 2], CertificateError> {
    if simplex.vertices.len() != 2 || !simplex.vertices.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(certificate_error(format!(
            "{name} vertices are not canonical"
        )));
    }
    Ok([simplex.vertices[0], simplex.vertices[1]])
}

fn triangle_vertices(
    simplex: &crate::CriticalSimplex,
    name: &str,
) -> Result<[usize; 3], CertificateError> {
    if simplex.vertices.len() != 3 || !simplex.vertices.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(certificate_error(format!(
            "{name} vertices are not canonical"
        )));
    }
    Ok([
        simplex.vertices[0],
        simplex.vertices[1],
        simplex.vertices[2],
    ])
}

fn valid_weight(value: f64) -> bool {
    value.is_finite() && value >= 0.0 && !is_negative_zero(value)
}

fn valid_finite_nonnegative(value: f64) -> bool {
    valid_weight(value)
}

fn valid_death(value: f64) -> bool {
    valid_weight(value) || (value.is_infinite() && value.is_sign_positive())
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

fn check_total_terms(count: usize, maximum: usize, kind: &str) -> Result<(), CertificateError> {
    if count > maximum {
        Err(certificate_error(format!(
            "{count} {kind} terms exceed the limit {maximum}"
        )))
    } else {
        Ok(())
    }
}

fn to_u64(value: usize, name: &str) -> Result<u64, CertificateError> {
    u64::try_from(value).map_err(|_| certificate_error(format!("{name} does not fit in u64")))
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

fn certificate_error(message: impl Into<String>) -> CertificateError {
    CertificateError::new(message)
}
