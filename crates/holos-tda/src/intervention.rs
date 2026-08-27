//! Certified finite H1 lifetime interventions.
//!
//! The supported query lowers independent edge weights in the declared
//! destroyer triangles of one finite class space. It asks that the complete
//! space die no later than a target scale. `Optimal` is relative to the
//! current checked reduction and its fixed destroyer simplices.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{
    CertificateLimits, EdgeKey, Error, ExplainedDiagram, IntervalGroupId, PersistenceProgram,
    ProgramTraceArtifact, ProgramTraceDecodeLimits, ProgramUpdateMode, SparseDistanceMatrix,
    VerifiedProgramTrace,
};

type Result<T, E = Error> = std::result::Result<T, E>;

const MAGIC: &[u8; 8] = b"HOLOSINT";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Deterministic search budget for a restricted intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterventionBudget {
    /// Largest number of candidate edits to check.
    pub max_candidates: usize,
}

impl InterventionBudget {
    /// Create a candidate-count budget.
    pub fn new(max_candidates: usize) -> Self {
        Self { max_candidates }
    }
}

impl Default for InterventionBudget {
    fn default() -> Self {
        Self { max_candidates: 1 }
    }
}

/// Strength of the returned intervention claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InterventionStatus {
    /// The feasible edit meets its lower bound inside the checked region.
    Optimal,
    /// The feasible edit has distinct checked lower and upper bounds.
    BoundedGap,
    /// The declared candidate budget ended without a continued feasible edit.
    BudgetLimited,
}

/// One independent edge-weight change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeWeightEdit {
    /// Edited edge.
    pub edge: EdgeKey,
    /// Weight before the edit.
    pub before: f64,
    /// Weight after the edit.
    pub after: f64,
}

/// Result of one restricted H1 intervention search.
#[derive(Debug, Clone)]
pub struct H1Intervention {
    /// Target class space at the initial graph.
    pub target: IntervalGroupId,
    /// Requested latest death scale.
    pub target_scale: f64,
    /// Strength of the returned claim.
    pub status: InterventionStatus,
    /// Checked lower bound on the maximum absolute edge change.
    pub lower_bound: f64,
    /// Feasible maximum absolute edge change, when one was found.
    pub upper_bound: Option<f64>,
    /// Feasible edge edits, empty when no candidate was certified.
    pub edits: Vec<EdgeWeightEdit>,
    /// Exact result after the edit, when one was found.
    pub result: Option<ExplainedDiagram>,
    /// Portable proof of the intervention, when one was found.
    pub artifact: Option<InterventionArtifact>,
}

impl PersistenceProgram {
    /// Find a checked independent-weight edit that shortens one finite H1
    /// class space.
    ///
    /// The target must satisfy `birth < target_scale < death`. The one
    /// current candidate lowers every edge above `target_scale` in every
    /// declared destroyer triangle. `Optimal` means optimal under the same
    /// reduction and critical-pair certificate. It is not a global inverse
    /// persistence claim across unrelated pairings.
    pub fn kill_h1_before(
        &self,
        target: IntervalGroupId,
        target_scale: f64,
        budget: InterventionBudget,
    ) -> Result<H1Intervention> {
        let space = validate_intervention_request(self, target, target_scale)?;
        if budget.max_candidates == 0 {
            return Ok(budget_limited_intervention(target, target_scale));
        }
        search_intervention(self, space, target, target_scale)
    }
}

