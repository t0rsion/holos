//! Source-bound persistent-class and circular-coordinate producers.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::{
    CertificateLimits, CircularCoordinateParams, IntegralCocycleTerm, PersistentClass,
    PersistentClassArtifact, PersistentCoordinateArtifact, RipsParams, SparseDistanceMatrix,
};

use super::args::{PersistentCircularCli, PersistentClassCli};
use super::circular_input::read_integral_lift;
use super::input::{invalid_input, read_persistent_input, write_via_temporary};

pub(super) fn run_persistent_class(cli: PersistentClassCli) -> crate::Result<()> {
    validate_persistent_output_paths(&cli.output, cli.record.as_deref(), None)?;
    let graph = read_persistent_input(&cli.input, cli.format, cli.threads)?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = build_persistent_class(
        &graph,
        cli.modulus,
        cli.threshold,
        cli.threads,
        cli.space,
        cli.basis,
        limits,
    )?;
    let bytes = artifact.encode(limits).map_err(invalid_input)?;
    if let Some(path) = &cli.record {
        write_record(
            path,
            class_record(&artifact, bytes.len()),
            cli.max_artifact_bytes,
        )?;
    }
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "wrote HOLOSPC class {} with {} cycle terms and {} chain terms to {}",
        artifact.class().id,
        artifact.cycle().len(),
        artifact.bounding_chain().len(),
        cli.output.display()
    );
    Ok(())
}

pub(super) fn run_persistent_circular(cli: PersistentCircularCli) -> crate::Result<()> {
    validate_persistent_output_paths(&cli.output, cli.record.as_deref(), cli.phases.as_deref())?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let (graph, lift) = read_persistent_circular_input(&cli, limits)?;
    let class = build_persistent_class(
        &graph,
        cli.modulus,
        cli.threshold,
        cli.threads,
        cli.space,
        cli.basis,
        limits,
    )?;
    let artifact = build_persistent_coordinate(&cli, &class, lift.as_deref())?;
    let (bytes, phase_bytes) = encode_persistent_coordinate(&cli, &artifact, limits)?;
    write_persistent_circular_outputs(&cli, &artifact, &bytes, phase_bytes.as_deref())?;
    println!(
        "wrote HOLOSPH coordinate on {} vertices with {} lift terms to {}",
        artifact.source().len(),
        artifact.integral().len(),
        cli.output.display()
    );
    Ok(())
}

fn read_persistent_circular_input(
    cli: &PersistentCircularCli,
    limits: CertificateLimits,
) -> crate::Result<(SparseDistanceMatrix, Option<Vec<IntegralCocycleTerm>>)> {
    let graph = read_persistent_input(&cli.input, cli.format, cli.threads)?;
    let lift =
        read_persistent_circular_lift(cli.integral_lift.as_deref(), limits.max_bytes, graph.len())?;
    Ok((graph, lift))
}

fn build_persistent_class(
    graph: &SparseDistanceMatrix,
    modulus: u32,
    threshold: Option<f64>,
    threads: usize,
    space: usize,
    basis: usize,
    limits: CertificateLimits,
) -> crate::Result<PersistentClassArtifact> {
    let params = persistence_params(modulus, threshold, threads);
    PersistentClassArtifact::build(graph, &params, space, basis, limits).map_err(invalid_input)
}

fn read_persistent_circular_lift(
    path: Option<&Path>,
    maximum_bytes: usize,
    vertex_count: usize,
) -> crate::Result<Option<Vec<IntegralCocycleTerm>>> {
    match path {
        Some(path) => read_integral_lift(path, maximum_bytes, vertex_count).map(Some),
        None => Ok(None),
    }
}

fn build_persistent_coordinate(
    cli: &PersistentCircularCli,
    class: &PersistentClassArtifact,
    lift: Option<&[IntegralCocycleTerm]>,
) -> crate::Result<PersistentCoordinateArtifact> {
    let params = CircularCoordinateParams {
        tolerance: cli.tolerance,
        max_iterations: cli.max_iterations,
        cohomology: crate::CohomologyLimits::default(),
    };
    PersistentCoordinateArtifact::build(class, params, lift)
}

fn encode_persistent_coordinate(
    cli: &PersistentCircularCli,
    artifact: &PersistentCoordinateArtifact,
    limits: CertificateLimits,
) -> crate::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let phase_bytes = encode_requested_phases(cli.phases.is_some(), artifact, limits.max_bytes)?;
    let bytes = artifact.encode(limits).map_err(invalid_input)?;
    Ok((bytes, phase_bytes))
}

fn encode_requested_phases(
    requested: bool,
    artifact: &PersistentCoordinateArtifact,
    maximum_bytes: usize,
) -> crate::Result<Option<Vec<u8>>> {
    if requested {
        encode_phase_bytes(artifact.phase(), maximum_bytes).map(Some)
    } else {
        Ok(None)
    }
}

fn write_persistent_circular_outputs(
    cli: &PersistentCircularCli,
    artifact: &PersistentCoordinateArtifact,
    bytes: &[u8],
    phase_bytes: Option<&[u8]>,
) -> crate::Result<()> {
    if let Some(path) = &cli.record {
        write_record(
            path,
            coordinate_record(artifact, bytes.len()),
            cli.max_artifact_bytes,
        )?;
    }
    write_via_temporary(&cli.output, bytes)?;
    write_requested_phases(cli.phases.as_deref(), phase_bytes)
}

