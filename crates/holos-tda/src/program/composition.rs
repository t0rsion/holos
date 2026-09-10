use std::collections::{BTreeMap, BTreeSet};

use crate::classes::{basis_class_id, canonical_space_basis, group_id, validate_h1_cocycle};
use crate::factorization::{
    FactorizationSummary, ProgramBlock, ProgramDecompositionSummary, program_blocks,
};
use crate::{
    AtlasArtifact, Bar, CertificateLimits, Cocycle, CocycleTerm, CriticalPair, EdgeKey, Error,
    ExplainedDiagram, PersistentClass, PersistentClassSpace, Result, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse,
};

use super::model::{PersistenceProgram, ProgramAtomInfo, ProgramAtomState, ProgramSummary};
use super::topology::{
    bar_bits_equal, critical_pair_key, critical_pair_order, diagram_bits_equal, h0_diagram,
    h0_provenance, map_critical_pair, previous_float, terminal_level,
};

impl PersistenceProgram {
    /// Compile a compositional H0 and H1 program.
    ///
    /// Each cyclic atom receives its own reduction certificate. Graphs
    /// without a useful split remain one exact atom.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> Result<Self> {
        if params.max_dim != 1 {
            return Err(Error::InvalidInput(
                "a persistence program requires max_dim equal to 1".into(),
            ));
        }
        let (factorization, blocks) = program_blocks(input, params.threshold)?;
        let (atoms, states) = compile_atoms(input, params, limits, &blocks)?;
        let result = compose_result(input, params, &states)?;
        let expected = rips_persistence_sparse(input, params)?;
        if !diagram_bits_equal(&result.diagram, &expected) {
            return Err(Error::InvalidInput(format!(
                "compositional and monolithic diagrams differ: expected {:?}, got {:?}",
                expected.bars, result.diagram.bars
            )));
        }
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let threshold = params.threshold.unwrap_or(f64::INFINITY);
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let summary = program_summary(factorization, &atoms, &states);
        let separator_edges = wider_separator_edges(&atoms);
        let (h0_deaths, h0_essential, _) = h0_provenance(input, params.threshold);
        Ok(Self {
            params: params.clone(),
            limits,
            graph: input.clone(),
            topology,
            active,
            separator_edges,
            h0_deaths,
            h0_essential,
            atoms,
            states,
            summary,
            result,
        })
    }

    pub(crate) fn from_verified_parts(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
        atoms: Vec<ProgramAtomInfo>,
        states: Vec<ProgramAtomState>,
        result: ExplainedDiagram,
    ) -> Self {
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let threshold = params.threshold.unwrap_or(f64::INFINITY);
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let factorization = FactorizationSummary {
            blocks: atoms.len(),
            cyclic_blocks: atoms.iter().filter(|atom| atom.cyclic).count(),
            bridge_edges: atoms.iter().filter(|atom| !atom.cyclic).count(),
            cyclic_edges: atoms
                .iter()
                .filter(|atom| atom.cyclic)
                .map(|atom| atom.edges.len())
                .sum(),
            largest_cyclic_block_edges: atoms
                .iter()
                .filter(|atom| atom.cyclic)
                .map(|atom| atom.edges.len())
                .max()
                .unwrap_or(0),
        };
        let (articulation_vertices, zero_simplex_separators, widest_separator) =
            separator_stats(&atoms);
        let summary = program_summary(
            ProgramDecompositionSummary {
                articulation: factorization,
                articulation_vertices,
                zero_simplex_separators,
                widest_separator,
                separator_candidates_checked: 0,
                separator_search_complete: true,
            },
            &atoms,
            &states,
        );
        let separator_edges = wider_separator_edges(&atoms);
        let (h0_deaths, h0_essential, _) = h0_provenance(input, params.threshold);
        Self {
            params: params.clone(),
            limits,
            graph: input.clone(),
            topology,
            active,
            separator_edges,
            h0_deaths,
            h0_essential,
            atoms,
            states,
            summary,
            result,
        }
    }
}

fn compile_atoms(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    limits: CertificateLimits,
    blocks: &[ProgramBlock],
) -> Result<(Vec<ProgramAtomInfo>, Vec<ProgramAtomState>)> {
    let atoms = atom_infos(input, blocks);
    let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
    let mut states = Vec::new();
    for atom in &atoms {
        if !atom.cyclic {
            continue;
        }
        let local = local_matrix(&atom.vertices, &atom.edges, input)?;
        let (artifact, _) = AtlasArtifact::compile(&local, &atom_params(params), limits)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let region = artifact
            .reduction_certificate()
            .compile_region(&local, limits)?;
        states.push(ProgramAtomState {
            info_index: atom.id,
            vertices: atom.vertices.clone(),
            edges: atom.edges.clone(),
            edge_positions: atom
                .edges
                .iter()
                .map(|edge| {
                    topology
                        .binary_search(edge)
                        .expect("atom edge is in the program topology")
                })
                .collect(),
            explained: artifact.explained().clone(),
            artifact,
            certified_graph: local,
            region,
        });
    }
    Ok((atoms, states))
}

