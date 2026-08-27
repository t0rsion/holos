//! Portable, independently checked persistence trajectories.
//!
//! A `HOLOSTRC` record contains the initial graph and proof-carrying atlas,
//! every later graph, each validity-region event, and a new proof at every
//! region boundary. The verifier checks proofs at boundaries and evaluates
//! all other steps from the current atlas without calling the persistence
//! solver.

use std::fmt;

use crate::{
    AtlasArtifact, AtlasDecodeLimits, AtlasEvaluation, CertificateLimits, EdgeKey, RipsParams,
    SparseDistanceMatrix, TopologyEvent, TopologyEventKind, UpdateMode,
};

const MAGIC: &[u8; 8] = b"HOLOSTRC";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing, decoding, or checking a trajectory artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryError {
    message: String,
}

impl TrajectoryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated trajectory rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TrajectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trajectory artifact: {}", self.message)
    }
}

impl std::error::Error for TrajectoryError {}

/// Decoder limits applied before trajectory collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TrajectoryDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted update count.
    pub max_steps: usize,
    /// Largest vertex count in one graph.
    pub max_vertices: usize,
    /// Largest total edge count across all graphs.
    pub max_total_edges: usize,
    /// Largest total event count.
    pub max_total_events: usize,
    /// Largest nested atlas envelope in bytes.
    pub max_atlas_bytes: usize,
}

impl Default for TrajectoryDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_steps: 10_000_000,
            max_vertices: 1_000_000,
            max_total_edges: 200_000_000,
            max_total_events: 100_000_000,
            max_atlas_bytes: 1 << 30,
        }
    }
}

/// One graph and its transition from the preceding atlas region.
#[derive(Debug, Clone)]
pub struct TrajectoryStep {
    input: SparseDistanceMatrix,
    mode: UpdateMode,
    events: Vec<TopologyEvent>,
    checkpoint: Option<AtlasArtifact>,
}

impl TrajectoryStep {
    /// Graph evaluated at this step.
    pub fn input(&self) -> &SparseDistanceMatrix {
        &self.input
    }

    /// Whether this step reuses the prior atlas or starts a new region.
    pub fn mode(&self) -> UpdateMode {
        self.mode
    }

    /// Events declared at this step.
    pub fn events(&self) -> &[TopologyEvent] {
        &self.events
    }

    /// Proof-carrying atlas at a region boundary.
    pub fn checkpoint(&self) -> Option<&AtlasArtifact> {
        self.checkpoint.as_ref()
    }
}

/// Self-contained graphs, events, and proofs for a persistence trajectory.
#[derive(Debug, Clone)]
pub struct TrajectoryArtifact {
    initial_input: SparseDistanceMatrix,
    initial_atlas: AtlasArtifact,
    steps: Vec<TrajectoryStep>,
}