fn validate_intervention_request(
    program: &PersistenceProgram,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<&crate::PersistentClassSpace> {
    if !target_scale.is_finite() || target_scale < 0.0 {
        return Err(Error::InvalidInput(format!(
            "intervention target scale must be non-negative and finite, got {target_scale}"
        )));
    }
    let space = program
        .result()
        .spaces
        .iter()
        .find(|space| space.id == target)
        .ok_or_else(|| Error::InvalidInput(format!("unknown class space {target}")))?;
    check_intervention_interval(space, target_scale)?;
    Ok(space)
}

fn check_intervention_interval(
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<()> {
    if space.interval.is_essential() {
        return Err(Error::InvalidInput(
            "the finite-death intervention does not support an essential class space".into(),
        ));
    }
    if target_scale <= space.interval.birth || target_scale >= space.interval.death {
        return Err(Error::InvalidInput(format!(
            "target scale must lie strictly inside ({}, {})",
            space.interval.birth, space.interval.death
        )));
    }
    Ok(())
}

fn budget_limited_intervention(target: IntervalGroupId, target_scale: f64) -> H1Intervention {
    H1Intervention {
        target,
        target_scale,
        status: InterventionStatus::BudgetLimited,
        lower_bound: 0.0,
        upper_bound: None,
        edits: Vec::new(),
        result: None,
        artifact: None,
    }
}

fn search_intervention(
    program: &PersistenceProgram,
    space: &crate::PersistentClassSpace,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<H1Intervention> {
    let edits = destroyer_edits(program.current_graph(), space, target_scale)?;
    if edits.is_empty() {
        return Err(Error::InvalidInput(
            "declared destroyer triangles need no edge edit".into(),
        ));
    }
    let updated = apply_edits(program.current_graph(), &edits)?;
    let trace = ProgramTraceArtifact::build(
        program.current_graph(),
        std::slice::from_ref(&updated),
        program.params(),
        program.limits(),
    )?;
    let verified = trace.verify(program.limits())?;
    if !continued_space_dies_by(&verified, target, target_scale) {
        return Ok(budget_limited_intervention(target, target_scale));
    }
    finish_intervention(program, space, target, target_scale, edits, trace, verified)
}

fn finish_intervention(
    program: &PersistenceProgram,
    space: &crate::PersistentClassSpace,
    target: IntervalGroupId,
    target_scale: f64,
    edits: Vec<EdgeWeightEdit>,
    trace: ProgramTraceArtifact,
    verified: VerifiedProgramTrace,
) -> Result<H1Intervention> {
    let upper_bound = maximum_edit(&edits);
    let reused = verified.steps[0].mode == ProgramUpdateMode::Reused;
    let lower_bound = if reused {
        space.interval.death - target_scale
    } else {
        0.0
    };
    let status = if reused && lower_bound.to_bits() == upper_bound.to_bits() {
        InterventionStatus::Optimal
    } else {
        InterventionStatus::BoundedGap
    };
    let artifact = InterventionArtifact {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound,
        edits: edits.clone(),
        trace,
    };
    artifact.verify(program.limits())?;
    Ok(H1Intervention {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound: Some(upper_bound),
        edits,
        result: Some(verified.final_program.result().clone()),
        artifact: Some(artifact),
    })
}

/// Failure while decoding or checking an intervention artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterventionError {
    message: String,
}

impl InterventionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated intervention rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InterventionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "intervention artifact: {}", self.message)
    }
}

impl std::error::Error for InterventionError {}

/// Decoder limits applied before intervention records are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct InterventionDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted edge-edit count.
    pub max_edits: usize,
    /// Largest accepted nested trace in bytes.
    pub max_trace_bytes: usize,
    /// Limits for the nested program trace.
    pub trace: ProgramTraceDecodeLimits,
}

impl Default for InterventionDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_edits: 100_000_000,
            max_trace_bytes: 1 << 30,
            trace: ProgramTraceDecodeLimits::default(),
        }
    }
}

/// Feasible H1 edit, bounds, and a nested independently checked trace.
#[derive(Debug, Clone)]
pub struct InterventionArtifact {
    target: IntervalGroupId,
    target_scale: f64,
    status: InterventionStatus,
    lower_bound: f64,
    upper_bound: f64,
    edits: Vec<EdgeWeightEdit>,
    trace: ProgramTraceArtifact,
}

impl InterventionArtifact {
    /// Target class-space identifier.
    pub fn target(&self) -> IntervalGroupId {
        self.target
    }

    /// Requested latest death scale.
    pub fn target_scale(&self) -> f64 {
        self.target_scale
    }

    /// Strength of the checked claim.
    pub fn status(&self) -> InterventionStatus {
        self.status
    }

