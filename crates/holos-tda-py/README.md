# holos-tda

Python bindings for holos, a Vietoris-Rips persistent homology engine.
The package installs the `holos-tda` CLI, which prints ripser-compatible
output.

```sh
pip install holos-tda
python -c "import holos_tda; print(holos_tda.rips_points([[0, 0], [1, 0], [0, 1]]))"
```

`uvx holos-tda points.csv` runs the CLI without installing.

Source and docs: https://github.com/t0rsion/holos
