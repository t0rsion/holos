//! Python bindings for holos-tda. The Python-facing API lives in
//! `python/holos_tda/__init__.py`. This module stays a thin shim.

use std::fmt::Write as _;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use holos_tda::collapse::{
    AdaptiveCollapseParams, CollapseObjective, CollapsePortfolioArtifact,
    CollapsePortfolioCandidate, CollapsePortfolioDecodeLimits, CollapsePortfolioLimits,
    CollapsePortfolioObjective, collapse_sparse_portfolio,
};
use holos_tda::{
    AtlasArtifact, AtlasDecodeLimits, AtlasEvaluation, CertificateLimits,
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyLimits, CorrespondenceMode, CoverageAction,
    CoverageFence, CoverageGeometry, CoverageGeometryLimits, CoverageLimits, CoverageSpecification,
    CoverageState, CoverageSynthesisArtifact, CoverageSynthesisLimits, DurableInterfaceStore,
    EndpointGradient, ExplicitReductionCertificate, FilteredSimplex, FilteredSimplicialComplex,
    GeometryBoundCoverageArtifact, GeometryBoundCoverageDecodeLimits, IndexDeltaProof, IndexEdit,
    IndexEvent, IndexEventKind, IndexParams, IndexSnapshotProof, IndexTransition, IndexUpdateMode,
    IndexWork, InterfaceMode, InterfacePolicy, InterventionArtifact, InterventionBudget,
    InterventionDecodeLimits, KineticEdge, KineticEventKind, KineticFiltration, KineticLimits,
    KineticZigzagArtifact, KineticZigzagArtifactLimits, KineticZigzagNodeKind, PersistenceAtlas,
    PersistenceIndex, PersistenceProgram, PlanarCoverageModel, PlanarPoint, PointEndpointGradient,
    PointPersistenceAtlas, ProgramArtifact, ProgramDecodeLimits, ProgramEvent, ProgramEventKind,
    ProgramTraceArtifact, ProgramTraceDecodeLimits, ProgramUpdate, ProgramUpdateMode, ProgramWork,
    ProofArtifact, RelativeInterfaceCertificate, ScalarGrade, TopologyEvent, TopologyEventKind,
    TopologyPatch, UpdateMode, ZigzagDirection, cohomology_relation, cohomology_space,
    evaluate_planar_coverage,
};
use holos_tda::{
    CollapseSchedule, DistanceMatrix, ExplainedDiagram, GraphFactorization, PointCloudGraph,
    PointCloudParams, RipsParams, SparseDistanceMatrix, SynthesisAction, SynthesisArtifact,
    SynthesisLimits, SynthesisState, TopologicalSpecification, rips_persistence,
    rips_persistence_sparse, rips_persistence_with_classes, rips_persistence_with_classes_sparse,
};

type Bars = Vec<(usize, f64, f64)>;
type ClassRecord = (
    String,
    String,
    usize,
    f64,
    Option<f64>,
    u32,
    f64,
    Vec<(usize, usize, u32)>,
);
type Explained = (Bars, Vec<ClassRecord>);
type GradientRecord = (String, Vec<(usize, usize)>);
type CriticalRecord = (Vec<usize>, f64, Option<(Vec<usize>, f64)>);
type SpaceRecord = (
    String,
    String,
    f64,
    Option<f64>,
    Vec<ClassRecord>,
    Vec<CriticalRecord>,
);
type SensitivityRecord = (String, GradientRecord, GradientRecord);
type AtlasResult = (Bars, Vec<SpaceRecord>, Vec<SensitivityRecord>);
type EventRecord = (
    String,
    Option<(usize, usize)>,
    Option<(usize, usize)>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);
type PointGradientValue = ((usize, usize), Vec<(usize, usize, f64)>);
type PointGradientRecord = Option<PointGradientValue>;
type PointSensitivityRecord = (String, PointGradientRecord, PointGradientRecord);
type ProgramSpaceRecord = (
    String,
    f64,
    Option<f64>,
    Vec<ClassRecord>,
    Vec<CriticalRecord>,
);
type ProgramResult = (Bars, Vec<ProgramSpaceRecord>);
type ProgramAtomRecord = (usize, Vec<usize>, Vec<(usize, usize)>, Vec<usize>, bool);
type ProgramSummaryRecord = (usize, usize, usize, usize, usize, usize, bool, usize, usize);
type WorkRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
);
type ProgramEventRecord = (
    String,
    Option<usize>,
    Option<(usize, usize)>,
    Option<String>,
);
type TransportRecord = (String, String, u32);
type ContinuationRecord = (String, Vec<String>, Vec<String>, Vec<TransportRecord>);
type CorrespondenceTermRecord = (String, u32);
type CorrespondenceVectorRecord = (Vec<CorrespondenceTermRecord>, Vec<CorrespondenceTermRecord>);
type CorrespondenceRecord = (
    String,
    String,
    f64,
    usize,
    usize,
    usize,
    usize,
    usize,
    Vec<CorrespondenceVectorRecord>,
);
type ProgramUpdateRecord = (
    String,
    Vec<ProgramEventRecord>,
    Vec<ContinuationRecord>,
    Vec<CorrespondenceRecord>,
    WorkRecord,
    ProgramResult,
);
type InterventionRecord = (
    String,
    String,
    f64,
    Option<f64>,
    Vec<((usize, usize), f64, f64)>,
    Option<ProgramResult>,
    Option<Vec<u8>>,
);
type IndexSummaryRecord = (
    (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ),
    (usize, usize, usize, usize, usize),
    (bool, usize, bool),
);
type InterfaceRecord = (
    String,
    usize,
    Vec<usize>,
    Vec<usize>,
    Vec<usize>,
    usize,
    usize,
    String,
    usize,
    Vec<usize>,
    (usize, usize, usize),
);
type IndexWorkRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    (usize, usize, usize, usize, usize),
    usize,
    usize,
    usize,
);
type IndexEventRecord = (String, Option<String>, Option<(usize, usize)>);
type IndexUpdateRecord = (
    String,
    Bars,
    Bars,
    Bars,
    Vec<IndexEventRecord>,
    Vec<CorrespondenceRecord>,
    IndexWorkRecord,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);
type IndexDiffRecord = (bool, usize, Bars, Bars);
type RelativeInterfaceRecord = (Vec<u8>, Bars, (usize, usize, usize));
type DistributedInterfaceRecord = (Vec<u8>, Vec<u8>, String, (usize, usize, usize, usize));
type FixedCohomologyRecord = (String, Vec<usize>, Vec<(String, Vec<(Vec<usize>, u32)>)>);
type CohomologyRelationRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    bool,
    Vec<(Vec<(String, u32)>, Vec<(String, u32)>)>,
);
type AffineEventRecord = (f64, f64, f64, Vec<String>);
type AffineCohomologyRecord = (f64, usize, usize, usize);
type KineticZigzagNodeRecord = (String, f64, usize, usize, String);
type KineticZigzagArrowRecord = (String, usize);
type KineticZigzagIntervalRecord = (String, usize, usize, usize);
type KineticZigzagRecord = (
    Vec<u8>,
    String,
    usize,
    Vec<KineticZigzagNodeRecord>,
    Vec<KineticZigzagArrowRecord>,
    Vec<KineticZigzagIntervalRecord>,
    Vec<usize>,
);
type CohomologyScenarioInput = (Vec<(usize, usize, f64)>, usize);
type CohomologyInterventionRecord = (
    Vec<u8>,
    String,
    Vec<(usize, usize, u64)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    Vec<Vec<usize>>,
    Vec<usize>,
    Vec<usize>,
);
type SynthesisRecord = (
    Vec<u8>,
    String,
    Vec<(usize, usize, u64)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    usize,
    Vec<usize>,
    Vec<usize>,
);
type CoverageCandidateInput = (usize, u64, Option<Vec<usize>>);
type RelativeCoverageRecord = (bool, Vec<(usize, usize, usize, u32)>, usize, usize);
type CoverageRecord = (
    Vec<u8>,
    String,
    Vec<(usize, u64, Vec<usize>)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    usize,
    usize,
    Option<usize>,
    usize,
);
type PortfolioRecord = (Vec<u8>, usize, Vec<(String, Vec<u64>, usize)>);
type ExplicitRecord = (Vec<u8>, Bars, Vec<usize>, Vec<usize>);

