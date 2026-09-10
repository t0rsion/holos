//! Deterministic graph and trajectory construction.

use holos_tda::{
    CertificateLimits, CorrespondenceMode, EdgeKey, PersistenceProgram, ProgramUpdateMode,
    RipsParams, SparseDistanceMatrix,
};

use crate::args::Options;

pub(crate) struct RepairTrajectory {
    pub(crate) graphs: Vec<SparseDistanceMatrix>,
    pub(crate) repaired_atoms: usize,
    pub(crate) rebuilt_atoms: usize,
}

struct AcceptedRepair {
    candidate: SparseDistanceMatrix,
    program: PersistenceProgram,
    repaired_atoms: usize,
    rebuilt_atoms: usize,
}

pub(crate) fn graph(options: &Options) -> Result<SparseDistanceMatrix, String> {
    let mut endpoints = Vec::new();
    for atom in 0..options.atoms {
        let first = 1 + atom * (options.atom_vertices - 1);
        let vertices: Vec<_> = std::iter::once(0)
            .chain(first..first + options.atom_vertices - 1)
            .collect();
        for v in 1..vertices.len() {
            for u in 0..v {
                endpoints.push((vertices[u], vertices[v]));
            }
        }
    }
    let mut state = options.seed;
    let mut order: Vec<_> = (0..endpoints.len())
        .map(|index| (next_random(&mut state), index))
        .collect();
    order.sort_unstable();
    let mut rank = vec![0; endpoints.len()];
    for (position, &(_, index)) in order.iter().enumerate() {
        rank[index] = position;
    }
    let denominator = endpoints.len() as f64 + 1.0;
    let triplets: Vec<_> = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| (u, v, 1.0 + 8.0 * (rank[index] + 1) as f64 / denominator))
        .collect();
    let vertices = 1 + options.atoms * (options.atom_vertices - 1);
    SparseDistanceMatrix::from_triplets(vertices, &triplets).map_err(|error| error.to_string())
}

pub(crate) fn accepted_trajectory(
    input: &SparseDistanceMatrix,
    options: &Options,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    let initial_order = edge_order(input);
    let updates = (1..=options.steps)
        .map(|step| {
            let triplets: Vec<_> = input
                .edges()
                .map(|(u, v, weight)| {
                    let labeled = if u == 0 { v } else { u };
                    let atom = (labeled - 1) / (options.atom_vertices - 1);
                    let offset = (step * (atom + 1)) as f64 * 1e-2;
                    (u, v, weight + offset)
                })
                .collect();
            SparseDistanceMatrix::from_triplets(input.len(), &triplets)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !updates
        .iter()
        .any(|updated| edge_order(updated) != initial_order)
    {
        return Err("accepted trajectory did not cross the complete edge order".into());
    }
    Ok(updates)
}

pub(crate) fn repair_trajectory(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    steps: usize,
) -> Result<RepairTrajectory, String> {
    let mut program = PersistenceProgram::compile(input, params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let cyclic: Vec<_> = program
        .atoms()
        .iter()
        .filter(|atom| atom.cyclic)
        .map(|atom| atom.edges.clone())
        .collect();
    let mut current = input.clone();
    let mut graphs = Vec::with_capacity(steps);
    let mut repaired_atoms = 0;
    let mut rebuilt_atoms = 0;
    for step in 0..steps {
        let edges = &cyclic[step % cyclic.len()];
        let accepted = find_repair(&program, &current, edges)?;
        let accepted = accepted
            .ok_or_else(|| format!("no one-atom guard-crossing update exists at step {step}"))?;
        repaired_atoms += accepted.repaired_atoms;
        rebuilt_atoms += accepted.rebuilt_atoms;
        let candidate = accepted.candidate;
        current = candidate.clone();
        program = accepted.program;
        graphs.push(candidate);
    }
    Ok(RepairTrajectory {
        graphs,
        repaired_atoms,
        rebuilt_atoms,
    })
}

fn find_repair(
    program: &PersistenceProgram,
    current: &SparseDistanceMatrix,
    edges: &[EdgeKey],
) -> Result<Option<AcceptedRepair>, String> {
    for left in 0..edges.len() {
        for right in left + 1..edges.len() {
            let candidate = swap_weights(current, edges[left], edges[right])?;
            let mut trial = program.clone();
            let update = trial
                .advance_with(&candidate, CorrespondenceMode::Omit)
                .map_err(|error| error.to_string())?;
            let changed_atoms = update.work.atoms_repaired + update.work.atoms_rebuilt;
            if update.mode == ProgramUpdateMode::Repaired
                && update.work.atoms_touched == 1
                && changed_atoms == 1
            {
                return Ok(Some(AcceptedRepair {
                    candidate,
                    program: trial,
                    repaired_atoms: update.work.atoms_repaired,
                    rebuilt_atoms: update.work.atoms_rebuilt,
                }));
            }
        }
    }
    Ok(None)
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn edge_order(input: &SparseDistanceMatrix) -> Vec<EdgeKey> {
    let mut weighted: Vec<_> = input
        .edges()
        .map(|(u, v, weight)| (weight, EdgeKey { u, v }))
        .collect();
    weighted.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
    weighted.into_iter().map(|(_, edge)| edge).collect()
}

fn swap_weights(
    input: &SparseDistanceMatrix,
    first: EdgeKey,
    second: EdgeKey,
) -> Result<SparseDistanceMatrix, String> {
    let first_weight = input.get(first.u, first.v);
    let second_weight = input.get(second.u, second.v);
    let triplets = input
        .edges()
        .map(|(u, v, value)| {
            let edge = EdgeKey { u, v };
            let value = if edge == first {
                second_weight
            } else if edge == second {
                first_weight
            } else {
                value
            };
            (u, v, value)
        })
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(input.len(), &triplets).map_err(|error| error.to_string())
}
