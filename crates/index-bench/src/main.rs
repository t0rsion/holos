//! Registered benchmark for versioned exact persistence indexes.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use holos_tda::{
    CertificateLimits, EdgeKey, IndexDeltaProof, IndexParams, IndexSnapshotProof, IndexUpdateMode,
    InterfacePolicy, PersistenceIndex, RipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{IndexProofState, ProofLimits};

const USAGE: &str = "\
Usage: index-bench --atoms A --atom-vertices K --seed S [options]

Generate weighted atoms that share one separator. Complete atoms exercise H1.
Cross-polytope boundary atoms exercise the requested positive dimension. Each
update changes one atom without changing the listed-edge envelope or edge
order. The study compares warm index transitions with cold index compilation,
serial with parallel alternatives, and warm proof deltas with cold snapshots.

Options:
  --steps K       cumulative version steps (default 24)
  --branches K    alternatives from one version (default 4)
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  --max-dim D     highest homology dimension (default 1)
  --cross-polytope
                  use sphere atoms joined at one vertex
  --zero-separator
                  set the shared separator edge to filtration value zero
  --materialize   retain a reduction over every parent scope
  -h, --help      print this text

Graph generation, initial compilation, proof construction, and warm-up are
outside timed arms. Every warm result must equal cold exact compilation.
";

#[derive(Clone, Copy)]
struct Options {
    atoms: usize,
    atom_vertices: usize,
    seed: u64,
    steps: usize,
    branches: usize,
    reps: usize,
    modulus: u32,
    max_dim: usize,
    cross_polytope: bool,
    zero_separator: bool,
    materialize: bool,
}

