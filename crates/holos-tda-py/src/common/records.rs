//! Shared Python record conversions.

use std::fmt::Write as _;

use pyo3::prelude::PyResult;

use super::config::display_err;
use super::core::*;
use super::types::*;

/// pyo3 converts essential death `f64::INFINITY` to `math.inf`.
pub(crate) fn to_bars(mut diagram: holos_tda::Diagram) -> Bars {
    diagram.canonicalize();
    diagram
        .bars
        .into_iter()
        .map(|b| (b.dim, b.birth, b.death))
        .collect()
}

pub(crate) fn to_explained(explained: ExplainedDiagram) -> holos_tda::Result<Explained> {
    let bars = to_bars(explained.diagram);
    let classes = explained
        .spaces
        .into_iter()
        .flat_map(|space| space.basis)
        .map(|class| {
            let provenance = class.provenance.as_ref().ok_or_else(|| {
                holos_tda::Error::InvalidInput(
                    "scalar persistence class has no interval-bound provenance".into(),
                )
            })?;
            let death = (!class.interval.is_essential()).then_some(class.interval.death);
            let terms = class
                .cocycle
                .terms
                .into_iter()
                .map(|term| (term.u, term.v, term.coefficient))
                .collect();
            Ok((
                class.group_id.to_string(),
                class.id.to_string(),
                class.basis_index,
                class.interval.birth,
                death,
                class.cocycle.modulus,
                class.cocycle.scale,
                terms,
                hex_digest(provenance.source_graph_digest()),
                hex_digest(provenance.class_digest()),
            ))
        })
        .collect::<holos_tda::Result<Vec<_>>>()?;
    Ok((bars, classes))
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut result = String::with_capacity(64);
    for byte in digest {
        write!(&mut result, "{byte:02x}").expect("writing to a String cannot fail");
    }
    result
}

pub(crate) fn class_record(class: holos_tda::PersistentClass) -> ClassRecord {
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

pub(crate) fn gradient_record(gradient: EndpointGradient) -> GradientRecord {
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

pub(crate) fn to_atlas_result(evaluation: AtlasEvaluation) -> AtlasResult {
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

pub(crate) fn to_program_result(explained: ExplainedDiagram) -> ProgramResult {
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

pub(crate) fn work_record(work: ProgramWork) -> WorkRecord {
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

pub(crate) fn program_event_kind(kind: ProgramEventKind) -> &'static str {
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

pub(crate) fn program_event_record(event: ProgramEvent) -> ProgramEventRecord {
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

pub(crate) fn continuation_kind(kind: holos_tda::ContinuationKind) -> &'static str {
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

pub(crate) fn continuation_record(value: holos_tda::ClassContinuation) -> ContinuationRecord {
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

pub(crate) fn correspondence_record(value: holos_tda::ClassCorrespondence) -> CorrespondenceRecord {
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

pub(crate) fn program_mode(mode: ProgramUpdateMode) -> &'static str {
    match mode {
        ProgramUpdateMode::Reused => "reused",
        ProgramUpdateMode::Repaired => "repaired",
        ProgramUpdateMode::Recompiled => "recompiled",
    }
}

pub(crate) fn program_update_record(update: ProgramUpdate) -> ProgramUpdateRecord {
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

pub(crate) fn digest_string(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

pub(crate) fn bars_from(bars: Vec<holos_tda::Bar>) -> Bars {
    bars.into_iter()
        .map(|bar| (bar.dim, bar.birth, bar.death))
        .collect()
}

pub(crate) fn index_mode(mode: IndexUpdateMode) -> &'static str {
    match mode {
        IndexUpdateMode::Unchanged => "unchanged",
        IndexUpdateMode::Repaired => "repaired",
        IndexUpdateMode::Composed => "composed",
        IndexUpdateMode::Relative => "relative",
        IndexUpdateMode::Rebuilt => "rebuilt",
        IndexUpdateMode::Recompiled => "recompiled",
    }
}

pub(crate) fn index_event_kind(kind: IndexEventKind) -> &'static str {
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

pub(crate) fn index_event_record(event: IndexEvent) -> IndexEventRecord {
    (
        index_event_kind(event.kind).into(),
        event.node.map(|digest| digest_string(&digest)),
        event.edge.map(|edge| (edge.u, edge.v)),
    )
}

pub(crate) fn index_work_record(work: IndexWork) -> IndexWorkRecord {
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

pub(crate) fn index_update_record(
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

pub(crate) fn program_decode_limits() -> ProgramDecodeLimits {
    ProgramDecodeLimits::default()
}

pub(crate) fn program_trace_decode_limits() -> ProgramTraceDecodeLimits {
    ProgramTraceDecodeLimits::default()
}

pub(crate) fn event_kind(kind: TopologyEventKind) -> String {
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

pub(crate) fn event_record(event: TopologyEvent) -> EventRecord {
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

pub(crate) fn point_gradient_record(gradient: PointEndpointGradient) -> PointGradientValue {
    (
        (gradient.edge.u, gradient.edge.v),
        gradient
            .terms
            .into_iter()
            .map(|term| (term.point, term.coordinate, term.value))
            .collect(),
    )
}

pub(crate) fn point_sensitivity_records(
    atlas: &PointPersistenceAtlas,
) -> Vec<PointSensitivityRecord> {
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
