//! Main persistence computation and explanation workflows.

use std::io::Write;
use std::path::Path;

use crate::collapse::wire::CollapseArtifact;
use crate::collapse::{AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective};
use crate::io::{self, OutputFormat};
use crate::{
    AtlasArtifact, CertificateLimits, CollapseSchedule, DistanceMatrix, ExplainedDiagram,
    PointCloudGraph, PointCloudParams, ProgramArtifact, RipsParams, SparseDistanceMatrix,
    lift_h1_classes, rips_persistence_with_classes_sparse,
};

use super::args::{Cli, DiagramFormat, InputFormat, Schedule};
use super::class_record::write_representatives;
use super::input::{infer_format, invalid_input, write_via_temporary};

pub(super) fn report_collapse(collapsed: &crate::collapse::CollapsedRips) {
    let s = &collapsed.stats;
    let epoch = match collapsed.certificate.algorithm_version() {
        1 => "passes",
        2 => "rounds",
        3 => "passes",
        _ => "epochs",
    };
    eprintln!(
        "collapse: kept {} of {} edges, removed {}, {} {epoch}",
        s.output_edges, s.input_edges, s.removed_edges, s.epochs
    );
    eprintln!(
        "collapse detail: {} edge tests, {} witness segments, \
         max common neighborhood {}",
        s.edge_tests, s.witness_segments, s.max_common_neighborhood
    );
    if collapsed.certificate.algorithm_version() == 3 {
        let completeness = match collapsed.certificate.completeness() {
            CollapseCompleteness::CompleteFixedPoint => "complete fixed point",
            CollapseCompleteness::BudgetLimited => "budget-limited partial collapse",
        };
        eprintln!(
            "collapse adaptive: {completeness}, {} work units, {} score evaluations",
            collapsed.certificate.work_used(),
            s.adaptive_score_evaluations
        );
    }
}

pub(super) fn write_collapse_artifact(
    collapsed: &crate::collapse::CollapsedRips,
    path: Option<&Path>,
) -> crate::Result<()> {
    report_collapse(collapsed);
    let Some(path) = path else {
        return Ok(());
    };
    let artifact = CollapseArtifact::from_result(collapsed).map_err(invalid_input)?;
    let bytes = artifact.encode().map_err(invalid_input)?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "collapse artifact: wrote {} bytes to {}",
        bytes.len(),
        path.display()
    );
    Ok(())
}

pub(super) fn collapse_for_explain(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> crate::Result<crate::collapse::CollapsedRips> {
    match params.collapse_schedule {
        CollapseSchedule::Serial => crate::collapse::collapse_sparse(matrix, params.threshold),
        CollapseSchedule::Ordered => crate::collapse::collapse_sparse_ordered_parallel(
            matrix,
            params.threshold,
            params.threads,
        ),
        CollapseSchedule::Rounds => crate::collapse::collapse_sparse_rounds_parallel(
            matrix,
            params.threshold,
            params.threads,
        ),
        CollapseSchedule::Adaptive => crate::collapse::collapse_sparse_adaptive(
            matrix,
            params.threshold,
            params.adaptive_collapse,
        ),
    }
}

pub(super) fn explain_sparse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<ExplainedDiagram> {
    if let Some(path) = &cli.program {
        return explain_with_program(matrix, params, cli, path);
    }
    if let Some(path) = &cli.atlas {
        return explain_with_atlas(matrix, params, cli, path);
    }
    explain_direct(matrix, params, cli)
}

pub(super) fn record_explain_collapse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<()> {
    if cli.collapse_edges {
        let collapsed = collapse_for_explain(matrix, params)?;
        write_collapse_artifact(&collapsed, cli.collapse_certificate.as_deref())?;
    }
    Ok(())
}

pub(super) fn explain_with_program(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    path: &Path,
) -> crate::Result<ExplainedDiagram> {
    record_explain_collapse(matrix, params, cli)?;
    let (artifact, program) =
        ProgramArtifact::compile(matrix, params, CertificateLimits::default())
            .map_err(invalid_input)?;
    if let Some(path) = &cli.representatives {
        write_representatives(path, program.result())?;
    }
    let explained = program.result().clone();
    let bytes = artifact.encode().map_err(invalid_input)?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "persistence program: wrote {} bytes and {} cyclic atoms to {}",
        bytes.len(),
        artifact.atoms().len(),
        path.display()
    );
    Ok(explained)
}

