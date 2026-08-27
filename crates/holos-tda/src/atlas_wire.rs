//! Portable proof-carrying persistence atlases.
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

/// Input binding, class atlas, and solver-independent reduction proof.
#[derive(Debug, Clone)]
pub struct AtlasArtifact {
    vertex_count: usize,
    threshold: Option<f64>,
    modulus: u32,
    input_digest: [u8; 32],
    explained: ExplainedDiagram,
    reduction: ReductionCertificate,
}

/// A proof-carrying atlas adapted by dependency-directed reduction repair.
#[derive(Debug, Clone)]
pub struct AtlasArtifactRepair {
    artifact: AtlasArtifact,
    mode: ReductionRepairMode,
    work: ReductionRepairWork,
}

impl AtlasArtifactRepair {
    /// Updated proof-carrying atlas.
    pub fn artifact(&self) -> &AtlasArtifact {
        &self.artifact
    }

    /// Whether the reduction was reused, repaired, or rebuilt.
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
    /// Produce a proof-carrying atlas for an exact H0 and H1 run.
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
    /// reduction before the artifact is returned.
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
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, WIRE_VERSION);
        out.push(F64_BITS_CODEC);
        put_u32(&mut out, self.modulus);
        put_usize(&mut out, self.vertex_count, "vertex count")?;
        put_optional_f64(&mut out, self.threshold);
        put_usize(&mut out, self.explained.diagram.bars.len(), "bar count")?;
        put_usize(&mut out, self.explained.spaces.len(), "space count")?;
        put_usize(&mut out, reduction.len(), "certificate byte count")?;
        out.extend_from_slice(&self.input_digest);
        for bar in &self.explained.diagram.bars {
            put_usize(&mut out, bar.dim, "bar dimension")?;
            put_u64(&mut out, bar.birth.to_bits());
            put_u64(&mut out, bar.death.to_bits());
        }
        for space in &self.explained.spaces {
            out.extend_from_slice(space.id.as_bytes());
            put_u64(&mut out, space.interval.birth.to_bits());
            put_u64(&mut out, space.interval.death.to_bits());
            put_usize(&mut out, space.basis.len(), "basis count")?;
            put_usize(&mut out, space.critical_pairs.len(), "critical-pair count")?;
            for pair in &space.critical_pairs {
                encode_critical(&mut out, &pair.birth)?;
                match &pair.death {
                    None => out.push(0),
                    Some(death) => {
                        out.push(1);
                        encode_critical(&mut out, death)?;
                    }
                }
            }
            for class in &space.basis {
                out.extend_from_slice(class.id.as_bytes());
                put_usize(&mut out, class.basis_index, "basis index")?;
                put_u64(&mut out, class.cocycle.scale.to_bits());
                put_usize(&mut out, class.cocycle.terms.len(), "cocycle term count")?;
                for term in &class.cocycle.terms {
                    put_usize(&mut out, term.u, "term endpoint")?;
                    put_usize(&mut out, term.v, "term endpoint")?;
                    put_u32(&mut out, term.coefficient);
                }
            }
        }
        out.extend_from_slice(&reduction);
        Ok(out)
    }

    /// Decode and structurally validate a bounded atlas envelope.
    pub fn decode(
        bytes: &[u8],
        limits: AtlasDecodeLimits,
        mut certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, AtlasArtifactError> {
        if bytes.len() > limits.max_bytes {
            return Err(AtlasArtifactError::new(format!(
                "{} bytes exceed the decoder limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Reader::new(bytes);
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
        let modulus = reader.u32()?;
        let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
        let threshold = reader.optional_f64()?;
        let bar_count = reader.bounded_usize("bar count", limits.max_bars)?;
        let space_count = reader.bounded_usize("space count", limits.max_spaces)?;
        let certificate_bytes =
            reader.bounded_usize("certificate byte count", limits.max_certificate_bytes)?;
        let input_digest = reader.array32()?;
        let minimum = bar_count
            .checked_mul(24)
            .and_then(|bars| {
                space_count
                    .checked_mul(64)
                    .and_then(|spaces| bars.checked_add(spaces))
            })
            .and_then(|records| records.checked_add(certificate_bytes))
            .ok_or_else(|| AtlasArtifactError::new("minimum record bytes overflow usize"))?;
        if minimum > reader.remaining() {
            return Err(AtlasArtifactError::new(format!(
                "record counts need at least {minimum} bytes, only {} remain",
                reader.remaining()
            )));
        }
        let mut bars = Vec::with_capacity(bar_count);
        for _ in 0..bar_count {
            bars.push(Bar {
                dim: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            });
        }
        let mut spaces = Vec::with_capacity(space_count);
        let mut basis_total = 0usize;
        let mut critical_total = 0usize;
        let mut term_total = 0usize;
        for _ in 0..space_count {
            let id = IntervalGroupId::from_bytes(reader.array32()?);
            let interval = Bar {
                dim: 1,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            };
            let basis_count = reader.usize()?;
            basis_total = basis_total
                .checked_add(basis_count)
                .ok_or_else(|| AtlasArtifactError::new("basis count overflows usize"))?;
            if basis_total > limits.max_basis {
                return Err(AtlasArtifactError::new(format!(
                    "{basis_total} basis classes exceed the limit {}",
                    limits.max_basis
                )));
            }
            let critical_count = reader.usize()?;
            critical_total = critical_total
                .checked_add(critical_count)
                .ok_or_else(|| AtlasArtifactError::new("critical-pair count overflows usize"))?;
            if critical_total > limits.max_critical_pairs {
                return Err(AtlasArtifactError::new(format!(
                    "{critical_total} critical pairs exceed the limit {}",
                    limits.max_critical_pairs
                )));
            }
            let mut critical_pairs = Vec::with_capacity(critical_count);
            for _ in 0..critical_count {
                let birth = decode_critical(&mut reader, vertex_count)?;
                let death = match reader.u8()? {
                    0 => None,
                    1 => Some(decode_critical(&mut reader, vertex_count)?),
                    tag => {
                        return Err(AtlasArtifactError::new(format!(
                            "unknown optional-critical tag {tag}"
                        )));
                    }
                };
                critical_pairs.push(CriticalPair { birth, death });
            }
            let mut basis = Vec::with_capacity(basis_count);
            for _ in 0..basis_count {
                let basis_id = BasisClassId::from_bytes(reader.array32()?);
                let basis_index = reader.usize()?;
                let scale = f64::from_bits(reader.u64()?);
                let count = reader.usize()?;
                term_total = term_total
                    .checked_add(count)
                    .ok_or_else(|| AtlasArtifactError::new("cocycle term count overflows usize"))?;
                if term_total > limits.max_terms {
                    return Err(AtlasArtifactError::new(format!(
                        "{term_total} cocycle terms exceed the limit {}",
                        limits.max_terms
                    )));
                }
                let term_bytes = count
                    .checked_mul(20)
                    .ok_or_else(|| AtlasArtifactError::new("cocycle term bytes overflow usize"))?;
                if term_bytes > reader.remaining().saturating_sub(certificate_bytes) {
                    return Err(AtlasArtifactError::new(
                        "cocycle terms exceed the remaining record bytes",
                    ));
                }
                let mut terms = Vec::with_capacity(count);
                for _ in 0..count {
                    terms.push(CocycleTerm {
                        u: reader.usize()?,
                        v: reader.usize()?,
                        coefficient: reader.u32()?,
                    });
                }
                basis.push(PersistentClass {
                    id: basis_id,
                    group_id: id,
                    basis_index,
                    interval,
                    cocycle: Cocycle {
                        modulus,
                        scale,
                        terms,
                    },
                });
            }
            spaces.push(PersistentClassSpace {
                id,
                interval,
                basis,
                critical_pairs,
            });
        }
        let nested = reader.take(certificate_bytes)?;
        certificate_limits.max_bytes = certificate_limits.max_bytes.min(certificate_bytes);
        let reduction = ReductionCertificate::decode(nested, certificate_limits)
            .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
        if reader.remaining() != 0 {
            return Err(AtlasArtifactError::new(format!(
                "{} trailing bytes after the envelope",
                reader.remaining()
            )));
        }
        let artifact = Self {
            vertex_count,
            threshold,
            modulus,
            input_digest,
            explained: ExplainedDiagram {
                diagram: Diagram { bars },
                spaces,
            },
            reduction,
        };
        artifact.check_structure(None)?;
        Ok(artifact)
    }

    /// Verify the proof and reconstruct the reusable atlas without calling
    /// the persistence solver.
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
        if let Some(input) = input {
            if input.len() != self.vertex_count
                || full_graph_digest(input, self.threshold) != self.input_digest
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
        }
        if self.reduction.vertex_count() != self.vertex_count
            || self.reduction.threshold().map(f64::to_bits) != self.threshold.map(f64::to_bits)
            || self.reduction.modulus() != self.modulus
        {
            return Err(AtlasArtifactError::new(
                "reduction header differs from the atlas header",
            ));
        }
        if !diagram_bits_equal(self.reduction.diagram(), &self.explained.diagram) {
            return Err(AtlasArtifactError::new(
                "reduction diagram differs from the atlas diagram",
            ));
        }
        let mut canonical = self.explained.diagram.clone();
        canonical.canonicalize();
        if !diagram_bits_equal(&canonical, &self.explained.diagram) {
            return Err(AtlasArtifactError::new("bars are not in canonical order"));
        }
        for (index, bar) in self.explained.diagram.bars.iter().enumerate() {
            check_bar(bar).map_err(|error| {
                AtlasArtifactError::new(format!("bar {index} is invalid: {error}"))
            })?;
        }
        let mut h1_bars: Vec<_> = self
            .explained
            .diagram
            .in_dim(1)
            .map(|bar| (bar.birth.to_bits(), bar.death.to_bits()))
            .collect();
        let mut space_bars = Vec::new();
        for (space_index, space) in self.explained.spaces.iter().enumerate() {
            if space.basis.is_empty() || space.critical_pairs.len() != space.basis.len() {
                return Err(AtlasArtifactError::new(format!(
                    "space {space_index} has inconsistent multiplicity"
                )));
            }
            let cocycles: Vec<_> = space
                .basis
                .iter()
                .map(|class| class.cocycle.clone())
                .collect();
            if let Some(input) = input {
                let canonical_basis = canonical_space_basis(input, self.modulus, &cocycles)
                    .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
                if !cocycle_lists_bits_equal(&canonical_basis, &cocycles) {
                    return Err(AtlasArtifactError::new(format!(
                        "space {space_index} basis is not in canonical row-reduced form"
                    )));
                }
            }
            if group_id(space.interval, self.modulus, &cocycles) != space.id {
                return Err(AtlasArtifactError::new(format!(
                    "space {space_index} identifier does not match its basis"
                )));
            }
            for (basis_index, class) in space.basis.iter().enumerate() {
                if class.group_id != space.id
                    || class.basis_index != basis_index
                    || class.interval != space.interval
                    || class.cocycle.modulus != self.modulus
                    || basis_class_id(space.id, basis_index, &class.cocycle) != class.id
                {
                    return Err(AtlasArtifactError::new(format!(
                        "space {space_index} basis {basis_index} is not canonical"
                    )));
                }
                check_cocycle_shape(&class.cocycle, self.vertex_count).map_err(|error| {
                    AtlasArtifactError::new(format!(
                        "space {space_index} basis {basis_index} is invalid: {error}"
                    ))
                })?;
                if let Some(input) = input {
                    validate_h1_cocycle(input, &class.cocycle)
                        .map_err(|error| AtlasArtifactError::new(error.to_string()))?;
                }
            }
            for pair in &space.critical_pairs {
                check_critical(&pair.birth, 2, self.vertex_count, space.interval.birth)?;
                match (&pair.death, space.interval.is_essential()) {
                    (None, true) => {}
                    (Some(death), false) => {
                        check_critical(death, 3, self.vertex_count, space.interval.death)?
                    }
                    _ => {
                        return Err(AtlasArtifactError::new(format!(
                            "space {space_index} critical pair has wrong death presence"
                        )));
                    }
                }
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
            space_bars.extend(std::iter::repeat_n(
                (
                    space.interval.birth.to_bits(),
                    space.interval.death.to_bits(),
                ),
                space.basis.len(),
            ));
        }
        h1_bars.sort_unstable();
        space_bars.sort_unstable();
        if h1_bars != space_bars {
            return Err(AtlasArtifactError::new(
                "class-space intervals do not match the H1 diagram",
            ));
        }
        for pair in self.explained.spaces.windows(2) {
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
        if let Some(input) = input {
            for space in &self.explained.spaces {
                for pair in &space.critical_pairs {
                    check_critical_value(input, &pair.birth)?;
                    if let Some(death) = &pair.death {
                        check_critical_value(input, death)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn check_bar(bar: &Bar) -> std::result::Result<(), &'static str> {
    if bar.dim > 1 {
        return Err("dimension exceeds one");
    }
    if !bar.birth.is_finite() || bar.birth < 0.0 || is_negative_zero(bar.birth) {
        return Err("birth is not a canonical non-negative finite value");
    }
    if bar.dim == 0 && bar.birth.to_bits() != 0 {
        return Err("H0 birth is not positive zero");
    }
    if bar.death.is_nan()
        || bar.death < 0.0
        || is_negative_zero(bar.death)
        || (bar.death.is_infinite() && !bar.death.is_sign_positive())
    {
        return Err("death is not a canonical non-negative value");
    }
    if bar.death.is_finite() && bar.death <= bar.birth {
        return Err("finite death does not follow birth");
    }
    Ok(())
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
        if term.u >= term.v
            || term.v >= vertex_count
            || term.coefficient == 0
            || term.coefficient >= cocycle.modulus
            || previous.is_some_and(|edge| edge >= (term.u, term.v))
        {
            return Err("terms are not canonical");
        }
        previous = Some((term.u, term.v));
    }
    Ok(())
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
