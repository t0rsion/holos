//! Wire format for persistence atlases.
//!
//! A `HOLOSATL` envelope binds the complete listed graph, its canonical H1
//! class spaces and critical simplices, and a nested algebraic reduction
//! certificate. Verification checks the reduction without calling the
//! persistence solver, validates each cocycle on the caller's graph, and
//! reconstructs the reusable validity region.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::classes::{basis_class_id, canonical_space_basis, group_id, validate_h1_cocycle};
use crate::{
    Bar, BasisClassId, CertificateLimits, Cocycle, CocycleTerm, CriticalPair, CriticalSimplex,
    Diagram, ExplainedDiagram, IntervalGroupId, PersistenceAtlas, PersistentClass,
    PersistentClassSpace, ReductionCertificate, ReductionRepairMode, ReductionRepairWork,
    RipsParams, SparseDistanceMatrix,
};

const MAGIC: &[u8; 8] = b"HOLOSATL";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing, decoding, or checking an atlas artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasArtifactError {
    message: String,
}

impl AtlasArtifactError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated artifact rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AtlasArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "atlas artifact: {}", self.message)
    }
}

impl std::error::Error for AtlasArtifactError {}

/// Decoder limits applied before atlas collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AtlasDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted bar count.
    pub max_bars: usize,
    /// Largest accepted class-space count.
    pub max_spaces: usize,
    /// Largest accepted total basis count.
    pub max_basis: usize,
    /// Largest accepted total critical-pair count.
    pub max_critical_pairs: usize,
    /// Largest accepted total cocycle term count.
    pub max_terms: usize,
    /// Largest accepted nested reduction certificate in bytes.
    pub max_certificate_bytes: usize,
}

impl Default for AtlasDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_bars: 100_000_000,
            max_spaces: 50_000_000,
            max_basis: 100_000_000,
            max_critical_pairs: 100_000_000,
            max_terms: 200_000_000,
            max_certificate_bytes: 1 << 30,
        }
    }
}

/// Input binding, class atlas, and nested reduction certificate.
#[derive(Debug, Clone)]
pub struct AtlasArtifact {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    input_digest: [u8; 32],
    explained: ExplainedDiagram,
    reduction: ReductionCertificate,
}

struct AtlasHeader {
    modulus: u32,
    vertex_count: usize,
    threshold: Option<f64>,
    bars: usize,
    spaces: usize,
    certificate_bytes: usize,
    input_digest: [u8; 32],
}

#[derive(Default)]
struct AtlasTotals {
    basis: usize,
    critical_pairs: usize,
    terms: usize,
}

struct SpaceHeader {
    id: IntervalGroupId,
    interval: Bar,
    basis: usize,
    critical_pairs: usize,
}

/// An atlas after reduction repair.
#[derive(Debug, Clone)]
pub struct AtlasArtifactRepair {
    artifact: AtlasArtifact,
    mode: ReductionRepairMode,
    work: ReductionRepairWork,
}

impl AtlasArtifactRepair {
    /// Updated atlas.
    pub fn artifact(&self) -> &AtlasArtifact {
        &self.artifact
    }

    /// How the reduction was adapted.
    pub fn mode(&self) -> ReductionRepairMode {
        self.mode
    }

    /// Exact boundary-column work performed by the repair.
    pub fn work(&self) -> ReductionRepairWork {
        self.work
    }

    pub(crate) fn into_artifact(self) -> AtlasArtifact {
        self.artifact
    }
}

