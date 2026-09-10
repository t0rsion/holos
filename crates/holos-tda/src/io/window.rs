use std::path::Path;

use crate::{Error, Result};

/// Bytes one window of a file holds. On a 36 MiB L3 a window this size is
/// still cache-warm when its parse starts.
const WINDOW_BYTES: usize = 16 << 20;

/// Feed the text of `path` to `on_window`, one window at a time, in file
/// order. A window ends after a newline or at the end of the file, so no
/// line spans two windows. `on_window` gets the 1-based number of the
/// window's first line and the window text, and returns the number of
/// newlines it saw. With `threads` above one, a second thread reads the
/// next window while `on_window` parses the current one; at one thread the
/// reads and the parses alternate.
pub(super) fn for_each_window(
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
pub(super) fn count_newlines(bytes: &[u8]) -> usize {
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
pub(super) struct Windows {
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
    pub(super) fn new(file: std::fs::File, size: usize) -> Self {
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
    pub(super) fn next_into(&mut self, buf: &mut Vec<u8>) -> std::io::Result<bool> {
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
