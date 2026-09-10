use std::path::Path;

use crate::{DistanceMatrix, Error, Result, SparseDistanceMatrix};

use super::scanner::{Parse, is_separator, is_skipped, parse_numbers, tokens};
use super::window::for_each_window;

/// Read a condensed lower-triangle distance matrix (ripser's `lower-distance`
/// format): all comma- and/or whitespace-separated numbers in file order,
/// row by row. Blank lines and lines starting with `#` are skipped.
///
/// `threads` is the worker budget for the parse. See `parse_condensed`.
pub fn read_lower_distance_matrix(path: &Path, threads: usize) -> Result<DistanceMatrix> {
    let name = path.display().to_string();
    let mut sink = CondensedSink::new(threads);
    for_each_window(path, threads, |first_line, text| {
        sink.window(&name, first_line, text)
    })?;
    DistanceMatrix::from_condensed(sink.data)
}

/// Parse the numbers of a condensed lower-triangle distance matrix from
/// `text`, in file order, in the format `read_lower_distance_matrix` reads.
/// `name` prefixes every error message; a reader passes the file path.
///
/// `threads` is the worker budget for the parse. The numbers and the error
/// message do not depend on the worker count. A file that holds all its
/// numbers on one line stays serial, because a chunk ends at a newline.
pub fn parse_condensed(name: &str, text: &str, threads: usize) -> Result<Vec<f64>> {
    let mut sink = CondensedSink::new(threads);
    sink.window(name, 1, text)?;
    Ok(sink.data)
}

/// The numbers gathered from the windows of one condensed input.
pub(super) struct CondensedSink {
    pub(super) parse: Parse,
    pub(super) data: Vec<f64>,
}

impl CondensedSink {
    pub(super) fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            data: Vec::new(),
        }
    }

    pub(super) fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
        let (chunks, newlines) = self.parse.map(text, first_line, |first, chunk| {
            condensed_chunk(name, first, chunk)
        });
        let total: usize = chunks
            .iter()
            .map_while(|chunk| chunk.as_ref().ok())
            .map(Vec::len)
            .sum();
        self.data.reserve(total);
        for chunk in chunks {
            let mut chunk = chunk?;
            if self.data.is_empty() {
                self.data = chunk;
            } else {
                self.data.append(&mut chunk);
            }
        }
        Ok(newlines)
    }
}

fn condensed_chunk(name: &str, first_line: usize, text: &str) -> Result<Vec<f64>> {
    // A written distance takes at least a digit and a separator; the
    // benchmark files average nineteen bytes per number.
    let mut data = Vec::with_capacity(text.len() / 12);
    for (idx, line) in text.lines().enumerate() {
        let lineno = first_line + idx;
        if is_skipped(line) {
            continue;
        }
        parse_numbers(line, |v| data.push(v))
            .map_err(|t| Error::InvalidInput(format!("{name}:{lineno}: not a number: {t:?}")))?;
    }
    Ok(data)
}

/// Parse a decimal vertex index at `bytes[i..]` on an ASCII line. Returns the
/// index and the position after it, or `None` if no digits start there or
/// the value overflows.
fn parse_index(bytes: &[u8], mut i: usize) -> Option<(usize, usize)> {
    let start = i;
    let mut value = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value
            .checked_mul(10)?
            .checked_add(usize::from(bytes[i] - b'0'))?;
        i += 1;
    }
    (i > start && (i == bytes.len() || is_separator(bytes[i]))).then_some((value, i))
}

/// Read a sparse distance matrix (ripser's `sparse` format): one `i j d`
/// triplet per line, separated by commas and/or whitespace. The number of
/// points is one more than the largest vertex index seen. Blank lines and
/// lines starting with `#` are skipped.
///
/// `threads` is the worker budget for the parse. See `parse_triplets`.
pub fn read_sparse_matrix(path: &Path, threads: usize) -> Result<SparseDistanceMatrix> {
    let name = path.display().to_string();
    let mut sink = TripletSink::new(threads);
    for_each_window(path, threads, |first_line, text| {
        sink.window(&name, first_line, text)
    })?;
    SparseDistanceMatrix::from_triplets(sink.n, &sink.triplets)
}

/// One `i j d` line of a sparse matrix file: two vertex indices and a distance.
pub type Triplet = (usize, usize, f64);

