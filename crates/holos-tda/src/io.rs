//! Readers for point clouds and for dense or sparse distance matrices.
//! Writers for the diagram output formats.

use std::io::Write;
use std::path::Path;

use rayon::prelude::*;

use crate::{Diagram, DistanceMatrix, Error, Result, SparseDistanceMatrix};

/// Bytes one window of a file holds. A reader parses one window at a time,
/// so it holds one window and the parsed values, not the whole text. On a
/// 36 MiB L3 a window this size is still cache-warm when its parse starts.
const WINDOW_BYTES: usize = 16 << 20;

/// Feed the text of `path` to `on_window`, one window at a time, in file
/// order. A window ends after a newline or at the end of the file, so no
/// line spans two windows. `on_window` gets the 1-based number of the
/// window's first line and the window text, and returns the number of
/// newlines it saw, which the parse counts anyway. With `threads` above one, a
/// second thread reads the next window while `on_window` parses the
/// current one; at one thread the reads and the parses alternate.
fn for_each_window(
    path: &Path,
    threads: usize,
    mut on_window: impl FnMut(usize, &str) -> Result<usize>,
) -> Result<()> {
    let io_err = |e: std::io::Error| Error::Io(format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(io_err)?;
    let mut windows = Windows::new(file, WINDOW_BYTES);
    let mut first_line = 1;
    let mut consume = |buf: &[u8]| -> Result<()> {
        // The same message `std::fs::read_to_string` gives on such bytes.
        let text = std::str::from_utf8(buf).map_err(|_| {
            Error::Io(format!(
                "{}: stream did not contain valid UTF-8",
                path.display()
            ))
        })?;
        first_line += on_window(first_line, text)?;
        Ok(())
    };
    if threads <= 1 {
        let mut buf = Vec::new();
        while windows.next_into(&mut buf).map_err(io_err)? {
            consume(&buf)?;
        }
        return Ok(());
    }
    // Two buffers rotate between the reader thread and this one. Every
    // window crosses the channel once and comes back empty, so no window is
    // allocated twice.
    let (full_tx, full_rx) = std::sync::mpsc::sync_channel::<std::io::Result<Vec<u8>>>(1);
    let (empty_tx, empty_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(2);
    for _ in 0..2 {
        empty_tx.send(Vec::new()).expect("the receiver is alive");
    }
    std::thread::scope(|scope| {
        scope.spawn(move || {
            while let Ok(mut buf) = empty_rx.recv() {
                match windows.next_into(&mut buf) {
                    Ok(true) => {
                        if full_tx.send(Ok(buf)).is_err() {
                            return;
                        }
                    }
                    Ok(false) => return,
                    Err(e) => {
                        let _ = full_tx.send(Err(e));
                        return;
                    }
                }
            }
        });
        // Both channel ends this thread holds drop with the closure, so an
        // early return frees a reader blocked on either channel.
        let empty_tx = empty_tx;
        for buf in full_rx {
            let buf = buf.map_err(io_err)?;
            consume(&buf)?;
            // A closed return channel means the reader has finished.
            let _ = empty_tx.send(buf);
        }
        Ok(())
    })
}

/// Append up to `size` bytes of `file` to `buf`, in reads the size of the
/// space left, and return how many arrived. Fewer than `size` means the end
/// of the file.
fn read_up_to(file: &mut std::fs::File, buf: &mut Vec<u8>, size: usize) -> std::io::Result<usize> {
    use std::io::Read;
    let start = buf.len();
    buf.resize(start + size, 0);
    let mut filled = start;
    while filled < start + size {
        match file.read(&mut buf[filled..start + size]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                buf.truncate(start);
                return Err(e);
            }
        }
    }
    buf.truncate(filled);
    Ok(filled - start)
}

/// The newlines in `bytes`, eight bytes at a time.
fn count_newlines(bytes: &[u8]) -> usize {
    let mut chunks = bytes.chunks_exact(8);
    let mut count = 0usize;
    for chunk in &mut chunks {
        // Zero the newline bytes, then set the high bit of exactly the
        // nonzero bytes: no carry crosses a byte because the low seven bits
        // are added first, so a zero byte, and only a zero byte, ends up
        // with its high bit clear.
        let word = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
        let x = word ^ 0x0a0a_0a0a_0a0a_0a0a;
        let nonzero = ((x & 0x7f7f_7f7f_7f7f_7f7f) + 0x7f7f_7f7f_7f7f_7f7f) | x;
        count += (!nonzero & 0x8080_8080_8080_8080).count_ones() as usize;
    }
    count + chunks.remainder().iter().filter(|&&b| b == b'\n').count()
}

/// A file cut into windows that end at newlines.
struct Windows {
    file: std::fs::File,
    /// Bytes a window holds before it is cut at its last newline.
    size: usize,
    /// Bytes the file reported it still holds, so a small file gets a
    /// small buffer instead of a whole window zeroed for it. A file that
    /// grows past this is still read to its end.
    remaining: u64,
    /// Bytes read past the last newline of the previous window; they start
    /// the next one.
    carry: Vec<u8>,
    done: bool,
}

impl Windows {
    fn new(file: std::fs::File, size: usize) -> Self {
        let remaining = file.metadata().map_or(size as u64, |m| m.len());
        Self {
            file,
            size,
            remaining,
            carry: Vec::new(),
            done: false,
        }
    }

    /// Fill `buf` with the next window. Returns false at the end of the
    /// file, with `buf` empty.
    fn next_into(&mut self, buf: &mut Vec<u8>) -> std::io::Result<bool> {
        buf.clear();
        if self.done {
            return Ok(false);
        }
        buf.append(&mut self.carry);
        // Read one window's worth on top of the carry, and one more each
        // time no newline appears, which happens only on a line longer than
        // the window.
        loop {
            // One byte past the reported length, so a file of exactly that
            // length ends the read here instead of on an empty read later.
            let want = usize::try_from(self.remaining.saturating_add(1))
                .unwrap_or(self.size)
                .clamp(1, self.size);
            let got = read_up_to(&mut self.file, buf, want)?;
            self.remaining = self.remaining.saturating_sub(got as u64);
            if got < want {
                self.done = true;
                return Ok(!buf.is_empty());
            }
            if let Some(pos) = buf.iter().rposition(|&b| b == b'\n') {
                self.carry.extend_from_slice(&buf[pos + 1..]);
                buf.truncate(pos + 1);
                return Ok(true);
            }
        }
    }
}

fn tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
}

