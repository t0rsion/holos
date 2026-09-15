# holos-tda

Python bindings for holos, a Vietoris-Rips persistent homology engine.
The package installs the `holos-tda` CLI, which prints ripser-compatible
output.

`SparseProgram` reuses valid articulation-separated atoms and repairs invalid
atoms. It reports exact class continuation and work, and writes portable
program, trace, and intervention artifacts. The optional `holos_tda.torch`
module computes strict finite H1 endpoint derivatives for distinct edge
weights.

`SparseIndex` maintains immutable persistence versions over separator
interfaces through the configured dimension. It composes relative filtered
cores, applies atomic active-topology patches, and emits proof records for
the separate checker.

`kinetic_zigzag` computes exact fixed-scale class dynamics for affine edge
weights. It returns a `HOLOSZZ` artifact for the separate checker.

`rips_points_classes`, `rips_condensed_classes`, and `rips_sparse_classes`
return source-bound H1 class records. Their `_class` circular functions check
the active graph, interval, field, representative scale, and class identity.
`circular_coordinates_class` accepts the same records with a square distance
matrix. Automatic lifting requires an odd prime, such as 47. A supplied lift
supports modulus two when continuation is not requested.

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
