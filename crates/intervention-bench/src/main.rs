#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus, CohomologyLimits,
    CohomologySpace, SparseDistanceMatrix, cohomology_restriction, cohomology_space,
};
use holos_tda_check::{
    ProofLimits, VerifiedCohomologyInterventionStatus, verify_cohomology_intervention,
};

type WeightedEdge = (usize, usize, f64);

struct Args {
    dimension: usize,
    components: usize,
    distractors: usize,
    modulus: u32,
    reps: usize,
}

struct Fixture {
    graph: SparseDistanceMatrix,
    base_edges: Vec<WeightedEdge>,
    space: CohomologySpace,
    scenarios: Vec<CohomologyInterventionScenario>,
    candidates: Vec<CohomologyInterventionCandidate>,
    target_basis: Vec<usize>,
    target_cost: u64,
}

struct Reference {
    selected: Vec<usize>,
    cost: u64,
    examined: u128,
}

fn arguments() -> Result<Args, String> {
    let values = std::env::args().skip(1).collect::<Vec<_>>();
    let value = |name: &str, default: &str| -> String {
        values
            .windows(2)
            .find(|pair| pair[0] == name)
            .map_or(default, |pair| pair[1].as_str())
            .to_owned()
    };
    let parse = |name: &str, default: &str| -> Result<usize, String> {
        value(name, default)
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    };
    Ok(Args {
        dimension: parse("--dimension", "1")?,
        components: parse("--components", "2")?,
        distractors: parse("--distractors", "8")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn fixture(args: &Args, scale: f64, limits: CohomologyLimits) -> Result<Fixture, String> {
    let isolated = args.distractors + 1;
    let component_vertices = 2 * (args.dimension + 1);
    let vertex_count = isolated + args.components * component_vertices;
    let mut base_edges = Vec::new();
    let mut true_candidates = Vec::new();
    for component in 0..args.components {
        let offset = isolated + component * component_vertices;
        for local_u in 0..component_vertices {
            for local_v in (local_u + 1)..component_vertices {
                if local_u / 2 != local_v / 2 {
                    base_edges.push((offset + local_u, offset + local_v, 1.0));
                }
            }
        }
        true_candidates.push(CohomologyInterventionCandidate::new(
            offset,
            offset + 1,
            3 + 2 * component as u64,
        ));
    }
    let graph = SparseDistanceMatrix::from_triplets(vertex_count, &base_edges)
        .map_err(|error| error.to_string())?;
    let space = cohomology_space(&graph, args.dimension, scale, args.modulus, limits)
        .map_err(|error| error.to_string())?;
    if space.rank() != args.components {
        return Err("cross-polytope component rank differs from the fixture".into());
    }

    let mut candidates = (1..=args.distractors)
        .map(|vertex| CohomologyInterventionCandidate::new(0, vertex, 100 + vertex as u64))
        .chain(true_candidates.iter().copied())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    let true_positions = true_candidates
        .iter()
        .map(|candidate| {
            candidates
                .binary_search(candidate)
                .expect("each true candidate was inserted")
        })
        .collect::<Vec<_>>();
    if true_positions != (args.distractors..candidates.len()).collect::<Vec<_>>() {
        return Err("true candidates must follow distractors in search order".into());
    }

    let mut target_basis = Vec::with_capacity(args.components);
    for position in true_positions {
        let edited = edited_graph(vertex_count, &base_edges, &candidates, &[position], scale)?;
        let edited_space = cohomology_space(&edited, args.dimension, scale, args.modulus, limits)
            .map_err(|error| error.to_string())?;
        let restriction = cohomology_restriction(&edited, &edited_space, &graph, &space)
            .map_err(|error| error.to_string())?;
        let killed = space
            .basis()
            .iter()
            .enumerate()
            .filter_map(|(basis, class)| (!restriction.image_contains(class.id)).then_some(basis))
            .collect::<Vec<_>>();
        if killed.len() != 1 {
            return Err("one component fill must kill exactly one canonical class".into());
        }
        target_basis.push(killed[0]);
    }
    if target_basis.iter().copied().collect::<BTreeSet<_>>().len() != args.components {
        return Err("component fills must kill distinct canonical classes".into());
    }
    let scenarios = target_basis
        .iter()
        .map(|target| {
            CohomologyInterventionScenario::from_graph(&graph, scale, *target)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let target_cost = true_candidates.iter().map(|candidate| candidate.cost).sum();
    Ok(Fixture {
        graph,
        base_edges,
        space,
        scenarios,
        candidates,
        target_basis,
        target_cost,
    })
}

fn edited_graph(
    vertex_count: usize,
    base_edges: &[WeightedEdge],
    candidates: &[CohomologyInterventionCandidate],
    selected: &[usize],
    scale: f64,
) -> Result<SparseDistanceMatrix, String> {
    let mut edges = base_edges.to_vec();
    edges.extend(selected.iter().map(|position| {
        let edge = candidates[*position].edge;
        (edge.u, edge.v, scale)
    }));
    SparseDistanceMatrix::from_triplets(vertex_count, &edges).map_err(|error| error.to_string())
}

fn kills_all(
    fixture: &Fixture,
    selected: &[usize],
    args: &Args,
    scale: f64,
    limits: CohomologyLimits,
) -> Result<bool, String> {
    let edited = edited_graph(
        fixture.graph.len(),
        &fixture.base_edges,
        &fixture.candidates,
        selected,
        scale,
    )?;
    let edited_space = cohomology_space(&edited, args.dimension, scale, args.modulus, limits)
        .map_err(|error| error.to_string())?;
    let restriction =
        cohomology_restriction(&edited, &edited_space, &fixture.graph, &fixture.space)
            .map_err(|error| error.to_string())?;
    Ok(fixture
        .target_basis
        .iter()
        .all(|basis| !restriction.image_contains(fixture.space.basis()[*basis].id)))
}

fn exhaustive_reference(
    fixture: &Fixture,
    args: &Args,
    scale: f64,
    limits: CohomologyLimits,
) -> Result<Reference, String> {
    let mut state = ExhaustiveState {
        fixture,
        args,
        scale,
        limits,
        best: None,
        examined: 0,
    };
    for size in 1..=args.components {
        state.visit(size, 0, &mut Vec::with_capacity(size))?;
    }
    let (cost, selected) = state
        .best
        .ok_or_else(|| "exhaustive reference found no feasible intervention".to_string())?;
    Ok(Reference {
        selected,
        cost,
        examined: state.examined,
    })
}

struct ExhaustiveState<'a> {
    fixture: &'a Fixture,
    args: &'a Args,
    scale: f64,
    limits: CohomologyLimits,
    best: Option<(u64, Vec<usize>)>,
    examined: u128,
}

impl ExhaustiveState<'_> {
    fn visit(
        &mut self,
        remaining: usize,
        start: usize,
        selected: &mut Vec<usize>,
    ) -> Result<(), String> {
        if remaining == 0 {
            self.examined += 1;
            if kills_all(self.fixture, selected, self.args, self.scale, self.limits)? {
                let cost = selected
                    .iter()
                    .try_fold(0u64, |sum, position| {
                        sum.checked_add(self.fixture.candidates[*position].cost)
                    })
                    .ok_or_else(|| "reference intervention cost overflows".to_string())?;
                if self.best.as_ref().is_none_or(|(best_cost, best)| {
                    cost < *best_cost || (cost == *best_cost && selected.as_slice() < best)
                }) {
                    self.best = Some((cost, selected.clone()));
                }
            }
            return Ok(());
        }
        let end = self.fixture.candidates.len() - remaining;
        for position in start..=end {
            selected.push(position);
            self.visit(remaining - 1, position + 1, selected)?;
            selected.pop();
        }
        Ok(())
    }
}

fn binomial(count: usize, chosen: usize) -> u128 {
    (0..chosen).fold(1u128, |value, position| {
        value * (count - position) as u128 / (position + 1) as u128
    })
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if !(1..=3).contains(&args.dimension)
        || args.components == 0
        || args.components > 8
        || args.distractors == 0
        || args.distractors > 64
        || args.reps < 5
    {
        return Err("dimension must be 1 through 3, components 1 through 8, distractors 1 through 64, and reps at least 5".into());
    }
    let scale = 2.0;
    let cohomology_limits = CohomologyLimits::default();
    let intervention_limits = CohomologyInterventionLimits::default();
    let fixture = fixture(&args, scale, cohomology_limits)?;
    let build = || {
        CohomologyInterventionArtifact::build(
            fixture.graph.len(),
            args.dimension,
            scale,
            args.modulus,
            &fixture.scenarios,
            &fixture.candidates,
            args.components,
            intervention_limits,
        )
        .map_err(|error| error.to_string())
    };
    let artifact = build()?;
    let proof = artifact
        .encode(intervention_limits)
        .map_err(|error| error.to_string())?;
    let checked = verify_cohomology_intervention(&proof, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    let reference = exhaustive_reference(&fixture, &args, scale, cohomology_limits)?;
    let expected_subsets = (1..=args.components)
        .map(|chosen| binomial(fixture.candidates.len(), chosen))
        .sum::<u128>();
    let selected = artifact
        .edits()
        .iter()
        .map(|candidate| {
            fixture
                .candidates
                .binary_search(candidate)
                .expect("known edit")
        })
        .collect::<Vec<_>>();
    if artifact.status() != CohomologyInterventionStatus::Optimal
        || checked.status != VerifiedCohomologyInterventionStatus::Optimal
        || artifact.before_ranks() != vec![args.components; args.components]
        || artifact.after_ranks() != vec![0; args.components]
        || artifact.edits().len() != args.components
        || artifact.lower_bound_cost() != Some(fixture.target_cost)
        || artifact.upper_bound_cost() != Some(fixture.target_cost)
        || artifact.root_blocker_bound() != fixture.target_cost
        || artifact.root_blockers().len() != args.components
        || checked.total_cost != Some(fixture.target_cost)
        || reference.cost != fixture.target_cost
        || reference.selected != selected
        || reference.examined != expected_subsets
    {
        return Err("certified and exhaustive intervention decisions differ".into());
    }

    let mut build_times = Vec::with_capacity(args.reps);
    let mut exhaustive_times = Vec::with_capacity(args.reps);
    let mut checker_times = Vec::with_capacity(args.reps);
    for repetition in 0..args.reps {
        if repetition % 2 == 0 {
            let start = Instant::now();
            black_box(build())?;
            build_times.push(start.elapsed().as_nanos());

            let start = Instant::now();
            black_box(exhaustive_reference(
                &fixture,
                &args,
                scale,
                cohomology_limits,
            ))?;
            exhaustive_times.push(start.elapsed().as_nanos());
        } else {
            let start = Instant::now();
            black_box(exhaustive_reference(
                &fixture,
                &args,
                scale,
                cohomology_limits,
            ))?;
            exhaustive_times.push(start.elapsed().as_nanos());

            let start = Instant::now();
            black_box(build())?;
            build_times.push(start.elapsed().as_nanos());
        }
        let start = Instant::now();
        black_box(verify_cohomology_intervention(
            &proof,
            ProofLimits::default(),
        ))
        .map_err(|error| error.to_string())?;
        checker_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-intervention-bench-v1 dimension={} modulus={} components={} vertices={} active_edges={} candidates={} distractors={} reps={} rank={} status={} edits={} optimal_cost={} lower_bound={} root_blockers={} oracle_calls={} search_nodes={} cache_hits={} exhaustive_subsets={} proof_bytes={} build_ns={} exhaustive_ns={} checker_ns={}",
        args.dimension,
        args.modulus,
        args.components,
        fixture.graph.len(),
        fixture.base_edges.len(),
        fixture.candidates.len(),
        args.distractors,
        args.reps,
        fixture.space.rank(),
        artifact.status(),
        artifact.edits().len(),
        fixture.target_cost,
        artifact.root_blocker_bound(),
        artifact.root_blockers().len(),
        artifact.oracle_calls(),
        artifact.search_nodes(),
        artifact.cache_hits(),
        reference.examined,
        proof.len(),
        median(build_times),
        median(exhaustive_times),
        median(checker_times),
    );
    Ok(())
}