impl TrajectoryArtifact {
    /// Compile and certify a sequence of sparse weighted graphs.
    pub fn build(
        initial: &SparseDistanceMatrix,
        updates: &[SparseDistanceMatrix],
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, TrajectoryError> {
        let (initial_atlas, mut atlas) =
            AtlasArtifact::compile(initial, params, certificate_limits)
                .map_err(|error| TrajectoryError::new(error.to_string()))?;
        let mut steps = Vec::with_capacity(updates.len());
        for input in updates {
            let events = atlas.events(input);
            if events.is_empty() {
                atlas
                    .evaluate(input)
                    .map_err(|error| TrajectoryError::new(error.to_string()))?;
                steps.push(TrajectoryStep {
                    input: input.clone(),
                    mode: UpdateMode::Reused,
                    events,
                    checkpoint: None,
                });
            } else {
                let (checkpoint, next) = AtlasArtifact::compile(input, params, certificate_limits)
                    .map_err(|error| TrajectoryError::new(error.to_string()))?;
                atlas = next;
                steps.push(TrajectoryStep {
                    input: input.clone(),
                    mode: UpdateMode::Recomputed,
                    events,
                    checkpoint: Some(checkpoint),
                });
            }
        }
        Ok(Self {
            initial_input: initial.clone(),
            initial_atlas,
            steps,
        })
    }

    /// Initial graph.
    pub fn initial_input(&self) -> &SparseDistanceMatrix {
        &self.initial_input
    }

    /// Initial proof-carrying atlas.
    pub fn initial_atlas(&self) -> &AtlasArtifact {
        &self.initial_atlas
    }

    /// Ordered trajectory steps.
    pub fn steps(&self) -> &[TrajectoryStep] {
        &self.steps
    }

    /// Encode the canonical `HOLOSTRC` version 1 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, TrajectoryError> {
        self.check_shape()?;
        let initial_atlas = encode_atlas(&self.initial_atlas)?;
        let checkpoints = encode_checkpoints(&self.steps)?;
        let mut out = Vec::new();
        encode_trajectory_header(&mut out, self.steps.len(), initial_atlas.len())?;
        encode_graph(&mut out, &self.initial_input)?;
        out.extend_from_slice(&initial_atlas);
        for (step, checkpoint) in self.steps.iter().zip(checkpoints) {
            encode_trajectory_step(&mut out, step, checkpoint.as_deref())?;
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded trajectory envelope.
    pub fn decode(
        bytes: &[u8],
        limits: TrajectoryDecodeLimits,
        atlas_limits: AtlasDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, TrajectoryError> {
        check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_trajectory_header(&mut reader, limits)?;
        let mut total_edges = 0usize;
        let initial_input = decode_graph(&mut reader, limits, &mut total_edges)?;
        let initial_atlas = decode_nested_atlas(
            &mut reader,
            header.initial_atlas_bytes,
            limits,
            atlas_limits,
            certificate_limits,
        )?;
        let mut total_events = 0usize;
        let mut context = TrajectoryDecodeContext {
            limits,
            atlas_limits,
            certificate_limits,
            total_edges: &mut total_edges,
            total_events: &mut total_events,
        };
        let steps = decode_trajectory_steps(&mut reader, header.step_count, &mut context)?;
        check_no_trailing_bytes(&reader)?;
        let artifact = Self {
            initial_input,
            initial_atlas,
            steps,
        };
        artifact.check_shape()?;
        Ok(artifact)
    }

    /// Verify all proofs and region transitions without the persistence
    /// solver.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedTrajectory, TrajectoryError> {
        self.check_shape()?;
        let mut atlas = verify_atlas(&self.initial_atlas, &self.initial_input, certificate_limits)?;
        let initial = atlas
            .evaluate(&self.initial_input)
            .map_err(|error| TrajectoryError::new(error.to_string()))?;
        let mut verified_steps = Vec::with_capacity(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            verified_steps.push(verify_trajectory_step(
                &mut atlas,
                step,
                index,
                certificate_limits,
            )?);
        }
        Ok(VerifiedTrajectory {
            initial,
            steps: verified_steps,
        })
    }

    fn check_shape(&self) -> std::result::Result<(), TrajectoryError> {
        for (index, step) in self.steps.iter().enumerate() {
            let checkpoint_matches = matches!(
                (step.mode, step.checkpoint.is_some()),
                (UpdateMode::Reused, false) | (UpdateMode::Recomputed, true)
            );
            if !checkpoint_matches {
                return Err(TrajectoryError::new(format!(
                    "step {index} checkpoint does not match its mode"
                )));
            }
            if (step.mode == UpdateMode::Reused) != step.events.is_empty() {
                return Err(TrajectoryError::new(format!(
                    "step {index} event count does not match its mode"
                )));
            }
        }
        Ok(())
    }
}

/// Checked evaluation at one trajectory step.
#[derive(Debug, Clone)]
pub struct VerifiedTrajectoryStep {
    /// Checked transition mode.
    pub mode: UpdateMode,
    /// Events derived from the preceding atlas.
    pub events: Vec<TopologyEvent>,
    /// Exact H0 and H1 result.
    pub evaluation: AtlasEvaluation,
}

/// Results reconstructed from a checked trajectory artifact.
#[derive(Debug, Clone)]
pub struct VerifiedTrajectory {
    /// Exact result for the initial graph.
    pub initial: AtlasEvaluation,
    /// Checked update results.
    pub steps: Vec<VerifiedTrajectoryStep>,
}

struct TrajectoryHeader {
    step_count: usize,
    initial_atlas_bytes: usize,
}

struct TrajectoryDecodeContext<'a> {
    limits: TrajectoryDecodeLimits,
    atlas_limits: AtlasDecodeLimits,
    certificate_limits: CertificateLimits,
    total_edges: &'a mut usize,
    total_events: &'a mut usize,
}

fn encode_atlas(atlas: &AtlasArtifact) -> Result<Vec<u8>, TrajectoryError> {
    atlas
        .encode()
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

fn encode_checkpoints(steps: &[TrajectoryStep]) -> Result<Vec<Option<Vec<u8>>>, TrajectoryError> {
    let mut checkpoints = Vec::with_capacity(steps.len());
    for step in steps {
        checkpoints.push(step.checkpoint.as_ref().map(encode_atlas).transpose()?);
    }
    Ok(checkpoints)
}

fn encode_trajectory_header(
    out: &mut Vec<u8>,
    step_count: usize,
    initial_atlas_bytes: usize,
) -> Result<(), TrajectoryError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    put_usize(out, step_count, "step count")?;
    put_usize(out, initial_atlas_bytes, "initial atlas byte count")?;
    Ok(())
}

fn encode_trajectory_step(
    out: &mut Vec<u8>,
    step: &TrajectoryStep,
    checkpoint: Option<&[u8]>,
) -> Result<(), TrajectoryError> {
    encode_graph(out, &step.input)?;
    out.push(mode_tag(step.mode));
    put_usize(out, step.events.len(), "event count")?;
    encode_events(out, &step.events)?;
    put_usize(
        out,
        checkpoint.map_or(0, <[u8]>::len),
        "checkpoint byte count",
    )?;
    if let Some(checkpoint) = checkpoint {
        out.extend_from_slice(checkpoint);
    }
    Ok(())
}

fn encode_events(out: &mut Vec<u8>, events: &[TopologyEvent]) -> Result<(), TrajectoryError> {
    for event in events {
        encode_event(out, event)?;
    }
    Ok(())
}

fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), TrajectoryError> {
    if bytes.len() > max_bytes {
        return Err(TrajectoryError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

fn decode_trajectory_header(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
) -> Result<TrajectoryHeader, TrajectoryError> {
    check_trajectory_identity(reader)?;
    Ok(TrajectoryHeader {
        step_count: reader.bounded_usize("step count", limits.max_steps)?,
        initial_atlas_bytes: reader
            .bounded_usize("initial atlas byte count", limits.max_atlas_bytes)?,
    })
}

fn check_trajectory_identity(reader: &mut Reader<'_>) -> Result<(), TrajectoryError> {
    if reader.take(8)? != MAGIC {
        return Err(TrajectoryError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(TrajectoryError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(TrajectoryError::new("unsupported scalar codec"));
    }
    Ok(())
}

fn decode_nested_atlas(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: TrajectoryDecodeLimits,
    atlas_limits: AtlasDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<AtlasArtifact, TrajectoryError> {
    decode_atlas(
        reader.take(byte_count)?,
        limits,
        atlas_limits,
        certificate_limits,
    )
}

fn decode_trajectory_steps(
    reader: &mut Reader<'_>,
    count: usize,
    context: &mut TrajectoryDecodeContext<'_>,
) -> Result<Vec<TrajectoryStep>, TrajectoryError> {
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        steps.push(decode_trajectory_step(reader, context)?);
    }
    Ok(steps)
}

fn decode_trajectory_step(
    reader: &mut Reader<'_>,
    context: &mut TrajectoryDecodeContext<'_>,
) -> Result<TrajectoryStep, TrajectoryError> {
    let input = decode_graph(reader, context.limits, context.total_edges)?;
    let mode = decode_mode(reader.u8()?)?;
    let event_count = reader.usize()?;
    add_event_count(
        context.total_events,
        event_count,
        context.limits.max_total_events,
    )?;
    let events = decode_events(reader, event_count)?;
    let checkpoint_bytes =
        reader.bounded_usize("checkpoint byte count", context.limits.max_atlas_bytes)?;
    let checkpoint = decode_checkpoint(reader, checkpoint_bytes, context)?;
    Ok(TrajectoryStep {
        input,
        mode,
        events,
        checkpoint,
    })
}

fn add_event_count(total: &mut usize, count: usize, limit: usize) -> Result<(), TrajectoryError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| TrajectoryError::new("event count overflows usize"))?;
    if *total > limit {
        return Err(TrajectoryError::new(format!(
            "{} events exceed the decoder limit {limit}",
            *total
        )));
    }
    Ok(())
}

fn decode_events(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<TopologyEvent>, TrajectoryError> {
    check_event_bytes(reader, count)?;
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        events.push(decode_event(reader)?);
    }
    Ok(events)
}

fn check_event_bytes(reader: &Reader<'_>, count: usize) -> Result<(), TrajectoryError> {
    let minimum = count
        .checked_mul(7)
        .ok_or_else(|| TrajectoryError::new("event bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(TrajectoryError::new("events exceed the remaining bytes"));
    }
    Ok(())
}

fn decode_checkpoint(
    reader: &mut Reader<'_>,
    byte_count: usize,
    context: &TrajectoryDecodeContext<'_>,
) -> Result<Option<AtlasArtifact>, TrajectoryError> {
    if byte_count == 0 {
        return Ok(None);
    }
    decode_nested_atlas(
        reader,
        byte_count,
        context.limits,
        context.atlas_limits,
        context.certificate_limits,
    )
    .map(Some)
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), TrajectoryError> {
    if reader.remaining() != 0 {
        return Err(TrajectoryError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}

fn verify_atlas(
    artifact: &AtlasArtifact,
    input: &SparseDistanceMatrix,
    certificate_limits: CertificateLimits,
) -> Result<crate::PersistenceAtlas, TrajectoryError> {
    artifact
        .verify(input, certificate_limits)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

fn verify_trajectory_step(
    atlas: &mut crate::PersistenceAtlas,
    step: &TrajectoryStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<VerifiedTrajectoryStep, TrajectoryError> {
    let events = atlas.events(&step.input);
    check_step_events(&events, &step.events, index)?;
    let mode = mode_for_events(&events);
    check_step_mode(step.mode, mode, index)?;
    let evaluation = evaluate_trajectory_step(atlas, step, mode, index, certificate_limits)?;
    Ok(VerifiedTrajectoryStep {
        mode,
        events,
        evaluation,
    })
}

fn check_step_events(
    actual: &[TopologyEvent],
    recorded: &[TopologyEvent],
    index: usize,
) -> Result<(), TrajectoryError> {
    if !events_bits_equal(actual, recorded) {
        return Err(TrajectoryError::new(format!(
            "step {index} events differ from the current atlas"
        )));
    }
    Ok(())
}

fn mode_for_events(events: &[TopologyEvent]) -> UpdateMode {
    if events.is_empty() {
        UpdateMode::Reused
    } else {
        UpdateMode::Recomputed
    }
}

fn check_step_mode(
    recorded: UpdateMode,
    actual: UpdateMode,
    index: usize,
) -> Result<(), TrajectoryError> {
    if recorded != actual {
        return Err(TrajectoryError::new(format!(
            "step {index} mode differs from its events"
        )));
    }
    Ok(())
}

fn evaluate_trajectory_step(
    atlas: &mut crate::PersistenceAtlas,
    step: &TrajectoryStep,
    mode: UpdateMode,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<AtlasEvaluation, TrajectoryError> {
    match (&step.checkpoint, mode) {
        (None, UpdateMode::Reused) => atlas
            .evaluate(&step.input)
            .map_err(|error| TrajectoryError::new(error.to_string())),
        (Some(checkpoint), UpdateMode::Recomputed) => {
            *atlas = verify_atlas(checkpoint, &step.input, certificate_limits)?;
            atlas
                .evaluate(&step.input)
                .map_err(|error| TrajectoryError::new(error.to_string()))
        }
        _ => Err(TrajectoryError::new(format!(
            "step {index} checkpoint does not match its mode"
        ))),
    }
}

fn decode_atlas(
    bytes: &[u8],
    trace_limits: TrajectoryDecodeLimits,
    mut atlas_limits: AtlasDecodeLimits,
    mut certificate_limits: CertificateLimits,
) -> std::result::Result<AtlasArtifact, TrajectoryError> {
    atlas_limits.max_bytes = atlas_limits
        .max_bytes
        .min(trace_limits.max_atlas_bytes)
        .min(bytes.len());
    certificate_limits.max_bytes = certificate_limits.max_bytes.min(bytes.len());
    AtlasArtifact::decode(bytes, atlas_limits, certificate_limits)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

fn encode_graph(
    out: &mut Vec<u8>,
    input: &SparseDistanceMatrix,
) -> std::result::Result<(), TrajectoryError> {
    let edges: Vec<_> = input.edges().collect();
    put_usize(out, input.len(), "graph vertex count")?;
    put_usize(out, edges.len(), "graph edge count")?;
    for (u, v, value) in edges {
        put_usize(out, u, "edge endpoint")?;
        put_usize(out, v, "edge endpoint")?;
        put_u64(out, value.to_bits());
    }
    Ok(())
}

fn decode_graph(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
    total_edges: &mut usize,
) -> std::result::Result<SparseDistanceMatrix, TrajectoryError> {
    let (vertices, edges) = decode_graph_counts(reader, limits)?;
    add_graph_edges(total_edges, edges, limits.max_total_edges)?;
    check_graph_bytes(reader, edges)?;
    let triplets = decode_graph_triplets(reader, vertices, edges)?;
    SparseDistanceMatrix::from_triplets(vertices, &triplets)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

fn decode_graph_counts(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
) -> Result<(usize, usize), TrajectoryError> {
    let vertices = reader.bounded_usize("graph vertex count", limits.max_vertices)?;
    let possible = vertices
        .checked_mul(vertices.saturating_sub(1))
        .map(|value| value / 2)
        .unwrap_or(usize::MAX);
    let edges = reader.bounded_usize("graph edge count", possible)?;
    Ok((vertices, edges))
}

fn add_graph_edges(total: &mut usize, count: usize, limit: usize) -> Result<(), TrajectoryError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| TrajectoryError::new("total edge count overflows usize"))?;
    if *total > limit {
        return Err(TrajectoryError::new(format!(
            "{} edges exceed the decoder limit {limit}",
            *total
        )));
    }
    Ok(())
}

fn check_graph_bytes(reader: &Reader<'_>, edges: usize) -> Result<(), TrajectoryError> {
    let bytes = edges
        .checked_mul(24)
        .ok_or_else(|| TrajectoryError::new("graph edge bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(TrajectoryError::new(
            "graph edges exceed the remaining bytes",
        ));
    }
    Ok(())
}

fn decode_graph_triplets(
    reader: &mut Reader<'_>,
    vertices: usize,
    edges: usize,
) -> Result<Vec<(usize, usize, f64)>, TrajectoryError> {
    let mut triplets = Vec::with_capacity(edges);
    let mut previous = None;
    for _ in 0..edges {
        let triplet = decode_graph_triplet(reader)?;
        check_graph_triplet(triplet, vertices, previous)?;
        previous = Some((triplet.0, triplet.1));
        triplets.push(triplet);
    }
    Ok(triplets)
}

fn decode_graph_triplet(reader: &mut Reader<'_>) -> Result<(usize, usize, f64), TrajectoryError> {
    Ok((
        reader.usize()?,
        reader.usize()?,
        f64::from_bits(reader.u64()?),
    ))
}

fn check_graph_triplet(
    triplet: (usize, usize, f64),
    vertices: usize,
    previous: Option<(usize, usize)>,
) -> Result<(), TrajectoryError> {
    let (u, v, value) = triplet;
    if u >= v || v >= vertices || previous.is_some_and(|edge| edge >= (u, v)) {
        return Err(TrajectoryError::new(
            "graph edges are not in strict canonical order",
        ));
    }
    if !value.is_finite() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
        return Err(TrajectoryError::new(
            "graph edge weight is not canonical and non-negative",
        ));
    }
    Ok(())
}

fn encode_event(
    out: &mut Vec<u8>,
    event: &TopologyEvent,
) -> std::result::Result<(), TrajectoryError> {
    out.push(event_kind_tag(event.kind));
    put_optional_edge(out, event.first)?;
    put_optional_edge(out, event.second)?;
    put_optional_f64(out, event.old_first);
    put_optional_f64(out, event.new_first);
    put_optional_f64(out, event.old_second);
    put_optional_f64(out, event.new_second);
    Ok(())
}

fn decode_event(reader: &mut Reader<'_>) -> std::result::Result<TopologyEvent, TrajectoryError> {
    Ok(TopologyEvent {
        kind: decode_event_kind(reader.u8()?)?,
        first: reader.optional_edge()?,
        second: reader.optional_edge()?,
        old_first: reader.optional_f64()?,
        new_first: reader.optional_f64()?,
        old_second: reader.optional_f64()?,
        new_second: reader.optional_f64()?,
    })
}

fn mode_tag(mode: UpdateMode) -> u8 {
    match mode {
        UpdateMode::Reused => 0,
        UpdateMode::Recomputed => 1,
    }
}

fn decode_mode(tag: u8) -> std::result::Result<UpdateMode, TrajectoryError> {
    match tag {
        0 => Ok(UpdateMode::Reused),
        1 => Ok(UpdateMode::Recomputed),
        _ => Err(TrajectoryError::new(format!(
            "unknown update-mode tag {tag}"
        ))),
    }
}

fn event_kind_tag(kind: TopologyEventKind) -> u8 {
    match kind {
        TopologyEventKind::VertexSetChanged => 0,
        TopologyEventKind::EdgeSetChanged => 1,
        TopologyEventKind::ThresholdCrossing => 2,
        TopologyEventKind::EqualitySplit => 3,
        TopologyEventKind::EqualityMerge => 4,
        TopologyEventKind::OrderSwap => 5,
    }
}

fn decode_event_kind(tag: u8) -> std::result::Result<TopologyEventKind, TrajectoryError> {
    match tag {
        0 => Ok(TopologyEventKind::VertexSetChanged),
        1 => Ok(TopologyEventKind::EdgeSetChanged),
        2 => Ok(TopologyEventKind::ThresholdCrossing),
        3 => Ok(TopologyEventKind::EqualitySplit),
        4 => Ok(TopologyEventKind::EqualityMerge),
        5 => Ok(TopologyEventKind::OrderSwap),
        _ => Err(TrajectoryError::new(format!(
            "unknown topology-event tag {tag}"
        ))),
    }
}

fn put_optional_edge(
    out: &mut Vec<u8>,
    edge: Option<EdgeKey>,
) -> std::result::Result<(), TrajectoryError> {
    match edge {
        None => out.push(0),
        Some(edge) => {
            out.push(1);
            put_usize(out, edge.u, "event edge endpoint")?;
            put_usize(out, edge.v, "event edge endpoint")?;
        }
    }
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

fn events_bits_equal(a: &[TopologyEvent], b: &[TopologyEvent]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.kind == b.kind
                && a.first == b.first
                && a.second == b.second
                && optional_f64_bits_equal(a.old_first, b.old_first)
                && optional_f64_bits_equal(a.new_first, b.new_first)
                && optional_f64_bits_equal(a.old_second, b.old_second)
                && optional_f64_bits_equal(a.new_second, b.new_second)
        })
}

fn optional_f64_bits_equal(a: Option<f64>, b: Option<f64>) -> bool {
    a.map(f64::to_bits) == b.map(f64::to_bits)
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
) -> std::result::Result<(), TrajectoryError> {
    let value = u64::try_from(value)
        .map_err(|_| TrajectoryError::new(format!("{label} does not fit the wire format")))?;
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

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], TrajectoryError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| TrajectoryError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(TrajectoryError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, TrajectoryError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, TrajectoryError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, TrajectoryError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, TrajectoryError> {
        usize::try_from(self.u64()?)
            .map_err(|_| TrajectoryError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, TrajectoryError> {
        let value = self.usize()?;
        if value > limit {
            return Err(TrajectoryError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_edge(&mut self) -> std::result::Result<Option<EdgeKey>, TrajectoryError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let u = self.usize()?;
                let v = self.usize()?;
                if u >= v {
                    return Err(TrajectoryError::new(
                        "event edge is not in canonical endpoint order",
                    ));
                }
                Ok(Some(EdgeKey { u, v }))
            }
            tag => Err(TrajectoryError::new(format!(
                "unknown optional-edge tag {tag}"
            ))),
        }
    }

    fn optional_f64(&mut self) -> std::result::Result<Option<f64>, TrajectoryError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let value = f64::from_bits(self.u64()?);
                if !value.is_finite() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
                    return Err(TrajectoryError::new(
                        "event scalar is not a canonical number",
                    ));
                }
                Ok(Some(value))
            }
            tag => Err(TrajectoryError::new(format!(
                "unknown optional-float tag {tag}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn graph(weights: [f64; 6]) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, weights[0]),
                (0, 2, weights[1]),
                (0, 3, weights[2]),
                (1, 2, weights[3]),
                (1, 3, weights[4]),
                (2, 3, weights[5]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn trajectory_round_trips_reuse_and_region_change() {
        let initial = graph([1.0, 2.0, 1.1, 1.2, 2.1, 1.3]);
        let reuse = graph([1.01, 2.01, 1.11, 1.21, 2.11, 1.31]);
        let change = graph([2.2, 2.0, 1.1, 1.2, 2.1, 1.3]);
        let artifact = TrajectoryArtifact::build(
            &initial,
            &[reuse, change],
            &RipsParams::new(1).with_modulus(3),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.steps[0].mode, UpdateMode::Reused);
        assert_eq!(artifact.steps[1].mode, UpdateMode::Recomputed);
        let bytes = artifact.encode().unwrap();
        let decoded = TrajectoryArtifact::decode(
            &bytes,
            TrajectoryDecodeLimits::default(),
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        let verified = decoded.verify(CertificateLimits::default()).unwrap();
        assert_eq!(verified.steps[0].mode, UpdateMode::Reused);
        assert_eq!(verified.steps[1].mode, UpdateMode::Recomputed);
    }

    #[test]
    fn changed_event_and_truncation_are_rejected() {
        let initial = graph([1.0, 2.0, 1.1, 1.2, 2.1, 1.3]);
        let change = graph([2.2, 2.0, 1.1, 1.2, 2.1, 1.3]);
        let mut artifact = TrajectoryArtifact::build(
            &initial,
            &[change],
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        artifact.steps[0].events[0].old_first = Some(7.0);
        assert!(artifact.verify(CertificateLimits::default()).is_err());

        let bytes = TrajectoryArtifact::build(
            &initial,
            &[],
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap()
        .encode()
        .unwrap();
        for end in 0..bytes.len() {
            assert!(
                TrajectoryArtifact::decode(
                    &bytes[..end],
                    TrajectoryDecodeLimits::default(),
                    AtlasDecodeLimits::default(),
                    CertificateLimits::default(),
                )
                .is_err()
            );
        }
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = TrajectoryArtifact::decode(
                &bytes,
                TrajectoryDecodeLimits::default(),
                AtlasDecodeLimits::default(),
                CertificateLimits::default(),
            );
        }
    }
}
