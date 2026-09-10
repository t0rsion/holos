use std::collections::BTreeMap;

use crate::proof::{ProofBar, ProofError, canonicalize_diagram, h0_diagram};

use super::claim::CocycleTermClaim;
use super::graph::ProgramGraph;
use super::trace_model::{AtomState, GlobalPair, ResultClass, ResultSpace, ResultState};
use super::trace_validation::validate_cocycle;
use super::verify::{basis_class_id, canonical_basis, group_id};

pub(super) fn compose_result(
    graph: &ProgramGraph,
    modulus: u32,
    threshold: Option<f64>,
    atoms: &[AtomState],
    max_bars: usize,
) -> Result<ResultState, ProofError> {
    let proof_graph = graph.proof_graph();
    let groups = collect_seeds(graph, threshold, atoms);
    let spaces = build_result_spaces(&proof_graph, modulus, groups)?;
    let diagram = compose_diagram(&proof_graph, threshold, &spaces, max_bars)?;
    Ok(ResultState { diagram, spaces })
}

fn collect_seeds(
    graph: &ProgramGraph,
    threshold: Option<f64>,
    atoms: &[AtomState],
) -> BTreeMap<(u64, u64), Seed> {
    let terminal = terminal_level(graph, threshold);
    let mut groups: BTreeMap<(u64, u64), Seed> = BTreeMap::new();
    for atom in atoms {
        for space in &atom.spaces {
            let scale = if space.interval.death.is_finite() {
                previous_float(space.interval.death)
            } else {
                terminal
            };
            let group = groups
                .entry((
                    space.interval.birth.to_bits(),
                    space.interval.death.to_bits(),
                ))
                .or_insert_with(|| Seed {
                    interval: space.interval,
                    scale,
                    terms: Vec::new(),
                    critical_pairs: Vec::new(),
                });
            group.terms.extend(space.basis.iter().map(|basis| {
                basis
                    .iter()
                    .map(|term| CocycleTermClaim {
                        u: atom.vertices[term.u],
                        v: atom.vertices[term.v],
                        coefficient: term.coefficient,
                    })
                    .collect::<Vec<_>>()
            }));
            group
                .critical_pairs
                .extend(space.critical_pairs.iter().map(|pair| {
                    GlobalPair {
                        birth: pair
                            .birth
                            .iter()
                            .map(|&vertex| atom.vertices[vertex])
                            .collect(),
                        death: pair.death.as_ref().map(|death| {
                            death.iter().map(|&vertex| atom.vertices[vertex]).collect()
                        }),
                    }
                }));
        }
    }
    groups
}

fn build_result_spaces(
    proof_graph: &crate::proof::Graph,
    modulus: u32,
    groups: BTreeMap<(u64, u64), Seed>,
) -> Result<Vec<ResultSpace>, ProofError> {
    let mut spaces = Vec::with_capacity(groups.len());
    for seed in groups.into_values() {
        let basis = canonical_basis(proof_graph, modulus, seed.scale, &seed.terms)?;
        if basis.len() != seed.critical_pairs.len() {
            return Err(ProofError::new(
                "composed class-space rank differs from critical-pair count",
            ));
        }
        for terms in &basis {
            validate_cocycle(proof_graph, modulus, seed.scale, terms)?;
        }
        let id = group_id(seed.interval, modulus, seed.scale, &basis);
        let classes = basis
            .into_iter()
            .enumerate()
            .map(|(index, terms)| ResultClass {
                id: basis_class_id(id, index, seed.scale, &terms),
                terms,
            })
            .collect();
        let mut critical_pairs = seed.critical_pairs;
        critical_pairs.sort_by(global_pair_order);
        spaces.push(ResultSpace {
            id,
            interval: seed.interval,
            scale: seed.scale,
            basis: classes,
        });
    }
    spaces.sort_by(|left, right| {
        left.interval
            .birth
            .total_cmp(&right.interval.birth)
            .then(left.interval.death.total_cmp(&right.interval.death))
            .then(left.id.cmp(&right.id))
    });
    Ok(spaces)
}

fn compose_diagram(
    proof_graph: &crate::proof::Graph,
    threshold: Option<f64>,
    spaces: &[ResultSpace],
    max_bars: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let mut diagram = h0_diagram(proof_graph, threshold)?;
    if diagram.len() > max_bars {
        return Err(ProofError::new("composed H0 diagram exceeds the bar limit"));
    }
    for space in spaces {
        let next = diagram
            .len()
            .checked_add(space.basis.len())
            .ok_or_else(|| ProofError::new("composed diagram bar count overflows usize"))?;
        if next > max_bars {
            return Err(ProofError::new("composed diagram exceeds the bar limit"));
        }
        diagram.extend(std::iter::repeat_n(space.interval, space.basis.len()));
    }
    canonicalize_diagram(&mut diagram);
    Ok(diagram)
}

struct Seed {
    interval: ProofBar,
    scale: f64,
    terms: Vec<Vec<CocycleTermClaim>>,
    critical_pairs: Vec<GlobalPair>,
}

pub(super) fn diagram_equal(left: &[ProofBar], right: &[ProofBar]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension == right.dimension
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

pub(super) fn bar_equal(left: ProofBar, right: ProofBar) -> bool {
    left.dimension == right.dimension
        && left.birth.to_bits() == right.birth.to_bits()
        && left.death.to_bits() == right.death.to_bits()
}

fn global_pair_order(left: &GlobalPair, right: &GlobalPair) -> std::cmp::Ordering {
    left.birth
        .cmp(&right.birth)
        .then_with(|| match (&left.death, &right.death) {
            (Some(left), Some(right)) => left.cmp(right),
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

fn previous_float(value: f64) -> f64 {
    f64::from_bits(value.to_bits() - 1)
}

pub(super) fn terminal_level(graph: &ProgramGraph, threshold: Option<f64>) -> f64 {
    threshold.unwrap_or_else(|| {
        graph
            .edges()
            .iter()
            .map(|edge| edge.value)
            .fold(0.0, f64::max)
    })
}

pub(super) fn same_optional_f64(left: Option<f64>, right: Option<f64>) -> bool {
    left.map(f64::to_bits) == right.map(f64::to_bits)
}