    /// Checked lower bound on the maximum edge change.
    pub fn lower_bound(&self) -> f64 {
        self.lower_bound
    }

    /// Feasible maximum edge change.
    pub fn upper_bound(&self) -> f64 {
        self.upper_bound
    }

    /// Applied independent edge edits.
    pub fn edits(&self) -> &[EdgeWeightEdit] {
        &self.edits
    }

    /// Nested exact program trace.
    pub fn trace(&self) -> &ProgramTraceArtifact {
        &self.trace
    }

    /// Encode the canonical `HOLOSINT` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, InterventionError> {
        self.check_shape()?;
        let trace = encode_trace(&self.trace)?;
        let mut out = Vec::new();
        encode_intervention_header(&mut out, self, trace.len())?;
        encode_edits(&mut out, &self.edits)?;
        out.extend_from_slice(&trace);
        Ok(out)
    }

    /// Decode and structurally validate a bounded intervention envelope.
    pub fn decode(
        bytes: &[u8],
        limits: InterventionDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, InterventionError> {
        check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_intervention_header(&mut reader, limits)?;
        check_record_bytes(&reader, header.edit_count, header.trace_bytes)?;
        let edits = decode_edits(&mut reader, header.edit_count)?;
        let trace = decode_trace(&mut reader, header.trace_bytes, limits, certificate_limits)?;
        check_no_trailing_bytes(&reader)?;
        let artifact = Self {
            target: header.target,
            target_scale: header.target_scale,
            status: header.status,
            lower_bound: header.lower_bound,
            upper_bound: header.upper_bound,
            edits,
            trace,
        };
        artifact.check_shape()?;
        Ok(artifact)
    }

    /// Check the edit, bounds, target continuation, and nested trace.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedIntervention, InterventionError> {
        self.check_shape()?;
        let verified = verify_trace(&self.trace, certificate_limits)?;
        check_trace_cardinality(&self.trace, &verified)?;
        let initial = self.trace.initial_graph();
        let updated = self.trace.steps()[0].graph();
        check_applied_edits(initial, updated, &self.edits)?;
        let space = find_target_space(&verified, self.target)?;
        check_target_interval(space, self.target_scale)?;
        check_target_death(&verified, self.target, self.target_scale)?;
        check_upper_bound(&self.edits, self.upper_bound)?;
        check_intervention_status(self, &verified, initial, space)?;
        Ok(VerifiedIntervention {
            status: self.status,
            target: self.target,
            target_scale: self.target_scale,
            lower_bound: self.lower_bound,
            upper_bound: self.upper_bound,
            edits: self.edits.clone(),
            result: verified.final_program.result().clone(),
        })
    }

    fn check_shape(&self) -> std::result::Result<(), InterventionError> {
        check_scalar_shape(self)?;
        check_edit_shape(&self.edits)
    }
}

/// Result of independently checking a feasible intervention.
#[derive(Debug, Clone)]
pub struct VerifiedIntervention {
    /// Strength of the checked claim.
    pub status: InterventionStatus,
    /// Initial target class space.
    pub target: IntervalGroupId,
    /// Requested latest death scale.
    pub target_scale: f64,
    /// Checked lower bound.
    pub lower_bound: f64,
    /// Feasible upper bound.
    pub upper_bound: f64,
    /// Checked edge edits.
    pub edits: Vec<EdgeWeightEdit>,
    /// Exact final diagram and class spaces.
    pub result: ExplainedDiagram,
}

struct InterventionHeader {
    target: IntervalGroupId,
    target_scale: f64,
    status: InterventionStatus,
    lower_bound: f64,
    upper_bound: f64,
    edit_count: usize,
    trace_bytes: usize,
}

fn encode_trace(trace: &ProgramTraceArtifact) -> Result<Vec<u8>, InterventionError> {
    trace
        .encode()
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn encode_intervention_header(
    out: &mut Vec<u8>,
    artifact: &InterventionArtifact,
    trace_bytes: usize,
) -> Result<(), InterventionError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    out.extend_from_slice(artifact.target.as_bytes());
    put_u64(out, artifact.target_scale.to_bits());
    out.push(status_tag(artifact.status));
    put_u64(out, artifact.lower_bound.to_bits());
    put_u64(out, artifact.upper_bound.to_bits());
    put_usize(out, artifact.edits.len(), "edge-edit count")?;
    put_usize(out, trace_bytes, "trace byte count")?;
    Ok(())
}

