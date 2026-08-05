//! Point-cloud and lower-distance-matrix input, diagram output.

use std::io::Write;
use std::path::Path;

use crate::{Diagram, DistanceMatrix, Error, Result, SparseDistanceMatrix};

fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

fn tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
}

fn is_skipped(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// Read a point cloud: one point per line, coordinates separated by commas
/// and/or whitespace. Blank lines and lines starting with `#` are skipped.
/// All points must have the same dimension.
pub fn read_point_cloud(path: &Path) -> Result<Vec<Vec<f64>>> {
    let text = read_to_string(path)?;
    let mut points: Vec<Vec<f64>> = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if is_skipped(line) {
            continue;
        }
        let point = tokens(line)
            .map(|t| {
                t.parse::<f64>().map_err(|_| {
                    Error::InvalidInput(format!("{}:{lineno}: not a number: {t:?}", path.display()))
                })
            })
            .collect::<Result<Vec<f64>>>()?;
        if point.is_empty() {
            // Separators only. Treat the line as blank.
            continue;
        }
        if let Some(first) = points.first() {
            if point.len() != first.len() {
                return Err(Error::InvalidInput(format!(
                    "{}:{lineno}: point has {} coordinates, expected {}",
                    path.display(),
                    point.len(),
                    first.len()
                )));
            }
        }
        points.push(point);
    }
    Ok(points)
}

/// Read a condensed lower-triangle distance matrix (ripser's `lower-distance`
/// format): all comma- and/or whitespace-separated numbers in file order,
/// row by row. Blank lines and lines starting with `#` are skipped.
pub fn read_lower_distance_matrix(path: &Path) -> Result<DistanceMatrix> {
    let text = read_to_string(path)?;
    let mut data = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if is_skipped(line) {
            continue;
        }
        for t in tokens(line) {
            let value = t.parse::<f64>().map_err(|_| {
                Error::InvalidInput(format!("{}:{lineno}: not a number: {t:?}", path.display()))
            })?;
            data.push(value);
        }
    }
    DistanceMatrix::from_condensed(data)
}

/// Read a sparse distance matrix (ripser's `sparse` format): one `i j d`
/// triplet per line, separated by commas and/or whitespace. The number of
/// points is one more than the largest vertex index seen. Blank lines and
/// lines starting with `#` are skipped.
pub fn read_sparse_matrix(path: &Path) -> Result<SparseDistanceMatrix> {
    let text = read_to_string(path)?;
    let mut triplets: Vec<(usize, usize, f64)> = Vec::new();
    let mut n = 0usize;
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if is_skipped(line) {
            continue;
        }
        let fields: Vec<&str> = tokens(line).collect();
        if fields.len() != 3 {
            return Err(Error::InvalidInput(format!(
                "{}:{lineno}: expected 'i j d', got {} fields",
                path.display(),
                fields.len()
            )));
        }
        let parse_vertex = |t: &str| {
            t.parse::<usize>().map_err(|_| {
                Error::InvalidInput(format!(
                    "{}:{lineno}: not a vertex index: {t:?}",
                    path.display()
                ))
            })
        };
        let i = parse_vertex(fields[0])?;
        let j = parse_vertex(fields[1])?;
        if i == usize::MAX || j == usize::MAX {
            return Err(Error::InvalidInput(format!(
                "{}:{lineno}: vertex index out of range",
                path.display()
            )));
        }
        let d = fields[2].parse::<f64>().map_err(|_| {
            Error::InvalidInput(format!(
                "{}:{lineno}: not a number: {:?}",
                path.display(),
                fields[2]
            ))
        })?;
        n = n.max(i + 1).max(j + 1);
        triplets.push((i, j, d));
    }
    SparseDistanceMatrix::from_triplets(n, &triplets)
}

/// Diagram serialization format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Mirrors ripser's stdout: per-dimension headers, ` [birth,death)` lines,
    /// empty death for essential classes.
    Ripser,
    /// `dim,birth,death` header, one bar per row, `inf` for essential deaths.
    Csv,
}