fn is_skipped(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The separators the byte scanner recognizes: comma and ASCII whitespace.
/// A line with a byte outside ASCII goes through `tokens`, which also
/// splits on Unicode whitespace, so both paths accept the same grammar.
#[inline]
fn is_separator(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b',' | b'\r' | 0x0b | 0x0c)
}

/// The end of the token that starts at `start` in an ASCII line.
fn token_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && !is_separator(bytes[end]) {
        end += 1;
    }
    end
}

/// Parse every number on `line` in order and hand each to `push`. Returns
/// the offending token when one is not a number. The ASCII path scans bytes
/// and parses in place; it gives the same bits as `str::parse::<f64>`.
fn parse_numbers(line: &str, mut push: impl FnMut(f64)) -> std::result::Result<(), &str> {
    if !line.is_ascii() {
        for t in tokens(line) {
            push(t.parse::<f64>().map_err(|_| t)?);
        }
        return Ok(());
    }
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        while i < n && is_separator(bytes[i]) {
            i += 1;
        }
        if i == n {
            break;
        }
        match fast_float2::parse_partial::<f64, _>(&bytes[i..]) {
            Ok((value, used)) if i + used == n || is_separator(bytes[i + used]) => {
                push(value);
                i += used;
            }
            _ => return Err(&line[i..token_end(bytes, i)]),
        }
    }
    Ok(())
}

/// The smallest text a parse splits across workers. A split costs a pool
/// build and one pass that counts the newlines of every chunk. Measured on
/// this machine's E-cores with `engine-bench`, condensed input, two workers
/// against serial: 1.48 ms to 0.89 ms at one mebibyte, and 0.168 ms to
/// 0.149 ms at 128 kibibytes. The smaller file saves too little to pay for
/// the split, so the parse takes it serially.
const MIN_PARALLEL_BYTES: usize = 1 << 20;