fn parse() -> Result<Option<Options>, String> {
    let mut atoms = None;
    let mut atom_vertices = None;
    let mut seed = None;
    let mut steps = 24;
    let mut branches = 4;
    let mut reps = 5;
    let mut modulus = 2;
    let mut max_dim = 1;
    let mut cross_polytope = false;
    let mut zero_separator = false;
    let mut materialize = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "-h" || argument == "--help" {
            return Ok(None);
        }
        if argument == "--zero-separator" {
            zero_separator = true;
            continue;
        }
        if argument == "--materialize" {
            materialize = true;
            continue;
        }
        if argument == "--cross-polytope" {
            cross_polytope = true;
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        match argument.as_str() {
            "--atoms" => atoms = Some(parse_value(&argument, &value)?),
            "--atom-vertices" => atom_vertices = Some(parse_value(&argument, &value)?),
            "--seed" => seed = Some(parse_value(&argument, &value)?),
            "--steps" => steps = parse_value(&argument, &value)?,
            "--branches" => branches = parse_value(&argument, &value)?,
            "--reps" => reps = parse_value(&argument, &value)?,
            "--modulus" => modulus = parse_value(&argument, &value)?,
            "--max-dim" => max_dim = parse_value(&argument, &value)?,
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    let options = Options {
        atoms: atoms.ok_or_else(|| "--atoms is required".to_string())?,
        atom_vertices: atom_vertices.ok_or_else(|| "--atom-vertices is required".to_string())?,
        seed: seed.ok_or_else(|| "--seed is required".to_string())?,
        steps,
        branches,
        reps,
        modulus,
        max_dim,
        cross_polytope,
        zero_separator,
        materialize,
    };
    if options.atoms < 2
        || options.atom_vertices < 4
        || options.steps == 0
        || options.branches < 2
        || options.branches > options.atoms
        || options.reps < 5
        || (options.cross_polytope
            && (options.atom_vertices < 6 || !options.atom_vertices.is_multiple_of(2)))
    {
        return Err("atoms must be at least 2, atom vertices at least 4, steps positive, branches between 2 and atoms, and reps at least 5; cross-polytope atoms must have an even vertex count of at least 6".into());
    }
    Ok(Some(options))
}

fn parse_value<T: std::str::FromStr>(label: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {label}: {value}"))
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn graph(options: Options) -> Result<(SparseDistanceMatrix, Vec<EdgeKey>, f64), String> {
    let mut endpoints = BTreeSet::new();
    let mut update_edges = Vec::with_capacity(options.atoms);
    for atom in 0..options.atoms {
        let shared = if options.cross_polytope { 1 } else { 2 };
        let first = shared + atom * (options.atom_vertices - shared);
        let vertices: Vec<_> = (0..shared)
            .chain(first..first + options.atom_vertices - shared)
            .collect();
        update_edges.push(if options.cross_polytope {
            EdgeKey {
                u: vertices[1],
                v: vertices[3],
            }
        } else {
            EdgeKey {
                u: first,
                v: first + 1,
            }
        });
        for v in 1..vertices.len() {
            for u in 0..v {
                if !options.cross_polytope || u / 2 != v / 2 {
                    endpoints.insert(EdgeKey {
                        u: vertices[u],
                        v: vertices[v],
                    });
                }
            }
        }
    }
    let endpoints: Vec<_> = endpoints.into_iter().collect();
    let mut state = options.seed;
    let mut order: Vec<_> = (0..endpoints.len())
        .map(|index| (next_random(&mut state), index))
        .collect();
    order.sort_unstable();
    let mut rank = vec![0usize; endpoints.len()];
    for (position, &(_, index)) in order.iter().enumerate() {
        rank[index] = position;
    }
    let spacing = 8.0 / (endpoints.len() as f64 + 1.0);
    let triplets: Vec<_> = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, edge)| {
            let value = if options.zero_separator && edge == (EdgeKey { u: 0, v: 1 }) {
                0.0
            } else {
                1.0 + spacing * (rank[index] + 1) as f64
            };
            (edge.u, edge.v, value)
        })
        .collect();
    let shared = if options.cross_polytope { 1 } else { 2 };
    let vertices = shared + options.atoms * (options.atom_vertices - shared);
    let graph = SparseDistanceMatrix::from_triplets(vertices, &triplets)
        .map_err(|error| error.to_string())?;
    Ok((graph, update_edges, spacing / 4.0))
}

fn set_weight(
    graph: &SparseDistanceMatrix,
    edge: EdgeKey,
    value: f64,
) -> Result<SparseDistanceMatrix, String> {
    let triplets: Vec<_> = graph
        .edges()
        .map(|(u, v, current)| {
            let current_edge = EdgeKey { u, v };
            (u, v, if current_edge == edge { value } else { current })
        })
        .collect();
    SparseDistanceMatrix::from_triplets(graph.len(), &triplets).map_err(|error| error.to_string())
}

fn trajectory(
    initial: &SparseDistanceMatrix,
    edges: &[EdgeKey],
    change: f64,
    steps: usize,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    let original: Vec<_> = edges
        .iter()
        .map(|edge| initial.get(edge.u, edge.v))
        .collect();
    let mut changed = vec![false; edges.len()];
    let mut current = initial.clone();
    let mut updates = Vec::with_capacity(steps);
    for step in 0..steps {
        let position = step % edges.len();
        changed[position] = !changed[position];
        let value = original[position] + if changed[position] { change } else { 0.0 };
        current = set_weight(&current, edges[position], value)?;
        updates.push(current.clone());
    }
    Ok(updates)
}

fn index_params(atom_vertices: usize, materialize: bool, cross_polytope: bool) -> IndexParams {
    let mut params = IndexParams::default();
    params.max_separator_width = if cross_polytope { 1 } else { 2 };
    params.leaf_vertices = atom_vertices;
    params.interface_policy = if materialize {
        InterfacePolicy::Materialize
    } else {
        InterfacePolicy::Compose
    };
    params
}

fn compile(
    graph: &SparseDistanceMatrix,
    params: &RipsParams,
    atom_vertices: usize,
    materialize: bool,
    cross_polytope: bool,
) -> Result<PersistenceIndex, String> {
    PersistenceIndex::compile(
        graph,
        params,
        index_params(atom_vertices, materialize, cross_polytope),
        CertificateLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn advance_all(
    initial: &PersistenceIndex,
    updates: &[SparseDistanceMatrix],
) -> Result<(Vec<PersistenceIndex>, usize, usize, usize), String> {
    let mut index = initial.clone();
    let mut versions = Vec::with_capacity(updates.len());
    let mut nodes_shared = 0usize;
    let mut columns_reused = 0usize;
    let mut columns_reduced = 0usize;
    for graph in updates {
        let transition = index.transition(graph).map_err(|error| error.to_string())?;
        if !matches!(
            transition.mode,
            IndexUpdateMode::Composed | IndexUpdateMode::Repaired | IndexUpdateMode::Rebuilt
        ) {
            return Err("timed update did not stay inside the index envelope".into());
        }
        nodes_shared += transition.work.nodes_shared;
        columns_reused += transition.work.reduction_columns_reused;
        columns_reduced += transition.work.reduction_columns_reduced;
        index = transition.index;
        versions.push(index.clone());
    }
    Ok((versions, nodes_shared, columns_reused, columns_reduced))
}

fn compile_all(
    params: &RipsParams,
    atom_vertices: usize,
    materialize: bool,
    cross_polytope: bool,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<PersistenceIndex>, String> {
    updates
        .iter()
        .map(|graph| compile(graph, params, atom_vertices, materialize, cross_polytope))
        .collect()
}

fn same_diagram(left: &PersistenceIndex, right: &PersistenceIndex) -> bool {
    left.diagram().bars.len() == right.diagram().bars.len()
        && left
            .diagram()
            .bars
            .iter()
            .zip(&right.diagram().bars)
            .all(|(left, right)| {
                left.dim == right.dim
                    && left.birth.to_bits() == right.birth.to_bits()
                    && left.death.to_bits() == right.death.to_bits()
            })
}

fn same_versions(left: &[PersistenceIndex], right: &[PersistenceIndex]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| same_diagram(left, right))
}

struct ProofSet {
    snapshot: Vec<u8>,
    deltas: Vec<Vec<u8>>,
    cold: Vec<Vec<u8>>,
    warm_nodes: usize,
    cold_nodes: usize,
    warm_terms: usize,
    cold_terms: usize,
}

fn proofs(
    initial: &PersistenceIndex,
    updates: &[SparseDistanceMatrix],
) -> Result<ProofSet, String> {
    let snapshot = IndexSnapshotProof::from_index(initial).map_err(|error| error.to_string())?;
    let mut warm_nodes = snapshot.summary().nodes;
    let mut warm_terms = snapshot.summary().terms;
    let snapshot = snapshot.encode().map_err(|error| error.to_string())?;
    let mut cold = vec![snapshot.clone()];
    let mut cold_nodes = initial.summary().nodes;
    let mut cold_terms = warm_terms;
    let mut deltas = Vec::with_capacity(updates.len());
    let mut index = initial.clone();
    for graph in updates {
        let transition = index.transition(graph).map_err(|error| error.to_string())?;
        let delta = IndexDeltaProof::between(&index, &transition.index)
            .map_err(|error| error.to_string())?;
        warm_nodes += delta.summary().nodes;
        warm_terms += delta.summary().terms;
        deltas.push(delta.encode().map_err(|error| error.to_string())?);
        index = transition.index;
        let proof = IndexSnapshotProof::from_index(&index).map_err(|error| error.to_string())?;
        cold_nodes += proof.summary().nodes;
        cold_terms += proof.summary().terms;
        cold.push(proof.encode().map_err(|error| error.to_string())?);
    }
    Ok(ProofSet {
        snapshot,
        deltas,
        cold,
        warm_nodes,
        cold_nodes,
        warm_terms,
        cold_terms,
    })
}

fn verify_warm(proofs: &ProofSet) -> Result<usize, String> {
    let (mut state, cold) =
        IndexProofState::verify_snapshot(&proofs.snapshot, ProofLimits::default())
            .map_err(|error| error.to_string())?;
    let mut checked = cold.nodes_checked;
    for delta in &proofs.deltas {
        checked += state
            .apply_delta(delta, ProofLimits::default())
            .map_err(|error| error.to_string())?
            .nodes_checked;
    }
    Ok(checked)
}

fn verify_cold(proofs: &ProofSet) -> Result<usize, String> {
    proofs
        .cold
        .iter()
        .map(|proof| {
            IndexProofState::verify_snapshot(proof, ProofLimits::default())
                .map(|(_, checked)| checked.nodes_checked)
                .map_err(|error| error.to_string())
        })
        .sum()
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let value = operation();
    (start.elapsed(), value)
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn run(options: Options) -> Result<(), String> {
    let (input, update_edges, change) = graph(options)?;
    let serial_params = RipsParams::new(options.max_dim).with_modulus(options.modulus);
    let mut parallel_params = serial_params.clone();
    parallel_params.threads = options.branches;
    let initial = compile(
        &input,
        &serial_params,
        options.atom_vertices,
        options.materialize,
        options.cross_polytope,
    )?;
    let parallel = compile(
        &input,
        &parallel_params,
        options.atom_vertices,
        options.materialize,
        options.cross_polytope,
    )?;
    let updates = trajectory(&input, &update_edges, change, options.steps)?;
    let alternatives = trajectory(&input, &update_edges, change, options.branches)?;

    let (warm, nodes_shared, columns_reused, columns_reduced) = advance_all(&initial, &updates)?;
    let cold = compile_all(
        &serial_params,
        options.atom_vertices,
        options.materialize,
        options.cross_polytope,
        &updates,
    )?;
    if !same_versions(&warm, &cold) {
        return Err("warm versions and cold compilation differ".into());
    }
    let warm_explained = warm
        .last()
        .unwrap()
        .explain()
        .map_err(|error| error.to_string())?;
    let cold_explained = cold
        .last()
        .unwrap()
        .explain()
        .map_err(|error| error.to_string())?;
    if warm_explained.spaces != cold_explained.spaces {
        return Err("warm and cold canonical class spaces differ".into());
    }

    let serial_branches = initial
        .branch(&alternatives)
        .map_err(|error| error.to_string())?;
    let parallel_branches = parallel
        .branch(&alternatives)
        .map_err(|error| error.to_string())?;
    if !serial_branches
        .iter()
        .zip(&parallel_branches)
        .all(|(left, right)| same_diagram(&left.transition.index, &right.transition.index))
    {
        return Err("serial and parallel alternatives differ".into());
    }

    let proofs = proofs(&initial, &updates)?;
    let warm_checked = verify_warm(&proofs)?;
    let cold_checked = verify_cold(&proofs)?;
    if warm_checked != proofs.warm_nodes || cold_checked != proofs.cold_nodes {
        return Err("proof checker counts differ from producer records".into());
    }

    let mut warm_times = Vec::with_capacity(options.reps);
    let mut cold_times = Vec::with_capacity(options.reps);
    let mut serial_branch_times = Vec::with_capacity(options.reps);
    let mut parallel_branch_times = Vec::with_capacity(options.reps);
    let mut warm_verify_times = Vec::with_capacity(options.reps);
    let mut cold_verify_times = Vec::with_capacity(options.reps);
    for repetition in 0..options.reps {
        let warm_first = repetition % 2 == 0;
        let (warm_time, warm_result, cold_time, cold_result) = if warm_first {
            let (warm_time, warm_result) = timed(|| advance_all(&initial, &updates));
            let (cold_time, cold_result) = timed(|| {
                compile_all(
                    &serial_params,
                    options.atom_vertices,
                    options.materialize,
                    options.cross_polytope,
                    &updates,
                )
            });
            (warm_time, warm_result, cold_time, cold_result)
        } else {
            let (cold_time, cold_result) = timed(|| {
                compile_all(
                    &serial_params,
                    options.atom_vertices,
                    options.materialize,
                    options.cross_polytope,
                    &updates,
                )
            });
            let (warm_time, warm_result) = timed(|| advance_all(&initial, &updates));
            (warm_time, warm_result, cold_time, cold_result)
        };
        warm_times.push(warm_time.as_nanos());
        cold_times.push(cold_time.as_nanos());
        let warm_versions = warm_result?.0;
        let cold_versions = cold_result?;
        if !same_versions(&warm_versions, &cold_versions) {
            return Err(format!("repetition {repetition} version results differ"));
        }
        black_box((warm_versions, cold_versions));

        let (serial_time, serial) = timed(|| initial.branch(&alternatives));
        let (parallel_time, parallel_results) = timed(|| parallel.branch(&alternatives));
        let serial = serial.map_err(|error| error.to_string())?;
        let parallel_results = parallel_results.map_err(|error| error.to_string())?;
        if !serial
            .iter()
            .zip(&parallel_results)
            .all(|(left, right)| same_diagram(&left.transition.index, &right.transition.index))
        {
            return Err(format!("repetition {repetition} branch results differ"));
        }
        serial_branch_times.push(serial_time.as_nanos());
        parallel_branch_times.push(parallel_time.as_nanos());
        black_box((serial, parallel_results));

        let verify_warm_first = repetition % 2 == 1;
        let (first_time, first) = if verify_warm_first {
            timed(|| verify_warm(&proofs))
        } else {
            timed(|| verify_cold(&proofs))
        };
        let (second_time, second) = if verify_warm_first {
            timed(|| verify_cold(&proofs))
        } else {
            timed(|| verify_warm(&proofs))
        };
        let (warm_count, cold_count) = if verify_warm_first {
            warm_verify_times.push(first_time.as_nanos());
            cold_verify_times.push(second_time.as_nanos());
            (first?, second?)
        } else {
            cold_verify_times.push(first_time.as_nanos());
            warm_verify_times.push(second_time.as_nanos());
            (second?, first?)
        };
        if warm_count != warm_checked || cold_count != cold_checked {
            return Err(format!("repetition {repetition} proof counts differ"));
        }
        black_box((warm_count, cold_count));
    }

    let warm_ns = median(warm_times);
    let cold_ns = median(cold_times);
    let serial_branch_ns = median(serial_branch_times);
    let parallel_branch_ns = median(parallel_branch_times);
    let warm_verify_ns = median(warm_verify_times);
    let cold_verify_ns = median(cold_verify_times);
    let warm_proof_bytes =
        proofs.snapshot.len() + proofs.deltas.iter().map(Vec::len).sum::<usize>();
    let cold_proof_bytes = proofs.cold.iter().map(Vec::len).sum::<usize>();
    let summary = initial.summary();
    println!(
        "format=holos-index-bench-v2 shape={} max_dim={} h2_bars={} atoms={} atom_vertices={} vertices={} edges={} seed={} steps={} branches={} reps={} modulus={} policy={} zero_separator={} interfaces={} composed_interfaces={} materialized_interfaces={} root_composed={} separators={} widest_separator={} largest_interface_vertices={} candidates_checked={} search_complete={} nodes_shared={} columns_reused={} columns_reduced={} warm_ns={} cold_ns={} update_speedup={:.6} branch_serial_ns={} branch_parallel_ns={} branch_speedup={:.6} warm_proof_bytes={} cold_proof_bytes={} proof_compression={:.6} warm_proof_nodes={} cold_proof_nodes={} warm_proof_terms={} cold_proof_terms={} warm_verify_ns={} cold_verify_ns={} verify_speedup={:.6}",
        if options.cross_polytope {
            "cross-polytope"
        } else {
            "complete"
        },
        options.max_dim,
        initial.diagram().in_dim(2).count(),
        options.atoms,
        options.atom_vertices,
        input.len(),
        input.num_edges(),
        options.seed,
        options.steps,
        options.branches,
        options.reps,
        options.modulus,
        if options.materialize {
            "materialize"
        } else {
            "compose"
        },
        options.zero_separator,
        summary.nodes,
        summary.composed_interfaces,
        summary.materialized_interfaces,
        summary.root_composed,
        summary.separators,
        summary.widest_separator,
        summary.largest_interface_vertices,
        summary.separator_candidates_checked,
        summary.separator_search_complete,
        nodes_shared,
        columns_reused,
        columns_reduced,
        warm_ns,
        cold_ns,
        cold_ns as f64 / warm_ns as f64,
        serial_branch_ns,
        parallel_branch_ns,
        serial_branch_ns as f64 / parallel_branch_ns as f64,
        warm_proof_bytes,
        cold_proof_bytes,
        cold_proof_bytes as f64 / warm_proof_bytes as f64,
        proofs.warm_nodes,
        proofs.cold_nodes,
        proofs.warm_terms,
        proofs.cold_terms,
        warm_verify_ns,
        cold_verify_ns,
        cold_verify_ns as f64 / warm_verify_ns as f64,
    );
    Ok(())
}

fn main() -> ExitCode {
    match parse() {
        Ok(None) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(options)) => match run(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("index-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("index-bench: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