impl AtlasArtifact {
    /// Produce an atlas artifact for an exact H0 and H1 run.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        Self::compile(input, params, certificate_limits).map(|(artifact, _)| artifact)
    }

    /// Produce an artifact and retain its ready-to-evaluate atlas.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<(Self, PersistenceAtlas), AtlasArtifactError> {
        let atlas = PersistenceAtlas::build(input, params)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let reduction = ReductionCertificate::build(input, params, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let artifact = Self {
            vertex_count: input.len(),
            threshold: params.threshold,
            modulus: params.modulus,
            input_digest: *atlas.input_digest(),
            explained: atlas.explained().clone(),
            reduction,
        };
        artifact.check_structure(Some(input))?;
        Ok((artifact, atlas))
    }

    /// Adapt this artifact to changed weights on the same listed graph.
    ///
    /// The reduction reuses its longest valid simplex prefixes. Canonical
    /// class spaces are recomputed and checked against the repaired
    /// reduction.
    pub fn repair(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<AtlasArtifactRepair, AtlasArtifactError> {
        self.check_structure(Some(current))?;
        let repair = self
            .reduction
            .repair(current, updated, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let mut params = RipsParams::new(1).with_modulus(self.modulus);
        params.threshold = self.threshold;
        let explained = crate::rips_persistence_with_classes_sparse(updated, &params)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(repair.certificate().diagram(), &explained.diagram) {
            return Err(AtlasArtifactError::new(
                "repaired reduction differs from the canonical class reduction",
            ));
        }
        let mode = repair.mode();
        let work = repair.work();
        let artifact = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            input_digest: full_graph_digest(updated, self.threshold),
            explained,
            reduction: repair.into_certificate(),
        };
        artifact.check_structure(Some(updated))?;
        Ok(AtlasArtifactRepair {
            artifact,
            mode,
            work,
        })
    }

    pub(crate) fn rebind(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        explained: ExplainedDiagram,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        self.check_structure(Some(current))?;
        let reduction = self
            .reduction
            .reindex(current, updated, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(reduction.diagram(), &explained.diagram) {
            return Err(AtlasArtifactError::new(
                "reindexed reduction differs from the evaluated class result",
            ));
        }
        let artifact = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            input_digest: full_graph_digest(updated, self.threshold),
            explained,
            reduction,
        };
        artifact.verify(updated, certificate_limits)?;
        Ok(artifact)
    }

    /// Diagram carried by the atlas.
    pub fn diagram(&self) -> &Diagram {
        &self.explained.diagram
    }

    /// Diagram, class spaces, cocycles, and critical pairs carried by the
    /// atlas.
    pub fn explained(&self) -> &ExplainedDiagram {
        &self.explained
    }

    /// Number of vertices bound to the atlas.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Canonical H1 class spaces carried by the atlas.
    pub fn spaces(&self) -> &[PersistentClassSpace] {
        &self.explained.spaces
    }

    /// Nested algebraic reduction certificate.
    pub fn reduction_certificate(&self) -> &ReductionCertificate {
        &self.reduction
    }

    /// Encode the canonical `HOLOSATL` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, AtlasArtifactError> {
        self.check_structure(None)?;
        let reduction = self
            .reduction
            .encode()
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        let mut out = Vec::new();
        encode_atlas_header(&mut out, self, reduction.len())?;
        encode_bars(&mut out, &self.explained.diagram)?;
        encode_spaces(&mut out, &self.explained.spaces)?;
        out.extend_from_slice(&reduction);
        Ok(out)
    }

    /// Decode and structurally validate a bounded atlas envelope.
    pub fn decode(
        bytes: &[u8],
        limits: AtlasDecodeLimits,
        mut certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        validate_atlas_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_atlas_prefix(&mut reader)?;
        let header = decode_atlas_header(&mut reader, limits)?;
        validate_minimum_records(&reader, &header)?;
        let bars = decode_bars(&mut reader, header.bars)?;
        let spaces = decode_spaces(&mut reader, &header, limits)?;
        let reduction = decode_nested_certificate(&mut reader, &header, &mut certificate_limits)?;
        finish_atlas_decode(&reader)?;
        let artifact = Self {
            vertex_count: header.vertex_count,
            threshold: header.threshold,
            modulus: header.modulus,
            input_digest: header.input_digest,
            explained: ExplainedDiagram {
                diagram: Diagram { bars },
                spaces,
            },
            reduction,
        };
        artifact.check_structure(None)?;
        Ok(artifact)
    }

    /// Verify the nested reduction and reconstruct the reusable atlas.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<PersistenceAtlas, AtlasArtifactError> {
        self.check_structure(Some(input))?;
        let (diagram, checked_pairs) = self
            .reduction
            .verify_with_h1_critical_pairs(input, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if !diagram_bits_equal(&diagram, &self.explained.diagram) {
            return Err(AtlasArtifactError::new(
                "checked reduction differs from the atlas diagram",
            ));
        }
        let mut declared_pairs = Vec::new();
        for space in &self.explained.spaces {
            for pair in &space.critical_pairs {
                declared_pairs.push((space.interval, pair.clone()));
            }
        }
        declared_pairs.sort_by(critical_pair_record_order);
        if !critical_pair_records_bits_equal(&checked_pairs, &declared_pairs) {
            return Err(AtlasArtifactError::new(
                "checked reduction differs from the declared critical pairs",
            ));
        }
        PersistenceAtlas::from_checked_parts(
            input,
            self.modulus,
            self.threshold,
            self.explained.clone(),
        )
        .map_err(|error| AtlasArtifactError::new(error.to_string()))
    }

    fn check_structure(
        &self,
        input: Option<&SparseDistanceMatrix>,
    ) -> std::result::Result<(), AtlasArtifactError> {
        let threshold = checked_threshold(self.threshold)?;
        check_input_binding(self, input, threshold)?;
        check_reduction_binding(self)?;
        check_diagram_structure(&self.explained.diagram)?;
        check_spaces_structure(self, input)?;
        check_space_order(&self.explained.spaces)?;
        check_critical_values(input, &self.explained.spaces)
    }
}

