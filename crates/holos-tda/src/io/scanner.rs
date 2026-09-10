use rayon::prelude::*;

use super::window::count_newlines;

pub(super) fn tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
}

pub(super) fn is_skipped(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The separators the byte scanner recognizes: comma and ASCII whitespace.
/// A line with a byte outside ASCII goes through `tokens`, which also
/// splits on Unicode whitespace, so both paths accept the same grammar.
#[inline]
pub(super) fn is_separator(b: u8) -> bool {
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
pub(super) fn parse_numbers(line: &str, push: impl FnMut(f64)) -> std::result::Result<(), &str> {
    if !line.is_ascii() {
        return parse_unicode_numbers(line, push);
    }
    parse_ascii_numbers(line, push)
}

fn parse_unicode_numbers(line: &str, mut push: impl FnMut(f64)) -> std::result::Result<(), &str> {
    for token in tokens(line) {
        push(token.parse::<f64>().map_err(|_| token)?);
    }
    Ok(())
}

fn parse_ascii_numbers(line: &str, mut push: impl FnMut(f64)) -> std::result::Result<(), &str> {
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
pub(super) const MIN_PARALLEL_BYTES: usize = 1 << 20;

/// The worker count a parse of `text` runs under, given the caller's budget
/// and the smallest text worth splitting.
pub(super) fn parse_workers(text: &str, threads: usize, min_bytes: usize) -> usize {
    if threads > 1 && text.len() >= min_bytes {
        threads
    } else {
        1
    }
}

/// Split `text` into `workers` slices, each ending just after a `\n` or at
/// the end of the text. No line spans two slices. A slice is empty when
/// the text holds fewer lines than workers.
pub(super) fn line_chunks(text: &str, workers: usize) -> Vec<&str> {
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
/// from `first_line`, and the chunk text. Outputs come back in file order.
/// The first error in file order is the one a caller reports. Line numbers
/// cost one pass over the text, which counts the newlines of every chunk.
pub(super) fn map_line_chunks<T: Send>(
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

/// Worker pool for one input, built once. Every reader and every text
/// parser feeds windows through one of these; a text parser feeds one
/// window.
pub(super) struct Parse {
    threads: usize,
    /// The smallest text that splits; tests lower it to split short texts.
    pub(super) min_bytes: usize,
    /// The pool belongs to this parse. The global pool would take every
    /// core, whatever the caller asked for. It builds on the first window
    /// that is large enough to split.
    pool: Option<rayon::ThreadPool>,
}

impl Parse {
    pub(super) fn new(threads: usize) -> Self {
        Self {
            threads,
            min_bytes: MIN_PARALLEL_BYTES,
            pool: None,
        }
    }

    /// Split `text` into line chunks on the pool, or parse it inline as one
    /// chunk when the budget is one or the text is small. Also returns the
    /// newlines in `text`.
    pub(super) fn map<T: Send>(
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