/// The worker count a parse of `text` runs under, given the caller's budget
/// and the smallest text worth splitting.
fn parse_workers(text: &str, threads: usize, min_bytes: usize) -> usize {
    if threads > 1 && text.len() >= min_bytes {
        threads
    } else {
        1
    }
}

/// Split `text` into `workers` slices, each ending just after a `\n` or at
/// the end of the text. No line spans two slices, so a slice parses on its
/// own. A slice is empty when the text holds fewer lines than workers.
fn line_chunks(text: &str, workers: usize) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut chunks = Vec::with_capacity(workers);
    let mut start = 0;
    for k in 1..workers {
        let mut end = (bytes.len() / workers * k).max(start);
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }
        if end < bytes.len() {
            end += 1;
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks.push(&text[start..]);
    chunks
}

/// Split `text` into `workers` line chunks and map `parse_chunk` over them
/// on `pool`, or inline when `pool` is `None`.
///
/// `parse_chunk` takes the 1-based number of the chunk's first line, counted
/// from `first_line`, and the chunk text, so it names the lines a serial
/// parse of the whole text names. The outputs come back in file order. Read
/// them in that order and the first error in file order is the one you
/// report. The line numbers cost one pass over the text, which counts the
/// newlines of every chunk.
fn map_line_chunks<T: Send>(
    pool: Option<&rayon::ThreadPool>,
    text: &str,
    first_line: usize,
    workers: usize,
    parse_chunk: impl Fn(usize, &str) -> T + Send + Sync,
) -> (Vec<T>, usize) {
    let chunks = line_chunks(text, workers);
    let newlines = |chunk: &&str| count_newlines(chunk.as_bytes());
    let counts: Vec<usize> = match pool {
        Some(pool) => pool.install(|| chunks.par_iter().map(newlines).collect()),
        None => chunks.iter().map(newlines).collect(),
    };
    let mut lineno = first_line;
    let first_lines: Vec<usize> = counts
        .iter()
        .map(|count| {
            let first = lineno;
            lineno += count;
            first
        })
        .collect();
    let results = match pool {
        Some(pool) => pool.install(|| {
            chunks
                .par_iter()
                .zip(&first_lines)
                .map(|(chunk, &first)| parse_chunk(first, chunk))
                .collect()
        }),
        None => chunks
            .iter()
            .zip(&first_lines)
            .map(|(chunk, &first)| parse_chunk(first, chunk))
            .collect(),
    };
    (results, lineno - first_line)
}

/// The parse of one input: a pool of the caller's workers, built once, and
/// the values gathered so far. Every reader and every text parser feeds
/// windows to one of these; a text parser feeds one window.
struct Parse {
    threads: usize,
    /// The smallest text that splits; tests lower it to split short texts.
    min_bytes: usize,
    /// The pool belongs to this parse. The global pool would take every
    /// core, whatever the caller asked for. It builds on the first window
    /// that is large enough to split.
    pool: Option<rayon::ThreadPool>,
}

impl Parse {
    fn new(threads: usize) -> Self {
        Self {
            threads,
            min_bytes: MIN_PARALLEL_BYTES,
            pool: None,
        }
    }

    /// Split `text` into line chunks on the pool, or parse it inline as one
    /// chunk when the budget is one or the text is small. Also returns the
    /// newlines in `text`.
    fn map<T: Send>(
        &mut self,
        text: &str,
        first_line: usize,
        parse_chunk: impl Fn(usize, &str) -> T + Send + Sync,
    ) -> (Vec<T>, usize) {
        let workers = parse_workers(text, self.threads, self.min_bytes);
        if workers <= 1 {
            let result = parse_chunk(first_line, text);
            return (vec![result], count_newlines(text.as_bytes()));
        }
        if self.pool.is_none() {
            // A pool this small should always build; if it does not, the
            // same chunks parse here and give the same answer.
            self.pool = rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .ok();
        }
        map_line_chunks(self.pool.as_ref(), text, first_line, workers, parse_chunk)
    }
}

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
/// chunk per thread. The points and the error message are the same either
/// way.
pub fn parse_point_cloud(name: &str, text: &str, threads: usize) -> Result<Vec<Vec<f64>>> {
    let mut sink = PointSink::new(threads);
    sink.window(name, 1, text)?;
    Ok(sink.points)
}

