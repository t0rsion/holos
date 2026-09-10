//! Fixed-scale cohomology, circular-coordinate, and kinetic workflows.

use std::fmt::Write as _;
use std::path::Path;

use crate::{
    CircularCoordinate, CircularCoordinateArtifact, CircularCoordinateParams, CohomologyLimits,
    KineticEventKind, KineticFiltration, KineticLimits, KineticZigzagArtifact,
    KineticZigzagArtifactLimits, PersistentClass, SparseDistanceMatrix, circular_coordinate,
    circular_coordinate_for_class, cocycle_from_ripser_terms, cohomology_relation,
    cohomology_space, continue_circular_coordinate,
};

use super::args::{CircularCli, CohomologyCli, KineticCli};
use super::class_record::read_persistent_class;
use super::input::{
    invalid_input, read_circular_cocycle_bounded, read_kinetic_edges, read_proof_input,
    write_phases, write_via_temporary,
};

pub(super) fn run_cohomology(cli: CohomologyCli) -> crate::Result<()> {
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, Some(cli.scale))?;
    let limits = CohomologyLimits::default();
    let space = cohomology_space(&graph, cli.dimension, cli.scale, cli.modulus, limits)?;
    println!(
        "H{} at {} over Z/{} has rank {} and space id {}",
        cli.dimension,
        cli.scale,
        cli.modulus,
        space.rank(),
        space.id()
    );
    for class in space.basis() {
        let mut terms = String::new();
        for (position, term) in class.terms.iter().enumerate() {
            if position != 0 {
                terms.push_str(", ");
            }
            write!(terms, "{:?}:{}", term.simplex, term.coefficient)
                .expect("writing to a string cannot fail");
        }
        println!("basis {} {} [{}]", class.basis_index, class.id, terms);
    }
    if let Some(other) = cli.other {
        let other_graph = read_proof_input(&other, cli.format, cli.threads, Some(cli.scale))?;
        let other_space =
            cohomology_space(&other_graph, cli.dimension, cli.scale, cli.modulus, limits)?;
        let relation = cohomology_relation(&graph, &space, &other_graph, &other_space, limits)?;
        println!(
            "relation rank {} from image ranks {} and {}, kernel ranks {} and {}; isomorphism {}",
            relation.relation_rank,
            relation.old_image_rank,
            relation.new_image_rank,
            relation.old_kernel_rank,
            relation.new_kernel_rank,
            relation.is_isomorphism()
        );
    }
    Ok(())
}

pub(super) fn run_circular(cli: CircularCli) -> crate::Result<()> {
    let params = CircularCoordinateParams {
        tolerance: cli.tolerance,
        max_iterations: cli.max_iterations,
        cohomology: CohomologyLimits::default(),
    };
    let selected = selected_class(&cli)?;
    let scale = circular_scale(selected.as_ref(), cli.scale)?;
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, Some(scale))?;
    let coordinate = circular_coordinate_input(&cli, selected.as_ref(), &graph, params, scale)?;
    if let Some(path) = &cli.phases {
        write_phases(path, &coordinate.phase)?;
    }
    let artifact = circular_artifact(&cli, &graph, &coordinate, params, scale)?;
    let bytes = artifact.encode().map_err(invalid_input)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "wrote {} circular states, {} coordinates, and {} active edges to {}",
        artifact.summary().states,
        artifact.summary().coordinates,
        artifact.summary().edges,
        cli.output.display()
    );
    Ok(())
}

fn circular_scale(selected: Option<&PersistentClass>, declared: Option<f64>) -> crate::Result<f64> {
    selected
        .map(|class| class.cocycle.scale)
        .or(declared)
        .ok_or_else(|| crate::Error::InvalidInput("raw circular cocycle rows require --at".into()))
}

fn circular_coordinate_input(
    cli: &CircularCli,
    selected: Option<&PersistentClass>,
    graph: &SparseDistanceMatrix,
    params: CircularCoordinateParams,
    scale: f64,
) -> crate::Result<CircularCoordinate> {
    if let Some(class) = selected {
        validate_selected_class(cli, class, scale)?;
        circular_coordinate_for_class(graph, class, params)
    } else {
        raw_circular_coordinate(cli, graph, params, scale)
    }
}

fn validate_selected_class(
    cli: &CircularCli,
    class: &PersistentClass,
    scale: f64,
) -> crate::Result<()> {
    if cli
        .scale
        .is_some_and(|declared| declared.to_bits() != scale.to_bits())
    {
        return Err(crate::Error::InvalidInput(
            "--at differs from the selected persistent class scale".into(),
        ));
    }
    if cli
        .modulus
        .is_some_and(|declared| declared != class.cocycle.modulus)
    {
        return Err(crate::Error::InvalidInput(
            "--modulus differs from the selected persistent class field".into(),
        ));
    }
    Ok(())
}

fn raw_circular_coordinate(
    cli: &CircularCli,
    graph: &SparseDistanceMatrix,
    params: CircularCoordinateParams,
    scale: f64,
) -> crate::Result<CircularCoordinate> {
    let modulus = cli.modulus.unwrap_or(47);
    let rows = read_circular_cocycle_bounded(&cli.cocycle, modulus, cli.max_record_bytes)?;
    let cocycle = cocycle_from_ripser_terms(graph, modulus, scale, &rows)?;
    circular_coordinate(graph, &cocycle, params)
}

