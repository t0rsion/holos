#![forbid(unsafe_code)]

use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CoverageAction, CoverageCompositionStatus, CoverageFence, CoverageLimits,
    CoverageSpecification, CoverageState, CoverageSynthesisArtifact, CoverageSynthesisLimits,
    PlanarCoverageModel, SparseDistanceMatrix, compose_coverage_frontiers, evaluate_coverage_plan,
};
use holos_tda_check::{ProofLimits, VerifiedCoverageStatus, verify_coverage};

struct Args {
    fixture: String,
    states: usize,
    redundant: usize,
    distractors: usize,
    failures: usize,
    modulus: u32,
    reps: usize,
}

struct Fixture {
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
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
        states: parse("--states", "2")?,
        redundant: parse("--redundant", "3")?,
        distractors: parse("--distractors", "2")?,
        failures: parse("--failures", "1")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn fixture(args: &Args) -> Result<Fixture, String> {
    match args.fixture.as_str() {
        "independent" => independent_fixture(args),
        "coupled" => coupled_fixture(args),
        _ => Err("fixture must be independent or coupled".into()),
    }
}

fn independent_fixture(args: &Args) -> Result<Fixture, String> {
    let width = args.redundant + args.distractors;
    let vertex_count = 4 + args.states * width;
    let mut states = Vec::with_capacity(args.states);
    let mut actions = Vec::with_capacity(args.states * width);
    let mut failable = Vec::with_capacity(args.states * width);
    let mut optimum = 0u64;
    for state in 0..args.states {
        let offset = 4 + state * width;
        let graph = wheel_graph(vertex_count, offset, args.redundant)?;
        states.push(
            CoverageState::new(0, state as u64, &graph, vec![0, 1, 2, 3], 1.0)
                .map_err(|error| error.to_string())?,
        );
        for local in 0..width {
            let vertex = offset + local;
            let cost = if local < args.redundant {
                10 + local as u64
            } else {
                1 + (local - args.redundant) as u64
            };
            actions.push(CoverageAction::new(vertex, cost, vec![state]));
            failable.push(vertex);
        }
        optimum = optimum
            .checked_add(
                (0..=args.failures)
                    .map(|local| 10 + local as u64)
                    .sum::<u64>(),
            )
            .ok_or_else(|| "coverage fixture optimum overflows".to_string())?;
    }
    let specification = specification(vertex_count, failable, args, states)?;
    Ok(Fixture {
        specification,
        actions,
        max_activations: args.states * (args.failures + 1),
        optimum,
    })
}

fn coupled_fixture(args: &Args) -> Result<Fixture, String> {
    let width = args.redundant + args.distractors;
    let vertex_count = 4 + width;
    let graph = wheel_graph(vertex_count, 4, args.redundant)?;
    let states = (0..args.states)
        .map(|step| {
            CoverageState::new(0, step as u64, &graph, vec![0, 1, 2, 3], 1.0)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let failable = (4..vertex_count).collect::<Vec<_>>();
    let specification = specification(vertex_count, failable, args, states)?;
    let actions = (0..width)
        .map(|local| {
            CoverageAction::throughout(
                4 + local,
                if local < args.redundant {
                    10 + local as u64
                } else {
                    1 + (local - args.redundant) as u64
                },
                &specification,
            )
        })
        .collect();
    Ok(Fixture {
        specification,
        actions,
        max_activations: args.failures + 1,
        optimum: (0..=args.failures).map(|local| 10 + local as u64).sum(),
    })
}

fn wheel_graph(
    vertex_count: usize,
    first_center: usize,
    center_count: usize,
) -> Result<SparseDistanceMatrix, String> {
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    for center in first_center..first_center + center_count {
        edges.extend((0..4).map(|fence| (fence, center, 1.0)));
    }
    SparseDistanceMatrix::from_triplets(vertex_count, &edges).map_err(|error| error.to_string())
}

fn specification(
    vertex_count: usize,
    failable: Vec<usize>,
    args: &Args,
    states: Vec<CoverageState>,
) -> Result<CoverageSpecification, String> {
    CoverageSpecification::new(
        vertex_count,
        PlanarCoverageModel::new(1.0, 1.0).map_err(|error| error.to_string())?,
        args.modulus,
        CoverageFence::new(vec![0, 1, 2, 3]).map_err(|error| error.to_string())?,
        failable,
        args.failures,
        states,
        CoverageLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn exhaustive(fixture: &Fixture) -> Result<(u64, u128), String> {
    let mut best = None;
    let mut examined = 0u128;
    for size in 0..=fixture.max_activations.min(fixture.actions.len()) {
        visit_subsets(fixture, size, 0, &mut Vec::new(), &mut examined, &mut best)?;
    }
    best.map(|cost| (cost, examined))
        .ok_or_else(|| "flat coverage reference found no plan".into())
}

fn visit_subsets(
    fixture: &Fixture,
    remaining: usize,
    start: usize,
    selected: &mut Vec<usize>,
    examined: &mut u128,
    best: &mut Option<u64>,
) -> Result<(), String> {
    if remaining == 0 {
        *examined += 1;
        if evaluate_coverage_plan(
            &fixture.specification,
            &fixture.actions,
            selected,
            CoverageLimits::default(),
        )
        .map_err(|error| error.to_string())?
        .criterion_holds
        {
            let cost = selected.iter().try_fold(0u64, |sum, position| {
                sum.checked_add(fixture.actions[*position].cost)
            });
            let cost = cost.ok_or_else(|| "flat coverage cost overflows".to_string())?;
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
    if args.states == 0
        || args.states > 8
        || args.redundant == 0
        || args.redundant > 8
        || args.failures >= args.redundant
        || args.distractors > 16
        || args.reps < 5
    {
        return Err("states must be 1 through 8, redundant sensors 1 through 8, failures below redundant sensors, distractors at most 16, and reps at least 5".into());
    }
    let fixture = fixture(&args)?;
    let limits = CoverageSynthesisLimits::default();
    let build = || {
        CoverageSynthesisArtifact::build(
            fixture.specification.clone(),
            fixture.actions.clone(),
            fixture.max_activations,
            limits,
        )
        .map_err(|error| error.to_string())
    };
    let composition = compose_coverage_frontiers(
        &fixture.specification,
        &fixture.actions,
        fixture.max_activations,
        limits,
    )
    .map_err(|error| error.to_string())?;
    let warm = build()?;
    let bytes = warm.encode(limits).map_err(|error| error.to_string())?;
    let checked =
        verify_coverage(&bytes, ProofLimits::default()).map_err(|error| error.to_string())?;
    if composition.status() != CoverageCompositionStatus::Optimal
        || composition.cost() != Some(fixture.optimum)
        || warm.upper_bound_cost() != Some(fixture.optimum)
        || checked.status != VerifiedCoverageStatus::Optimal
        || checked.total_cost != Some(fixture.optimum)
    {
        return Err("coverage fixture has a wrong certified optimum".into());
    }
    let (flat_cost, examined) = exhaustive(&fixture)?;
    if flat_cost != fixture.optimum {
        return Err("flat coverage reference differs from the fixture optimum".into());
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
            verify_coverage(black_box(&bytes), ProofLimits::default())
                .map_err(|error| error.to_string())?,
        );
        check_times.push(start.elapsed().as_nanos());
        let start = Instant::now();
        black_box(exhaustive(&fixture)?);
        flat_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-coverage-bench-v1 fixture={} modulus={} states={} components={} redundant={} distractors={} failure_budget={} candidates={} activations={} optimal_cost={} lower_bound={} oracle_calls={} frontier_calls={} proof_checks={} proof_nodes={} failure_checks={} artifact_bytes={} flat_subsets={} build_ns={} check_ns={} flat_ns={} status=optimal",
        args.fixture,
        args.modulus,
        args.states,
        composition.frontiers().len(),
        args.redundant,
        args.distractors,
        args.failures,
        fixture.actions.len(),
        warm.selected().len(),
        fixture.optimum,
        warm.lower_bound_cost().unwrap_or(0),
        warm.producer_oracle_calls(),
        composition.oracle_calls(),
        warm.proof_topology_checks(),
        warm.proof_nodes(),
        warm.selected_failure_checks(),
        bytes.len(),
        examined,
        median(build_times),
        median(check_times),
        median(flat_times),
    );
    Ok(())
}