pub(crate) fn atom_infos(
    input: &SparseDistanceMatrix,
    blocks: &[ProgramBlock],
) -> Vec<ProgramAtomInfo> {
    let mut counts = vec![0usize; input.len()];
    for block in blocks {
        for &vertex in &block.vertices {
            counts[vertex] += 1;
        }
    }
    let atoms: Vec<_> = blocks
        .iter()
        .enumerate()
        .map(|(id, block)| ProgramAtomInfo {
            id,
            vertices: block.vertices.clone(),
            edges: block
                .edges
                .iter()
                .map(|&[u, v]| EdgeKey::new(u, v))
                .collect(),
            separator_vertices: block
                .vertices
                .iter()
                .copied()
                .filter(|&vertex| counts[vertex] > 1)
                .collect(),
            cyclic: block.edges.len() >= block.vertices.len(),
        })
        .collect();
    atoms
}

pub(crate) fn atom_params(params: &RipsParams) -> RipsParams {
    let mut atom = RipsParams::new(1).with_modulus(params.modulus);
    atom.threshold = params.threshold;
    atom
}

pub(crate) fn local_matrix(
    vertices: &[usize],
    edges: &[EdgeKey],
    input: &SparseDistanceMatrix,
) -> Result<SparseDistanceMatrix> {
    let triplets: Vec<_> = edges
        .iter()
        .map(|edge| {
            let u = vertices
                .binary_search(&edge.u)
                .expect("atom contains its edge endpoint");
            let v = vertices
                .binary_search(&edge.v)
                .expect("atom contains its edge endpoint");
            (u, v, input.get(edge.u, edge.v))
        })
        .collect();
    SparseDistanceMatrix::from_triplets(vertices.len(), &triplets)
}

fn program_summary(
    decomposition: ProgramDecompositionSummary,
    atoms: &[ProgramAtomInfo],
    states: &[ProgramAtomState],
) -> ProgramSummary {
    debug_assert!(decomposition.articulation.blocks <= atoms.len());
    ProgramSummary {
        atoms: atoms.len(),
        cyclic_atoms: atoms.iter().filter(|atom| atom.cyclic).count(),
        articulation_vertices: decomposition.articulation_vertices,
        zero_simplex_separators: decomposition.zero_simplex_separators,
        widest_separator: decomposition.widest_separator,
        separator_candidates_checked: decomposition.separator_candidates_checked,
        separator_search_complete: decomposition.separator_search_complete,
        largest_cyclic_atom_edges: atoms
            .iter()
            .filter(|atom| atom.cyclic)
            .map(|atom| atom.edges.len())
            .max()
            .unwrap_or(0),
        complete_guards: states
            .iter()
            .map(|state| state.region.complete_guards().len())
            .sum(),
        guards: states.iter().map(|state| state.region.guards().len()).sum(),
    }
}

fn separator_stats(atoms: &[ProgramAtomInfo]) -> (usize, usize, usize) {
    let mut separators = BTreeSet::new();
    let mut articulations = BTreeSet::new();
    for (position, left) in atoms.iter().enumerate() {
        for right in &atoms[position + 1..] {
            let intersection: Vec<_> = left
                .vertices
                .iter()
                .copied()
                .filter(|vertex| right.vertices.binary_search(vertex).is_ok())
                .collect();
            if intersection.len() > 1 {
                separators.insert(intersection);
            } else if let Some(&vertex) = intersection.first() {
                articulations.insert(vertex);
            }
        }
    }
    let widest = separators.iter().map(Vec::len).max().unwrap_or(1);
    (articulations.len(), separators.len(), widest)
}

fn wider_separator_edges(atoms: &[ProgramAtomInfo]) -> Vec<EdgeKey> {
    let mut edges = BTreeSet::new();
    for (position, left) in atoms.iter().enumerate() {
        for right in &atoms[position + 1..] {
            let intersection: Vec<_> = left
                .vertices
                .iter()
                .copied()
                .filter(|vertex| right.vertices.binary_search(vertex).is_ok())
                .collect();
            if intersection.len() < 2 {
                continue;
            }
            for (position, &u) in intersection.iter().enumerate() {
                for &v in &intersection[position + 1..] {
                    edges.insert(EdgeKey::new(u, v));
                }
            }
        }
    }
    edges.into_iter().collect()
}