fn selected_class(cli: &CircularCli) -> crate::Result<Option<PersistentClass>> {
    let Some(selection) = &cli.class else {
        return Ok(None);
    };
    let [space, basis] = selection.as_slice() else {
        return Err(crate::Error::InvalidInput(
            "--class needs one class-space position and one basis position".into(),
        ));
    };
    read_persistent_class(&cli.cocycle, *space, *basis, cli.max_record_bytes).map(Some)
}

fn circular_artifact(
    cli: &CircularCli,
    graph: &SparseDistanceMatrix,
    coordinate: &CircularCoordinate,
    params: CircularCoordinateParams,
    scale: f64,
) -> crate::Result<CircularCoordinateArtifact> {
    let Some(other_path) = &cli.other else {
        return CircularCoordinateArtifact::from_coordinate(graph, coordinate);
    };
    let other = read_proof_input(other_path, cli.format, cli.threads, Some(scale))?;
    let continuation = continue_circular_coordinate(graph, coordinate, &other, params)?;
    write_continued_phases(cli, &continuation)?;
    eprintln!(
        "circular continuation: {:?}, ambiguity rank {}",
        continuation.topology.kind,
        continuation.topology.ambiguity.len()
    );
    CircularCoordinateArtifact::from_continuation(
        graph,
        coordinate,
        &other,
        &continuation,
        params.cohomology,
    )
}

fn write_continued_phases(
    cli: &CircularCli,
    continuation: &crate::CircularCoordinateContinuation,
) -> crate::Result<()> {
    let Some(path) = &cli.continued_phases else {
        return Ok(());
    };
    let continued = continuation.coordinate.as_ref().ok_or_else(|| {
        crate::Error::InvalidInput(format!(
            "continued phases need a unique continuation, got {:?}",
            continuation.topology.kind
        ))
    })?;
    write_phases(path, &continued.phase)
}

fn kinetic_kind(kind: &KineticEventKind) -> String {
    match kind {
        KineticEventKind::ThresholdCrossing { edge } => {
            format!("threshold ({}, {})", edge.u, edge.v)
        }
        KineticEventKind::EdgeOrderSwap { first, second } => format!(
            "order ({}, {}) with ({}, {})",
            first.u, first.v, second.u, second.v
        ),
    }
}

pub(super) fn run_kinetic(cli: KineticCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let schedule = trajectory.events(cli.scale)?;
    report_kinetic_schedule(&schedule);
    if let (Some(dimension), Some(scale)) = (cli.dimension, cli.scale) {
        run_kinetic_cohomology(&cli, &trajectory, dimension, scale)?;
    }
    Ok(())
}

pub(super) fn report_kinetic_schedule(schedule: &crate::KineticSchedule) {
    println!(
        "certified {} isolated events and {} persistent ties on [{}, {}]",
        schedule.events.len(),
        schedule.persistent_ties,
        schedule.start,
        schedule.end
    );
    for event in &schedule.events {
        let kinds = event
            .kinds
            .iter()
            .map(kinetic_kind)
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "event {} in [{}, {}]: {}",
            event.time, event.lower, event.upper, kinds
        );
    }
}

pub(super) fn run_kinetic_cohomology(
    cli: &KineticCli,
    trajectory: &KineticFiltration,
    dimension: usize,
    scale: f64,
) -> crate::Result<()> {
    let relations =
        trajectory.cohomology_events(dimension, scale, cli.modulus, CohomologyLimits::default())?;
    for event in relations {
        println!(
            "H{} event {}: rank {} to {}, relation rank {}",
            dimension,
            event.event.time,
            event.before_rank,
            event.after_rank,
            event.relation.relation_rank
        );
    }
    if let Some(output) = &cli.zigzag {
        write_kinetic_zigzag(cli, trajectory, dimension, scale, output)?;
    }
    Ok(())
}

fn write_kinetic_zigzag(
    cli: &KineticCli,
    trajectory: &KineticFiltration,
    dimension: usize,
    scale: f64,
    output: &Path,
) -> crate::Result<()> {
    let limits = KineticZigzagArtifactLimits {
        max_bytes: cli.max_artifact_bytes,
        ..KineticZigzagArtifactLimits::default()
    };
    let (artifact, zigzag) =
        KineticZigzagArtifact::build(trajectory, dimension, scale, cli.modulus, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(output, &bytes)?;
    let summary = artifact.summary();
    println!(
        "wrote H{} kinetic zigzag with {} nodes, {} arrows, {} interval spaces, {} interval copies, and {} bytes",
        dimension,
        summary.nodes,
        summary.arrows,
        summary.intervals,
        summary.interval_copies,
        bytes.len()
    );
    for interval in zigzag.barcode.intervals {
        println!(
            "zigzag [{}..={}] multiplicity {} id {}",
            interval.start, interval.end, interval.multiplicity, interval.id
        );
    }
    Ok(())
}
