//! Input parsing, bounded reads, and atomic output helpers.

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use crate::io;
use crate::{DistanceMatrix, KineticEdge, PointCloudGraph, PointCloudParams, SparseDistanceMatrix};

use super::args::InputFormat;

pub(crate) fn infer_format(path: &Path) -> InputFormat {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext)
            if ["csv", "pts", "xyz"]
                .iter()
                .any(|k| ext.eq_ignore_ascii_case(k)) =>
        {
            InputFormat::PointCloud
        }
        _ => InputFormat::LowerDistance,
    }
}

pub(crate) fn write_via_temporary(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        crate::Error::InvalidInput(format!("output path {} has no file name", path.display()))
    })?;
    for nonce in 0..100u32 {
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            nonce
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(crate::Error::Io(format!(
                    "cannot create output file {}: {error}",
                    path.display()
                )));
            }
        };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| match std::fs::rename(&temporary, path) {
                Ok(()) => Ok(()),
                Err(_) if path.is_file() => {
                    std::fs::remove_file(path).and_then(|()| std::fs::rename(&temporary, path))
                }
                Err(error) => Err(error),
            });
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            return Err(crate::Error::Io(format!(
                "cannot write output file {}: {error}",
                path.display()
            )));
        }
        return Ok(());
    }
    Err(crate::Error::Io(format!(
        "cannot create a temporary file for output {}",
        path.display()
    )))
}
pub(crate) fn read_bounded_artifact(
    path: &Path,
    maximum: usize,
    label: &str,
) -> crate::Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|error| {
        crate::Error::Io(format!("cannot open {label} {}: {error}", path.display()))
    })?;
    let read_limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            crate::Error::Io(format!("cannot read {label} {}: {error}", path.display()))
        })?;
    if bytes.len() > maximum {
        return Err(crate::Error::InvalidInput(format!(
            "{label} has {} bytes, above the limit {}",
            bytes.len(),
            maximum
        )));
    }
    Ok(bytes)
}

pub(crate) fn invalid_input(error: impl std::fmt::Display) -> crate::Error {
    crate::Error::InvalidInput(error.to_string())
}

fn for_each_data_line(
    text: &str,
    mut parse: impl FnMut(usize, &str) -> crate::Result<()>,
) -> crate::Result<()> {
    for (line_index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if !line.is_empty() {
            parse(line_index + 1, line)?;
        }
    }
    Ok(())
}

pub(crate) fn read_proof_input(
    input: &Path,
    format: Option<InputFormat>,
    threads: usize,
    threshold: Option<f64>,
) -> crate::Result<SparseDistanceMatrix> {
    let format = format.unwrap_or_else(|| infer_format(input));
    match format {
        InputFormat::Sparse => io::read_sparse_matrix(input, threads.max(1)),
        InputFormat::PointCloud => {
            let points = io::read_point_cloud(input, threads.max(1))?;
            if let Some(threshold) = threshold {
                PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map(PointCloudGraph::into_matrix)
            } else {
                let dense = DistanceMatrix::from_points(&points)?;
                dense.to_sparse_at(dense.enclosing_radius())
            }
        }
        InputFormat::LowerDistance => {
            let dense = io::read_lower_distance_matrix(input, threads.max(1))?;
            dense.to_sparse_at(threshold.unwrap_or_else(|| dense.enclosing_radius()))
        }
    }
}

pub(crate) fn read_persistent_input(
    input: &Path,
    format: Option<InputFormat>,
    threads: usize,
) -> crate::Result<SparseDistanceMatrix> {
    let format = format.unwrap_or_else(|| infer_format(input));
    match format {
        InputFormat::Sparse => io::read_sparse_matrix(input, threads.max(1)),
        InputFormat::PointCloud => {
            let points = io::read_point_cloud(input, threads.max(1))?;
            let dense = DistanceMatrix::from_points(&points)?;
            dense.to_sparse_at(f64::INFINITY)
        }
        InputFormat::LowerDistance => {
            let dense = io::read_lower_distance_matrix(input, threads.max(1))?;
            dense.to_sparse_at(f64::INFINITY)
        }
    }
}

pub(crate) fn read_circular_cocycle(
    path: &Path,
    modulus: u32,
) -> crate::Result<Vec<(usize, usize, u32)>> {
    read_circular_cocycle_bounded(path, modulus, 1usize << 30)
}

pub(crate) fn read_circular_cocycle_bounded(
    path: &Path,
    modulus: u32,
    maximum_bytes: usize,
) -> crate::Result<Vec<(usize, usize, u32)>> {
    let bytes = read_bounded_artifact(path, maximum_bytes, "circular cocycle")?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| crate::Error::InvalidInput(format!("cocycle is not UTF-8: {error}")))?;
    let mut terms = Vec::new();
    for_each_data_line(text, |line_number, line| {
        let fields = line
            .split(|character: char| character.is_ascii_whitespace() || character == ',')
            .filter(|field| !field.is_empty())
            .collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(crate::Error::InvalidInput(format!(
                "{}:{}: expected u v coefficient",
                path.display(),
                line_number
            )));
        }
        let u = fields[0].parse::<usize>().map_err(|error| {
            crate::Error::InvalidInput(format!(
                "{}:{}: invalid first endpoint: {error}",
                path.display(),
                line_number
            ))
        })?;
        let v = fields[1].parse::<usize>().map_err(|error| {
            crate::Error::InvalidInput(format!(
                "{}:{}: invalid second endpoint: {error}",
                path.display(),
                line_number
            ))
        })?;
        let coefficient = fields[2].parse::<i64>().map_err(|error| {
            crate::Error::InvalidInput(format!(
                "{}:{}: invalid coefficient: {error}",
                path.display(),
                line_number
            ))
        })?;
        if modulus == 0 {
            return Err(crate::Error::InvalidInput(
                "circular modulus must be positive".into(),
            ));
        }
        terms.push((u, v, coefficient.rem_euclid(i64::from(modulus)) as u32));
        Ok(())
    })?;
    Ok(terms)
}

pub(crate) fn write_phases(path: &Path, phases: &[f64]) -> crate::Result<()> {
    let mut output = String::new();
    for (vertex, phase) in phases.iter().enumerate() {
        writeln!(output, "{vertex} {phase}").expect("writing to a string cannot fail");
    }
    write_via_temporary(path, output.as_bytes())
}

pub(crate) fn read_kinetic_edges(
    path: &Path,
    maximum_bytes: usize,
) -> crate::Result<Vec<KineticEdge>> {
    let bytes = read_bounded_artifact(path, maximum_bytes, "affine trajectory")?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| crate::Error::InvalidInput(format!("trajectory is not UTF-8: {error}")))?;
    let mut edges = Vec::new();
    for_each_data_line(text, |line_number, line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(crate::Error::InvalidInput(format!(
                "trajectory line {} needs u, v, intercept, and velocity",
                line_number
            )));
        }
        let parse_usize = |position: usize| {
            fields[position].parse::<usize>().map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "trajectory line {} has an invalid vertex: {error}",
                    line_number
                ))
            })
        };
        let parse_f64 = |position: usize| {
            fields[position].parse::<f64>().map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "trajectory line {} has an invalid coefficient: {error}",
                    line_number
                ))
            })
        };
        edges.push(KineticEdge {
            u: parse_usize(0)?,
            v: parse_usize(1)?,
            intercept: parse_f64(2)?,
            velocity: parse_f64(3)?,
        });
        Ok(())
    })?;
    Ok(edges)
}