pub(crate) fn compose_result(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    states: &[ProgramAtomState],
) -> Result<ExplainedDiagram> {
    let mut seeds = Vec::new();
    let terminal = terminal_level(input, params.threshold);
    for state in states {
        for space in &state.explained.spaces {
            let interval = space.interval;
            let scale = if interval.death.is_finite() {
                previous_float(interval.death)
            } else {
                terminal
            };
            let cocycles = space
                .basis
                .iter()
                .map(|class| Cocycle {
                    modulus: class.cocycle.modulus,
                    scale,
                    terms: class
                        .cocycle
                        .terms
                        .iter()
                        .map(|term| CocycleTerm {
                            u: state.vertices[term.u],
                            v: state.vertices[term.v],
                            coefficient: term.coefficient,
                        })
                        .collect(),
                })
                .collect();
            let critical_pairs = space
                .critical_pairs
                .iter()
                .map(|pair| map_critical_pair(pair, &state.vertices))
                .collect();
            seeds.push(SpaceSeed {
                interval,
                cocycles,
                critical_pairs,
            });
        }
    }
    let spaces = merge_spaces(input, params.modulus, seeds)?;
    let (mut diagram, _) = h0_diagram(input, params.threshold);
    for space in &spaces {
        diagram
            .bars
            .extend(std::iter::repeat_n(space.interval, space.basis.len()));
    }
    diagram.canonicalize();
    Ok(ExplainedDiagram { diagram, spaces })
}

#[derive(Debug)]
struct SpaceSeed {
    interval: Bar,
    cocycles: Vec<Cocycle>,
    critical_pairs: Vec<CriticalPair>,
}

fn merge_spaces(
    input: &SparseDistanceMatrix,
    modulus: u32,
    seeds: Vec<SpaceSeed>,
) -> Result<Vec<PersistentClassSpace>> {
    let mut groups: BTreeMap<(u64, u64), SpaceSeed> = BTreeMap::new();
    for seed in seeds {
        let key = (seed.interval.birth.to_bits(), seed.interval.death.to_bits());
        let group = groups.entry(key).or_insert_with(|| SpaceSeed {
            interval: seed.interval,
            cocycles: Vec::new(),
            critical_pairs: Vec::new(),
        });
        group.cocycles.extend(seed.cocycles);
        group.critical_pairs.extend(seed.critical_pairs);
    }
    let mut spaces = Vec::with_capacity(groups.len());
    for mut seed in groups.into_values() {
        let cocycles = canonical_space_basis(input, modulus, &seed.cocycles)?;
        if cocycles.len() != seed.critical_pairs.len() {
            return Err(Error::InvalidInput(format!(
                "composed class-space rank {} differs from critical-pair count {}",
                cocycles.len(),
                seed.critical_pairs.len()
            )));
        }
        for cocycle in &cocycles {
            validate_h1_cocycle(input, cocycle)?;
        }
        seed.critical_pairs.sort_by(critical_pair_order);
        let id = group_id(seed.interval, modulus, &cocycles);
        let basis = cocycles
            .into_iter()
            .enumerate()
            .map(|(basis_index, cocycle)| PersistentClass {
                id: basis_class_id(id, basis_index, &cocycle),
                group_id: id,
                basis_index,
                interval: seed.interval,
                cocycle,
                provenance: None,
            })
            .collect();
        spaces.push(PersistentClassSpace {
            id,
            interval: seed.interval,
            basis,
            critical_pairs: seed.critical_pairs,
        });
    }
    spaces.sort_by(|a, b| {
        a.interval
            .birth
            .total_cmp(&b.interval.birth)
            .then(a.interval.death.total_cmp(&b.interval.death))
            .then(a.id.cmp(&b.id))
    });
    Ok(spaces)
}

pub(super) fn reweight_explained(
    input: &SparseDistanceMatrix,
    previous: &ExplainedDiagram,
    evaluation: &crate::CertifiedRegionEvaluation,
    modulus: u32,
    threshold: Option<f64>,
) -> Result<Option<ExplainedDiagram>> {
    if evaluation.h1_critical_pairs().len() != previous.class_count() {
        return Ok(None);
    }
    let records: BTreeMap<_, _> = evaluation
        .h1_critical_pairs()
        .iter()
        .map(|(bar, pair)| (critical_pair_key(pair), (*bar, pair.clone())))
        .collect();
    let terminal = terminal_level(input, threshold);
    let mut seeds = Vec::new();
    for space in &previous.spaces {
        let mut interval = None;
        let mut pairs = Vec::with_capacity(space.critical_pairs.len());
        for pair in &space.critical_pairs {
            let Some((bar, updated_pair)) = records.get(&critical_pair_key(pair)) else {
                return Ok(None);
            };
            if interval.is_some_and(|interval: Bar| !bar_bits_equal(interval, *bar)) {
                return Ok(None);
            }
            interval = Some(*bar);
            pairs.push(updated_pair.clone());
        }
        let Some(interval) = interval else {
            return Ok(None);
        };
        let scale = if interval.death.is_finite() {
            previous_float(interval.death)
        } else {
            terminal
        };
        let cocycles: Vec<_> = space
            .basis
            .iter()
            .map(|class| Cocycle {
                modulus,
                scale,
                terms: class.cocycle.terms.clone(),
            })
            .collect();
        if cocycles
            .iter()
            .any(|cocycle| validate_h1_cocycle(input, cocycle).is_err())
        {
            return Ok(None);
        }
        seeds.push(SpaceSeed {
            interval,
            cocycles,
            critical_pairs: pairs,
        });
    }
    let spaces = merge_spaces(input, modulus, seeds)?;
    Ok(Some(ExplainedDiagram {
        diagram: evaluation.diagram().clone(),
        spaces,
    }))
}
