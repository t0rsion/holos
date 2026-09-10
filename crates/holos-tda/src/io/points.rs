use std::path::Path;

use crate::{Error, Result};

use super::scanner::{Parse, is_skipped, parse_numbers};
use super::window::for_each_window;

/// Read a point cloud: one point per line, coordinates separated by commas
/// and/or whitespace. Blank lines and lines starting with `#` are skipped.
/// All points must have the same dimension.
///
/// `threads` is the worker budget for the parse. See `parse_point_cloud`.
pub fn read_point_cloud(path: &Path, threads: usize) -> Result<Vec<Vec<f64>>> {
    let name = path.display().to_string();
    let mut sink = PointSink::new(threads);
    for_each_window(path, threads, |first_line, text| {
        sink.window(&name, first_line, text)
    })?;
    Ok(sink.points)
}

/// Parse a point cloud from `text`, in the format `read_point_cloud` reads.
/// `name` prefixes every error message; a reader passes the file path.
///
/// `threads` is the worker budget for the parse. With more than one thread
/// and a text of at least one mebibyte, the parse splits into one line
/// chunk per thread. The points and the error message do not depend on the
/// worker count.
pub fn parse_point_cloud(name: &str, text: &str, threads: usize) -> Result<Vec<Vec<f64>>> {
    let mut sink = PointSink::new(threads);
    sink.window(name, 1, text)?;
    Ok(sink.points)
}

/// The points gathered from the windows of one input.
pub(super) struct PointSink {
    pub(super) parse: Parse,
    pub(super) points: Vec<Vec<f64>>,
    /// The width of the first point of the file, once one is read.
    width: Option<usize>,
}

impl PointSink {
    pub(super) fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            points: Vec::new(),
            width: None,
        }
    }

    pub(super) fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
        let (chunks, newlines) = self.parse.map(text, first_line, |first, chunk| {
            point_cloud_chunk(name, first, chunk)
        });
        let total: usize = chunks.iter().map(|chunk| chunk.points.len()).sum();
        self.points.reserve(total);
        for mut chunk in chunks {
            if let Some(first) = chunk.points.first() {
                match self.width {
                    None => self.width = Some(first.len()),
                    // A serial parse measures every point against the first
                    // point of the file. A chunk measures against its own
                    // first point, so the file-wide check lands here, and it
                    // lands on a line at or before any line the chunk itself
                    // rejected.
                    Some(w) if first.len() != w => {
                        return Err(Error::InvalidInput(format!(
                            "{name}:{}: point has {} coordinates, expected {w}",
                            chunk.first_line,
                            first.len()
                        )));
                    }
                    Some(_) => {}
                }
            }
            if let Some(error) = chunk.error {
                return Err(error);
            }
            if self.points.is_empty() {
                self.points = chunk.points;
            } else {
                self.points.append(&mut chunk.points);
            }
        }
        Ok(newlines)
    }
}

/// One line chunk of a point cloud parse.
struct PointChunk {
    /// Line number of the chunk's first point. Zero when it read none.
    first_line: usize,
    points: Vec<Vec<f64>>,
    /// Parse error from the first bad line, if any.
    error: Option<Error>,
}

fn point_cloud_chunk(name: &str, first_line: usize, text: &str) -> PointChunk {
    let mut chunk = PointChunk {
        first_line: 0,
        points: Vec::new(),
        error: None,
    };
    for (idx, line) in text.lines().enumerate() {
        let lineno = first_line + idx;
        if is_skipped(line) {
            continue;
        }
        let width = chunk.points.first().map_or(0, Vec::len);
        let mut point = Vec::with_capacity(width);
        if let Err(t) = parse_numbers(line, |v| point.push(v)) {
            chunk.error = Some(Error::InvalidInput(format!(
                "{name}:{lineno}: not a number: {t:?}"
            )));
            break;
        }
        if point.is_empty() {
            // The line holds separators only, so treat it as blank.
            continue;
        }
        if chunk.points.is_empty() {
            chunk.first_line = lineno;
        } else if point.len() != width {
            chunk.error = Some(Error::InvalidInput(format!(
                "{name}:{lineno}: point has {} coordinates, expected {width}",
                point.len()
            )));
            break;
        }
        chunk.points.push(point);
    }
    chunk
}
