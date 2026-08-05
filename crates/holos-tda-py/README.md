# holos-tda

Vietoris-Rips persistent homology with a ripser-class engine, written in
Rust. This package ships Python bindings and the `holos-tda` command-line
tool. The tool prints ripser-compatible output.

```sh
pip install holos-tda        # or: uvx holos-tda points.csv
python -c "import holos_tda; print(holos_tda.rips_points([[0,0],[1,0],[0,1]]))"
```

Source, documentation, and benchmarks: https://github.com/t0rsion/holos