pub(super) fn explain_with_atlas(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    path: &Path,
) -> crate::Result<ExplainedDiagram> {
    record_explain_collapse(matrix, params, cli)?;
    let artifact = AtlasArtifact::build(matrix, params, CertificateLimits::default())
        .map_err(invalid_input)?;
    if let Some(path) = &cli.representatives {
        write_representatives(path, artifact.explained())?;
    }
    let explained = artifact.explained().clone();
    let bytes = artifact.encode().map_err(invalid_input)?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "persistence atlas: wrote {} bytes to {}",
        bytes.len(),
        path.display()
    );
    Ok(explained)
}

pub(super) fn explain_direct(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<ExplainedDiagram> {
    let explained = if cli.collapse_edges {
        let collapsed = collapse_for_explain(matrix, params)?;
        write_collapse_artifact(&collapsed, cli.collapse_certificate.as_deref())?;
        let mut inner = params.clone();
        inner.collapse_edges = false;
        inner.threshold = Some(collapsed.certificate.terminal_level());
        let reduced = rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
        lift_h1_classes(&collapsed, reduced)?
    } else {
        rips_persistence_with_classes_sparse(matrix, params)?
    };
    if let Some(path) = &cli.representatives {
        write_representatives(path, &explained)?;
    }
    Ok(explained)
}

pub(super) fn run(cli: Cli) -> crate::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    let params = compute_params(&cli);
    validate_compute_options(&cli)?;
    let explain = explain_enabled(&cli);
    let (mut diagram, n_points) = match format {
        InputFormat::Sparse => compute_sparse_input(&cli, &params, explain)?,
        InputFormat::PointCloud => compute_point_input(&cli, &params, explain)?,
        InputFormat::LowerDistance => compute_lower_input(&cli, &params, explain)?,
    };
    diagram.canonicalize();
    write_cli_diagram(&cli, &diagram, n_points)
}

pub(super) fn compute_params(cli: &Cli) -> RipsParams {
    let adaptive_collapse = AdaptiveCollapseParams {
        objective: cli
            .collapse_objective
            .map(Into::into)
            .unwrap_or(if cli.dim <= 1 {
                CollapseObjective::H1
            } else {
                CollapseObjective::H2
            }),
        work_limit: cli.collapse_work_limit,
    };
    RipsParams {
        max_dim: cli.dim,
        // None lets the library apply the input's own default.
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads: cli.threads.max(1),
        use_emergent_pairs: !cli.no_emergent_pairs,
        use_apparent_pairs: !cli.no_apparent_pairs,
        use_clearing: !cli.no_clearing,
        use_adjacency_rows: !cli.no_adjacency_rows,
        collapse_edges: false,
        collapse_schedule: cli.collapse_schedule.unwrap_or(Schedule::Serial).into(),
        adaptive_collapse,
        engine: cli.engine.into(),
        dense_storage: cli.dense_storage.into(),
        factorization: cli.factorization.into(),
    }
}

pub(super) fn validate_compute_options(cli: &Cli) -> crate::Result<()> {
    validate_collapse_options(cli)?;
    validate_explain_options(cli)
}