/// Parse `i j d` triplets from `text`, in the format `read_sparse_matrix`
/// reads. Returns the vertex count (one more than the largest index seen)
/// and the triplets in file order. `name` prefixes every error message; a
/// reader passes the file path.
///
/// `threads` is the worker budget for the parse. The triplets and the error
/// message do not depend on the worker count.
pub fn parse_triplets(name: &str, text: &str, threads: usize) -> Result<(usize, Vec<Triplet>)> {
    let mut sink = TripletSink::new(threads);
    sink.window(name, 1, text)?;
    Ok((sink.n, sink.triplets))
}

/// The triplets gathered from the windows of one sparse input.
pub(super) struct TripletSink {
    pub(super) parse: Parse,
    pub(super) n: usize,
    pub(super) triplets: Vec<Triplet>,
}

impl TripletSink {
    pub(super) fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            n: 0,
            triplets: Vec::new(),
        }
    }

    pub(super) fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
        let (chunks, newlines) = self.parse.map(text, first_line, |first, chunk| {
            triplet_chunk(name, first, chunk)
        });
        let total: usize = chunks
            .iter()
            .map_while(|chunk| chunk.as_ref().ok())
            .map(|(_, triplets)| triplets.len())
            .sum();
        self.triplets.reserve(total);
        for chunk in chunks {
            let (chunk_n, mut chunk_triplets) = chunk?;
            self.n = self.n.max(chunk_n);
            if self.triplets.is_empty() {
                self.triplets = chunk_triplets;
            } else {
                self.triplets.append(&mut chunk_triplets);
            }
        }
        Ok(newlines)
    }
}

fn triplet_chunk(name: &str, first_line: usize, text: &str) -> Result<(usize, Vec<Triplet>)> {
    // A triplet line takes at least six bytes.
    let mut triplets: Vec<Triplet> = Vec::with_capacity(text.len() / 24);
    let mut n = 0usize;
    for (idx, line) in text.lines().enumerate() {
        let lineno = first_line + idx;
        if is_skipped(line) {
            continue;
        }
        let (i, j, d) = parse_triplet(line)
            .ok_or(())
            .or_else(|()| parse_triplet_slow(name, lineno, line))?;
        if i == usize::MAX || j == usize::MAX {
            return Err(Error::InvalidInput(format!(
                "{name}:{lineno}: vertex index out of range"
            )));
        }
        n = n.max(i + 1).max(j + 1);
        triplets.push((i, j, d));
    }
    Ok((n, triplets))
}

/// The byte-scanning triplet parser. `None` sends the line to the slow
/// parser, which either accepts it (a non-ASCII separator) or names the
/// field that failed.
fn parse_triplet(line: &str) -> Option<(usize, usize, f64)> {
    if !line.is_ascii() {
        return None;
    }
    let bytes = line.as_bytes();
    let n = bytes.len();
    let skip = |mut i: usize| {
        while i < n && is_separator(bytes[i]) {
            i += 1;
        }
        i
    };
    let (i, pos) = parse_index(bytes, skip(0))?;
    let (j, pos) = parse_index(bytes, skip(pos))?;
    let pos = skip(pos);
    let (d, used) = fast_float2::parse_partial::<f64, _>(&bytes[pos..]).ok()?;
    let pos = pos + used;
    if pos < n && !is_separator(bytes[pos]) {
        return None;
    }
    (skip(pos) == n).then_some((i, j, d))
}

fn parse_triplet_slow(name: &str, lineno: usize, line: &str) -> Result<(usize, usize, f64)> {
    let fields: Vec<&str> = tokens(line).collect();
    if fields.len() != 3 {
        return Err(Error::InvalidInput(format!(
            "{name}:{lineno}: expected 'i j d', got {} fields",
            fields.len()
        )));
    }
    let parse_vertex = |t: &str| {
        t.parse::<usize>()
            .map_err(|_| Error::InvalidInput(format!("{name}:{lineno}: not a vertex index: {t:?}")))
    };
    let i = parse_vertex(fields[0])?;
    let j = parse_vertex(fields[1])?;
    let d = fields[2].parse::<f64>().map_err(|_| {
        Error::InvalidInput(format!("{name}:{lineno}: not a number: {:?}", fields[2]))
    })?;
    Ok((i, j, d))
}
