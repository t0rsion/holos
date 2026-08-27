#![forbid(unsafe_code)]

use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CohomologyLimits, SparseDistanceMatrix, SynthesisAction, SynthesisArtifact, SynthesisLimits,
    SynthesisState, TopologicalSpecification, cohomology_restriction, cohomology_space,
};
use holos_tda_check::{ProofLimits, VerifiedSynthesisStatus, verify_synthesis};

struct Args {
    fixture: String,
    dimension: usize,
    components: usize,
    distractors: usize,
    states: usize,
    modulus: u32,
    reps: usize,
}

struct Fixture {
    specification: TopologicalSpecification,
    actions: Vec<SynthesisAction>,
    optimum: u64,
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
        fixture: value("--fixture", "independent"),
        dimension: parse("--dimension", "1")?,
        components: parse("--components", "2")?,
        distractors: parse("--distractors", "8")?,
        states: parse("--states", "3")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn fixture(args: &Args) -> Result<Fixture, String> {
    match args.fixture.as_str() {
        "independent" => independent_fixture(args),
        "overlap" => overlap_fixture(args),
        _ => Err("fixture must be independent or overlap".into()),
    }
}

fn independent_fixture(args: &Args) -> Result<Fixture, String> {
    let scale = 1.0;
    let isolated = args.distractors + 1;
    let component_vertices = 2 * (args.dimension + 1);
    let vertex_count = isolated + args.components * component_vertices;
    let mut edges = Vec::new();
    let mut true_actions = Vec::new();
    for component in 0..args.components {
        let offset = isolated + component * component_vertices;
        for u in 0..component_vertices {
            for v in u + 1..component_vertices {
                if u / 2 != v / 2 {
                    edges.push((offset + u, offset + v, scale));
                }
            }
        }
        true_actions.push((offset, offset + 1, 3 + 2 * component as u64));
    }
    let graph = SparseDistanceMatrix::from_triplets(vertex_count, &edges)
        .map_err(|error| error.to_string())?;
    let space = cohomology_space(
        &graph,
        args.dimension,
        scale,
        args.modulus,
        CohomologyLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    if space.rank() != args.components {
        return Err("fixture cohomology rank differs from its component count".into());
    }
    let target = space.full_subspace();
    let states = (0..args.states)
        .map(|step| {
            SynthesisState::from_subspace(0, step as u64, &graph, scale, &space, &target, 0)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let specification =
        TopologicalSpecification::new(vertex_count, args.dimension, scale, args.modulus, states);
    let mut actions = (1..=args.distractors)
        .map(|vertex| SynthesisAction::throughout(0, vertex, 100 + vertex as u64, &specification))
        .chain(
            true_actions
                .iter()
                .map(|&(u, v, cost)| SynthesisAction::throughout(u, v, cost, &specification)),
        )
        .collect::<Vec<_>>();
    actions.sort();
    let optimum = true_actions.iter().map(|action| action.2).sum();
    Ok(Fixture {
        specification,
        actions,
        optimum,
    })
}

fn overlap_fixture(args: &Args) -> Result<Fixture, String> {
    if args.dimension != 1 || args.components != 2 || args.states != 3 {
        return Err("overlap needs dimension 1, components 2, and states 3".into());
    }
    let isolated = args.distractors + 1;
    let vertex_count = isolated + 4;
    let square = [
        (isolated, isolated + 1, 1.0),
        (isolated + 1, isolated + 2, 1.0),
        (isolated + 2, isolated + 3, 1.0),
        (isolated, isolated + 3, 1.0),
    ];
    let graph = SparseDistanceMatrix::from_triplets(vertex_count, &square)
        .map_err(|error| error.to_string())?;
    let space = cohomology_space(&graph, 1, 1.0, args.modulus, CohomologyLimits::default())
        .map_err(|error| error.to_string())?;
    if space.rank() != 1 {
        return Err("overlap fixture does not have one H1 class".into());
    }
    let target = space.full_subspace();
    let states = (0..3)
        .map(|step| {
            SynthesisState::from_subspace(0, step, &graph, 1.0, &space, &target, 0)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let specification = TopologicalSpecification::new(vertex_count, 1, 1.0, args.modulus, states);
    let mut actions = (1..=args.distractors)
        .map(|vertex| SynthesisAction::throughout(0, vertex, 100, &specification))
        .chain([
            SynthesisAction::new(isolated, isolated + 2, 1, vec![0, 1]),
            SynthesisAction::new(isolated, isolated + 2, 1, vec![0, 2]),
            SynthesisAction::new(isolated, isolated + 2, 1, vec![1, 2]),
        ])
        .collect::<Vec<_>>();
    actions.sort();
    Ok(Fixture {
        specification,
        actions,
        optimum: 2,
    })
}

fn feasible(fixture: &Fixture, selected: &[usize], args: &Args) -> Result<bool, String> {
    for (state_index, state) in fixture.specification.states().iter().enumerate() {
        let base_edges = state
            .active_edges()
            .iter()
            .map(|edge| (edge.u, edge.v, 0.0))
            .collect::<Vec<_>>();
        let mut edited_edges = base_edges.clone();
        edited_edges.extend(selected.iter().filter_map(|position| {
            let action = &fixture.actions[*position];
            action
                .states()
                .binary_search(&state_index)
                .is_ok()
                .then_some((action.edge.u, action.edge.v, 0.0))
        }));
        let base =
            SparseDistanceMatrix::from_triplets(fixture.specification.vertex_count(), &base_edges)
                .map_err(|error| error.to_string())?;
        let edited = SparseDistanceMatrix::from_triplets(
            fixture.specification.vertex_count(),
            &edited_edges,
        )
        .map_err(|error| error.to_string())?;
        let limits = CohomologyLimits::default();
        let base_space = cohomology_space(
            &base,
            args.dimension,
            fixture.specification.scale(),
            args.modulus,
            limits,
        )
        .map_err(|error| error.to_string())?;
        let target = base_space
            .subspace_from_coordinates(
                &state
                    .target()
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|term| (term.basis, term.coefficient))
                            .collect()
                    })
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| error.to_string())?;
        let edited_space = cohomology_space(
            &edited,
            args.dimension,
            fixture.specification.scale(),
            args.modulus,
            limits,
        )
        .map_err(|error| error.to_string())?;
        let restriction = cohomology_restriction(&edited, &edited_space, &base, &base_space)
            .map_err(|error| error.to_string())?;
        if restriction
            .image_intersection_rank(&base_space, &target)
            .map_err(|error| error.to_string())?
            > state.max_surviving_rank()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn exhaustive(fixture: &Fixture, args: &Args) -> Result<(u64, u128), String> {
    let mut best = None;
    let mut examined = 0u128;
    for size in 0..=args.components {
        visit_subsets(
            fixture,
            args,
            size,
            0,
            &mut Vec::new(),
            &mut examined,
            &mut best,
        )?;
    }
    best.map(|value| (value, examined))
        .ok_or_else(|| "flat synthesis reference found no plan".into())
}

#[allow(clippy::too_many_arguments)]
fn visit_subsets(
    fixture: &Fixture,
    args: &Args,
    remaining: usize,
    start: usize,
    selected: &mut Vec<usize>,
    examined: &mut u128,
    best: &mut Option<u64>,
) -> Result<(), String> {
    if remaining == 0 {
        *examined += 1;
        if feasible(fixture, selected, args)? {
            let cost = selected.iter().try_fold(0u64, |sum, position| {
                sum.checked_add(fixture.actions[*position].cost)
            });
            let cost = cost.ok_or_else(|| "flat synthesis cost overflows".to_string())?;
            if best.is_none_or(|current| cost < current) {
                *best = Some(cost);
            }
        }
        return Ok(());
    }
    if remaining > fixture.actions.len().saturating_sub(start) {
        return Ok(());
    }
    for position in start..=fixture.actions.len() - remaining {
        selected.push(position);
        visit_subsets(
            fixture,
            args,
            remaining - 1,
            position + 1,
            selected,
            examined,
            best,
        )?;
        selected.pop();
    }
    Ok(())
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if !(1..=3).contains(&args.dimension)
        || args.components == 0
        || args.components > 6
        || args.distractors == 0
        || args.distractors > 64
        || args.states == 0
        || args.states > 32
        || args.reps < 5
    {
        return Err("dimension must be 1 through 3, components 1 through 6, distractors 1 through 64, states 1 through 32, and reps at least 5".into());
    }
    let fixture = fixture(&args)?;
    let synthesis_limits = SynthesisLimits::default();
    let build = || {
        SynthesisArtifact::build(
            fixture.specification.clone(),
            fixture.actions.clone(),
            args.components,
            synthesis_limits,
        )
        .map_err(|error| error.to_string())
    };
    let warm = build()?;
    let bytes = warm
        .encode(synthesis_limits)
        .map_err(|error| error.to_string())?;
    let checked =
        verify_synthesis(&bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    if warm.upper_bound_cost() != Some(fixture.optimum)
        || checked.status != VerifiedSynthesisStatus::Optimal
        || checked.total_cost != Some(fixture.optimum)
    {
        return Err("synthesis fixture has a wrong certified optimum".into());
    }
    let (flat_cost, examined) = exhaustive(&fixture, &args)?;
    if flat_cost != fixture.optimum {
        return Err("flat synthesis reference differs from the fixture optimum".into());
    }
    let mut build_times = Vec::with_capacity(args.reps);
    let mut check_times = Vec::with_capacity(args.reps);
    let mut flat_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(build()?);
        build_times.push(start.elapsed().as_nanos());
        let start = Instant::now();
        black_box(
            verify_synthesis(black_box(&bytes), ProofLimits::default())
                .map_err(|error| error.to_string())?,
        );
        check_times.push(start.elapsed().as_nanos());
        let start = Instant::now();
        black_box(exhaustive(&fixture, &args)?);
        flat_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-synthesis-bench-v1 fixture={} dimension={} modulus={} components={} states={} candidates={} edits={} optimal_cost={} lower_bound={} oracle_calls={} proof_checks={} proof_nodes={} artifact_bytes={} flat_subsets={} build_ns={} check_ns={} flat_ns={} status=optimal",
        args.fixture,
        args.dimension,
        args.modulus,
        args.components,
        args.states,
        fixture.actions.len(),
        warm.selected().len(),
        fixture.optimum,
        warm.lower_bound_cost().unwrap_or(0),
        warm.producer_oracle_calls(),
        warm.proof_topology_checks(),
        warm.proof_nodes(),
        bytes.len(),
        examined,
        median(build_times),
        median(check_times),
        median(flat_times),
    );
    Ok(())
}