// The argument list mirrors the Python keyword signature one-to-one.
#[allow(clippy::too_many_arguments)]
fn params(
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<RipsParams> {
    let mut p = RipsParams::new(max_dim).with_modulus(modulus);
    p.threshold = threshold;
    p.threads = threads.max(1);
    p.factorization = parse_factorization(factorization)?;
    p.collapse_edges = collapse_edges;
    p.collapse_schedule = parse_collapse_schedule(collapse_schedule)?;
    validate_collapse_settings(
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
        p.collapse_schedule,
    )?;
    let objective = parse_collapse_objective(collapse_objective)?;
    let mut adaptive = AdaptiveCollapseParams::new(objective);
    if let Some(limit) = collapse_work_limit {
        adaptive = adaptive.with_work_limit(limit);
    }
    p.adaptive_collapse = adaptive;
    Ok(p)
}

fn parse_factorization(value: &str) -> PyResult<GraphFactorization> {
    match value {
        "auto" => Ok(GraphFactorization::Auto),
        "off" => Ok(GraphFactorization::Off),
        "force" => Ok(GraphFactorization::Force),
        value => Err(PyValueError::new_err(format!(
            "factorization must be auto, off, or force, not {value}"
        ))),
    }
}

fn parse_collapse_schedule(value: &str) -> PyResult<CollapseSchedule> {
    match value {
        "serial" => Ok(CollapseSchedule::Serial),
        "ordered" => Ok(CollapseSchedule::Ordered),
        "rounds" => Ok(CollapseSchedule::Rounds),
        "adaptive" => Ok(CollapseSchedule::Adaptive),
        value => Err(PyValueError::new_err(format!(
            "collapse_schedule must be serial, ordered, rounds, or adaptive, not {value}"
        ))),
    }
}

fn validate_collapse_settings(
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
    schedule: CollapseSchedule,
) -> PyResult<()> {
    if !collapse_edges
        && (collapse_schedule != "serial"
            || collapse_objective != "h2"
            || collapse_work_limit.is_some())
    {
        return Err(PyValueError::new_err(
            "collapse settings require collapse_edges=True",
        ));
    }
    if (collapse_objective != "h2" || collapse_work_limit.is_some())
        && schedule != CollapseSchedule::Adaptive
    {
        return Err(PyValueError::new_err(
            "collapse_objective and collapse_work_limit require collapse_schedule='adaptive'",
        ));
    }
    Ok(())
}

fn parse_collapse_objective(value: &str) -> PyResult<CollapseObjective> {
    match value {
        "h1" => Ok(CollapseObjective::H1),
        "h2" => Ok(CollapseObjective::H2),
        value => Err(PyValueError::new_err(format!(
            "collapse_objective must be h1 or h2, not {value}"
        ))),
    }
}

fn to_err(e: holos_tda::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn display_err(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// Essential bars keep death = f64::INFINITY. pyo3 converts it to math.inf.
fn to_bars(mut diagram: holos_tda::Diagram) -> Bars {
    diagram.canonicalize();
    diagram
        .bars
        .into_iter()
        .map(|b| (b.dim, b.birth, b.death))
        .collect()
}

fn to_explained(explained: ExplainedDiagram) -> Explained {
    let bars = to_bars(explained.diagram);
    let classes = explained
        .spaces
        .into_iter()
        .flat_map(|space| space.basis)
        .map(|class| {
            let death = (!class.interval.is_essential()).then_some(class.interval.death);
            let terms = class
                .cocycle
                .terms
                .into_iter()
                .map(|term| (term.u, term.v, term.coefficient))
                .collect();
            (
                class.group_id.to_string(),
                class.id.to_string(),
                class.basis_index,
                class.interval.birth,
                death,
                class.cocycle.modulus,
                class.cocycle.scale,
                terms,
            )
        })
        .collect();
    (bars, classes)
}

fn class_record(class: holos_tda::PersistentClass) -> ClassRecord {
    let death = (!class.interval.is_essential()).then_some(class.interval.death);
    let terms = class
        .cocycle
        .terms
        .into_iter()
        .map(|term| (term.u, term.v, term.coefficient))
        .collect();
    (
        class.group_id.to_string(),
        class.id.to_string(),
        class.basis_index,
        class.interval.birth,
        death,
        class.cocycle.modulus,
        class.cocycle.scale,
        terms,
    )
}

fn gradient_record(gradient: EndpointGradient) -> GradientRecord {
    match gradient {
        EndpointGradient::Edge(edge) => ("edge".into(), vec![(edge.u, edge.v)]),
        EndpointGradient::Tied(edges) => (
            "tied".into(),
            edges.into_iter().map(|edge| (edge.u, edge.v)).collect(),
        ),
        EndpointGradient::Essential => ("essential".into(), Vec::new()),
        _ => ("unknown".into(), Vec::new()),
    }
}

fn to_atlas_result(evaluation: AtlasEvaluation) -> AtlasResult {
    let bars = to_bars(evaluation.diagram);
    let spaces = evaluation
        .spaces
        .into_iter()
        .map(|evaluated| {
            let space = evaluated.space;
            let death = (!space.interval.is_essential()).then_some(space.interval.death);
            let basis = space.basis.into_iter().map(class_record).collect();
            let critical = space
                .critical_pairs
                .into_iter()
                .map(|pair| {
                    (
                        pair.birth.vertices,
                        pair.birth.value,
                        pair.death.map(|death| (death.vertices, death.value)),
                    )
                })
                .collect();
            (
                evaluated.lineage.to_string(),
                space.id.to_string(),
                space.interval.birth,
                death,
                basis,
                critical,
            )
        })
        .collect();
    let sensitivities = evaluation
        .sensitivities
        .into_iter()
        .map(|sensitivity| {
            (
                sensitivity.lineage.to_string(),
                gradient_record(sensitivity.birth),
                gradient_record(sensitivity.death),
            )
        })
        .collect();
    (bars, spaces, sensitivities)
}

fn to_program_result(explained: ExplainedDiagram) -> ProgramResult {
    let bars = to_bars(explained.diagram);
    let spaces = explained
        .spaces
        .into_iter()
        .map(|space| {
            let death = (!space.interval.is_essential()).then_some(space.interval.death);
            let basis = space.basis.into_iter().map(class_record).collect();
            let critical = space
                .critical_pairs
                .into_iter()
                .map(|pair| {
                    (
                        pair.birth.vertices,
                        pair.birth.value,
                        pair.death.map(|death| (death.vertices, death.value)),
                    )
                })
                .collect();
            (
                space.id.to_string(),
                space.interval.birth,
                death,
                basis,
                critical,
            )
        })
        .collect();
    (bars, spaces)
}

fn work_record(work: ProgramWork) -> WorkRecord {
    (
        work.edges_checked,
        work.h0_edges_scanned,
        work.guards_checked,
        work.atoms_touched,
        work.atoms_reused,
        work.atoms_repaired,
        work.atoms_rebuilt,
        work.reduction_columns_reused,
        work.reduction_columns_reduced,
        work.reduction_column_additions,
    )
}

fn program_event_kind(kind: ProgramEventKind) -> &'static str {
    match kind {
        ProgramEventKind::VertexSetChanged => "vertex_set_changed",
        ProgramEventKind::EdgeSetChanged => "edge_set_changed",
        ProgramEventKind::ThresholdCrossing => "threshold_crossing",
        ProgramEventKind::GuardFailed => "guard_failed",
        ProgramEventKind::AtomRebuilt => "atom_rebuilt",
        ProgramEventKind::ReductionSuffixRepaired => "reduction_suffix_repaired",
        ProgramEventKind::SeparatorContractChanged => "separator_contract_changed",
        _ => "unknown",
    }
}

fn program_event_record(event: ProgramEvent) -> ProgramEventRecord {
    (
        program_event_kind(event.kind).into(),
        event.atom,
        event.edge.map(|edge| (edge.u, edge.v)),
        event.guard.map(|guard| match guard {
            holos_tda::ReductionGuardKind::ChangeOfBasis => "change_of_basis".into(),
            holos_tda::ReductionGuardKind::Pivot => "pivot".into(),
            _ => "unknown".into(),
        }),
    )
}

fn continuation_kind(kind: holos_tda::ContinuationKind) -> &'static str {
    match kind {
        holos_tda::ContinuationKind::Isomorphism => "isomorphism",
        holos_tda::ContinuationKind::Split => "split",
        holos_tda::ContinuationKind::Merge => "merge",
        holos_tda::ContinuationKind::Mixing => "mixing",
        holos_tda::ContinuationKind::Birth => "birth",
        holos_tda::ContinuationKind::Death => "death",
        holos_tda::ContinuationKind::Ambiguous => "ambiguous",
        _ => "unknown",
    }
}

fn continuation_record(value: holos_tda::ClassContinuation) -> ContinuationRecord {
    (
        continuation_kind(value.kind).into(),
        value
            .old_spaces
            .into_iter()
            .map(|space| space.to_string())
            .collect(),
        value
            .new_spaces
            .into_iter()
            .map(|space| space.to_string())
            .collect(),
        value
            .transport
            .into_iter()
            .map(|item| (item.old.to_string(), item.new.to_string(), item.coefficient))
            .collect(),
    )
}

fn correspondence_record(value: holos_tda::ClassCorrespondence) -> CorrespondenceRecord {
    (
        value.old_space.to_string(),
        value.new_space.to_string(),
        value.scale,
        value.old_rank,
        value.new_rank,
        value.old_image_rank,
        value.new_image_rank,
        value.relation_rank,
        value
            .basis
            .into_iter()
            .map(|vector| {
                (
                    vector
                        .old
                        .into_iter()
                        .map(|term| (term.basis.to_string(), term.coefficient))
                        .collect(),
                    vector
                        .new
                        .into_iter()
                        .map(|term| (term.basis.to_string(), term.coefficient))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn program_mode(mode: ProgramUpdateMode) -> &'static str {
    match mode {
        ProgramUpdateMode::Reused => "reused",
        ProgramUpdateMode::Repaired => "repaired",
        ProgramUpdateMode::Recompiled => "recompiled",
    }
}

fn program_update_record(update: ProgramUpdate) -> ProgramUpdateRecord {
    (
        program_mode(update.mode).into(),
        update
            .events
            .into_iter()
            .map(program_event_record)
            .collect(),
        update
            .continuation
            .into_iter()
            .map(continuation_record)
            .collect(),
        update
            .correspondence
            .into_iter()
            .map(correspondence_record)
            .collect(),
        work_record(update.work),
        to_program_result(update.result),
    )
}

fn digest_string(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn bars_from(bars: Vec<holos_tda::Bar>) -> Bars {
    bars.into_iter()
        .map(|bar| (bar.dim, bar.birth, bar.death))
        .collect()
}

fn index_mode(mode: IndexUpdateMode) -> &'static str {
    match mode {
        IndexUpdateMode::Unchanged => "unchanged",
        IndexUpdateMode::Repaired => "repaired",
        IndexUpdateMode::Composed => "composed",
        IndexUpdateMode::Relative => "relative",
        IndexUpdateMode::Rebuilt => "rebuilt",
        IndexUpdateMode::Recompiled => "recompiled",
    }
}

fn index_event_kind(kind: IndexEventKind) -> &'static str {
    match kind {
        IndexEventKind::ThresholdCrossing => "threshold_crossing",
        IndexEventKind::ReductionRepaired => "reduction_repaired",
        IndexEventKind::ReductionRebuilt => "reduction_rebuilt",
        IndexEventKind::InterfaceComposed => "interface_composed",
        IndexEventKind::RelativeCoreRebuilt => "relative_core_rebuilt",
        IndexEventKind::RelativeCoreComposed => "relative_core_composed",
        IndexEventKind::EnvelopeRecompiled => "envelope_recompiled",
        _ => "unknown",
    }
}

fn index_event_record(event: IndexEvent) -> IndexEventRecord {
    (
        index_event_kind(event.kind).into(),
        event.node.map(|digest| digest_string(&digest)),
        event.edge.map(|edge| (edge.u, edge.v)),
    )
}

fn index_work_record(work: IndexWork) -> IndexWorkRecord {
    (
        work.edges_checked,
        work.nodes_touched,
        work.nodes_shared,
        work.nodes_repaired,
        work.nodes_rebuilt,
        work.nodes_composed,
        (
            work.relative_nodes_rebuilt,
            work.relative_nodes_composed,
            work.relative_input_cells,
            work.relative_core_cells,
            work.relative_cancellations,
        ),
        work.reduction_columns_reused,
        work.reduction_columns_reduced,
        work.reduction_column_additions,
    )
}

fn index_update_record(
    old: &PersistenceIndex,
    transition: &IndexTransition,
) -> PyResult<IndexUpdateRecord> {
    let (delta_proof, snapshot_proof) = if transition.mode == IndexUpdateMode::Recompiled {
        let proof = IndexSnapshotProof::from_index(&transition.index).map_err(display_err)?;
        (None, Some(proof.encode().map_err(display_err)?))
    } else {
        let proof = IndexDeltaProof::between(old, &transition.index).map_err(display_err)?;
        (Some(proof.encode().map_err(display_err)?), None)
    };
    Ok((
        index_mode(transition.mode).into(),
        to_bars(transition.index.diagram().clone()),
        bars_from(transition.delta.removed.clone()),
        bars_from(transition.delta.added.clone()),
        transition
            .events
            .clone()
            .into_iter()
            .map(index_event_record)
            .collect(),
        transition
            .correspondence
            .clone()
            .into_iter()
            .map(correspondence_record)
            .collect(),
        index_work_record(transition.work),
        digest_string(&transition.index.version()),
        delta_proof,
        snapshot_proof,
    ))
}

fn program_decode_limits() -> ProgramDecodeLimits {
    ProgramDecodeLimits::default()
}

fn program_trace_decode_limits() -> ProgramTraceDecodeLimits {
    ProgramTraceDecodeLimits::default()
}

fn event_kind(kind: TopologyEventKind) -> String {
    match kind {
        TopologyEventKind::VertexSetChanged => "vertex_set_changed",
        TopologyEventKind::EdgeSetChanged => "edge_set_changed",
        TopologyEventKind::ThresholdCrossing => "threshold_crossing",
        TopologyEventKind::EqualitySplit => "equality_split",
        TopologyEventKind::EqualityMerge => "equality_merge",
        TopologyEventKind::OrderSwap => "order_swap",
        _ => "unknown",
    }
    .into()
}

fn event_record(event: TopologyEvent) -> EventRecord {
    (
        event_kind(event.kind),
        event.first.map(|edge| (edge.u, edge.v)),
        event.second.map(|edge| (edge.u, edge.v)),
        event.old_first,
        event.new_first,
        event.old_second,
        event.new_second,
    )
}

fn point_gradient_record(gradient: PointEndpointGradient) -> PointGradientValue {
    (
        (gradient.edge.u, gradient.edge.v),
        gradient
            .terms
            .into_iter()
            .map(|term| (term.point, term.coordinate, term.value))
            .collect(),
    )
}

fn point_sensitivity_records(atlas: &PointPersistenceAtlas) -> Vec<PointSensitivityRecord> {
    atlas
        .sensitivities()
        .into_iter()
        .map(|sensitivity| {
            (
                sensitivity.lineage.to_string(),
                sensitivity.birth.map(point_gradient_record),
                sensitivity.death.map(point_gradient_record),
            )
        })
        .collect()
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn rips_points(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        match threshold {
            Some(threshold) => {
                let graph = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map_err(to_err)?;
                rips_persistence_sparse(graph.matrix(), &params)
                    .map(to_bars)
                    .map_err(to_err)
            }
            None => {
                let dist = DistanceMatrix::from_points(&points).map_err(to_err)?;
                rips_persistence(&dist, &params)
                    .map(to_bars)
                    .map_err(to_err)
            }
        }
    })
}

/// Reorder a `pdist` layout into the layout the core constructor wants.
///
/// SciPy's `pdist` emits the upper triangle row by row (d01, d02, ..., d12,
/// ...). The core constructor wants the lower triangle (d10, d20, d21, ...).
/// The Python contract is the pdist one.
fn pdist_to_lower(data: Vec<f64>) -> Result<Vec<f64>, holos_tda::Error> {
    let m = data.len();
    let n = ((1.0 + 8.0 * m as f64).sqrt() as usize).div_ceil(2);
    if n * (n - 1) / 2 != m {
        return Err(holos_tda::Error::InvalidInput(format!(
            "condensed length {m} is not n(n-1)/2 for any n"
        )));
    }
    let mut lower = vec![0.0; m];
    let mut pos = 0;
    for i in 0..n {
        for j in i + 1..n {
            lower[j * (j - 1) / 2 + i] = data[pos];
            pos += 1;
        }
    }
    Ok(lower)
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn rips_condensed(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let lower = pdist_to_lower(data).map_err(to_err)?;
        let dist = DistanceMatrix::from_condensed(lower).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence(&dist, &params)
            .map(to_bars)
            .map_err(to_err)
    })
}

// The argument list mirrors the Python keyword signature one-to-one.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
fn rips_sparse(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Bars> {
    py.detach(|| {
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_sparse(&dist, &params)
            .map(to_bars)
            .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (points, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn rips_points_classes(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        match threshold {
            Some(threshold) => {
                let graph = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map_err(to_err)?;
                rips_persistence_with_classes_sparse(graph.matrix(), &params)
                    .map(to_explained)
                    .map_err(to_err)
            }
            None => {
                let dist = DistanceMatrix::from_points(&points).map_err(to_err)?;
                rips_persistence_with_classes(&dist, &params)
                    .map(to_explained)
                    .map_err(to_err)
            }
        }
    })
}

#[pyfunction]
#[pyo3(signature = (data, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn rips_condensed_classes(
    py: Python<'_>,
    data: Vec<f64>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let lower = pdist_to_lower(data).map_err(to_err)?;
        let dist = DistanceMatrix::from_condensed(lower).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_with_classes(&dist, &params)
            .map(to_explained)
            .map_err(to_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn rips_sparse_classes(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<Explained> {
    py.detach(|| {
        let dist = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let params = params(
            max_dim,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        rips_persistence_with_classes_sparse(&dist, &params)
            .map(to_explained)
            .map_err(to_err)
    })
}

/// Compiled sparse graph atlas with portable proof bytes.
#[pyclass(name = "SparseAtlas")]
struct PySparseAtlas {
    input: SparseDistanceMatrix,
    atlas: PersistenceAtlas,
    artifact: AtlasArtifact,
    params: RipsParams,
}

#[pymethods]
impl PySparseAtlas {
    /// Canonical `HOLOSATL` bytes for the current compiled region.
    #[getter]
    fn artifact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Evaluate the graph most recently supplied to this object.
    fn result(&self, py: Python<'_>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&self.input)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Evaluate weights inside the current region without reduction.
    fn evaluate(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<AtlasResult> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            self.atlas
                .evaluate(&input)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Return every event that prevents reuse at new weights.
    fn events(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<Vec<EventRecord>> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            Ok(self
                .atlas
                .events(&input)
                .into_iter()
                .map(event_record)
                .collect())
        })
    }

    /// Reuse this atlas or compile a new proof-carrying region.
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<(String, Vec<EventRecord>, AtlasResult)> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let events = self.atlas.events(&input);
            let mode = if events.is_empty() {
                self.input = input;
                UpdateMode::Reused
            } else {
                let (artifact, atlas) =
                    AtlasArtifact::compile(&input, &self.params, CertificateLimits::default())
                        .map_err(display_err)?;
                self.input = input;
                self.atlas = atlas;
                self.artifact = artifact;
                UpdateMode::Recomputed
            };
            let result = self
                .atlas
                .evaluate(&self.input)
                .map(to_atlas_result)
                .map_err(to_err)?;
            let mode = match mode {
                UpdateMode::Reused => "reused",
                UpdateMode::Recomputed => "recomputed",
            };
            Ok((
                mode.into(),
                events.into_iter().map(event_record).collect(),
                result,
            ))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn compile_sparse_atlas(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<PySparseAtlas> {
    py.detach(|| {
        let params = params(
            1,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let (artifact, atlas) =
            AtlasArtifact::compile(&input, &params, CertificateLimits::default())
                .map_err(display_err)?;
        Ok(PySparseAtlas {
            input,
            atlas,
            artifact,
            params,
        })
    })
}

#[pyfunction]
fn load_sparse_atlas(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    artifact: Vec<u8>,
) -> PyResult<PySparseAtlas> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let artifact = AtlasArtifact::decode(
            &artifact,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let atlas = artifact
            .verify(&input, CertificateLimits::default())
            .map_err(display_err)?;
        let mut params = RipsParams::new(1).with_modulus(artifact.modulus());
        params.threshold = artifact.threshold();
        Ok(PySparseAtlas {
            input,
            atlas,
            artifact,
            params,
        })
    })
}

/// Immutable exact persistence over filtered separator interfaces.
#[pyclass(name = "SparseIndex")]
struct PySparseIndex {
    index: PersistenceIndex,
}

#[pymethods]
impl PySparseIndex {
    /// Canonical `HOLOSIP` bytes for the complete current version.
    #[getter]
    fn snapshot<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let proof = IndexSnapshotProof::from_index(&self.index).map_err(display_err)?;
        let bytes = proof.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Content identifier of the current immutable root.
    #[getter]
    fn version(&self) -> String {
        digest_string(&self.index.version())
    }

    /// Highest homology dimension maintained by this index.
    #[getter]
    fn max_dim(&self) -> usize {
        self.index.params().max_dim
    }

    /// Exact current diagram through the configured homology dimension.
    fn result(&self, py: Python<'_>) -> Bars {
        py.detach(|| to_bars(self.index.diagram().clone()))
    }

    /// Compute canonical H1 class spaces on demand.
    fn explain(&self, py: Python<'_>) -> PyResult<ProgramResult> {
        py.detach(|| self.index.explain().map(to_program_result).map_err(to_err))
    }

    /// Structural size of the compiled interface tree.
    fn summary(&self) -> IndexSummaryRecord {
        let summary = self.index.summary();
        (
            (
                summary.nodes,
                summary.leaves,
                summary.separators,
                summary.component_splits,
                summary.widest_separator,
                summary.largest_interface_vertices,
                summary.largest_interface_edges,
                summary.composed_interfaces,
                summary.materialized_interfaces,
            ),
            (
                summary.relative_interfaces,
                summary.relative_input_cells,
                summary.relative_core_cells,
                summary.largest_relative_core_cells,
                summary.relative_cancellations,
            ),
            (
                summary.root_composed,
                summary.separator_candidates_checked,
                summary.separator_search_complete,
            ),
        )
    }

    /// Composed and materialized interfaces in deterministic preorder.
    fn interfaces(&self) -> Vec<InterfaceRecord> {
        self.index
            .interfaces()
            .into_iter()
            .map(|interface| {
                (
                    digest_string(&interface.digest),
                    interface.depth,
                    interface.vertices,
                    interface.separator,
                    interface.protected_vertices,
                    interface.edges,
                    interface.children,
                    match interface.mode {
                        InterfaceMode::Relative => "relative",
                        InterfaceMode::Materialized => "materialized",
                        InterfaceMode::Disjoint => "disjoint",
                        InterfaceMode::ZeroSimplex => "zero_simplex",
                        InterfaceMode::ZeroCone => "zero_cone",
                    }
                    .into(),
                    interface.reduction_columns,
                    interface.columns_by_dimension,
                    (
                        interface.relative_input_cells,
                        interface.relative_core_cells,
                        interface.relative_cancellations,
                    ),
                )
            })
            .collect()
    }

    /// Create and install an exact next version.
    #[pyo3(signature = (n, triplets, correspondence=true))]
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
        correspondence: bool,
    ) -> PyResult<IndexUpdateRecord> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let old = self.index.clone();
            let transition = old.transition_with(&input, mode).map_err(to_err)?;
            let record = index_update_record(&old, &transition)?;
            self.index = transition.index;
            Ok(record)
        })
    }

    /// Apply an atomic active-topology patch inside the edge envelope.
    #[pyo3(signature = (edits, correspondence=true))]
    fn patch(
        &mut self,
        py: Python<'_>,
        edits: Vec<(String, usize, usize, Option<f64>)>,
        correspondence: bool,
    ) -> PyResult<IndexUpdateRecord> {
        py.detach(|| {
            let edits = edits
                .into_iter()
                .map(|(kind, u, v, value)| match (kind.as_str(), value) {
                    ("set", Some(value)) => Ok(IndexEdit::set_weight(u, v, value)),
                    ("activate", Some(value)) => Ok(IndexEdit::activate(u, v, value)),
                    ("deactivate", None) => Ok(IndexEdit::deactivate(u, v)),
                    ("set" | "activate", None) => {
                        Err(PyValueError::new_err(format!("{kind} requires a value")))
                    }
                    ("deactivate", Some(_)) => {
                        Err(PyValueError::new_err("deactivate does not accept a value"))
                    }
                    _ => Err(PyValueError::new_err(format!(
                        "patch kind must be set, activate, or deactivate, not {kind}"
                    ))),
                })
                .collect::<PyResult<Vec<_>>>()?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let old = self.index.clone();
            let patch = TopologyPatch::new(edits);
            let transition = old.transition_patch_with(&patch, mode).map_err(to_err)?;
            let record = index_update_record(&old, &transition)?;
            self.index = transition.index;
            Ok(record)
        })
    }

    /// Apply an ordered version batch atomically.
    #[pyo3(signature = (n, updates, correspondence=true))]
    fn update_many(
        &mut self,
        py: Python<'_>,
        n: usize,
        updates: Vec<Vec<(usize, usize, f64)>>,
        correspondence: bool,
    ) -> PyResult<Vec<IndexUpdateRecord>> {
        py.detach(|| {
            let updates = updates
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let mut candidate = self.index.clone();
            let mut records = Vec::with_capacity(updates.len());
            for input in &updates {
                let transition = candidate.transition_with(input, mode).map_err(to_err)?;
                records.push(index_update_record(&candidate, &transition)?);
                candidate = transition.index;
            }
            self.index = candidate;
            Ok(records)
        })
    }

    /// Advance independent alternatives without changing this version.
    fn fork(
        &self,
        py: Python<'_>,
        n: usize,
        alternatives: Vec<Vec<(usize, usize, f64)>>,
    ) -> PyResult<Vec<(IndexUpdateRecord, Py<PySparseIndex>)>> {
        let branches = py.detach(|| {
            let alternatives = alternatives
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            self.index.branch(&alternatives).map_err(to_err)
        })?;
        branches
            .into_iter()
            .map(|branch| {
                let record = index_update_record(&self.index, &branch.transition)?;
                let index = branch.transition.index;
                Ok((record, Py::new(py, PySparseIndex { index })?))
            })
            .collect()
    }

    /// Compare roots, sharing, and exact diagrams with another version.
    fn diff(&self, other: &PySparseIndex) -> IndexDiffRecord {
        let diff = self.index.diff(&other.index);
        (
            diff.same_envelope,
            diff.shared_nodes,
            bars_from(diff.diagram.removed),
            bars_from(diff.diagram.added),
        )
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, modulus=2, threads=1, separator_width=4, separator_search_limit=100_000, leaf_vertices=4, interface_policy="relative"))]
#[allow(clippy::too_many_arguments)]
fn compile_sparse_index(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    separator_width: usize,
    separator_search_limit: usize,
    leaf_vertices: usize,
    interface_policy: &str,
) -> PyResult<PySparseIndex> {
    py.detach(|| {
        let params = params(
            max_dim, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let mut index_params = IndexParams::default();
        index_params.max_separator_width = separator_width;
        index_params.separator_search_limit = separator_search_limit;
        index_params.leaf_vertices = leaf_vertices;
        index_params.interface_policy = match interface_policy {
            "relative" => InterfacePolicy::Relative,
            "compose" => InterfacePolicy::Compose,
            "materialize" => InterfacePolicy::Materialize,
            value => {
                return Err(PyValueError::new_err(format!(
                    "interface_policy must be 'relative', 'compose', or 'materialize', got {value:?}"
                )));
            }
        };
        let index =
            PersistenceIndex::compile(&input, &params, index_params, CertificateLimits::default())
                .map_err(to_err)?;
        Ok(PySparseIndex { index })
    })
}

/// Compositional sparse persistence with checked local updates.
#[pyclass(name = "SparseProgram")]
struct PySparseProgram {
    program: PersistenceProgram,
    artifact: ProgramArtifact,
}

#[pymethods]
impl PySparseProgram {
    /// Canonical `HOLOSPRG` bytes for the current checked program.
    #[getter]
    fn artifact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Canonical `HOLOSPF` bytes for the current checked state.
    #[getter]
    fn proof<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let proof = ProofArtifact::from_program(&self.program).map_err(display_err)?;
        let bytes = proof.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Exact diagram and canonical H1 class spaces at the current graph.
    fn result(&self, py: Python<'_>) -> ProgramResult {
        py.detach(|| to_program_result(self.program.result().clone()))
    }

    /// Structural program size and result-sensitive guard count.
    fn summary(&self) -> ProgramSummaryRecord {
        let summary = self.program.summary();
        (
            summary.atoms,
            summary.cyclic_atoms,
            summary.articulation_vertices,
            summary.zero_simplex_separators,
            summary.widest_separator,
            summary.separator_candidates_checked,
            summary.separator_search_complete,
            summary.largest_cyclic_atom_edges,
            summary.guards,
        )
    }

    /// Articulation-separated atoms in stable program order.
    fn atoms(&self) -> Vec<ProgramAtomRecord> {
        self.program
            .atoms()
            .iter()
            .map(|atom| {
                (
                    atom.id,
                    atom.vertices.clone(),
                    atom.edges.iter().map(|edge| (edge.u, edge.v)).collect(),
                    atom.separator_vertices.clone(),
                    atom.cyclic,
                )
            })
            .collect()
    }

    /// Evaluate a graph inside every touched result-sensitive region.
    fn evaluate(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<(Bars, WorkRecord)> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let evaluation = self.program.evaluate_diagram(&input).map_err(to_err)?;
            Ok((to_bars(evaluation.diagram), work_record(evaluation.work)))
        })
    }

    /// Reuse valid atoms, rebuild invalid atoms, or recompile after topology changes.
    #[pyo3(signature = (n, triplets, correspondence=true))]
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
        correspondence: bool,
    ) -> PyResult<ProgramUpdateRecord> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let update = self.program.advance_with(&input, mode).map_err(to_err)?;
            self.artifact = ProgramArtifact::from_program(&self.program).map_err(display_err)?;
            Ok(program_update_record(update))
        })
    }

    /// Apply an ordered update batch atomically.
    #[pyo3(signature = (n, updates, correspondence=true))]
    fn update_many(
        &mut self,
        py: Python<'_>,
        n: usize,
        updates: Vec<Vec<(usize, usize, f64)>>,
        correspondence: bool,
    ) -> PyResult<Vec<ProgramUpdateRecord>> {
        py.detach(|| {
            let updates = updates
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            let mode = if correspondence {
                CorrespondenceMode::Exact
            } else {
                CorrespondenceMode::Omit
            };
            let results = self
                .program
                .advance_batch_with(&updates, mode)
                .map_err(to_err)?;
            self.artifact = ProgramArtifact::from_program(&self.program).map_err(display_err)?;
            Ok(results.into_iter().map(program_update_record).collect())
        })
    }

    /// Advance independent alternatives without changing this program.
    fn fork(
        &self,
        py: Python<'_>,
        n: usize,
        alternatives: Vec<Vec<(usize, usize, f64)>>,
    ) -> PyResult<Vec<(ProgramUpdateRecord, Py<PySparseProgram>)>> {
        let branches = py.detach(|| {
            let alternatives = alternatives
                .into_iter()
                .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
                .collect::<PyResult<Vec<_>>>()?;
            self.program.branch(&alternatives).map_err(to_err)
        })?;
        branches
            .into_iter()
            .map(|branch| {
                let (update, program) = branch.into_parts();
                let update = program_update_record(update);
                let artifact = ProgramArtifact::from_program(&program).map_err(display_err)?;
                Ok((update, Py::new(py, PySparseProgram { program, artifact })?))
            })
            .collect()
    }

    /// Certify one restricted finite H1 lifetime intervention.
    fn intervene(
        &self,
        py: Python<'_>,
        space: usize,
        before: f64,
        budget: usize,
    ) -> PyResult<InterventionRecord> {
        py.detach(|| {
            let target = self.program.result().spaces.get(space).ok_or_else(|| {
                PyValueError::new_err(format!(
                    "H1 class-space index {space} is out of range for {} spaces",
                    self.program.result().spaces.len()
                ))
            })?;
            let result = self
                .program
                .kill_h1_before(target.id, before, InterventionBudget::new(budget))
                .map_err(to_err)?;
            let status = match result.status {
                holos_tda::InterventionStatus::Optimal => "optimal",
                holos_tda::InterventionStatus::BoundedGap => "bounded_gap",
                holos_tda::InterventionStatus::BudgetLimited => "budget_limited",
                _ => "unknown",
            };
            let artifact = result
                .artifact
                .map(|artifact| artifact.encode().map_err(display_err))
                .transpose()?;
            Ok((
                status.into(),
                result.target.to_string(),
                result.lower_bound,
                result.upper_bound,
                result
                    .edits
                    .into_iter()
                    .map(|edit| ((edit.edge.u, edit.edge.v), edit.before, edit.after))
                    .collect(),
                result.result.map(to_program_result),
                artifact,
            ))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, threshold=None, modulus=2, threads=1))]