pub(super) fn validate_collapse_options(cli: &Cli) -> crate::Result<()> {
    if cli.collapse_schedule.is_some() && !cli.collapse_edges {
        return Err(crate::Error::InvalidInput(
            "--collapse-schedule requires --collapse-edges".into(),
        ));
    }
    if cli.collapse_certificate.is_some() && !cli.collapse_edges {
        return Err(crate::Error::InvalidInput(
            "--collapse-certificate requires --collapse-edges".into(),
        ));
    }
    if (cli.collapse_objective.is_some() || cli.collapse_work_limit.is_some())
        && cli.collapse_schedule != Some(Schedule::Adaptive)
    {
        return Err(crate::Error::InvalidInput(
            "--collapse-objective and --collapse-work-limit require --collapse-schedule adaptive"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_explain_options(cli: &Cli) -> crate::Result<()> {
    if cli.atlas.is_some() && cli.program.is_some() {
        return Err(crate::Error::InvalidInput(
            "--atlas and --program cannot be used together".into(),
        ));
    }
    let explain = explain_enabled(cli);
    if explain && cli.dim < 1 {
        return Err(crate::Error::InvalidInput(
            "--representatives, --atlas, and --program require --dim of at least 1".into(),
        ));
    }
    if (cli.atlas.is_some() || cli.program.is_some()) && cli.dim != 1 {
        return Err(crate::Error::InvalidInput(
            "--atlas and --program require --dim 1".into(),
        ));
    }
    Ok(())
}

pub(super) fn explain_enabled(cli: &Cli) -> bool {
    cli.representatives.is_some() || cli.atlas.is_some() || cli.program.is_some()
}

pub(super) fn compute_sparse_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let matrix = io::read_sparse_matrix(&cli.input, params.threads)?;
    report_sparse_input(&matrix, cli.threshold);
    let diagram = compute_sparse_matrix(&matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

pub(super) fn report_sparse_input(matrix: &SparseDistanceMatrix, threshold: Option<f64>) {
    match threshold {
        Some(value) => eprintln!(
            "{} points, {} edges, threshold {value}",
            matrix.len(),
            matrix.num_edges()
        ),
        None => eprintln!(
            "{} points, {} edges, no threshold (all listed edges)",
            matrix.len(),
            matrix.num_edges()
        ),
    }
}

pub(super) fn compute_sparse_matrix(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    explain: bool,
) -> crate::Result<crate::Diagram> {
    if explain {
        return Ok(explain_sparse(matrix, params, cli)?.diagram);
    }
    if cli.collapse_edges {
        return crate::collapse_and_solve(matrix, params, |collapsed| {
            write_collapse_artifact(collapsed, cli.collapse_certificate.as_deref())
        });
    }
    crate::rips_persistence_sparse(matrix, params)
}

pub(super) fn compute_point_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let points = io::read_point_cloud(&cli.input, params.threads)?;
    let Some(threshold) = cli.threshold else {
        let matrix = DistanceMatrix::from_points(&points)?;
        report_dense_input(&matrix, None);
        let diagram = compute_dense_matrix(&matrix, params, cli, explain)?;
        return Ok((diagram, matrix.len()));
    };
    let built = PointCloudGraph::build(
        &points,
        PointCloudParams::new(threshold).with_threads(params.threads),
    )?;
    let matrix = built.matrix();
    eprintln!(
        "{} points, {} edges, threshold {threshold}, {:?} point construction",
        matrix.len(),
        matrix.num_edges(),
        built.stats().strategy
    );
    let diagram = compute_sparse_matrix(matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

pub(super) fn compute_lower_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let matrix = io::read_lower_distance_matrix(&cli.input, params.threads)?;
    report_dense_input(&matrix, cli.threshold);
    let diagram = compute_dense_matrix(&matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

pub(super) fn report_dense_input(matrix: &DistanceMatrix, threshold: Option<f64>) {
    match threshold {
        Some(value) => eprintln!("{} points, threshold {value}", matrix.len()),
        None => eprintln!(
            "{} points, threshold {} (enclosing radius)",
            matrix.len(),
            matrix.enclosing_radius()
        ),
    }
}

pub(super) fn compute_dense_matrix(
    matrix: &DistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    explain: bool,
) -> crate::Result<crate::Diagram> {
    if explain {
        let threshold = cli.threshold.unwrap_or_else(|| matrix.enclosing_radius());
        let sparse = matrix.to_sparse_at(threshold)?;
        return Ok(explain_sparse(&sparse, params, cli)?.diagram);
    }
    if cli.collapse_edges {
        return crate::collapse_and_solve(matrix, params, |collapsed| {
            write_collapse_artifact(collapsed, cli.collapse_certificate.as_deref())
        });
    }
    crate::rips_persistence(matrix, params)
}

pub(super) fn write_cli_diagram(
    cli: &Cli,
    diagram: &crate::Diagram,
    n_points: usize,
) -> crate::Result<()> {
    let output = match cli.output {
        DiagramFormat::Ripser => OutputFormat::Ripser,
        DiagramFormat::Csv => OutputFormat::Csv,
    };
    // Stdout flushes on every line.
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(1 << 16, stdout.lock());
    io::write_diagram(
        &mut out,
        diagram,
        output,
        cli.dim.min(n_points.saturating_sub(1)),
    )?;
    out.flush()
        .map_err(|error| crate::Error::Io(error.to_string()))
}
