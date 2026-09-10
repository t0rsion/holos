use std::io::Write;

use crate::{Diagram, Error, Result};

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
/// empty top dimensions still appear. The header count follows holos's
/// effective dimension (ripser clamps at n-2, holos at n-1).
pub fn write_diagram<W: Write>(
    w: &mut W,
    diagram: &Diagram,
    format: OutputFormat,
    max_dim: usize,
) -> Result<()> {
    match format {
        OutputFormat::Ripser => write_ripser_diagram(w, diagram, max_dim),
        OutputFormat::Csv => write_csv_diagram(w, diagram),
    }
}

fn write_ripser_diagram<W: Write>(w: &mut W, diagram: &Diagram, max_dim: usize) -> Result<()> {
    for dim in 0..=max_dim {
        writeln!(w, "persistence intervals in dim {dim}:").map_err(io_error)?;
        for bar in diagram.in_dim(dim) {
            write_ripser_bar(w, bar)?;
        }
    }
    Ok(())
}

fn write_ripser_bar<W: Write>(w: &mut W, bar: &crate::Bar) -> Result<()> {
    if bar.is_essential() {
        writeln!(w, " [{}, )", bar.birth).map_err(io_error)
    } else {
        writeln!(w, " [{},{})", bar.birth, bar.death).map_err(io_error)
    }
}

fn write_csv_diagram<W: Write>(w: &mut W, diagram: &Diagram) -> Result<()> {
    writeln!(w, "dim,birth,death").map_err(io_error)?;
    for bar in &diagram.bars {
        // `f64` display writes infinity as the documented `inf` marker.
        writeln!(w, "{},{},{}", bar.dim, bar.birth, bar.death).map_err(io_error)?;
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> Error {
    Error::Io(error.to_string())
}