fn check_input_binding(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    threshold: f64,
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    if input.len() != artifact.vertex_count
        || full_graph_digest(input, artifact.threshold) != artifact.input_digest
    {
        return Err(AtlasArtifactError::new(
            "complete input graph binding does not match",
        ));
    }
    if threshold.is_finite()
        && input
            .edges()
            .any(|(_, _, value)| value.is_nan() || value < 0.0)
    {
        return Err(AtlasArtifactError::new("input graph is not canonical"));
    }
    Ok(())
}

fn check_reduction_binding(
    artifact: &AtlasArtifact,
) -> std::result::Result<(), AtlasArtifactError> {
    if artifact.reduction.vertex_count() != artifact.vertex_count
        || artifact.reduction.threshold().map(f64::to_bits) != artifact.threshold.map(f64::to_bits)
        || artifact.reduction.modulus() != artifact.modulus
    {
        return Err(AtlasArtifactError::new(
            "reduction header differs from the atlas header",
        ));
    }
    if !diagram_bits_equal(artifact.reduction.diagram(), &artifact.explained.diagram) {
        return Err(AtlasArtifactError::new(
            "reduction diagram differs from the atlas diagram",
        ));
    }
    Ok(())
}

fn check_diagram_structure(diagram: &Diagram) -> std::result::Result<(), AtlasArtifactError> {
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&canonical, diagram) {
        return Err(AtlasArtifactError::new("bars are not in canonical order"));
    }
    for (index, bar) in diagram.bars.iter().enumerate() {
        check_bar(bar)
            .map_err(|error| AtlasArtifactError::new(format!("bar {index} is invalid: {error}")))?;
    }
    Ok(())
}

fn check_spaces_structure(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut space_bars = Vec::new();
    for (index, space) in artifact.explained.spaces.iter().enumerate() {
        check_space_structure(artifact, input, index, space)?;
        space_bars.extend(std::iter::repeat_n(
            (
                space.interval.birth.to_bits(),
                space.interval.death.to_bits(),
            ),
            space.basis.len(),
        ));
    }
    check_space_intervals(&artifact.explained.diagram, &mut space_bars)
}

fn check_space_structure(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    if space.basis.is_empty() || space.critical_pairs.len() != space.basis.len() {
        return Err(AtlasArtifactError::new(format!(
            "space {index} has inconsistent multiplicity"
        )));
    }
    let cocycles = space
        .basis
        .iter()
        .map(|class| class.cocycle.clone())
        .collect::<Vec<_>>();
    check_canonical_basis(input, artifact.modulus, index, &cocycles)?;
    if group_id(space.interval, artifact.modulus, &cocycles) != space.id {
        return Err(AtlasArtifactError::new(format!(
            "space {index} identifier does not match its basis"
        )));
    }
    check_space_basis(artifact, input, index, space)?;
    check_critical_pairs(artifact.vertex_count, index, space)
}

