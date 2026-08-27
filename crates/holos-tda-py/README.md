# holos-tda

Python bindings for holos, a Vietoris-Rips persistent homology engine.
The package installs the `holos-tda` CLI, which prints ripser-compatible
output.

The Python API also compiles checked sparse H0 and H1 persistence programs.
`SparseProgram` reuses valid articulation-separated atoms, repairs invalid
atoms, reports exact class continuation and work, and writes portable program,
trace, and intervention artifacts. The optional `holos_tda.torch` module
provides strict finite H1 endpoint derivatives for distinct edge weights.

`SparseIndex` maintains immutable persistence versions over separator
interfaces through the configured dimension. It composes exact relative
filtered cores, applies atomic active-topology patches, and emits independently
checkable proof records.

`kinetic_zigzag` computes exact fixed-scale class dynamics for affine edge
weights. It returns open-cell and event nodes, restriction arrows,
generalized ranks, interval multiplicities, and a self-contained `HOLOSZZ`
artifact for the separate checker.

`intervene_cohomology` finds one minimum-cost set of candidate edges across
declared graph scenarios. It returns an exact plan or a checked bound. The
separate checker repeats every fixed-scale cohomology and search step from the
`HOLOSCI` artifact.

`relative_coverage` returns an exact fence-filling chain.
`synthesize_coverage` and `synthesize_affine_coverage` find a minimum-cost
sensor activation plan across communication states and bounded failures. They
return a `HOLOSCOV` artifact for the separate checker. Physical coverage is
conditional on the controlled-boundary domain, placement, fence, and
communication assumptions.

```sh
pip install holos-tda
python -c "import holos_tda; print(holos_tda.rips_points([[0, 0], [1, 0], [0, 1]]))"
```

`uvx holos-tda points.csv` runs the CLI without installing.

Source and docs: https://github.com/t0rsion/holos