fn write_requested_phases(path: Option<&Path>, bytes: Option<&[u8]>) -> crate::Result<()> {
    match (path, bytes) {
        (Some(path), Some(bytes)) => write_via_temporary(path, bytes),
        (None, None) => Ok(()),
        _ => Err(crate::Error::InvalidInput(
            "persistent coordinate phase output is incomplete".into(),
        )),
    }
}

fn validate_persistent_output_paths(
    artifact: &Path,
    record: Option<&Path>,
    phases: Option<&Path>,
) -> crate::Result<()> {
    let mut requested = vec![("artifact", artifact)];
    if let Some(path) = record {
        requested.push(("record", path));
    }
    if let Some(path) = phases {
        requested.push(("phase", path));
    }
    let normalized = requested
        .into_iter()
        .map(|(name, path)| normalize_output_path(path).map(|normalized| (name, path, normalized)))
        .collect::<crate::Result<Vec<_>>>()?;
    for (index, (left_name, left_path, left)) in normalized.iter().enumerate() {
        for (right_name, right_path, right) in normalized.iter().skip(index + 1) {
            if left == right {
                return Err(crate::Error::InvalidInput(format!(
                    "persistent output paths for {left_name} ({}) and {right_name} ({}) refer to the same file",
                    left_path.display(),
                    right_path.display()
                )));
            }
        }
    }
    Ok(())
}

fn normalize_output_path(path: &Path) -> crate::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                crate::Error::Io(format!(
                    "cannot resolve output path {}: {error}",
                    path.display()
                ))
            })?
            .join(path)
    };
    let mut prefix = absolute.clone();
    let mut suffix = Vec::<OsString>::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(&prefix) {
            let mut normalized = canonical;
            for component in suffix.iter().rev() {
                normalized.push(component);
            }
            return Ok(lexically_normalize_path(&normalized));
        }
        let Some(name) = prefix.file_name() else {
            return Ok(lexically_normalize_path(&absolute));
        };
        suffix.push(name.to_os_string());
        if !prefix.pop() {
            return Ok(lexically_normalize_path(&absolute));
        }
    }
}

fn lexically_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let removes_normal = matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                );
                if removes_normal {
                    normalized.pop();
                } else if !path.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    normalized
}

fn persistence_params(modulus: u32, threshold: Option<f64>, threads: usize) -> RipsParams {
    let mut params = RipsParams::new(1)
        .with_modulus(modulus)
        .with_threads(threads);
    params.threshold = threshold;
    params
}

fn certificate_limits(maximum: usize) -> CertificateLimits {
    CertificateLimits {
        max_bytes: maximum,
        ..CertificateLimits::default()
    }
}

fn write_record(path: &Path, value: Value, maximum: usize) -> crate::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| {
        crate::Error::InvalidInput(format!("cannot encode persistent workflow record: {error}"))
    })?;
    bytes.push(b'\n');
    if bytes.len() > maximum {
        return Err(crate::Error::InvalidInput(format!(
            "persistent workflow record has {} bytes, above the limit {}",
            bytes.len(),
            maximum
        )));
    }
    write_via_temporary(path, &bytes)
}

fn encode_phase_bytes(phases: &[f64], maximum: usize) -> crate::Result<Vec<u8>> {
    let mut output = String::new();
    for (vertex, phase) in phases.iter().enumerate() {
        writeln!(output, "{vertex} {phase}").expect("writing to a string cannot fail");
        if output.len() > maximum {
            return Err(crate::Error::InvalidInput(format!(
                "persistent coordinate phases exceed the byte limit {maximum}"
            )));
        }
    }
    Ok(output.into_bytes())
}

fn class_record(artifact: &PersistentClassArtifact, bytes: usize) -> Value {
    let class = artifact.class();
    json!({
        "format": "holos-persistent-class-v1",
        "artifact_bytes": bytes,
        "interval": interval_record(class),
        "class": {
            "group_id": class.group_id.to_string(),
            "class_id": class.id.to_string(),
            "basis_index": class.basis_index,
            "modulus": class.cocycle.modulus,
            "scale": class.cocycle.scale,
            "cocycle": class
                .cocycle
                .terms
                .iter()
                .map(|term| json!([term.u, term.v, term.coefficient]))
                .collect::<Vec<_>>(),
        },
        "critical_pair": {
            "birth": artifact.critical_pair().birth.vertices,
            "death": artifact
                .critical_pair()
                .death
                .as_ref()
                .map(|simplex| simplex.vertices.clone()),
        },
        "cycle": artifact
            .cycle()
            .iter()
            .map(|term| json!([term.u, term.v, term.coefficient]))
            .collect::<Vec<_>>(),
        "bounding_chain": artifact
            .bounding_chain()
            .iter()
            .map(|term| json!([term.vertices, term.coefficient]))
            .collect::<Vec<_>>(),
    })
}

fn coordinate_record(artifact: &PersistentCoordinateArtifact, bytes: usize) -> Value {
    let mut record = class_record(artifact.class_artifact(), bytes);
    record["format"] = json!("holos-persistent-coordinate-v1");
    record["coordinate"] = json!({
        "field_multiplier": artifact.field_multiplier(),
        "divisibility": artifact.divisibility(),
        "relative_residual": artifact.relative_residual(),
        "iterations": artifact.iterations(),
        "phase": artifact.phase(),
    });
    record
}

fn interval_record(class: &PersistentClass) -> Value {
    json!({
        "dimension": class.interval.dim,
        "birth": class.interval.birth,
        "death": class.interval.death.is_finite().then_some(class.interval.death),
    })
}