/// Write a diagram to `w` in the given format.
///
/// `max_dim` fixes how many dimension headers the ripser format prints, so
/// empty top dimensions still appear. The syntax matches ripser. The header
/// count follows holos's effective dimension (ripser clamps at n-2, holos at
/// n-1). The bars themselves are the same either way.
pub fn write_diagram<W: Write>(
    w: &mut W,
    diagram: &Diagram,
    format: OutputFormat,
    max_dim: usize,
) -> Result<()> {
    let io_err = |e: std::io::Error| Error::Io(e.to_string());
    match format {
        OutputFormat::Ripser => {
            for dim in 0..=max_dim {
                writeln!(w, "persistence intervals in dim {dim}:").map_err(io_err)?;
                for bar in diagram.in_dim(dim) {
                    if bar.is_essential() {
                        writeln!(w, " [{}, )", bar.birth).map_err(io_err)?;
                    } else {
                        writeln!(w, " [{},{})", bar.birth, bar.death).map_err(io_err)?;
                    }
                }
            }
        }
        OutputFormat::Csv => {
            writeln!(w, "dim,birth,death").map_err(io_err)?;
            for bar in &diagram.bars {
                // f64 Display renders infinity as "inf". That string is the
                // documented essential-death marker.
                writeln!(w, "{},{},{}", bar.dim, bar.birth, bar.death).map_err(io_err)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bar;
    use std::path::PathBuf;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, contents: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("holos_io_test_{}_{name}", std::process::id()));
            std::fs::write(&path, contents).unwrap();
            TempFile(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn point_cloud_mixed_separators() {
        let f = TempFile::new("pc_mixed.csv", "0.0, 1.0\n2.0\t3.0\n4.0 5.0\n");
        let points = read_point_cloud(f.path()).unwrap();
        assert_eq!(points, vec![vec![0.0, 1.0], vec![2.0, 3.0], vec![4.0, 5.0]]);
    }

    #[test]
    fn point_cloud_skips_comments_and_blanks() {
        let f = TempFile::new(
            "pc_comments.csv",
            "# header comment\n\n1.0 2.0\n   \n# mid comment\n3.0 4.0\n",
        );
        let points = read_point_cloud(f.path()).unwrap();
        assert_eq!(points, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    }

    #[test]
    fn point_cloud_inconsistent_dimension_reports_line() {
        let f = TempFile::new("pc_baddim.csv", "1.0 2.0\n\n1.0 2.0 3.0\n");
        let err = read_point_cloud(f.path()).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
        assert!(msg.contains(":3:"), "missing line number: {msg}");
        assert!(msg.contains("3 coordinates"), "{msg}");
        assert!(msg.contains("expected 2"), "{msg}");
    }

    #[test]
    fn point_cloud_bad_token_reports_line() {
        let f = TempFile::new("pc_badtok.csv", "1.0 2.0\n1.0 oops\n");
        let err = read_point_cloud(f.path()).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
        assert!(msg.contains(":2:"), "missing line number: {msg}");
        assert!(msg.contains("oops"), "{msg}");
    }

    #[test]
    fn missing_file_is_io_error_with_path() {
        let path = std::env::temp_dir().join(format!(
            "holos_io_test_{}_does_not_exist",
            std::process::id()
        ));
        let err = read_point_cloud(&path).unwrap_err();
        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("does_not_exist"));
    }

    #[test]
    fn lower_distance_round_trip() {
        // n = 3 condensed lower triangle: d(1,0), d(2,0), d(2,1).
        let f = TempFile::new("ld_roundtrip.lower", "1.5 2.5, 3.5\n");
        let m = read_lower_distance_matrix(f.path()).unwrap();
        let direct = DistanceMatrix::from_condensed(vec![1.5, 2.5, 3.5]).unwrap();
        assert_eq!(m.len(), 3);
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(m.get(i, j), direct.get(i, j));
            }
        }
        assert_eq!(m.get(1, 0), 1.5);
        assert_eq!(m.get(2, 0), 2.5);
        assert_eq!(m.get(2, 1), 3.5);
    }

    #[test]
    fn lower_distance_skips_comments_and_spans_lines() {
        let f = TempFile::new("ld_comments.lower", "# 4 points\n1 2\n3 4\n\n5, 6\n");
        let m = read_lower_distance_matrix(f.path()).unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m.get(3, 2), 6.0);
    }

    #[test]
    fn lower_distance_bad_length_errors() {
        let f = TempFile::new("ld_badlen.lower", "1 2\n");
        let err = read_lower_distance_matrix(f.path()).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
        assert!(msg.contains("condensed length"), "{msg}");
    }

    #[test]
    fn lower_distance_bad_token_reports_line() {
        let f = TempFile::new("ld_badtok.lower", "1.0\n2.0 x\n");
        let err = read_lower_distance_matrix(f.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(":2:"), "missing line number: {msg}");
        assert!(msg.contains('x'), "{msg}");
    }

    fn sample_diagram() -> Diagram {
        let mut diagram = Diagram {
            bars: vec![
                Bar {
                    dim: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                },
                Bar {
                    dim: 0,
                    birth: 0.0,
                    death: 0.25,
                },
                Bar {
                    dim: 1,
                    birth: 0.5,
                    death: 1.0,
                },
            ],
        };
        diagram.canonicalize();
        diagram
    }

    #[test]
    fn ripser_output_format() {
        let mut out = Vec::new();
        write_diagram(&mut out, &sample_diagram(), OutputFormat::Ripser, 1).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "persistence intervals in dim 0:\n [0,0.25)\n [0, )\npersistence intervals in dim 1:\n [0.5,1)\n"
        );
    }

    #[test]
    fn csv_output_format() {
        let mut out = Vec::new();
        write_diagram(&mut out, &sample_diagram(), OutputFormat::Csv, 1).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "dim,birth,death\n0,0,0.25\n0,0,inf\n1,0.5,1\n"
        );
    }

    #[test]
    fn empty_diagram_ripser_output_prints_headers_only() {
        let mut out = Vec::new();
        write_diagram(&mut out, &Diagram::default(), OutputFormat::Ripser, 1).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "persistence intervals in dim 0:\npersistence intervals in dim 1:\n"
        );
    }
}
