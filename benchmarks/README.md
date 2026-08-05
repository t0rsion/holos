# Benchmarks

`run.sh` measures holos against [ripser](https://github.com/Ripser/ripser).
It writes every measurement to `results.txt` (full log) and `results.md`
(provenance header plus markdown table). Both files are gitignored. The table
in the top-level README is pasted verbatim from `results.md`. Every number
quoted anywhere must be reproducible by rerunning this script.

The script exits nonzero if any run's diagrams disagree: a timing whose
diagrams do not match is void.

## Usage

```sh
RIPSER_BIN=/path/to/ripser ./run.sh
# CARGO may carry a toolchain: CARGO="cargo +1.92" RIPSER_BIN=... ./run.sh
```

## Methodology

- **Identical inputs.** `gen_cloud.py N 3 42` generates each point cloud
  deterministically (uniform unit cube, stdlib Mersenne Twister). The script
  converts that cloud once to ripser's `lower-distance` format. Both tools
  read the same distance file. Neither tool recomputes distances from
  coordinates.
- **Fair threshold pairing.** The "default" run passes no threshold to either
  tool. Both holos and ripser then fall back to the enclosing radius, so the
  run compares full persistence on equal terms. The "fixed" runs pass the same
  explicit `--threshold` to both tools. One maxdim-2 case (N=500, threshold
  0.4) exercises the dimension-generic path.
- **Single-threaded.** Both binaries are serial. The results record the thread
  count (1).
- **Same machine, same run.** Both tools run back to back in one invocation.
  The results record the CPU model and the date.
- **Wall time and peak RSS** come from `measure.py`, which needs no
  `/usr/bin/time` dependency. Wall clock is `time.monotonic` around the
  process. Peak RSS is the kernel's own high-water mark (`VmHWM` in
  `/proc/PID/status`), sampled every 0.5 ms for the first 20 ms and every
  10 ms after that. A sample counts only once the child's `/proc/PID/cmdline`
  shows the target binary, because before exec the child still maps the parent
  interpreter's image. That image is also why the script does not use `wait4`'s
  `ru_maxrss`: it bakes in a ~13 MB fork floor. The child's stdout goes to a
  file, never a pipe, so a large diagram output cannot fill the 64 KB pipe
  buffer and deadlock. Linux only.
- **Agreement is part of every run.** The script parses both outputs and
  compares them as interval multisets per dimension. The tolerance is 1e-5
  absolute, which matches the precision of ripser's f32 output. Matching is
  greedy over sorted bars, not positional: ripser prints f32-rounded values,
  so near-equal births can sort in a different order than holos's f64 output.
  Each result block carries a `DIAGRAMS_MATCH yes/no` line. Any "no" fails the
  whole run.
- **Build identity.** The results header records the holos git commit, the
  sha256 and path of both binaries, and the `--version` output. It also records
  the exact cargo build command with the `[profile.release]` flags from
  Cargo.toml, the compiler versions, and ripser's compile flags when a Makefile
  sits next to the binary (vendored build). The predecessor project once
  benchmarked a stale binary that was both fast and wrong. A timing that cannot
  be tied to an exact build is worthless. The recorded `cc` version is only a
  proxy for how the ripser binary was built, unless it was built on the same
  machine.
