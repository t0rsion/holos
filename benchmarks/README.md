# Benchmarks

`run.sh` measures holos against [ripser](https://github.com/Ripser/ripser).
It writes every measurement to `results.txt` (the full log) and `results.md`
(a provenance header and a markdown table). Both files are gitignored. It is
an engineering instrument: the registered north-star study below carries the
public engine numbers, and no public claim may cite `results.md`. A rerun of
this script must reproduce every number quoted anywhere.

If the diagrams of any run disagree, the script exits nonzero. A timing whose
diagrams do not match is void.

## Study scope

The tracked harness covers the public engine and collapse studies. The v0.7
research harness covers the integrated proof, synthesis, and coverage paths.
Historical one-release drivers and records remain in the local archive. They
do not define a public v0.7 claim.

`research_bench.sh` is the v0.7 certified-workflow study. It measures exact
portfolio production and linked checking, explicit-complex production and
independent checking, and geometry-bound coverage production and independent
checking. Set `REPS` and `OUTPUT` to control repetitions and the generated
record. The three constructed cases are fixed in the `research-bench` crate.

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

## Registered north-star study

`north_star.sh` measures the shipped engine against ripser and giotto-ph on a
held-out corpus. It is the study behind the public performance statements of
this release. Nothing else may be cited for them: the engineering benchmark
below tunes the engine, and its numbers decide landing and nothing more.

| study | corpus | runner | records |
|---|---|---|---|
| north-star engine study, run 1 (revealed) | `north_star_corpus.toml` | `north_star.sh` | `results_north_star.*`, `results_north_star_manifest.txt` |
| north-star engine study, confirmation | `north_star_confirm_corpus.toml` | `CORPUS=north_star_confirm_corpus.toml ./north_star.sh` | the same names; the record header names the corpus |

Run 1 ran on `north_star_corpus.toml` at the first freeze and revealed that
corpus (its amendment 3 says how). The public claims of 0.6.0 read the
confirmation corpus, which repeats every entry with a new seed and the same
rules; run 1's records are archived beside the release as the revealed run.

The corpus carries the decision rule, the noise rule, the pinning rule, the
validity rule, the arms, the competitors, the timing protocol, and the sampling
rule, quoted from PLAN.md, so a result cannot pick its criterion afterwards. It
is frozen: the entries, sizes, seeds, thresholds, thread counts, and rules do
not change in response to a result, and every later change adds a numbered
amendment.

Amendment 1, dated 2026-08-18, came before any registered timing. It puts a
floor under every graded ratio, moves the measurement controller off the timed
cores, says what a complete study is, and requires the routing constants of the
grading strata to be audited against H3 at the freeze. `[meta.amendments]` of
the corpus carries its text and the list of what it changed.

### Protocol

One frozen pass, in three parts. There is no screen and no confirmation set:
the corpus is held out as a whole, and it runs once.

- serial. Every holos arm, the A/A control, and one ripser build, each a fresh
  process on one physical core. The serial grade reads it.
- multicore. H3 and giotto-ph 0.2.4, both at four physical cores, on the
  entries the corpus marks `multicore = true`. The multicore grade reads it.
- CPU scaling. The `[[scaling]]` entries at 1, 2, and 4 physical cores, with H2
  at four cores as the attribution diagnostic. It is descriptive and carries no
  grade.

Four holos arms are frozen: H0 is public 0.5.0, H1 adds sparse cofacet
streaming, H2 adds dense low-threshold routing on H1, and H3 is the release.
`HOLOS_ARMS` defaults to the commits in the corpus. No arm is forced to an
engine or a storage form; each build runs the path it ships, and the tables
carry one column per arm.

The competitors are the stock f32 ripser build, the same source built with
`-D USE_COEFFICIENTS` for the odd-prime entries, the audited matched-precision
double build, and giotto-ph 0.2.4. The double build is a diagnostic: it gets a
table of its own, it is never pooled with the stock arm, and no grade reads it.
Ripser++ is not an arm.

### Timing

holos and ripser are fresh processes, timed by `measure.py`, which pins the
child to the run's CPU list through `MEASURE_AFFINITY`. Pinning the child
instead of running it under `taskset` keeps `argv[0]` the target binary, which
is what the peak RSS sampler matches on. giotto-ph has no command-line tool, so
it is timed in process the way `giotto_compare.sh` times it: one monotonic
clock around the `ripser_parallel` call alone. That clock excludes process
start and input parse, which holos pays inside its own number, so the
comparison favors giotto-ph. The record says so.

Each command gets one warm-up run, which is also its agreement run, and then
the timed repetitions. One repetition runs every command of the entry once, and
each repetition starts the cycle one place further on. The repetition count is
the smallest multiple of the command count at or above `REPS` (default 5), so
every command takes every position equally often.

Every arm and every competitor is compared against the H3 diagram as interval
multisets within `TOLERANCE`. A mismatch voids the entry.

### The A/A control and the noise band

A second copy of the H3 binary, checked to hash the same, runs as its own arm
on every entry of every pass. Its per-entry ratio against H3 is what the
harness reports for two identical binaries. The band of a pass is the 95th
percentile of the absolute distance from 1.0 of those ratios, and a per-entry
ratio inside the band is a tie. The per-entry limit is therefore the wider of
5% and the band.

Amendment 1 puts a floor under every graded ratio. A ratio whose denominator
median is under 20 ms is descriptive: the tables report it, and it enters no
pass or fail median, no per-entry clause, no regression aggregate, and no band.
The denominator is the competitor median for a serial or multicore ratio, the
fastest earlier arm for a regression ratio, and the H3 median for an A/A ratio.
Every median covers the graded entries alone, and the tables mark each
descriptive entry and name the count behind each median.

### Grading strata

The decision rule grades four strata. They are derived, not declared:
`dense-selected` and `sparse-selected` from the engine the frozen routing rule
of H3 selects for the entry, `maxdim-1` and `maxdim-2` from the entry's
dimension. The runner computes the routing decision from the point count and
the edge count at the threshold, records both tests behind it, and never reads
a timing to classify an entry. It also prints the routing constants it used on
the `routing_rule` line of the manifest; Amendment 2 audits that line against
the routing rule in `crates/holos-tda/src/lib.rs` at H3, which is when H3 is a
named commit. Every entry also names one of the twelve input strata of the
engineering corpus, and the tables report the input strata first.

### Pinning

The pinning rule names physical cores, so four cores means four cores without
SMT: one logical CPU per core. The corpus holds the lists for the study
machine, `0`, `0,2`, and `0,2,12,14`. The runner refuses to start when the
topology is unknown, when a listed CPU is outside the allowed set, or when a
list does not hold the physical cores it claims. It checks the one logical CPU
per core assumption against the kernel's own sibling lists,
`/sys/devices/system/cpu/cpu*/topology/thread_siblings_list`, and records the
check in the manifest. `ALLOW_ANY_TOPOLOGY=1` overrides the refusal and marks
the run void.

Amendment 1 moves the measurement controller off the timed cores. The runner
re-execs itself under `taskset` on one logical CPU outside every timed physical
core, so its own process, `measure.py`, and the peak RSS sampler inside it
never share a core with a run they time. The CPU is
`[meta.pinning].controller_cpu`, logical CPU 8 on the study machine, and
`NS_CONTROLLER_CPU` overrides it. A controller CPU that is a timed CPU or the
SMT sibling of one is refused, and there is no override for that. Timed
children set their own affinity, through `MEASURE_AFFINITY` for holos and
ripser and through `taskset` for giotto-ph, so none of them inherits the
controller's pin. Untimed work, the arm builds and the input generation, runs
on the allowed CPU set.

### Freeze checks

The runner hashes the corpus before it generates the first input and again
before it times the first run, and stops if the two differ. It refuses a dirty
worktree unless `ALLOW_DIRTY=1`, which the record discloses. It refuses a
version 0 corpus unless `ALLOW_DRAFT=1`, a giotto-ph other than the registered
version unless `ALLOW_ANY_GPH_VERSION=1`, and a missing competitor build that
`[meta.required_competitors]` asks for unless `ALLOW_MISSING_COMPETITORS=1`;
each override voids what it touches. `results_north_star_manifest.txt` records
the corpus version and sha256, every arm's commit and binary sha256, every
competitor's hash or version, the CPU lists, the controller CPU and its check,
the topology check, the routing constants, the kernel, the toolchain, and the
date.

### A complete study

Amendment 1 says what makes a study complete, and the manifest writes
`study_valid=yes` only for a complete study: a clean worktree, the frozen
corpus at version 1 or later, the frozen arms of `[meta.arm_commits]`, every
competitor build the corpus requires, giotto-ph importable at the registered
0.2.4, a verified topology and controller placement, no `NS_ONLY` filter, and
no void entry. Anything else writes `study_valid=no` and lists every reason,
both in the manifest and at the top of the markdown record.

```sh
CARGO="cargo +1.92" RIPSER_BIN=/path/to/ripser \
    RIPSER_COEFF_BIN=/path/to/ripser-coeff RIPSER_F64_BIN=/path/to/ripser-f64 \
    taskset -c 0-3,12-15 ./north_star.sh
```

`NS_ONLY` takes a comma-separated list of id globs, so one stratum can be rerun
by hand; a filtered run is recorded as filtered and grades nothing.
`north_star_tables.py` turns the record into the tables, the noise band, and
the grades; it measures nothing. The grades print the strata first, and the
overall median is never read alone.

## Engineering benchmark (not registered)

`engine_bench.sh` measures holos against ripser on identical inputs. It is an
engineering instrument, not a study: it has no decision rule, no manifest, and
no protocol gate, and no public claim may cite its numbers. It exists because
PLAN.md requires a change to be tuned on disclosed data and then landed on a
disjoint set that was never looked at.

The corpus is `engineering_corpus.toml`. It holds two sets. The tuning set is
disclosed: rerun it after every change and pick constants from it. The landing
set uses disjoint seeds and disjoint point counts, and it runs once, after the
constants are frozen. The runner refuses to overwrite an existing landing
record unless `ALLOW_RERUN=1`, and the record then discloses the rerun.

### Strata

Every entry names a stratum, an input regime the engine has to meet. The
tuning and the landing set both carry every stratum, with disjoint seeds and
sizes inside each one. The summary reports a median per stratum first; an
overall median never stands alone, because a median over unlike regimes hides
the regime that lost.

| stratum | what it holds |
|:--|:--|
| `baseline` | uniform, spherical, toroidal, and clustered clouds at several thresholds |
| `knn` | k nearest neighbor graphs, k = 8, 15, and 30, as native sparse input |
| `lowthresh` | large vertex counts far below the enclosing radius: isolates, trees, many components |
| `quantized` | lattice, duplicated, and identical points; ties at the threshold boundary |
| `skewed` | preferential attachment and planted blocks: heavy degree tails and communities |
| `nearclique` | clouds at `tau = 1.0`, where almost every pair is an edge |
| `embedding` | Euclidean clouds in 8, 32, and 128 dimensions |
| `nonmetric` | weights that satisfy no triangle inequality, absent edges at +inf, disconnected input |
| `deepdim` | homology in dimensions 2, 3, and 4 on small inputs |
| `oddprime` | coefficients in Z/3 and Z/5 |
| `collapsed` | a real collapsed graph, read back as native sparse input |
| `memory` | a sparse graph whose widening to a full matrix is quick and whose matrix is not small |

### Inputs

A cloud entry takes one seeded cloud from `gen_cloud.py` and one threshold
`T = tau * enclosing radius`. `densify_to_sparse.py` writes the condensed
lower-distance file and the triplet file from the same distances, so both
tools read one input at one threshold.

A graph entry takes one seeded graph from `gen_graph.py`, which writes the
triplet file alone: a kNN graph, preferential attachment, planted blocks, a
random forest, or independent nonmetric weights. Its threshold is
`T = tau * (the largest weight drawn)`. There is no dense file of such an
input, so every configuration of a graph entry reads the triplet file.

An entry with `collapse = true` has its graph reduced first, by the certified
serial edge collapse, through `engine-bench --emit-collapsed`. The collapse is
untimed, the barcode does not change, and the reduced graph is then a native
sparse input.

`tau = 1.0` puts the threshold exactly on a realised distance under both
rules, which is the threshold-boundary tie case.

Sizes follow the cost of the slowest configuration. The sparse engine on a
near-complete graph grows faster than `n^4`, so a cloud at `tau = 1.0` stays at
a few hundred points while a cloud at `tau = 0.3` reaches `n = 3000`. One arm
reruns the tuning set in about an hour on four physical cores, and every
further arm costs about as much again.

### Timing and arms

The primary total is a fresh process. holos and ripser both start, read the
same file at the same threshold, and exit; `measure.py` times them and reads
their peak RSS. Both get one warm-up run and the same number of timed
repetitions, `REPS` (default 5).

`HOLOS_ARMS` names the holos builds to time, as a space-separated
`label=commit` list; the default is `tree=@`, the working tree. Any other
value is a git commit. The runner checks it out under `HIST_DIR/<sha>/` with
`git worktree add --detach`, builds `holos-tda` there into that checkout's own
target directory, and records the sha of every binary in the provenance
header. An existing binary is reused unless `REBUILD_HIST=1`. Remove a
checkout with `git worktree remove` when it is no longer wanted. Every arm
runs every entry, and the tables carry one column per arm and configuration.

`HIST_DIR` defaults to `benchmarks/data/hist`. One arm holds one full release
build, so four arms need a few gigabytes; point `HIST_DIR` at a filesystem
with room to spare, and set `CARGO_TARGET_DIR` to move the working tree's own
build the same way. The runner honors cargo's variable and keeps the absolute
path out of the record.

An arm whose `holos` takes `--engine auto|dense|sparse` runs `auto`,
`forced-dense`, and `forced-sparse` on one input file, and `sparse-file` on
the triplet file. In `sparse-file` holos reads the triplet file with
`--format sparse --engine auto`, and ripser reads it with `--format sparse`,
at the same threshold and the same dimension. Routing holds the file fixed
and varies the engine. `sparse-file` holds the format fixed, so every
stratum carries one sparse-input ratio that compares like with like. An arm
built before routing has no such flag, so its configurations are the input
files instead: `dense` reads the lower-distance file and `sparse` reads the
triplet file. The runner probes each arm's `--help` and picks the axis per
arm; nothing else changes.

On an entry whose primary file is already the triplet file, which is every
graph entry and every collapsed entry, `sparse-file` is the `auto`
configuration again. The runner keys an arm's timings by file and engine, so
it runs that command once and reports both columns from it. The tables carry
the `sparse-file` column beside the routing columns, and the section named
Sparse-file comparison gives its stratum medians with the entry count behind
each one.

The dense storage form is a second axis, and the runner does not walk it. A
tree arm and `crates/engine-bench` both take `--dense-storage
auto|compact|square`, so either can time the compact and the full matrix
against each other on one entry. Running both doubles every dense
configuration, so the runner times the shipped rule alone and the storage
tuning runs by hand.

The in-process driver `crates/engine-bench` runs beside the fresh processes
and times the parse, the distance build, the graph build, and the reduction on
separate clocks. Those phase medians are a diagnostic table, not the headline,
and only the working tree has a driver. It also supplies the peak RSS of one
engine entry point at a time, from one extra single-repetition process per
entry point, which is what the `memory` stratum reads.

The driver computes the entry's reference diagram and checks its own engine
entry points against each other bar for bar. Every arm and every ripser run is
then compared against that reference within the tolerance. A mismatch voids
the entry and fails the run.

The reference ripser build reads no odd modulus and no `--modulus` flag, so an
entry marks `competitor = "none"` where ripser cannot read its input. The
runner skips the external arm for such an entry, reports no ratio for it, and
still checks the entry against the driver's reference.

```sh
CARGO="cargo +1.92" RIPSER_BIN=/path/to/ripser taskset -c 0-7 ./engine_bench.sh
# one stratum, two builds:
HOLOS_ARMS="h0=<sha> tree=@" ONLY='t-knn-*' \
    CARGO="cargo +1.92" RIPSER_BIN=/path/to/ripser taskset -c 0-7 ./engine_bench.sh
# once, after the tuning is finished:
CARGO="cargo +1.92" RIPSER_BIN=/path/to/ripser taskset -c 0-7 ./engine_bench.sh --landing
```

Results land in `results_engine_tuning.{txt,md}` and
`results_engine_landing.{txt,md}`, both gitignored. `ONLY` takes a
comma-separated list of id globs, so one stratum can be rerun alone.
`ONLY_CONFIGS` takes a comma-separated list of configuration names, so one
configuration can be timed again without repeating the rest; the record
names both filters.
`DRIVER_REPS` (default 3) is the driver's minimum repetition count, rounded up
per entry to keep the rotation balanced. `engine_tables.py` turns the record
into the markdown tables and the stratum medians; it measures nothing.
`crates/engine-bench --help` documents every field the runner parses.