fn check_canonical_basis(
    input: Option<&SparseDistanceMatrix>,
    modulus: u32,
    index: usize,
    cocycles: &[Cocycle],
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    let canonical = canonical_space_basis(input, modulus, cocycles)
        .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
    if !cocycle_lists_bits_equal(&canonical, cocycles) {
        Err(AtlasArtifactError::new(format!(
            "space {index} basis is not in canonical row-reduced form"
        )))
    } else {
        Ok(())
    }
}

fn check_space_basis(
    artifact: &AtlasArtifact,
    input: Option<&SparseDistanceMatrix>,
    space_index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    for (basis_index, class) in space.basis.iter().enumerate() {
        check_class_structure(artifact, space_index, basis_index, space, class)?;
        if let Some(input) = input {
            validate_h1_cocycle(input, &class.cocycle)
                .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        }
    }
    Ok(())
}

fn check_class_structure(
    artifact: &AtlasArtifact,
    space_index: usize,
    basis_index: usize,
    space: &PersistentClassSpace,
    class: &PersistentClass,
) -> std::result::Result<(), AtlasArtifactError> {
    if class.group_id != space.id
        || class.basis_index != basis_index
        || class.interval != space.interval
        || class.cocycle.modulus != artifact.modulus
        || basis_class_id(space.id, basis_index, &class.cocycle) != class.id
    {
        return Err(AtlasArtifactError::new(format!(
            "space {space_index} basis {basis_index} is not canonical"
        )));
    }
    check_cocycle_shape(&class.cocycle, artifact.vertex_count).map_err(|error| {
        AtlasArtifactError::new(format!(
            "space {space_index} basis {basis_index} is invalid: {error}"
        ))
    })
}

fn check_critical_pairs(
    vertex_count: usize,
    space_index: usize,
    space: &PersistentClassSpace,
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in &space.critical_pairs {
        check_critical_pair(vertex_count, space_index, space.interval, pair)?;
    }
    if space
        .critical_pairs
        .windows(2)
        .any(|pairs| !critical_pair_order(&pairs[0], &pairs[1]).is_lt())
    {
        return Err(AtlasArtifactError::new(format!(
            "space {space_index} critical pairs are not in canonical order"
        )));
    }
    Ok(())
}

fn check_critical_pair(
    vertex_count: usize,
    space_index: usize,
    interval: Bar,
    pair: &CriticalPair,
) -> std::result::Result<(), AtlasArtifactError> {
    check_critical(&pair.birth, 2, vertex_count, interval.birth)?;
    match (&pair.death, interval.is_essential()) {
        (None, true) => Ok(()),
        (Some(death), false) => check_critical(death, 3, vertex_count, interval.death),
        _ => Err(AtlasArtifactError::new(format!(
            "space {space_index} critical pair has wrong death presence"
        ))),
    }
}

fn check_space_intervals(
    diagram: &Diagram,
    space_bars: &mut Vec<(u64, u64)>,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut h1_bars = diagram
        .in_dim(1)
        .map(|bar| (bar.birth.to_bits(), bar.death.to_bits()))
        .collect::<Vec<_>>();
    h1_bars.sort_unstable();
    space_bars.sort_unstable();
    if h1_bars != *space_bars {
        Err(AtlasArtifactError::new(
            "class-space intervals do not match the H1 diagram",
        ))
    } else {
        Ok(())
    }
}

fn check_space_order(
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in spaces.windows(2) {
        let order = pair[0]
            .interval
            .birth
            .total_cmp(&pair[1].interval.birth)
            .then(pair[0].interval.death.total_cmp(&pair[1].interval.death))
            .then(pair[0].id.cmp(&pair[1].id));
        if !order.is_lt() {
            return Err(AtlasArtifactError::new(
                "class spaces are not in strict canonical order",
            ));
        }
    }
    Ok(())
}

fn check_critical_values(
    input: Option<&SparseDistanceMatrix>,
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    let Some(input) = input else {
        return Ok(());
    };
    for space in spaces {
        for pair in &space.critical_pairs {
            check_critical_value(input, &pair.birth)?;
            if let Some(death) = &pair.death {
                check_critical_value(input, death)?;
            }
        }
    }
    Ok(())
}

