# Benchmarks

`run.sh` measures holos against [ripser](https://github.com/Ripser/ripser) and
writes everything it measures to `results.txt` (full log) and `results.md`
(provenance header + markdown table), both gitignored. The table in the
top-level README is pasted verbatim from `results.md`; any number quoted
anywhere must be reproducible by rerunning this script.

The script exits nonzero if any run's diagrams disagree: a timing whose
diagrams do not match is void.

## Usage

```sh
RIPSER_BIN=/path/to/ripser ./run.sh
# CARGO may carry a toolchain: CARGO="cargo +1.92" RIPSER_BIN=... ./run.sh
```

## Methodology

- **Identical inputs.** Point clouds are generated deterministically
  (`gen_cloud.py N 3 42`, uniform unit cube, stdlib Mersenne Twister) and
  converted once to ripser's `lower-distance` format. Both tools read the same
  distance file; neither recomputes distances from coordinates.
- **Fair threshold pairing.** The "default" run passes no threshold to either
  tool: both holos and ripser default to the enclosing radius, so this compares
  full persistence on equal terms. The "fixed" runs pass the same explicit
  `--threshold` to both. One maxdim-2 case (N=500, threshold 0.4) exercises the
  dimension-generic path.
- **Single-threaded.** Both binaries are serial; thread count (1) is recorded.
- **Same machine, same run.** Both tools run back to back in one invocation;
  CPU model and date are recorded in the results.
- **Wall time and peak RSS** via `measure.py` (no `/usr/bin/time` dependency):
  wall clock is `time.monotonic` around the process; peak RSS is the kernel's
  own high-water mark (`VmHWM` in `/proc/PID/status`), sampled every 0.5 ms
  for the first 20 ms and every 10 ms after. Samples are accepted only once
  the child's `/proc/PID/cmdline` shows the target binary, because before
  exec the child still maps the parent interpreter's image (which is also why
  `wait4`'s `ru_maxrss` is not used: it bakes in that ~13 MB fork floor).
  The child's stdout is redirected to a file, never a pipe, so large diagram
  outputs cannot fill the 64 KB pipe buffer and deadlock. Linux only.
- **Agreement is part of every run.** Both outputs are parsed and compared as
  interval multisets per dimension (tolerance 1e-5 absolute, matching the
  precision of ripser's f32 output). Matching is greedy over sorted bars, not
  positional: ripser prints f32-rounded values, so near-equal births can sort
  in a different order than holos's f64 output. Each result block carries a
  `DIAGRAMS_MATCH yes/no` line, and any "no" fails the whole run.
- **Build identity.** The results header records the holos git commit, the
  sha256 and path of both binaries, `--version` output, the exact cargo build
  command with the `[profile.release]` flags from Cargo.toml, compiler
  versions, and ripser's compile flags when a Makefile sits next to the binary
  (vendored build). This exists because the predecessor project once
  benchmarked a stale binary that was both fast and wrong; a timing that
  cannot be tied to an exact build is worthless. (The recorded `cc` version is
  only a proxy for how the ripser binary was built, unless it was built on the
  same machine.)