fn encode_edits(out: &mut Vec<u8>, edits: &[EdgeWeightEdit]) -> Result<(), InterventionError> {
    for edit in edits {
        put_usize(out, edit.edge.u, "edge endpoint")?;
        put_usize(out, edit.edge.v, "edge endpoint")?;
        put_u64(out, edit.before.to_bits());
        put_u64(out, edit.after.to_bits());
    }
    Ok(())
}

fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), InterventionError> {
    if bytes.len() > max_bytes {
        return Err(InterventionError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

fn decode_intervention_header(
    reader: &mut Reader<'_>,
    limits: InterventionDecodeLimits,
) -> Result<InterventionHeader, InterventionError> {
    check_intervention_identity(reader)?;
    let target = IntervalGroupId::from_bytes(reader.array32()?);
    let (target_scale, status, lower_bound, upper_bound) = decode_claim(reader)?;
    let (edit_count, trace_bytes) = decode_intervention_counts(reader, limits)?;
    Ok(InterventionHeader {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound,
        edit_count,
        trace_bytes,
    })
}

fn check_intervention_identity(reader: &mut Reader<'_>) -> Result<(), InterventionError> {
    if reader.take(8)? != MAGIC {
        return Err(InterventionError::new("wrong magic bytes"));
    }
    if reader.u16()? != WIRE_VERSION {
        return Err(InterventionError::new("unsupported wire version"));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(InterventionError::new("unsupported scalar codec"));
    }
    Ok(())
}

fn decode_claim(
    reader: &mut Reader<'_>,
) -> Result<(f64, InterventionStatus, f64, f64), InterventionError> {
    Ok((
        f64::from_bits(reader.u64()?),
        decode_status(reader.u8()?)?,
        f64::from_bits(reader.u64()?),
        f64::from_bits(reader.u64()?),
    ))
}

fn decode_intervention_counts(
    reader: &mut Reader<'_>,
    limits: InterventionDecodeLimits,
) -> Result<(usize, usize), InterventionError> {
    Ok((
        reader.bounded_usize("edge-edit count", limits.max_edits)?,
        reader.bounded_usize("trace byte count", limits.max_trace_bytes)?,
    ))
}

fn check_record_bytes(
    reader: &Reader<'_>,
    edit_count: usize,
    trace_bytes: usize,
) -> Result<(), InterventionError> {
    let fixed = edit_count
        .checked_mul(32)
        .and_then(|edits| edits.checked_add(trace_bytes))
        .ok_or_else(|| InterventionError::new("record bytes overflow usize"))?;
    if fixed > reader.remaining() {
        return Err(InterventionError::new("record exceeds the remaining bytes"));
    }
    Ok(())
}

fn decode_edits(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<EdgeWeightEdit>, InterventionError> {
    let mut edits = Vec::with_capacity(count);
    for _ in 0..count {
        edits.push(EdgeWeightEdit {
            edge: EdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            },
            before: f64::from_bits(reader.u64()?),
            after: f64::from_bits(reader.u64()?),
        });
    }
    Ok(edits)
}

fn decode_trace(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: InterventionDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<ProgramTraceArtifact, InterventionError> {
    ProgramTraceArtifact::decode(reader.take(byte_count)?, limits.trace, certificate_limits)
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), InterventionError> {
    if reader.remaining() != 0 {
        return Err(InterventionError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}

fn verify_trace(
    trace: &ProgramTraceArtifact,
    certificate_limits: CertificateLimits,
) -> Result<VerifiedProgramTrace, InterventionError> {
    trace
        .verify(certificate_limits)
        .map_err(|error| InterventionError::new(error.to_string()))
}

fn check_trace_cardinality(
    trace: &ProgramTraceArtifact,
    verified: &VerifiedProgramTrace,
) -> Result<(), InterventionError> {
    if trace.steps().len() != 1 || verified.steps.len() != 1 {
        return Err(InterventionError::new(
            "intervention trace must contain exactly one update",
        ));
    }
    Ok(())
}

fn check_applied_edits(
    initial: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    edits: &[EdgeWeightEdit],
) -> Result<(), InterventionError> {
    let applied =
        apply_edits(initial, edits).map_err(|error| InterventionError::new(error.to_string()))?;
    if !graph_bits_equal(&applied, updated) {
        return Err(InterventionError::new(
            "edge edits do not reproduce the traced graph",
        ));
    }
    Ok(())
}

fn find_target_space(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
) -> Result<&crate::PersistentClassSpace, InterventionError> {
    verified
        .initial_result
        .spaces
        .iter()
        .find(|space| space.id == target)
        .ok_or_else(|| InterventionError::new("target space is absent initially"))
}

fn check_target_interval(
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<(), InterventionError> {
    let outside = space.interval.is_essential()
        || target_scale <= space.interval.birth
        || target_scale >= space.interval.death;
    if outside {
        return Err(InterventionError::new(
            "target scale is outside the finite target interval",
        ));
    }
    Ok(())
}

fn check_target_death(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<(), InterventionError> {
    if !continued_space_dies_by(verified, target, target_scale) {
        return Err(InterventionError::new(
            "the continued target space does not die by the requested scale",
        ));
    }
    Ok(())
}

fn check_upper_bound(edits: &[EdgeWeightEdit], upper_bound: f64) -> Result<(), InterventionError> {
    if maximum_edit(edits).to_bits() != upper_bound.to_bits() {
        return Err(InterventionError::new(
            "upper bound differs from the applied edit",
        ));
    }
    Ok(())
}

fn check_intervention_status(
    artifact: &InterventionArtifact,
    verified: &VerifiedProgramTrace,
    initial: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
) -> Result<(), InterventionError> {
    match artifact.status {
        InterventionStatus::Optimal => check_optimal_claim(artifact, verified, initial, space),
        InterventionStatus::BoundedGap => check_bounded_gap_claim(artifact.lower_bound),
        InterventionStatus::BudgetLimited => Err(InterventionError::new(
            "a budget-limited search has no feasible artifact",
        )),
    }
}

fn check_optimal_claim(
    artifact: &InterventionArtifact,
    verified: &VerifiedProgramTrace,
    initial: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
) -> Result<(), InterventionError> {
    if verified.steps[0].mode != ProgramUpdateMode::Reused {
        return Err(InterventionError::new(
            "an optimal claim left the checked reduction region",
        ));
    }
    let expected = destroyer_edits(initial, space, artifact.target_scale)
        .map_err(|error| InterventionError::new(error.to_string()))?;
    let lower = space.interval.death - artifact.target_scale;
    check_optimal_edit_and_bounds(artifact, &expected, lower)
}

fn check_optimal_edit_and_bounds(
    artifact: &InterventionArtifact,
    expected: &[EdgeWeightEdit],
    lower: f64,
) -> Result<(), InterventionError> {
    let differs = !edits_bits_equal(expected, &artifact.edits)
        || lower.to_bits() != artifact.lower_bound.to_bits()
        || lower.to_bits() != artifact.upper_bound.to_bits();
    if differs {
        return Err(InterventionError::new(
            "optimal edit or matching bound is not canonical",
        ));
    }
    Ok(())
}

fn check_bounded_gap_claim(lower_bound: f64) -> Result<(), InterventionError> {
    if lower_bound.to_bits() != 0 {
        return Err(InterventionError::new(
            "bounded-gap lower bound must be zero",
        ));
    }
    Ok(())
}

fn check_scalar_shape(artifact: &InterventionArtifact) -> Result<(), InterventionError> {
    let invalid = !artifact.target_scale.is_finite()
        || artifact.target_scale < 0.0
        || !artifact.lower_bound.is_finite()
        || artifact.lower_bound < 0.0
        || !artifact.upper_bound.is_finite()
        || artifact.upper_bound < artifact.lower_bound;
    if invalid {
        return Err(InterventionError::new(
            "target scale or bounds are not canonical",
        ));
    }
    Ok(())
}

fn check_edit_shape(edits: &[EdgeWeightEdit]) -> Result<(), InterventionError> {
    let invalid = edits.is_empty()
        || edits.windows(2).any(|pair| pair[0].edge >= pair[1].edge)
        || edits.iter().any(|edit| !edit_is_canonical(edit));
    if invalid {
        return Err(InterventionError::new(
            "edge edits are not canonical strict decreases",
        ));
    }
    Ok(())
}

fn edit_is_canonical(edit: &EdgeWeightEdit) -> bool {
    edit.edge.u < edit.edge.v
        && edit.before.is_finite()
        && edit.after.is_finite()
        && edit.after >= 0.0
        && edit.after < edit.before
}

fn destroyer_edits(
    graph: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<Vec<EdgeWeightEdit>> {
    let mut edges = BTreeSet::new();
    for pair in &space.critical_pairs {
        let death = pair.death.as_ref().ok_or_else(|| {
            Error::InvalidInput("finite class space has no destroyer triangle".into())
        })?;
        let [u, v, w]: [usize; 3] = death
            .vertices
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidInput("destroyer is not a triangle".into()))?;
        for edge in [EdgeKey::new(u, v), EdgeKey::new(u, w), EdgeKey::new(v, w)] {
            if graph.get(edge.u, edge.v) > target_scale {
                edges.insert(edge);
            }
        }
    }
    Ok(edges
        .into_iter()
        .map(|edge| EdgeWeightEdit {
            edge,
            before: graph.get(edge.u, edge.v),
            after: target_scale,
        })
        .collect())
}

fn apply_edits(
    graph: &SparseDistanceMatrix,
    edits: &[EdgeWeightEdit],
) -> Result<SparseDistanceMatrix> {
    let by_edge: BTreeMap<_, _> = edits.iter().map(|edit| (edit.edge, edit)).collect();
    let triplets: Vec<_> = graph
        .edges()
        .map(|(u, v, value)| {
            let edge = EdgeKey::new(u, v);
            let value = by_edge.get(&edge).map_or(value, |edit| edit.after);
            (u, v, value)
        })
        .collect();
    if by_edge
        .keys()
        .any(|edge| graph.get(edge.u, edge.v).is_infinite())
    {
        return Err(Error::InvalidInput(
            "intervention edit names an absent edge".into(),
        ));
    }
    SparseDistanceMatrix::from_triplets(graph.len(), &triplets)
}

fn continued_space_dies_by(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
    target_scale: f64,
) -> bool {
    let Some(initial) = verified
        .initial_result
        .spaces
        .iter()
        .find(|space| space.id == target)
    else {
        return false;
    };
    let Some(step) = verified.steps.first() else {
        return false;
    };
    let final_by_basis: BTreeMap<_, _> = verified
        .final_program
        .result()
        .spaces
        .iter()
        .flat_map(|space| space.basis.iter().map(move |class| (class.id, space)))
        .collect();
    let mapped: BTreeMap<_, _> = step
        .continuation
        .iter()
        .filter(|record| record.old_spaces.contains(&target))
        .flat_map(|record| record.transport.iter().map(|term| (term.old, term.new)))
        .collect();
    initial.basis.iter().all(|class| {
        mapped
            .get(&class.id)
            .and_then(|new| final_by_basis.get(new))
            .is_some_and(|space| space.interval.death <= target_scale)
    })
}

fn maximum_edit(edits: &[EdgeWeightEdit]) -> f64 {
    edits
        .iter()
        .map(|edit| edit.before - edit.after)
        .fold(0.0f64, f64::max)
}

fn graph_bits_equal(a: &SparseDistanceMatrix, b: &SparseDistanceMatrix) -> bool {
    a.len() == b.len()
        && a.num_edges() == b.num_edges()
        && a.edges()
            .zip(b.edges())
            .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2.to_bits() == b.2.to_bits())
}

fn edits_bits_equal(a: &[EdgeWeightEdit], b: &[EdgeWeightEdit]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.edge == b.edge
                && a.before.to_bits() == b.before.to_bits()
                && a.after.to_bits() == b.after.to_bits()
        })
}

fn status_tag(status: InterventionStatus) -> u8 {
    match status {
        InterventionStatus::Optimal => 0,
        InterventionStatus::BoundedGap => 1,
        InterventionStatus::BudgetLimited => 2,
    }
}

fn decode_status(tag: u8) -> std::result::Result<InterventionStatus, InterventionError> {
    match tag {
        0 => Ok(InterventionStatus::Optimal),
        1 => Ok(InterventionStatus::BoundedGap),
        2 => Ok(InterventionStatus::BudgetLimited),
        _ => Err(InterventionError::new(format!(
            "unknown intervention-status tag {tag}"
        ))),
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), InterventionError> {
    let value = u64::try_from(value)
        .map_err(|_| InterventionError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
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

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], InterventionError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| InterventionError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(InterventionError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, InterventionError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, InterventionError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, InterventionError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, InterventionError> {
        usize::try_from(self.u64()?)
            .map_err(|_| InterventionError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, InterventionError> {
        let value = self.usize()?;
        if value > limit {
            return Err(InterventionError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], InterventionError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

impl From<InterventionError> for Error {
    fn from(error: InterventionError) -> Self {
        Self::InvalidInput(error.to_string())
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
                (0, 2, 3.0),
                (1, 3, 3.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn finite_class_intervention_is_feasible_and_independently_checked() {
        let graph = square();
        let program = PersistenceProgram::compile(
            &graph,
            &crate::RipsParams::new(1).with_modulus(3),
            CertificateLimits::default(),
        )
        .unwrap();
        let target = program.result().spaces[0].id;
        let intervention = program
            .kill_h1_before(target, 2.5, InterventionBudget::default())
            .unwrap();
        assert!(matches!(
            intervention.status,
            InterventionStatus::Optimal | InterventionStatus::BoundedGap
        ));
        assert!(intervention.upper_bound.is_some());
        assert!(
            intervention
                .result
                .as_ref()
                .unwrap()
                .spaces
                .iter()
                .all(|space| space.interval.death <= 2.5)
        );
        let artifact = intervention.artifact.unwrap();
        let bytes = artifact.encode().unwrap();
        let decoded = InterventionArtifact::decode(
            &bytes,
            InterventionDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        decoded.verify(CertificateLimits::default()).unwrap();
    }

    #[test]
    fn zero_budget_and_essential_classes_are_honest() {
        let graph = square();
        let program = PersistenceProgram::compile(
            &graph,
            &crate::RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        let target = program.result().spaces[0].id;
        let limited = program
            .kill_h1_before(target, 2.5, InterventionBudget::new(0))
            .unwrap();
        assert_eq!(limited.status, InterventionStatus::BudgetLimited);
        assert!(limited.artifact.is_none());

        let cycle = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let essential = PersistenceProgram::compile(
            &cycle,
            &crate::RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(
            essential
                .kill_h1_before(
                    essential.result().spaces[0].id,
                    2.0,
                    InterventionBudget::default()
                )
                .is_err()
        );
    }

    #[test]
    fn mutations_and_truncation_are_rejected() {
        let graph = square();
        let program = PersistenceProgram::compile(
            &graph,
            &crate::RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        let intervention = program
            .kill_h1_before(
                program.result().spaces[0].id,
                2.5,
                InterventionBudget::default(),
            )
            .unwrap();
        let artifact = intervention.artifact.unwrap();
        let bytes = artifact.encode().unwrap();
        assert!((0..bytes.len()).all(|end| {
            InterventionArtifact::decode(
                &bytes[..end],
                InterventionDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        }));
        let mut changed = artifact.clone();
        changed.upper_bound += 1.0;
        assert!(changed.verify(CertificateLimits::default()).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = InterventionArtifact::decode(
                &bytes,
                InterventionDecodeLimits::default(),
                CertificateLimits::default(),
            );
        }
    }
}