/// The points gathered from the windows of one input.
struct PointSink {
    parse: Parse,
    points: Vec<Vec<f64>>,
    /// The width of the first point of the file, once one is read.
    width: Option<usize>,
}

impl PointSink {
    fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            points: Vec::new(),
            width: None,
        }
    }

    fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
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
    /// The line the chunk stopped on. Its points end just before that line.
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
/// `threads` is the worker budget for the parse. With more than one thread
/// and a text of at least one mebibyte, the parse splits into one line
/// chunk per thread. The numbers and the error message are the same either
/// way. A file that holds all its numbers on one line stays serial, because
/// a chunk ends at a newline.
pub fn parse_condensed(name: &str, text: &str, threads: usize) -> Result<Vec<f64>> {
    let mut sink = CondensedSink::new(threads);
    sink.window(name, 1, text)?;
    Ok(sink.data)
}

/// The numbers gathered from the windows of one condensed input.
struct CondensedSink {
    parse: Parse,
    data: Vec<f64>,
}

impl CondensedSink {
    fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            data: Vec::new(),
        }
    }

    fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
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
/// `threads` is the worker budget for the parse. With more than one thread
/// and a text of at least one mebibyte, the parse splits into one line
/// chunk per thread. The triplets and the error message are the same either
/// way.
pub fn parse_triplets(name: &str, text: &str, threads: usize) -> Result<(usize, Vec<Triplet>)> {
    let mut sink = TripletSink::new(threads);
    sink.window(name, 1, text)?;
    Ok((sink.n, sink.triplets))
}

/// The triplets gathered from the windows of one sparse input.
struct TripletSink {
    parse: Parse,
    n: usize,
    triplets: Vec<Triplet>,
}

impl TripletSink {
    fn new(threads: usize) -> Self {
        Self {
            parse: Parse::new(threads),
            n: 0,
            triplets: Vec::new(),
        }
    }

    fn window(&mut self, name: &str, first_line: usize, text: &str) -> Result<usize> {
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

/// The text parsers with `workers` forced, whatever the text size. Tests use
/// these to split short texts.
#[cfg(test)]
fn parse_condensed_in(name: &str, text: &str, workers: usize) -> Result<Vec<f64>> {
    let mut sink = CondensedSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok(sink.data)
}

#[cfg(test)]
fn parse_point_cloud_in(name: &str, text: &str, workers: usize) -> Result<Vec<Vec<f64>>> {
    let mut sink = PointSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok(sink.points)
}

#[cfg(test)]
fn parse_triplets_in(name: &str, text: &str, workers: usize) -> Result<(usize, Vec<Triplet>)> {
    let mut sink = TripletSink::new(workers);
    sink.parse.min_bytes = 0;
    sink.window(name, 1, text)?;
    Ok((sink.n, sink.triplets))
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
        let points = read_point_cloud(f.path(), 1).unwrap();
        assert_eq!(points, vec![vec![0.0, 1.0], vec![2.0, 3.0], vec![4.0, 5.0]]);
    }

    #[test]
    fn point_cloud_skips_comments_and_blanks() {
        let f = TempFile::new(
            "pc_comments.csv",
            "# header comment\n\n1.0 2.0\n   \n# mid comment\n3.0 4.0\n",
        );
        let points = read_point_cloud(f.path(), 1).unwrap();
        assert_eq!(points, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    }

    #[test]
    fn point_cloud_inconsistent_dimension_reports_line() {
        let f = TempFile::new("pc_baddim.csv", "1.0 2.0\n\n1.0 2.0 3.0\n");
        let err = read_point_cloud(f.path(), 1).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
        assert!(msg.contains(":3:"), "missing line number: {msg}");
        assert!(msg.contains("3 coordinates"), "{msg}");
        assert!(msg.contains("expected 2"), "{msg}");
    }

    #[test]
    fn point_cloud_bad_token_reports_line() {
        let f = TempFile::new("pc_badtok.csv", "1.0 2.0\n1.0 oops\n");
        let err = read_point_cloud(f.path(), 1).unwrap_err();
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
        let err = read_point_cloud(&path, 1).unwrap_err();
        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("does_not_exist"));
    }

    #[test]
    fn lower_distance_round_trip() {
        // n = 3 condensed lower triangle: d(1,0), d(2,0), d(2,1).
        let f = TempFile::new("ld_roundtrip.lower", "1.5 2.5, 3.5\n");
        let m = read_lower_distance_matrix(f.path(), 1).unwrap();
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
        let m = read_lower_distance_matrix(f.path(), 1).unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m.get(3, 2), 6.0);
    }

