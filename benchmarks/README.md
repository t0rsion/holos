# Benchmarks

`run.sh` measures holos against [ripser](https://github.com/Ripser/ripser).
It writes every measurement to `results.txt` (the full log) and `results.md`
(a provenance header and a markdown table). Both files are gitignored. The
table in the top-level README is pasted verbatim from `results.md`. A rerun of
this script must reproduce every number quoted anywhere.

If the diagrams of any run disagree, the script exits nonzero. A timing whose
diagrams do not match is void.

## Usage

```sh
RIPSER_BIN=/path/to/ripser ./run.sh
# CARGO may carry a toolchain: CARGO="cargo +1.92" RIPSER_BIN=... ./run.sh
```

## Methodology

- Identical inputs. `gen_cloud.py N 3 42` generates each point cloud
  deterministically, uniform in the unit cube, from the stdlib Mersenne
  Twister. The script converts that cloud once to ripser's `lower-distance`
  format. Both tools read the same distance file. Neither tool recomputes
  distances from coordinates.
- Fair threshold pairing. The "default" run passes no threshold to either
  tool. holos and ripser then both fall back to the enclosing radius, so the
  run compares full persistence on equal terms. The "fixed" runs pass the same
  explicit `--threshold` to both tools. One maxdim-2 case (N=500, threshold
  0.4) exercises the dimension-generic path.
- Single-threaded. Both binaries are serial. The results record the thread
  count (1).
- Same machine, same run. Both tools run back to back in one invocation. The
  results record the CPU model and the date.
- Wall time and peak RSS come from `measure.py`, which does not need
  `/usr/bin/time`. The wall clock is `time.monotonic` around the process. Peak
  RSS is the kernel's own high-water mark, `VmHWM` in `/proc/PID/status`,
  sampled every 0.5 ms for the first 20 ms and every 10 ms after that. A
  sample counts only once the child's `/proc/PID/cmdline` shows the target
  binary, because before exec the child still maps the parent interpreter's
  image. That image is also why the script does not use `wait4`'s
  `ru_maxrss`: it bakes in a fork floor of about 13 MB. The child's stdout
  goes to a file, never a pipe, so a large diagram output cannot fill the
  64 KB pipe buffer and deadlock. Linux only.
- Agreement is part of every run. The script parses both outputs and compares
  them as interval multisets per dimension. The tolerance is 1e-5 absolute,
  which matches the precision of ripser's f32 output. Matching is greedy over
  sorted bars, not positional: ripser prints f32-rounded values, so near-equal
  births can sort in a different order than holos's f64 output. Each result
  block carries a `DIAGRAMS_MATCH yes/no` line. Any "no" fails the whole run.
- Build identity. The results header records the holos git commit, the sha256
  and path of both binaries, and the `--version` output. It also records the
  exact cargo build command with the `[profile.release]` flags from
  Cargo.toml, the compiler versions, and ripser's compile flags when a
  Makefile sits next to the binary (a vendored build). The predecessor project
  once benchmarked a stale binary that was both fast and wrong. A timing that
  cannot be tied to an exact build is worthless. The recorded `cc` version is
  only a proxy for how the ripser binary was built, unless it was built on the
  same machine.

## Preregistration: edge-collapse break-even study

This section fixes what gets measured and what each outcome may be called, so
a result cannot pick its own criterion afterwards. It was written and frozen
before any run of this registered corpus, screen or confirmation. The corpus
is `collapse_corpus.toml`, version 3, dated 2026-08-05. `collapse_bench.sh`
runs it.

The question is whether removing dominated edges before the reduction pays
for the time it costs.

### Version 2 and the timings that preceded it

Version 2 amends version 1 of the same date. Nothing in this corpus had run
under the protocol when the amendment was made, neither a screen entry nor a
confirmation entry, so no result depends on the version it replaces. Four
things changed.

The disclosure was wrong. Version 1 said this section "was written before any
collapse timing existed". That was false. Informal timings during development
came first:

| family | n | threshold |
|:--|--:|:--|
| torus | 400 | enclosing radius |
| clusters | 300 | enclosing radius |
| sphere | 300 | enclosing radius |
| cube | 800 | tau 0.3 |

The first three guided the collapser's optimization. The fourth came from one
smoke invocation that tested the runner's plumbing, at the configuration
screen entry `a-cube-t03` also uses. All four shaped which families, sizes,
and tau values the corpus holds. None of them is evidence for anything. Their
numbers are recorded nowhere, and they came from builds that no longer exist.
The decision rule below reads confirmation entries only. The claim this
section can make is the narrower one at the top: written and frozen before any
run of this registered corpus. The screen entry the smoke run touched is not
held out, and it runs again from scratch.

The decision rule ignored mode 2. Version 1 graded on the mode-3 speedup over
mode 1 alone, although the same section says mode 2 exists to separate the
sparse enumerator from the collapse. A rule that never reads mode 2 can call a
pure enumerator win a collapse win. The rule below reads both, and names the
enumerator case for what it is.

"Adjacent on one axis" was undefined. Version 1 asked for confirmation entries
adjacent on one axis, taken in the order tau, then n, then threads. No pair of
confirmation entries can satisfy that: they move tau and n together, so a
region could never form. The corpus now declares an ordered chain per family,
and the rule below reads consecutive positions on a chain.

The held-out set was not enforced. Version 1 let `--confirm` start as soon as
a screen results file was nonempty, which one `touch` satisfies. The screen
now leaves a manifest, and `--confirm` checks it.

### Families and grids

`gen_cloud.py` generates four families. Each one is seeded, stdlib only, and
byte-identical on rerun:

| family | shape |
|:--|:--|
| cube | uniform in the unit cube |
| sphere | uniform on the sphere of radius 0.5 |
| clusters | 8 Gaussian clusters, sigma 0.05 |
| torus | uniform on a torus, major radius 0.35, minor radius 0.15 |

Axes: n, ambient dimension (3 and 5), max_dim (1 and 2), threshold fraction
tau in {0.3, 0.5, 0.8, 1.0}, field p in {2, 5}, threads in {1, 8}. The
threshold is `T = tau * R`, where R is the enclosing radius of that cloud;
tau = 1.0 reproduces the dense default exactly. Each entry also records the
observed edge density, the mean and maximum sparse degree, and the
isolated-vertex count, because those describe the input better than n and tau
do.

The screen is a fractional design, not a full crossing: family against tau at
p 2, threads 1, max_dim 2 (block a); the field and thread axes one at a time
at the tau 0.5 cell (block b); both at once at the tau 0.8 cell of two
families (block c); the max_dim and ambient-dimension axes at low tau and
larger n (block d). n falls as tau rises, so one run stays in the
seconds-to-minutes range on eight pinned cores.

### Sampling rule

One cloud per entry. Screen entries use seed 4201. Confirmation entries use
seeds 9137, 9138, and 9139, which the screen never uses. No entry is
resampled, and no entry is dropped for being slow or unfavorable.

### The three dense modes

Every entry is reduced three ways from one distance computation:

1. the dense path on the condensed lower-distance file, threshold T;
2. the same distances thresholded at T and converted to sparse triplets, run
   through `--format sparse` without collapse;
3. the same triplets with `--collapse-edges`.

Mode 2 sits between the other two on purpose. Mode 3 against mode 1 mixes two
effects, the sparse enumerator and the collapse; mode 3 against mode 2
isolates the collapse alone. A claim about collapse rests on both numbers.

`densify_to_sparse.py` does the conversion. It writes the dense and the sparse
input from one set of `math.dist` results, so the two carry bit-identical
values. It also relabels vertices, isolated ones first, because the sparse
reader takes the point count from the largest index it sees. The barcode does
not depend on the labeling.

The three diagrams must be identical before any timing is recorded. The
comparison is exact, with no tolerance: all three outputs come from the same
f64 printer, and collapse must not move an endpoint. A mismatch voids the
entry and fails the run.

### Screen, then confirm

The screen runs first and decides nothing publicly. It guides optimization
effort and locates any boundary where collapse starts to pay.

The confirmation set is held out, and the runner enforces that. A screen run
that reaches the end writes `results_collapse_manifest.txt`, which records the
corpus version, the sha256 of `collapse_corpus.toml`, and the id of every
screen entry that finished with identical diagrams. Then `--confirm` refuses
to start unless that manifest exists, unless its recorded hash still matches
the corpus on disk, and unless every registered `[[screen]]` id appears in it.
A filtered screen run (`ONLY=`) or a voided entry leaves the manifest short,
and `--confirm` refuses. Editing the corpus after the screen changes the hash,
and `--confirm` refuses.

`ALLOW_NO_SCREEN=1` still overrides all of that. A run that uses it carries a
`SCREEN-PROTOCOL-BYPASSED` line at the top of both results files, so the
record says on its own face that it is void for the decision rule.

Before the confirmation set runs, one predeclared rule may extend it: densify
near the boundary. If the screen shows a sign change of the
mode-3-against-mode-1 speedup between two adjacent grid points on one ordered
axis, that is one point at or above 1.0 and its neighbor below it, then the
midpoint configuration joins the confirmation set. tau midpoints are the
arithmetic mean rounded to two decimals, n midpoints the geometric mean
rounded to the nearest 10, thread midpoints the geometric mean rounded to the
nearest integer. Ordered axes are tau, n, and threads; family and modulus have
no midpoint. At most six midpoints are added, and none after a confirmation
entry has run.

Every midpoint added this way must appear in the corpus under
`[meta.amendments]`, in the `midpoints` note, with its date, the pair it
straddles, and the chain position it takes. A midpoint absent from that note
is not part of the confirmation set, whatever else the corpus says.

### Decision rule

The public README wording follows this rule and nothing else. Every median
below is taken over the entries of one qualifying region, one speedup per
entry, and every entry of that region must have identical diagrams across the
three modes.

- Positive, meaning collapse is recommended for a named regime: over a
  qualifying region, the median mode-3 speedup over mode 1 is at least 1.15x
  and the median mode-3 speedup over mode 2 is at least 1.05x. The README
  names the regime by its chain and its entries, and quotes both measured
  ranges.
- Enumerator-positive, meaning the end-to-end gain is real but does not
  establish a material collapse win: over a qualifying region, the median
  mode-3 speedup over mode 1 is at least 1.15x, and the median mode-3 speedup
  over mode 2 is below 1.05x. The README credits the sparse enumerator for the
  gain and does not call collapse a win. Two sub-bands fix the collapse
  wording: at 0.95x to 1.05x over mode 2, collapse is reported as neutral
  within the band; below 0.95x over mode 2, collapse is reported as a measured
  slowdown in that regime, with the number quoted.
- Conditional, meaning collapse is mentioned with caveats: the median mode-3
  speedup over mode 1 is at least 0.95x and below 1.15x.
- Negative: otherwise. The README reports the null result.

Positive and enumerator-positive both need a qualifying region. If no
qualifying region reaches 1.15x over mode 1, the grade is conditional or
negative on the median mode-3 speedup over mode 1 across the whole
confirmation set.

A qualifying region is two or more entries of one family's declared chain that
are consecutive by `chain_index` in `collapse_corpus.toml`. That chain is the
only ordering this rule reads. Entries with `chain = "none"` vary one axis off
their family baseline, stand alone, and belong to no region. A single winning
entry is not a region.

Collapse stays off by default whatever the outcome, and the flag and the
standalone API ship either way.

### Arms against giotto-ph

`giotto_compare.sh` adds three comparisons against giotto-ph on one
thresholded graph built from the same cloud:

- arm a, both products collapse-free on that graph;
- arm b, both reducers on one externally collapsed graph, collapse off on both
  sides. GUDHI collapses the graph once, so the arm times the two reducers and
  nothing else. This is the equal-core comparison. Neither product can emit a
  collapsed graph, so an external collapser is the only honest way to feed both
  the same reduced input;
- arm c, end to end, each product running its own collapse.

Every arm records the giotto-ph version, both collapse settings, and the edge
counts. giotto-ph does not report its own edge counts, so those read `n/a`. An
arm whose module is missing prints a skip line and does not fail the run.

### Usage

```sh
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_bench.sh
# then, only after a complete screen and any midpoints:
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_bench.sh --confirm
```

Results land in `results_collapse.{txt,md}`, and the confirmation run in
`results_collapse_confirm.{txt,md}`, so it cannot overwrite the screen it
depends on. The screen also leaves `results_collapse_manifest.txt`, which is
what `--confirm` checks. All of these are gitignored. Each mode gets one
warm-up run and at least five timed runs, and the tables carry the median, the
IQR, and the peak RSS over the timed runs.

## Registered scaling studies of the parallel collapse schedules

Two registered studies measure the two parallel collapse schedules against the
serial collapse. Each has its own frozen corpus, its own runner, and its own
result names, so the two record sets never overwrite each other. Both runners
use the in-process driver `crates/collapse-bench`. It runs every configuration
of one entry in one process, times each phase on its own clock, checks the
diagrams of all configurations bar for bar before any timing counts, and
prints one key=value line per record. The corpus files carry the decision
rule, the threshold and sampling rules, the configuration list, and every
amendment, so a result cannot pick its criterion afterwards.

| study | corpus | runner | records |
|---|---|---|---|
| rounds schedule (algorithm version 2) | `collapse_corpus_v05.toml` | `collapse_scaling_rounds.sh` | `results_rounds_screen.*`, `results_rounds_confirm.*`, `results_rounds_manifest.txt` |
| ordered schedule (version 1 replayed) | `collapse_corpus_v06.toml` | `collapse_scaling_ordered.sh` | `results_ordered_screen.*`, `results_ordered_confirm.*`, `results_ordered_manifest.txt` |

Both follow the screen-then-confirm protocol of the break-even study. The
screen writes a manifest that names the corpus version and sha256, the commit
and the entry filter it ran under, and every entry it finished. `--confirm`
refuses to start unless that manifest covers the whole registered screen, ran
unfiltered at this checkout's commit, and the corpus still hashes the same. A
filtered `--confirm` is refused. Each entry runs one agreement pass and then
its timed repetitions in a rotated order. REPS (default 5) is a minimum, and
each entry runs the smallest multiple of its configuration count at or above
it, so every configuration takes every position of the rotation equally often.
The runner voids an entry whose rotation is not balanced, whose peak-RSS probe
failed, or, for the ordered study, whose in-driver gate did not compare the
ordered output with the serial output field for field.

```sh
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_scaling_rounds.sh
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_scaling_rounds.sh --confirm
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_scaling_ordered.sh
CARGO="cargo +1.92" taskset -c 0-7 ./collapse_scaling_ordered.sh --confirm
```

The tables that these runners write are the records behind the performance
statements in the changelog. `crates/collapse-bench --help` documents every
field the runners parse.