fn encode_atlas_header(
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

fn encode_bars(
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

fn encode_spaces(
    out: &mut Vec<u8>,
    spaces: &[PersistentClassSpace],
) -> std::result::Result<(), AtlasArtifactError> {
    for space in spaces {
        encode_space(out, space)?;
    }
    Ok(())
}

fn encode_space(
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

fn encode_critical_pairs(
    out: &mut Vec<u8>,
    pairs: &[CriticalPair],
) -> std::result::Result<(), AtlasArtifactError> {
    for pair in pairs {
        encode_critical(out, &pair.birth)?;
        encode_optional_critical(out, pair.death.as_ref())?;
    }
    Ok(())
}

fn encode_optional_critical(
    out: &mut Vec<u8>,
    critical: Option<&CriticalSimplex>,
) -> std::result::Result<(), AtlasArtifactError> {
    match critical {
        None => out.push(0),
        Some(critical) => {
            out.push(1);
            encode_critical(out, critical)?;
        }
    }
    Ok(())
}

fn encode_basis(
    out: &mut Vec<u8>,
    basis: &[PersistentClass],
) -> std::result::Result<(), AtlasArtifactError> {
    for class in basis {
        encode_class(out, class)?;
    }
    Ok(())
}

fn encode_class(
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
    Ok(())
}

fn validate_atlas_size(
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

fn decode_atlas_prefix(reader: &mut Reader<'_>) -> std::result::Result<(), AtlasArtifactError> {
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

fn decode_atlas_header(
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

fn validate_minimum_records(
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

fn decode_bars(
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

fn decode_spaces(
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

fn decode_space(
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

fn decode_space_header(
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

fn add_atlas_total(
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

fn decode_critical_pairs(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    count: usize,
) -> std::result::Result<Vec<CriticalPair>, AtlasArtifactError> {
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        pairs.push(CriticalPair {
            birth: decode_critical(reader, vertex_count)?,
            death: decode_optional_critical(reader, vertex_count)?,
        });
    }
    Ok(pairs)
}

fn decode_optional_critical(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> std::result::Result<Option<CriticalSimplex>, AtlasArtifactError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_critical(reader, vertex_count).map(Some),
        tag => Err(AtlasArtifactError::new(format!(
            "unknown optional-critical tag {tag}"
        ))),
    }
}

fn decode_basis(
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

fn decode_class(
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
    })
}

fn validate_term_bytes(
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

fn decode_terms(
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

fn decode_nested_certificate(
    reader: &mut Reader<'_>,
    header: &AtlasHeader,
    limits: &mut CertificateLimits,
) -> std::result::Result<ReductionCertificate, AtlasArtifactError> {
    let nested = reader.take(header.certificate_bytes)?;
    limits.max_bytes = limits.max_bytes.min(header.certificate_bytes);
    ReductionCertificate::decode(nested, *limits)
        .map_err(|error| AtlasArtifactError::new(error.to_string()))
}

fn finish_atlas_decode(reader: &Reader<'_>) -> std::result::Result<(), AtlasArtifactError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(AtlasArtifactError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )))
    }
}

fn check_bar(bar: &Bar) -> std::result::Result<(), &'static str> {
    if bar.dim > 1 {
        return Err("dimension exceeds one");
    }
    check_bar_birth(bar)?;
    check_bar_death(bar.death)?;
    if bar.death.is_finite() && bar.death <= bar.birth {
        return Err("finite death does not follow birth");
    }
    Ok(())
}

fn check_bar_birth(bar: &Bar) -> std::result::Result<(), &'static str> {
    if !bar.birth.is_finite() || bar.birth < 0.0 || is_negative_zero(bar.birth) {
        return Err("birth is not a canonical non-negative finite value");
    }
    if bar.dim == 0 && bar.birth.to_bits() != 0 {
        return Err("H0 birth is not positive zero");
    }
    Ok(())
}

fn check_bar_death(death: f64) -> std::result::Result<(), &'static str> {
    if death.is_nan()
        || death < 0.0
        || is_negative_zero(death)
        || (death.is_infinite() && !death.is_sign_positive())
    {
        Err("death is not a canonical non-negative value")
    } else {
        Ok(())
    }
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

fn check_cocycle_shape(
    cocycle: &Cocycle,
    vertex_count: usize,
) -> std::result::Result<(), &'static str> {
    if !cocycle.scale.is_finite() || cocycle.scale < 0.0 || is_negative_zero(cocycle.scale) {
        return Err("scale is not a canonical non-negative finite value");
    }
    if cocycle.terms.is_empty() || cocycle.terms[0].coefficient != 1 {
        return Err("terms are empty or not normalized");
    }
    let mut previous = None;
    for term in &cocycle.terms {
        if !canonical_cocycle_term(term, previous, vertex_count, cocycle.modulus) {
            return Err("terms are not canonical");
        }
        previous = Some((term.u, term.v));
    }
    Ok(())
}

fn canonical_cocycle_term(
    term: &CocycleTerm,
    previous: Option<(usize, usize)>,
    vertex_count: usize,
    modulus: u32,
) -> bool {
    term.u < term.v
        && term.v < vertex_count
        && term.coefficient != 0
        && term.coefficient < modulus
        && previous.is_none_or(|edge| edge < (term.u, term.v))
}

fn checked_threshold(threshold: Option<f64>) -> std::result::Result<f64, AtlasArtifactError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
        return Err(AtlasArtifactError::new(format!(
            "threshold must be canonical and non-negative, got {value}"
        )));
    }
    Ok(value)
}

fn check_critical(
    simplex: &CriticalSimplex,
    size: usize,
    vertex_count: usize,
    expected_value: f64,
) -> std::result::Result<(), AtlasArtifactError> {
    if simplex.vertices.len() != size
        || simplex
            .vertices
            .iter()
            .any(|&vertex| vertex >= vertex_count)
        || simplex.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || !simplex.value.is_finite()
        || simplex.value < 0.0
        || is_negative_zero(simplex.value)
        || simplex.value.to_bits() != expected_value.to_bits()
    {
        return Err(AtlasArtifactError::new(
            "critical simplex is not canonical for its interval",
        ));
    }
    Ok(())
}

fn check_critical_value(
    input: &SparseDistanceMatrix,
    simplex: &CriticalSimplex,
) -> std::result::Result<(), AtlasArtifactError> {
    let mut value = 0.0f64;
    for i in 0..simplex.vertices.len() {
        for j in i + 1..simplex.vertices.len() {
            let edge = input.get(simplex.vertices[i], simplex.vertices[j]);
            if !edge.is_finite() {
                return Err(AtlasArtifactError::new(
                    "critical simplex contains an absent edge",
                ));
            }
            value = value.max(edge);
        }
    }
    if value.to_bits() != simplex.value.to_bits() {
        return Err(AtlasArtifactError::new(
            "critical simplex value differs from its graph filtration value",
        ));
    }
    Ok(())
}

fn full_graph_digest(input: &SparseDistanceMatrix, threshold: Option<f64>) -> [u8; 32] {
    let edges: Vec<_> = input.edges().collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-persistence-atlas-v1");
    hash.update((input.len() as u64).to_be_bytes());
    hash.update(
        threshold
            .map(f64::to_bits)
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update((edges.len() as u64).to_be_bytes());
    for (u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn encode_critical(
    out: &mut Vec<u8>,
    simplex: &CriticalSimplex,
) -> std::result::Result<(), AtlasArtifactError> {
    put_usize(out, simplex.vertices.len(), "critical simplex size")?;
    for &vertex in &simplex.vertices {
        put_usize(out, vertex, "critical simplex vertex")?;
    }
    put_u64(out, simplex.value.to_bits());
    Ok(())
}

fn decode_critical(
    reader: &mut Reader<'_>,
    vertex_count: usize,
) -> std::result::Result<CriticalSimplex, AtlasArtifactError> {
    let size = reader.usize()?;
    if size != 2 && size != 3 {
        return Err(AtlasArtifactError::new(format!(
            "critical simplex has unsupported size {size}"
        )));
    }
    let mut vertices = Vec::with_capacity(size);
    for _ in 0..size {
        let vertex = reader.usize()?;
        if vertex >= vertex_count {
            return Err(AtlasArtifactError::new(format!(
                "critical simplex vertex {vertex} is outside {vertex_count}"
            )));
        }
        vertices.push(vertex);
    }
    Ok(CriticalSimplex {
        vertices,
        value: f64::from_bits(reader.u64()?),
    })
}

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

fn cocycle_lists_bits_equal(a: &[Cocycle], b: &[Cocycle]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.modulus == b.modulus && a.scale.to_bits() == b.scale.to_bits() && a.terms == b.terms
        })
}

fn critical_pair_order(a: &CriticalPair, b: &CriticalPair) -> std::cmp::Ordering {
    a.birth.vertices.cmp(&b.birth.vertices).then_with(|| {
        a.death
            .as_ref()
            .map(|simplex| &simplex.vertices)
            .cmp(&b.death.as_ref().map(|simplex| &simplex.vertices))
    })
}

fn critical_pair_record_order(
    a: &(Bar, CriticalPair),
    b: &(Bar, CriticalPair),
) -> std::cmp::Ordering {
    a.0.birth
        .total_cmp(&b.0.birth)
        .then(a.0.death.total_cmp(&b.0.death))
        .then_with(|| critical_pair_order(&a.1, &b.1))
}

fn critical_pair_records_bits_equal(a: &[(Bar, CriticalPair)], b: &[(Bar, CriticalPair)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|((a_bar, a_pair), (b_bar, b_pair))| {
            a_bar.birth.to_bits() == b_bar.birth.to_bits()
                && a_bar.death.to_bits() == b_bar.death.to_bits()
                && critical_simplex_bits_equal(&a_pair.birth, &b_pair.birth)
                && match (&a_pair.death, &b_pair.death) {
                    (None, None) => true,
                    (Some(a), Some(b)) => critical_simplex_bits_equal(a, b),
                    _ => false,
                }
        })
}

fn critical_simplex_bits_equal(a: &CriticalSimplex, b: &CriticalSimplex) -> bool {
    a.vertices == b.vertices && a.value.to_bits() == b.value.to_bits()
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), AtlasArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| AtlasArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], AtlasArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| AtlasArtifactError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(AtlasArtifactError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, AtlasArtifactError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, AtlasArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> std::result::Result<u32, AtlasArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, AtlasArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, AtlasArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| AtlasArtifactError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, AtlasArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(AtlasArtifactError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_f64(&mut self) -> std::result::Result<Option<f64>, AtlasArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(AtlasArtifactError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], AtlasArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn square() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn atlas_round_trips_and_verifies_without_the_solver() {
        let input = square();
        let params = RipsParams::new(1).with_modulus(3);
        let artifact = AtlasArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
        let bytes = artifact.encode().unwrap();
        let decoded = AtlasArtifact::decode(
            &bytes,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        let atlas = decoded
            .verify(&input, CertificateLimits::default())
            .unwrap();
        assert_eq!(atlas.explained().spaces, artifact.spaces());
    }

    #[test]
    fn mutations_limits_and_wrong_inputs_are_rejected() {
        let input = square();
        let params = RipsParams::new(1);
        let artifact = AtlasArtifact::build(&input, &params, CertificateLimits::default()).unwrap();
        let bytes = artifact.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(
                AtlasArtifact::decode(
                    &bytes[..end],
                    AtlasDecodeLimits::default(),
                    CertificateLimits::default(),
                )
                .is_err()
            );
        }
        let limits = AtlasDecodeLimits {
            max_bytes: bytes.len() - 1,
            ..AtlasDecodeLimits::default()
        };
        assert!(AtlasArtifact::decode(&bytes, limits, CertificateLimits::default()).is_err());

        let other =
            SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap();
        assert!(
            artifact
                .verify(&other, CertificateLimits::default())
                .is_err()
        );

        let mut changed_pair = artifact.clone();
        changed_pair.explained.spaces[0].critical_pairs[0]
            .birth
            .vertices = vec![2, 3];
        assert!(
            changed_pair
                .verify(&input, CertificateLimits::default())
                .is_err()
        );
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = AtlasArtifact::decode(
                &bytes,
                AtlasDecodeLimits::default(),
                CertificateLimits::default(),
            );
        }
    }
}
