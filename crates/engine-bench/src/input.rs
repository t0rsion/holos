use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use holos_tda::collapse::collapse_sparse;
use holos_tda::io::{self, OutputFormat, write_diagram};
use holos_tda::{Diagram, DistanceMatrix, SparseDistanceMatrix};

use super::model::{Args, Format, Parsed};

/// Read and parse the input file with the library's own parsers. The parse
/// is a phase of its own, so this stops short of the matrix, which
/// holos_tda::io::read_lower_distance_matrix would build in the same call.
pub(super) fn read_input(path: &str, format: Format, threads: usize) -> Result<Parsed, String> {
    let name = file_stem(path);
    let text = fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
    let parsed = match format {
        Format::Points => {
            Parsed::Points(io::parse_point_cloud(&name, &text, threads).map_err(|e| e.to_string())?)
        }
        Format::LowerDistance => Parsed::Condensed(
            io::parse_condensed(&name, &text, threads).map_err(|e| e.to_string())?,
        ),
        Format::Sparse => {
            let (n, triplets) =
                io::parse_triplets(&name, &text, threads).map_err(|e| e.to_string())?;
            Parsed::Triplets(n, triplets)
        }
    };
    Ok(parsed)
}

/// The file name without its extension. Recorded output carries no absolute
/// path, as benchmarks/_common.sh requires.
pub(super) fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

/// The verified diagram in ripser's format, for the comparison the runner
/// makes against ripser's own output.
pub(super) fn write_reference_diagram(
    path: &str,
    diagram: &Diagram,
    max_dim: usize,
) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", file_stem(path)))?;
    let mut out = BufWriter::new(file);
    write_diagram(&mut out, diagram, OutputFormat::Ripser, max_dim).map_err(|e| e.to_string())
}

/// Collapse the input and write the reduced graph, then stop. No clock
/// runs: this is a preprocessing step, and the collapse itself is measured
/// by the collapse benchmarks.
pub(super) fn emit_collapsed(args: &Args, path: &str) -> Result<(), String> {
    let sparse = sparse_input(args)?;
    let points = sparse.len();
    let before = sparse.num_edges();
    let collapsed =
        collapse_sparse(&sparse, Some(args.threshold)).map_err(|e| format!("collapse: {e}"))?;
    let after = collapsed.matrix.num_edges();
    let (labels, isolated) = isolated_first_labels(&collapsed.matrix);
    let rows = relabelled_edges(&collapsed.matrix, &labels);
    write_sparse_rows(path, &rows)?;
    println!(
        "collapsed entry={} n={} edges_in={} edges_out={} isolated={} threshold={} kept={:.6}",
        args.entry,
        points,
        before,
        after,
        isolated,
        args.threshold_text,
        kept_fraction(before, after)
    );
    Ok(())
}

fn sparse_input(args: &Args) -> Result<SparseDistanceMatrix, String> {
    let sparse = match read_input(&args.input, args.format, args.parse_threads)? {
        Parsed::Triplets(n, triplets) => {
            SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())?
        }
        Parsed::Points(points) => {
            let dist = DistanceMatrix::from_points(&points).map_err(|e| e.to_string())?;
            threshold_to_sparse(&dist, args.threshold)?
        }
        Parsed::Condensed(data) => {
            let dist = DistanceMatrix::from_condensed(data).map_err(|e| e.to_string())?;
            threshold_to_sparse(&dist, args.threshold)?
        }
    };
    Ok(sparse)
}

fn isolated_first_labels(sparse: &SparseDistanceMatrix) -> (Vec<usize>, usize) {
    let mut degree = vec![0usize; sparse.len()];
    for (u, v, _) in sparse.edges() {
        degree[u] += 1;
        degree[v] += 1;
    }
    let isolated = degree.iter().filter(|&&value| value == 0).count();
    let mut labels = vec![0usize; sparse.len()];
    let mut next = 0;
    for pass in [0usize, 1] {
        for (vertex, &value) in degree.iter().enumerate() {
            if (value == 0) == (pass == 0) {
                labels[vertex] = next;
                next += 1;
            }
        }
    }
    (labels, isolated)
}

fn relabelled_edges(sparse: &SparseDistanceMatrix, labels: &[usize]) -> Vec<(usize, usize, f64)> {
    let mut rows: Vec<(usize, usize, f64)> = sparse
        .edges()
        .map(|(u, v, distance)| (labels[u].max(labels[v]), labels[u].min(labels[v]), distance))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    rows
}

fn write_sparse_rows(path: &str, rows: &[(usize, usize, f64)]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", file_stem(path)))?;
    let mut out = BufWriter::new(file);
    for (u, v, d) in rows {
        writeln!(out, "{u} {v} {d:?}").map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn kept_fraction(before: usize, after: usize) -> f64 {
    if before == 0 {
        0.0
    } else {
        after as f64 / before as f64
    }
}

/// The thresholded graph of a dense matrix, as the sparse engine sees it.
/// The vertex count comes from the matrix, so a vertex with no edge keeps
/// its place and its essential H0 bar.
pub(super) fn threshold_to_sparse(
    dist: &DistanceMatrix,
    threshold: f64,
) -> Result<SparseDistanceMatrix, String> {
    let n = dist.len();
    let mut triplets: Vec<(usize, usize, f64)> = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dist.get(i, j);
            if d.is_finite() && d <= threshold {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())
}