    #[test]
    fn lower_distance_bad_length_errors() {
        let f = TempFile::new("ld_badlen.lower", "1 2\n");
        let err = read_lower_distance_matrix(f.path(), 1).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, Error::InvalidInput(_)), "{msg}");
        assert!(msg.contains("condensed length"), "{msg}");
    }

    #[test]
    fn lower_distance_bad_token_reports_line() {
        let f = TempFile::new("ld_badtok.lower", "1.0\n2.0 x\n");
        let err = read_lower_distance_matrix(f.path(), 1).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(":2:"), "missing line number: {msg}");
        assert!(msg.contains('x'), "{msg}");
    }

    fn slow_numbers(text: &str) -> Vec<f64> {
        text.lines()
            .filter(|l| !is_skipped(l))
            .flat_map(|l| tokens(l).map(|t| t.parse::<f64>().unwrap()))
            .collect()
    }

    #[test]
    fn scanner_matches_str_parse_bit_for_bit() {
        let text = "0.5098973069436853, 1e-320 -0.0\n+3.25e2\t.5 5. inf -Infinity NaN\n\
                    17976931348623157e292 2.2250738585072014e-308 4.9e-324 1e400\n";
        let fast = parse_condensed("t", text, 1).unwrap();
        let slow = slow_numbers(text);
        assert_eq!(fast.len(), slow.len());
        for (a, b) in fast.iter().zip(&slow) {
            assert!(
                a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
                "{a} vs {b}"
            );
        }
    }

    #[test]
    fn scanner_rejects_what_str_parse_rejects() {
        for bad in [
            "1e", "1_000", "0x10", "1.2.3", "--1", "1e5x", "-", "+", "nanx", "1,,x",
        ] {
            let text = format!("1.0\n{bad}\n");
            let err = parse_condensed("t", &text, 1).unwrap_err().to_string();
            assert!(err.contains("t:2: not a number"), "{bad}: {err}");
        }
    }

    #[test]
    fn scanner_falls_back_on_unicode_whitespace() {
        // U+00A0 is whitespace to `char::is_whitespace`, so both paths split on it.
        let text = "1.0\u{a0}2.0\n3.0\n";
        assert_eq!(parse_condensed("t", text, 1).unwrap(), vec![1.0, 2.0, 3.0]);
        let (n, t) = parse_triplets("t", "0\u{a0}1 0.5\n", 1).unwrap();
        assert_eq!((n, t), (2, vec![(0, 1, 0.5)]));
    }

    #[test]
    fn triplet_scanner_accepts_the_slow_grammar() {
        let (n, t) = parse_triplets("t", "# c\n\n 3,1 0.25\r\n+2 0 1e-3\n", 1).unwrap();
        assert_eq!(n, 4);
        assert_eq!(t, vec![(3, 1, 0.25), (2, 0, 1e-3)]);
        for (bad, msg) in [
            ("1 2", "expected 'i j d', got 2 fields"),
            ("1 2 3 4", "expected 'i j d', got 4 fields"),
            ("-1 2 0.5", "not a vertex index: \"-1\""),
            ("1 x 0.5", "not a vertex index: \"x\""),
            ("1 2 abc", "not a number: \"abc\""),
            ("99999999999999999999999 2 0.5", "not a vertex index"),
        ] {
            let err = parse_triplets("t", bad, 1).unwrap_err().to_string();
            assert!(err.contains(&format!("t:1: {msg}")), "{bad}: {err}");
        }
    }

    proptest::proptest! {
        #[test]
        fn scanner_matches_str_parse_on_random_text(
            values in proptest::collection::vec(proptest::num::f64::ANY, 0..40),
            seps in proptest::collection::vec(0u8..4, 0..40),
        ) {
            let mut text = String::new();
            for (i, v) in values.iter().enumerate() {
                let sep = match seps.get(i).copied().unwrap_or(0) { 0 => " ", 1 => ",", 2 => "\n", _ => "\t," };
                if i % 3 == 0 { text.push_str(&format!("{v:e}")); } else { text.push_str(&v.to_string()); }
                text.push_str(sep);
            }
            let fast = parse_condensed("t", &text, 1).unwrap();
            let slow = slow_numbers(&text);
            proptest::prop_assert_eq!(fast.len(), slow.len());
            for (a, b) in fast.iter().zip(&slow) {
                proptest::prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
            }
        }
    }

    /// Forty lines of two coordinates each, with `bad` replaced by `line`.
    fn lines_with(bad: usize, line: &str) -> String {
        let mut text = String::new();
        for lineno in 1..=40 {
            if lineno == bad {
                text.push_str(line);
            } else {
                text.push_str("1.0 2.0");
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn chunked_condensed_reports_the_serial_error() {
        // Line 1 lands in the first chunk of four, line 20 in the middle,
        // line 40 in the last.
        for bad in [1, 20, 40] {
            let text = lines_with(bad, "1.0 oops");
            let serial = parse_condensed_in("t", &text, 1).unwrap_err().to_string();
            let chunked = parse_condensed_in("t", &text, 4).unwrap_err().to_string();
            assert_eq!(serial, chunked);
            assert!(
                serial.contains(&format!("t:{bad}: not a number")),
                "{serial}"
            );
        }
    }

    #[test]
    fn chunked_condensed_reports_the_first_error_in_file_order() {
        let mut text = String::new();
        for lineno in 1..=40 {
            match lineno {
                12 => text.push_str("1.0 first\n"),
                31 => text.push_str("1.0 second\n"),
                _ => text.push_str("1.0 2.0\n"),
            }
        }
        for workers in [1, 2, 4, 8] {
            let err = parse_condensed_in("t", &text, workers)
                .unwrap_err()
                .to_string();
            assert_eq!(
                err, "invalid input: t:12: not a number: \"first\"",
                "{workers}"
            );
        }
    }

    #[test]
    fn chunked_condensed_accepts_the_serial_grammar() {
        // CRLF, a comment, blank and separator-only lines, a line the byte
        // scanner hands to the Unicode fallback, and no final newline.
        let text = "# comment\r\n1.0, 2.0\r\n\r\n3.0\u{a0}4.0\n   \n\t,\n5.0 6.0";
        let serial = parse_condensed_in("t", text, 1).unwrap();
        assert_eq!(serial, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(parse_condensed_in("t", text, 4).unwrap(), serial);
    }

    #[test]
    fn chunked_point_cloud_accepts_the_serial_grammar() {
        let text = "# comment\r\n1.0, 2.0\r\n\r\n3.0\u{a0}4.0\n   \n\t,\n5.0 6.0";
        let serial = parse_point_cloud_in("t", text, 1).unwrap();
        assert_eq!(serial, vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
        assert_eq!(parse_point_cloud_in("t", text, 4).unwrap(), serial);
    }

    #[test]
    fn chunked_point_cloud_reports_the_serial_width_error() {
        // The wrong width can precede a bad token inside one chunk, and it
        // can be the chunk's first point, which no chunk-local comparison
        // sees. Sweep both lines over every position.
        for wide in 1..=40 {
            let mut text = String::new();
            for lineno in 1..=40 {
                if lineno == wide {
                    text.push_str("1.0 2.0 3.0\n");
                } else if lineno == wide + 4 {
                    text.push_str("1.0 oops\n");
                } else {
                    text.push_str("1.0 2.0\n");
                }
            }
            let serial = parse_point_cloud_in("t", &text, 1);
            for workers in [2, 4, 8] {
                let chunked = parse_point_cloud_in("t", &text, workers);
                match (&serial, &chunked) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "wide {wide}, workers {workers}"),
                    (Err(a), Err(b)) => {
                        assert_eq!(
                            a.to_string(),
                            b.to_string(),
                            "wide {wide}, workers {workers}"
                        )
                    }
                    _ => panic!("wide {wide}, workers {workers}: paths disagree"),
                }
            }
        }
    }

    #[test]
    fn chunked_triplets_match_the_serial_parse() {
        let mut text = String::from("# comment\n\n");
        for i in 0..40u32 {
            text.push_str(&format!("{i} {} 0.{i}\r\n", i + 1));
        }
        text.push_str("0\u{a0}7 0.5\n41 3 0.25");
        let serial = parse_triplets_in("t", &text, 1).unwrap();
        assert_eq!(serial.0, 42);
        for workers in [2, 4, 8] {
            assert_eq!(parse_triplets_in("t", &text, workers).unwrap(), serial);
        }
    }

    #[test]
    fn chunked_triplets_report_the_serial_error() {
        for bad in [1, 20, 40] {
            let mut text = String::new();
            for lineno in 1..=40u32 {
                if lineno == bad {
                    text.push_str("3 x 0.5\n");
                } else {
                    text.push_str("1 2 0.5\n");
                }
            }
            let serial = parse_triplets_in("t", &text, 1).unwrap_err().to_string();
            assert_eq!(
                parse_triplets_in("t", &text, 4).unwrap_err().to_string(),
                serial
            );
            assert!(
                serial.contains(&format!("t:{bad}: not a vertex index")),
                "{serial}"
            );
        }
    }

    #[test]
    fn the_public_parse_splits_a_text_over_the_threshold() {
        let mut text = String::with_capacity(MIN_PARALLEL_BYTES + 64);
        while text.len() < MIN_PARALLEL_BYTES {
            text.push_str("0.5098973069436853 1.25e-3 -7\n");
        }
        assert_eq!(parse_workers(&text, 4, MIN_PARALLEL_BYTES), 4);
        let serial = parse_condensed("t", &text, 1).unwrap();
        for threads in [2, 4, 8] {
            let parallel = parse_condensed("t", &text, threads).unwrap();
            assert_eq!(serial.len(), parallel.len());
            assert!(
                serial
                    .iter()
                    .zip(&parallel)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "{threads} workers"
            );
        }
        text.push_str("1.0 nope\n");
        assert_eq!(
            parse_condensed("t", &text, 4).unwrap_err().to_string(),
            parse_condensed("t", &text, 1).unwrap_err().to_string()
        );
    }

    #[test]
    fn a_small_text_stays_serial() {
        assert_eq!(parse_workers("1.0 2.0\n", 8, MIN_PARALLEL_BYTES), 1);
        assert_eq!(parse_workers("1.0 2.0\n", 1, MIN_PARALLEL_BYTES), 1);
    }

    #[test]
    fn line_chunks_cover_the_text_and_end_at_newlines() {
        for text in ["", "\n", "a\nb\n", "a\nb", "abc", "a\n\n\nb\n"] {
            for workers in [2, 3, 8] {
                let chunks = line_chunks(text, workers);
                assert_eq!(chunks.concat(), text, "{text:?} {workers}");
                let last = chunks.iter().rposition(|chunk| !chunk.is_empty());
                for (k, chunk) in chunks.iter().enumerate() {
                    assert!(
                        chunk.is_empty() || chunk.ends_with('\n') || Some(k) == last,
                        "{text:?} {workers}: {chunk:?}"
                    );
                }
                let split: Vec<&str> = chunks.iter().flat_map(|c| c.lines()).collect();
                let whole: Vec<&str> = text.lines().collect();
                assert_eq!(split, whole, "{text:?} {workers}");
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn chunked_parse_matches_serial_on_random_text(
            values in proptest::collection::vec(proptest::num::f64::ANY, 0..40),
            seps in proptest::collection::vec(0u8..5, 0..40),
            kinds in proptest::collection::vec(0u8..8, 0..40),
        ) {
            let mut text = String::new();
            for (i, v) in values.iter().enumerate() {
                match kinds.get(i).copied().unwrap_or(3) {
                    0 => text.push_str("# comment"),
                    1 => text.push_str("oops"),
                    2 => text.push_str(&format!("{v:e}")),
                    _ => text.push_str(&v.to_string()),
                }
                text.push_str(match seps.get(i).copied().unwrap_or(0) {
                    0 => " ",
                    1 => ",",
                    2 => "\n",
                    3 => "\r\n",
                    _ => "\t",
                });
            }
            match (parse_condensed_in("t", &text, 1), parse_condensed_in("t", &text, 3)) {
                (Ok(serial), Ok(chunked)) => {
                    proptest::prop_assert_eq!(serial.len(), chunked.len());
                    for (a, b) in serial.iter().zip(&chunked) {
                        proptest::prop_assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
                    }
                }
                (Err(serial), Err(chunked)) => {
                    proptest::prop_assert_eq!(serial.to_string(), chunked.to_string());
                }
                _ => proptest::prop_assert!(false, "one path failed and the other did not"),
            }
        }
    }

    #[test]
    fn newline_count_matches_the_scalar_count() {
        let mut text = Vec::new();
        for i in 0..1000u32 {
            text.push(if i % 3 == 0 { b'\n' } else { (i % 251) as u8 });
        }
        for end in [0, 1, 7, 8, 9, 15, 16, 17, 100, 999, 1000] {
            let slice = &text[..end];
            assert_eq!(
                count_newlines(slice),
                slice.iter().filter(|&&b| b == b'\n').count()
            );
        }
        assert_eq!(count_newlines(&[0x0b; 16]), 0);
        assert_eq!(count_newlines(&[0x8a; 16]), 0);
        assert_eq!(count_newlines(&[0x0a; 16]), 16);
    }

    /// The window reader against the one-window parse, at a window of 64
    /// bytes so a small file crosses many windows: a line longer than the
    /// window, a missing final newline, and a bad token in a late window.
    #[test]
    fn windows_give_the_one_window_parse() {
        let mut text = String::new();
        for i in 0..400 {
            text.push_str(&format!("{}.5 {}e-3\n", i, i * 7));
        }
        text.push_str(&"1.0 ".repeat(100));
        text.push_str("\n# comment\n\n2.0 3.0");
        let f = TempFile::new("win_ok.lower", &text);
        let file = std::fs::File::open(f.path()).unwrap();
        let mut windows = Windows::new(file, 64);
        let mut joined = Vec::new();
        let mut buf = Vec::new();
        let mut count = 0;
        while windows.next_into(&mut buf).unwrap() {
            assert!(buf.ends_with(b"\n") || joined.len() + buf.len() == text.len());
            joined.extend_from_slice(&buf);
            count += 1;
        }
        assert_eq!(joined, text.as_bytes());
        assert!(count > 10, "{count} windows");
        for threads in [1, 3] {
            let mut sink = CondensedSink::new(threads);
            let mut windows = Windows::new(std::fs::File::open(f.path()).unwrap(), 64);
            let mut first_line = 1;
            let mut buf = Vec::new();
            while windows.next_into(&mut buf).unwrap() {
                let chunk = std::str::from_utf8(&buf).unwrap();
                sink.window("t", first_line, chunk).unwrap();
                first_line += buf.iter().filter(|&&b| b == b'\n').count();
            }
            assert_eq!(sink.data, parse_condensed("t", &text, 1).unwrap());
        }
        let bad = format!("{text}\n7 8 x\n");
        let f = TempFile::new("win_bad.lower", &bad);
        for threads in [1, 4] {
            let err = read_lower_distance_matrix(f.path(), threads)
                .unwrap_err()
                .to_string();
            let serial = parse_condensed("t", &bad, 1).unwrap_err().to_string();
            assert_eq!(err.split(':').nth(2), serial.split(':').nth(2), "{err}");
            assert!(err.contains(":405: not a number: \"x\""), "{err}");
        }
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