fn compile_sparse_program(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
) -> PyResult<PySparseProgram> {
    py.detach(|| {
        let params = params(
            1, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let (artifact, program) =
            ProgramArtifact::compile(&input, &params, CertificateLimits::default())
                .map_err(display_err)?;
        Ok(PySparseProgram { program, artifact })
    })
}

#[pyfunction]
fn load_sparse_program(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    artifact: Vec<u8>,
) -> PyResult<PySparseProgram> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let artifact = ProgramArtifact::decode(
            &artifact,
            program_decode_limits(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let program = artifact
            .verify(&input, CertificateLimits::default())
            .map_err(display_err)?;
        Ok(PySparseProgram { program, artifact })
    })
}

#[pyfunction]
#[pyo3(signature = (n, initial, updates, threshold=None, modulus=2, threads=1))]
#[allow(clippy::too_many_arguments)]
fn compile_sparse_program_trace(
    py: Python<'_>,
    n: usize,
    initial: Vec<(usize, usize, f64)>,
    updates: Vec<Vec<(usize, usize, f64)>>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
) -> PyResult<Vec<u8>> {
    py.detach(|| {
        let params = params(
            1, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let initial = SparseDistanceMatrix::from_triplets(n, &initial).map_err(to_err)?;
        let updates = updates
            .into_iter()
            .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
            .collect::<PyResult<Vec<_>>>()?;
        ProgramTraceArtifact::build(&initial, &updates, &params, CertificateLimits::default())
            .map_err(display_err)?
            .encode()
            .map_err(display_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, initial, updates, threshold=None, modulus=2, threads=1))]
#[allow(clippy::too_many_arguments)]
fn compile_sparse_proof(
    py: Python<'_>,
    n: usize,
    initial: Vec<(usize, usize, f64)>,
    updates: Vec<Vec<(usize, usize, f64)>>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
) -> PyResult<Vec<u8>> {
    py.detach(|| {
        let params = params(
            1, threshold, modulus, threads, "off", false, "serial", "h2", None,
        )?;
        let initial = SparseDistanceMatrix::from_triplets(n, &initial).map_err(to_err)?;
        let updates = updates
            .into_iter()
            .map(|triplets| SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err))
            .collect::<PyResult<Vec<_>>>()?;
        ProofArtifact::build(&initial, &updates, &params, CertificateLimits::default())
            .map_err(display_err)?
            .encode()
            .map_err(display_err)
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, protected=Vec::new(), max_dim=1, threshold=None, modulus=2))]
fn compile_relative_interface(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    protected: Vec<usize>,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> PyResult<RelativeInterfaceRecord> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let mut params = RipsParams::new(max_dim).with_modulus(modulus);
        params.threshold = threshold;
        let certificate = RelativeInterfaceCertificate::build(
            &input,
            &params,
            &protected,
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let artifact = certificate
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let work = certificate.work();
        Ok((
            artifact,
            to_bars(certificate.diagram().clone()),
            (work.input_cells, work.cancellations, work.core_cells),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, max_dim=1, threshold=None, threads=4, score="columns", adaptive_objective="h1", adaptive_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn compile_collapse_portfolio(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    max_dim: usize,
    threshold: Option<f64>,
    threads: usize,
    score: &str,
    adaptive_objective: &str,
    adaptive_work_limit: Option<u64>,
) -> PyResult<PortfolioRecord> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let adaptive_objective = parse_collapse_objective(adaptive_objective)?;
        let candidates = [
            CollapsePortfolioCandidate::Serial,
            CollapsePortfolioCandidate::Rounds { threads },
            CollapsePortfolioCandidate::Adaptive {
                objective: adaptive_objective,
                work_limit: adaptive_work_limit,
            },
        ];
        let objective = match score {
            "edges" => CollapsePortfolioObjective::Edges,
            "columns" => CollapsePortfolioObjective::ReductionColumns {
                max_homology_dimension: max_dim,
            },
            value => {
                return Err(PyValueError::new_err(format!(
                    "score must be edges or columns, not {value}"
                )));
            }
        };
        let limits = CollapsePortfolioLimits::default().with_max_homology_dimension(max_dim);
        let portfolio =
            collapse_sparse_portfolio(&input, threshold, &candidates, objective, limits)
                .map_err(to_err)?;
        let artifact =
            CollapsePortfolioArtifact::from_portfolio(&portfolio, limits).map_err(to_err)?;
        let bytes = artifact
            .encode(limits, CollapsePortfolioDecodeLimits::default())
            .map_err(to_err)?;
        let entries = portfolio
            .entries()
            .iter()
            .map(|entry| {
                (
                    collapse_candidate_name(entry.candidate()).to_owned(),
                    entry.score().simplex_counts().to_vec(),
                    entry.result().matrix.num_edges(),
                )
            })
            .collect();
        Ok((bytes, portfolio.selected_index(), entries))
    })
}

fn collapse_candidate_name(candidate: CollapsePortfolioCandidate) -> &'static str {
    match candidate {
        CollapsePortfolioCandidate::Serial => "serial",
        CollapsePortfolioCandidate::Rounds { .. } => "rounds",
        CollapsePortfolioCandidate::Adaptive { .. } => "adaptive",
        _ => "other",
    }
}

#[pyfunction]
#[pyo3(signature = (simplices, max_dim=1, modulus=2))]
fn compile_explicit_persistence(
    py: Python<'_>,
    simplices: Vec<(Vec<usize>, f64)>,
    max_dim: usize,
    modulus: u32,
) -> PyResult<ExplicitRecord> {
    py.detach(|| {
        let complex = explicit_complex(simplices, max_dim)?;
        let certificate = ExplicitReductionCertificate::build(
            &complex,
            max_dim,
            modulus,
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let bytes = certificate
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let simplex_counts = certificate
            .complex()
            .simplices()
            .iter()
            .map(Vec::len)
            .collect();
        let column_counts = certificate.columns().iter().map(Vec::len).collect();
        Ok((
            bytes,
            to_bars(certificate.diagram().clone()),
            simplex_counts,
            column_counts,
        ))
    })
}

fn explicit_complex(
    simplices: Vec<(Vec<usize>, f64)>,
    max_dim: usize,
) -> PyResult<FilteredSimplicialComplex<ScalarGrade>> {
    let mut groups = vec![Vec::new(); max_dim.saturating_add(2)];
    for (vertices, grade) in simplices {
        if vertices.is_empty() {
            return Err(PyValueError::new_err("an explicit simplex cannot be empty"));
        }
        let dimension = vertices.len() - 1;
        if dimension >= groups.len() {
            groups.resize_with(dimension + 1, Vec::new);
        }
        let grade = ScalarGrade::new(grade).map_err(display_err)?;
        groups[dimension].push(FilteredSimplex::new(vertices, grade));
    }
    let mut labels = groups[0]
        .iter()
        .filter_map(|simplex| simplex.vertices().first().copied())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();
    FilteredSimplicialComplex::new(labels, groups).map_err(display_err)
}

#[pyfunction]
#[pyo3(signature = (artifacts, store, separator=Vec::new(), protected=Vec::new()))]
fn merge_relative_interfaces(
    py: Python<'_>,
    artifacts: Vec<Vec<u8>>,
    store: String,
    separator: Vec<usize>,
    protected: Vec<usize>,
) -> PyResult<DistributedInterfaceRecord> {
    py.detach(|| {
        let store = DurableInterfaceStore::open(store).map_err(display_err)?;
        let commit = store
            .commit(
                &artifacts,
                &separator,
                &protected,
                CertificateLimits::default(),
            )
            .map_err(display_err)?;
        let manifest = commit.manifest().encode().map_err(display_err)?;
        let result = commit
            .certificate()
            .encode(CertificateLimits::default())
            .map_err(display_err)?;
        let work = commit.work();
        Ok((
            manifest,
            result,
            commit.manifest().job().to_string(),
            (
                work.shards,
                work.folds_reused,
                work.folds_computed,
                work.peak_artifact_bytes,
            ),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, dimension, scale, modulus=2))]
fn fixed_cohomology(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<FixedCohomologyRecord> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let space = cohomology_space(
            &graph,
            dimension,
            scale,
            modulus,
            CohomologyLimits::default(),
        )
        .map_err(to_err)?;
        Ok((
            space.id().to_string(),
            space.simplex_counts().to_vec(),
            space
                .basis()
                .iter()
                .map(|class| {
                    (
                        class.id.to_string(),
                        class
                            .terms
                            .iter()
                            .map(|term| (term.simplex.clone(), term.coefficient))
                            .collect(),
                    )
                })
                .collect(),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, old_triplets, new_triplets, dimension, scale, modulus=2))]
fn relate_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    old_triplets: Vec<(usize, usize, f64)>,
    new_triplets: Vec<(usize, usize, f64)>,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<CohomologyRelationRecord> {
    py.detach(|| {
        let old_graph = SparseDistanceMatrix::from_triplets(n, &old_triplets).map_err(to_err)?;
        let new_graph = SparseDistanceMatrix::from_triplets(n, &new_triplets).map_err(to_err)?;
        let limits = CohomologyLimits::default();
        let old =
            cohomology_space(&old_graph, dimension, scale, modulus, limits).map_err(to_err)?;
        let new =
            cohomology_space(&new_graph, dimension, scale, modulus, limits).map_err(to_err)?;
        let relation =
            cohomology_relation(&old_graph, &old, &new_graph, &new, limits).map_err(to_err)?;
        let basis = relation
            .basis
            .iter()
            .map(|vector| {
                (
                    vector
                        .old
                        .iter()
                        .map(|term| (term.class.to_string(), term.coefficient))
                        .collect(),
                    vector
                        .new
                        .iter()
                        .map(|term| (term.class.to_string(), term.coefficient))
                        .collect(),
                )
            })
            .collect();
        Ok((
            relation.old_rank,
            relation.new_rank,
            relation.old_image_rank,
            relation.new_image_rank,
            relation.relation_rank,
            relation.is_isomorphism(),
            basis,
        ))
    })
}

fn affine_kind(kind: &KineticEventKind) -> String {
    match kind {
        KineticEventKind::ThresholdCrossing { edge } => {
            format!("threshold:{}:{}", edge.u, edge.v)
        }
        KineticEventKind::EdgeOrderSwap { first, second } => {
            format!("order:{}:{}:{}:{}", first.u, first.v, second.u, second.v)
        }
    }
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, threshold=None, dimension=None, modulus=2))]
#[allow(clippy::too_many_arguments)]
fn affine_events(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    threshold: Option<f64>,
    dimension: Option<usize>,
    modulus: u32,
) -> PyResult<(Vec<AffineEventRecord>, Vec<AffineCohomologyRecord>, usize)> {
    py.detach(|| {
        if dimension.is_some() && threshold.is_none() {
            return Err(PyValueError::new_err(
                "dimension requires a fixed threshold",
            ));
        }
        let trajectory = KineticFiltration::new(
            n,
            edges
                .into_iter()
                .map(|(u, v, intercept, velocity)| KineticEdge {
                    u,
                    v,
                    intercept,
                    velocity,
                })
                .collect(),
            start,
            end,
            KineticLimits::default(),
        )
        .map_err(to_err)?;
        let schedule = trajectory.events(threshold).map_err(to_err)?;
        let events = schedule
            .events
            .into_iter()
            .map(|event| {
                (
                    event.time,
                    event.lower,
                    event.upper,
                    event.kinds.iter().map(affine_kind).collect(),
                )
            })
            .collect();
        let relations = match (dimension, threshold) {
            (Some(dimension), Some(scale)) => trajectory
                .cohomology_events(dimension, scale, modulus, CohomologyLimits::default())
                .map_err(to_err)?
                .into_iter()
                .map(|event| {
                    (
                        event.event.time,
                        event.before_rank,
                        event.after_rank,
                        event.relation.relation_rank,
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok((events, relations, schedule.persistent_ties))
    })
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, dimension, scale, modulus=2))]
#[allow(clippy::too_many_arguments)]
fn kinetic_zigzag(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    dimension: usize,
    scale: f64,
    modulus: u32,
) -> PyResult<KineticZigzagRecord> {
    py.detach(|| {
        let trajectory = KineticFiltration::new(
            n,
            edges
                .into_iter()
                .map(|(u, v, intercept, velocity)| KineticEdge {
                    u,
                    v,
                    intercept,
                    velocity,
                })
                .collect(),
            start,
            end,
            KineticLimits::default(),
        )
        .map_err(to_err)?;
        let limits = KineticZigzagArtifactLimits::default();
        let (artifact, zigzag) =
            KineticZigzagArtifact::build(&trajectory, dimension, scale, modulus, limits)
                .map_err(to_err)?;
        let artifact = artifact.encode(limits).map_err(to_err)?;
        let nodes = zigzag
            .nodes
            .into_iter()
            .map(|node| {
                let (kind, time) = match node.kind {
                    KineticZigzagNodeKind::OpenCell { sample } => ("open", sample),
                    KineticZigzagNodeKind::Event(event) => ("event", event.time),
                };
                (
                    kind.into(),
                    time,
                    node.rank,
                    node.active_edges,
                    node.space.to_string(),
                )
            })
            .collect();
        let arrows = zigzag
            .arrows
            .into_iter()
            .map(|arrow| {
                (
                    match arrow.direction {
                        ZigzagDirection::Forward => "forward",
                        ZigzagDirection::Backward => "backward",
                    }
                    .into(),
                    arrow.restriction.rank,
                )
            })
            .collect();
        let intervals = zigzag
            .barcode
            .intervals
            .into_iter()
            .map(|interval| {
                (
                    interval.id.to_string(),
                    interval.start,
                    interval.end,
                    interval.multiplicity,
                )
            })
            .collect();
        Ok((
            artifact,
            zigzag.barcode.module.to_string(),
            zigzag.persistent_ties,
            nodes,
            arrows,
            intervals,
            zigzag.barcode.generalized_ranks,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, scenarios, candidates, dimension, scale, max_edits, modulus=2, oracle_limit=1_000_000, node_limit=1_000_000))]
#[allow(clippy::too_many_arguments)]
fn intervene_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    scenarios: Vec<CohomologyScenarioInput>,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CohomologyInterventionRecord> {
    py.detach(|| {
        let scenarios = scenarios
            .into_iter()
            .map(|(triplets, target)| {
                let graph = SparseDistanceMatrix::from_triplets(n, &triplets)?;
                CohomologyInterventionScenario::from_graph(&graph, scale, target)
            })
            .collect::<holos_tda::Result<Vec<_>>>()
            .map_err(to_err)?;
        let mut candidates = candidates
            .into_iter()
            .map(|(u, v, cost)| CohomologyInterventionCandidate::new(u, v, cost))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| candidate.edge);
        let before = candidates.len();
        candidates.dedup_by_key(|candidate| candidate.edge);
        if candidates.len() != before {
            return Err(PyValueError::new_err("candidates repeat an edge"));
        }
        let limits = CohomologyInterventionLimits::default()
            .with_max_oracle_calls(oracle_limit)
            .with_max_search_nodes(node_limit);
        let artifact = CohomologyInterventionArtifact::build(
            n,
            dimension,
            scale,
            modulus,
            &scenarios,
            &candidates,
            max_edits,
            limits,
        )
        .map_err(to_err)?;
        let bytes = artifact.encode(limits).map_err(to_err)?;
        Ok((
            bytes,
            artifact.status().to_string(),
            artifact
                .edits()
                .iter()
                .map(|candidate| (candidate.edge.u, candidate.edge.v, candidate.cost))
                .collect(),
            artifact.lower_bound_cost(),
            artifact.upper_bound_cost(),
            artifact.oracle_calls(),
            artifact.search_nodes(),
            artifact.cache_hits(),
            artifact.root_blockers().to_vec(),
            artifact.before_ranks().to_vec(),
            artifact.after_ranks().to_vec(),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, triplets, active, fence, broadcast_radius, sensing_radius, modulus=2))]
#[allow(clippy::too_many_arguments)]
fn check_relative_coverage(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    active: Vec<usize>,
    fence: Vec<usize>,
    broadcast_radius: f64,
    sensing_radius: f64,
    modulus: u32,
) -> PyResult<RelativeCoverageRecord> {
    py.detach(|| {
        let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let fence = CoverageFence::new(fence).map_err(to_err)?;
        let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
        let evaluation = evaluate_planar_coverage(
            &graph,
            &active,
            &fence,
            modulus,
            model,
            CoverageLimits::default(),
        )
        .map_err(to_err)?;
        Ok((
            evaluation.criterion_holds,
            evaluation
                .witness
                .into_iter()
                .map(|term| (term.a, term.b, term.c, term.coefficient))
                .collect(),
            evaluation.active_edges,
            evaluation.active_triangles,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (n, states, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
fn synthesize_finite_coverage(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let specification = finite_coverage_specification(
            n,
            states,
            fence,
            base,
            failable,
            broadcast_radius,
            sensing_radius,
            failure_budget,
            modulus,
        )?;
        let actions = coverage_actions(candidates, &specification)?;
        build_coverage_record(
            specification,
            actions,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

#[pyfunction]
#[pyo3(signature = (n, states, coordinates, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
fn synthesize_geometric_coverage(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    coordinates: Vec<Vec<(f64, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let specification = finite_coverage_specification(
            n,
            states,
            fence,
            base,
            failable,
            broadcast_radius,
            sensing_radius,
            failure_budget,
            modulus,
        )?;
        let actions = coverage_actions(candidates, &specification)?;
        let geometry = CoverageGeometry::new(
            &specification,
            coordinates
                .into_iter()
                .map(|state| {
                    state
                        .into_iter()
                        .map(|(x, y)| PlanarPoint::new(x, y))
                        .collect::<holos_tda::Result<Vec<_>>>()
                })
                .collect::<holos_tda::Result<Vec<_>>>()
                .map_err(to_err)?,
            CoverageGeometryLimits::default(),
        )
        .map_err(to_err)?;
        build_geometric_coverage_record(
            specification,
            actions,
            geometry,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn finite_coverage_specification(
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    modulus: u32,
) -> PyResult<CoverageSpecification> {
    let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
    let fence = CoverageFence::new(fence).map_err(to_err)?;
    let base = coverage_base(fence.vertices(), &base);
    let states = states
        .into_iter()
        .enumerate()
        .map(|(step, triplets)| {
            let graph = SparseDistanceMatrix::from_triplets(n, &triplets)?;
            CoverageState::new(0, step as u64, &graph, base.clone(), broadcast_radius)
        })
        .collect::<holos_tda::Result<Vec<_>>>()
        .map_err(to_err)?;
    CoverageSpecification::new(
        n,
        model,
        modulus,
        fence,
        failable,
        failure_budget,
        states,
        CoverageLimits::default(),
    )
    .map_err(to_err)
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, fence, base, failable, candidates, broadcast_radius, sensing_radius, failure_budget, max_activations, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
fn synthesize_affine_coverage(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    fence: Vec<usize>,
    base: Vec<usize>,
    failable: Vec<usize>,
    candidates: Vec<CoverageCandidateInput>,
    broadcast_radius: f64,
    sensing_radius: f64,
    failure_budget: usize,
    max_activations: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    py.detach(|| {
        let trajectory = KineticFiltration::new(
            n,
            edges
                .into_iter()
                .map(|(u, v, intercept, velocity)| KineticEdge {
                    u,
                    v,
                    intercept,
                    velocity,
                })
                .collect(),
            start,
            end,
            KineticLimits::default(),
        )
        .map_err(to_err)?;
        let model = PlanarCoverageModel::new(broadcast_radius, sensing_radius).map_err(to_err)?;
        let fence = CoverageFence::new(fence).map_err(to_err)?;
        let base = coverage_base(fence.vertices(), &base);
        let specification = CoverageSpecification::from_kinetic(
            &trajectory,
            0,
            model,
            modulus,
            fence,
            failable,
            failure_budget,
            base,
            CoverageLimits::default(),
        )
        .map_err(to_err)?;
        let actions = coverage_actions(candidates, &specification)?;
        build_coverage_record(
            specification,
            actions,
            max_activations,
            oracle_limit,
            node_limit,
        )
    })
}

fn coverage_base(fence: &[usize], additional: &[usize]) -> Vec<usize> {
    let mut base = fence.iter().chain(additional).copied().collect::<Vec<_>>();
    base.sort_unstable();
    base.dedup();
    base
}

fn coverage_actions(
    candidates: Vec<CoverageCandidateInput>,
    specification: &CoverageSpecification,
) -> PyResult<Vec<CoverageAction>> {
    let mut actions = candidates
        .into_iter()
        .map(|(vertex, cost, states)| {
            CoverageAction::new(
                vertex,
                cost,
                states.unwrap_or_else(|| (0..specification.states().len()).collect()),
            )
        })
        .collect::<Vec<_>>();
    actions.sort_by_key(|action| action.vertex);
    if actions
        .windows(2)
        .any(|pair| pair[0].vertex == pair[1].vertex)
    {
        return Err(PyValueError::new_err(
            "coverage candidates repeat a sensor vertex",
        ));
    }
    Ok(actions)
}

fn build_coverage_record(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    let limits = CoverageSynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)
            .map_err(to_err)?;
    let bytes = artifact.encode(limits).map_err(to_err)?;
    Ok(coverage_record(&artifact, bytes))
}

fn build_geometric_coverage_record(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    geometry: CoverageGeometry,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<CoverageRecord> {
    let limits = CoverageSynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)
            .map_err(to_err)?;
    let mut record = coverage_record(&artifact, Vec::new());
    let geometry_limits = CoverageGeometryLimits::default();
    record.0 = GeometryBoundCoverageArtifact::build(artifact, geometry, limits, geometry_limits)
        .map_err(to_err)?
        .encode(
            limits,
            geometry_limits,
            GeometryBoundCoverageDecodeLimits::default(),
        )
        .map_err(to_err)?;
    Ok(record)
}

fn coverage_record(artifact: &CoverageSynthesisArtifact, bytes: Vec<u8>) -> CoverageRecord {
    (
        bytes,
        artifact.status().to_string(),
        artifact
            .selected()
            .iter()
            .map(|index| {
                let action = &artifact.actions()[*index];
                (action.vertex, action.cost, action.states().to_vec())
            })
            .collect(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.producer_search_nodes(),
        artifact.proof_nodes(),
        artifact.proof_topology_checks(),
        artifact.selected_failure_checks(),
        artifact.minimum_witness_triangles(),
        artifact.specification().states().len(),
    )
}

#[pyfunction]
#[pyo3(signature = (n, states, candidates, dimension, scale, max_rank, max_edits, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
fn synthesize_fixed_cohomology(
    py: Python<'_>,
    n: usize,
    states: Vec<Vec<(usize, usize, f64)>>,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_rank: usize,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
    py.detach(|| {
        let mut obligations = Vec::new();
        for (step, triplets) in states.into_iter().enumerate() {
            let graph = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let space = cohomology_space(
                &graph,
                dimension,
                scale,
                modulus,
                CohomologyLimits::default(),
            )
            .map_err(to_err)?;
            if space.rank() <= max_rank {
                continue;
            }
            let target = space.full_subspace();
            obligations.push(
                SynthesisState::from_subspace(
                    0,
                    step as u64,
                    &graph,
                    scale,
                    &space,
                    &target,
                    max_rank,
                )
                .map_err(to_err)?,
            );
        }
        let specification =
            TopologicalSpecification::new(n, dimension, scale, modulus, obligations);
        let actions = synthesis_actions(candidates, &specification)?;
        build_synthesis_record(specification, actions, max_edits, oracle_limit, node_limit)
    })
}

#[pyfunction]
#[pyo3(signature = (n, edges, start, end, candidates, dimension, scale, max_rank, max_edits, modulus=2, oracle_limit=2_000_000, node_limit=2_000_000))]
#[allow(clippy::too_many_arguments)]
fn synthesize_affine_cohomology(
    py: Python<'_>,
    n: usize,
    edges: Vec<(usize, usize, f64, f64)>,
    start: f64,
    end: f64,
    candidates: Vec<(usize, usize, u64)>,
    dimension: usize,
    scale: f64,
    max_rank: usize,
    max_edits: usize,
    modulus: u32,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
    py.detach(|| {
        let trajectory = KineticFiltration::new(
            n,
            edges
                .into_iter()
                .map(|(u, v, intercept, velocity)| KineticEdge {
                    u,
                    v,
                    intercept,
                    velocity,
                })
                .collect(),
            start,
            end,
            KineticLimits::default(),
        )
        .map_err(to_err)?;
        let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
            &trajectory,
            0,
            dimension,
            scale,
            modulus,
            max_rank,
            CohomologyLimits::default(),
        )
        .map_err(to_err)?;
        let actions = synthesis_actions(candidates, &specification)?;
        build_synthesis_record(specification, actions, max_edits, oracle_limit, node_limit)
    })
}

fn synthesis_actions(
    candidates: Vec<(usize, usize, u64)>,
    specification: &TopologicalSpecification,
) -> PyResult<Vec<SynthesisAction>> {
    if specification.states().is_empty() {
        return Ok(Vec::new());
    }
    let mut actions = candidates
        .into_iter()
        .map(|(u, v, cost)| SynthesisAction::throughout(u, v, cost, specification))
        .collect::<Vec<_>>();
    actions.sort();
    let before = actions.len();
    actions.dedup_by_key(|action| action.edge);
    if actions.len() != before {
        return Err(PyValueError::new_err("candidates repeat an edge"));
    }
    Ok(actions)
}

fn build_synthesis_record(
    specification: TopologicalSpecification,
    actions: Vec<SynthesisAction>,
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
) -> PyResult<SynthesisRecord> {
    let limits = SynthesisLimits::default()
        .with_max_oracle_calls(oracle_limit)
        .with_max_search_nodes(node_limit);
    let artifact =
        SynthesisArtifact::build(specification, actions, max_edits, limits).map_err(to_err)?;
    let bytes = artifact.encode(limits).map_err(to_err)?;
    Ok((
        bytes,
        artifact.status().to_string(),
        artifact
            .selected()
            .iter()
            .map(|position| {
                let action = &artifact.actions()[*position];
                (action.edge.u, action.edge.v, action.cost)
            })
            .collect(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.producer_search_nodes(),
        artifact.proof_nodes(),
        artifact.proof_topology_checks(),
        artifact.before_ranks().to_vec(),
        artifact.after_ranks().to_vec(),
    ))
}

#[pyfunction]
fn verify_program_trace(
    py: Python<'_>,
    artifact: Vec<u8>,
) -> PyResult<(usize, usize, usize, usize)> {
    py.detach(|| {
        let artifact = ProgramTraceArtifact::decode(
            &artifact,
            program_trace_decode_limits(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let verified = artifact
            .verify(CertificateLimits::default())
            .map_err(display_err)?;
        let reused = verified
            .steps
            .iter()
            .filter(|step| step.mode == ProgramUpdateMode::Reused)
            .count();
        let repaired = verified
            .steps
            .iter()
            .filter(|step| step.mode == ProgramUpdateMode::Repaired)
            .count();
        let recompiled = verified.steps.len() - reused - repaired;
        Ok((verified.steps.len(), reused, repaired, recompiled))
    })
}

#[pyfunction]
fn verify_intervention(py: Python<'_>, artifact: Vec<u8>) -> PyResult<InterventionRecord> {
    py.detach(|| {
        let artifact = InterventionArtifact::decode(
            &artifact,
            InterventionDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let verified = artifact
            .verify(CertificateLimits::default())
            .map_err(display_err)?;
        let status = match verified.status {
            holos_tda::InterventionStatus::Optimal => "optimal",
            holos_tda::InterventionStatus::BoundedGap => "bounded_gap",
            holos_tda::InterventionStatus::BudgetLimited => "budget_limited",
            _ => "unknown",
        };
        Ok((
            status.into(),
            verified.target.to_string(),
            verified.lower_bound,
            Some(verified.upper_bound),
            verified
                .edits
                .into_iter()
                .map(|edit| ((edit.edge.u, edit.edge.v), edit.before, edit.after))
                .collect(),
            Some(to_program_result(verified.result)),
            None,
        ))
    })
}

/// Compiled Euclidean point atlas with a checked displacement radius.
#[pyclass(name = "PointAtlas")]
struct PyPointAtlas {
    points: Vec<Vec<f64>>,
    atlas: PointPersistenceAtlas,
}

#[pymethods]
impl PyPointAtlas {
    /// Conservative per-point Euclidean displacement radius.
    #[getter]
    fn coordinate_radius(&self) -> f64 {
        self.atlas.coordinate_radius()
    }

    /// Evaluate the point cloud most recently supplied to this object.
    fn result(&self, py: Python<'_>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&self.points)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Analytic endpoint gradients with respect to point coordinates.
    fn sensitivities(&self, py: Python<'_>) -> Vec<PointSensitivityRecord> {
        py.detach(|| point_sensitivity_records(&self.atlas))
    }

    /// Evaluate points inside the checked displacement radius.
    fn evaluate(&self, py: Python<'_>, points: Vec<Vec<f64>>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&points)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Reuse this point atlas or recompile after a radius event.
    fn update(&mut self, py: Python<'_>, points: Vec<Vec<f64>>) -> PyResult<(String, AtlasResult)> {
        py.detach(|| {
            let update = self.atlas.update(&points).map_err(to_err)?;
            let mode = match update.mode {
                UpdateMode::Reused => "reused",
                UpdateMode::Recomputed => "recomputed",
            };
            self.points = points;
            self.atlas = update.atlas;
            Ok((mode.into(), to_atlas_result(update.evaluation)))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (points, threshold, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
fn compile_points_atlas(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    threshold: f64,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<PyPointAtlas> {
    py.detach(|| {
        let params = params(
            1,
            Some(threshold),
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        let atlas = PointPersistenceAtlas::build(&points, &params).map_err(to_err)?;
        Ok(PyPointAtlas { points, atlas })
    })
}

#[pyfunction]
fn run_cli(py: Python<'_>, argv: Vec<String>) -> i32 {
    py.detach(|| holos_tda::cli::run_cli(argv))
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_persistence(m)?;
    register_programs(m)?;
    register_proofs(m)?;
    register_topology(m)?;
    register_synthesis(m)?;
    register_types(m)?;
    register_metadata(m)
}

fn register_persistence(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_persistence_diagrams(m)?;
    register_persistence_classes(m)
}

fn register_persistence_diagrams(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(rips_points, m)?)?;
    m.add_function(wrap_pyfunction!(rips_condensed, m)?)?;
    m.add_function(wrap_pyfunction!(rips_sparse, m)?)?;
    Ok(())
}

fn register_persistence_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(rips_points_classes, m)?)?;
    m.add_function(wrap_pyfunction!(rips_condensed_classes, m)?)?;
    m.add_function(wrap_pyfunction!(rips_sparse_classes, m)?)?;
    Ok(())
}

fn register_programs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_atlas_and_index(m)?;
    register_program_artifacts(m)
}

fn register_atlas_and_index(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compile_sparse_atlas, m)?)?;
    m.add_function(wrap_pyfunction!(load_sparse_atlas, m)?)?;
    m.add_function(wrap_pyfunction!(compile_sparse_index, m)?)?;
    Ok(())
}

fn register_program_artifacts(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compile_sparse_program, m)?)?;
    m.add_function(wrap_pyfunction!(load_sparse_program, m)?)?;
    m.add_function(wrap_pyfunction!(compile_sparse_program_trace, m)?)?;
    Ok(())
}

fn register_proofs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_proof_builders(m)?;
    register_proof_checkers(m)
}

fn register_proof_builders(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_v07_proof_builders(m)?;
    register_interface_proof_builders(m)
}

fn register_v07_proof_builders(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compile_collapse_portfolio, m)?)?;
    m.add_function(wrap_pyfunction!(compile_explicit_persistence, m)?)?;
    m.add_function(wrap_pyfunction!(compile_sparse_proof, m)?)?;
    Ok(())
}

fn register_interface_proof_builders(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compile_relative_interface, m)?)?;
    m.add_function(wrap_pyfunction!(merge_relative_interfaces, m)?)?;
    Ok(())
}

fn register_proof_checkers(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(verify_program_trace, m)?)?;
    m.add_function(wrap_pyfunction!(verify_intervention, m)?)?;
    Ok(())
}

fn register_topology(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_cohomology(m)?;
    register_applications(m)
}

fn register_cohomology(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fixed_cohomology, m)?)?;
    m.add_function(wrap_pyfunction!(relate_fixed_cohomology, m)?)?;
    m.add_function(wrap_pyfunction!(affine_events, m)?)?;
    Ok(())
}

fn register_applications(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(kinetic_zigzag, m)?)?;
    m.add_function(wrap_pyfunction!(intervene_fixed_cohomology, m)?)?;
    m.add_function(wrap_pyfunction!(check_relative_coverage, m)?)?;
    Ok(())
}

fn register_synthesis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_coverage_synthesis(m)?;
    register_cohomology_synthesis(m)
}

fn register_coverage_synthesis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(synthesize_finite_coverage, m)?)?;
    m.add_function(wrap_pyfunction!(synthesize_geometric_coverage, m)?)?;
    m.add_function(wrap_pyfunction!(synthesize_affine_coverage, m)?)?;
    Ok(())
}

fn register_cohomology_synthesis(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(synthesize_fixed_cohomology, m)?)?;
    m.add_function(wrap_pyfunction!(synthesize_affine_cohomology, m)?)?;
    Ok(())
}

fn register_types(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compile_points_atlas, m)?)?;
    m.add_class::<PySparseAtlas>()?;
    m.add_class::<PySparseIndex>()?;
    m.add_class::<PySparseProgram>()?;
    m.add_class::<PyPointAtlas>()?;
    m.add_function(wrap_pyfunction!(run_cli, m)?)?;
    Ok(())
}

fn register_metadata(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", holos_tda::VERSION)?;
    m.add("GIT_HASH", holos_tda::GIT_HASH)?;
    Ok(())
}
