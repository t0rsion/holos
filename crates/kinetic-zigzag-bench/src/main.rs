use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CohomologyLimits, KineticEdge, KineticFiltration, KineticLimits, KineticZigzagArtifact,
    KineticZigzagArtifactLimits,
};
use holos_tda_check::{ProofLimits, verify_kinetic_zigzag};

struct Args {
    dimension: usize,
    components: usize,
    modulus: u32,
    reps: usize,
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
        dimension: parse("--dimension", "2")?,
        components: parse("--components", "8")?,
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn trajectory(dimension: usize, components: usize) -> Result<KineticFiltration, String> {
    let atom_vertices = 2 * (dimension + 1);
    let mut edges = Vec::new();
    for component in 0..components {
        let offset = component * atom_vertices;
        for local_u in 0..atom_vertices {
            for local_v in local_u + 1..atom_vertices {
                if local_u / 2 != local_v / 2 {
                    edges.push(KineticEdge {
                        u: offset + local_u,
                        v: offset + local_v,
                        intercept: 0.0,
                        velocity: 0.0,
                    });
                }
            }
        }
        let crossing = (component + 1) as f64 / (components + 1) as f64;
        edges.push(KineticEdge {
            u: offset,
            v: offset + 1,
            intercept: 1.0 + crossing,
            velocity: -1.0,
        });
    }
    KineticFiltration::new(
        atom_vertices * components,
        edges,
        0.0,
        1.0,
        KineticLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if args.reps < 5 || !(1..=3).contains(&args.dimension) || args.components < 2 {
        return Err(
            "reps must be at least 5, dimension must be 1, 2, or 3, and components must be at least 2"
                .into(),
        );
    }
    let trajectory = trajectory(args.dimension, args.components)?;
    let limits = KineticZigzagArtifactLimits::default();
    let (artifact, zigzag) =
        KineticZigzagArtifact::build(&trajectory, args.dimension, 1.0, args.modulus, limits)
            .map_err(|error| error.to_string())?;
    let proof = artifact.encode(limits).map_err(|error| error.to_string())?;
    let checked =
        verify_kinetic_zigzag(&proof, ProofLimits::default()).map_err(|error| error.to_string())?;
    let expected_nodes = 2 * args.components + 1;
    let expected_intervals = (0..args.components)
        .map(|event| (0, 2 * event, 1))
        .collect::<Vec<_>>();
    let observed_intervals = zigzag
        .barcode
        .intervals
        .iter()
        .map(|interval| (interval.start, interval.end, interval.multiplicity))
        .collect::<Vec<_>>();
    if zigzag.nodes.len() != expected_nodes
        || zigzag.nodes[0].rank != args.components
        || zigzag.nodes.last().map(|node| node.rank) != Some(0)
        || observed_intervals != expected_intervals
        || checked.nodes != expected_nodes
        || checked.intervals != args.components
        || checked.interval_copies != args.components
    {
        return Err("kinetic zigzag differs from the disjoint-sphere fixture".into());
    }
    if zigzag.arrows.iter().enumerate().any(|(position, arrow)| {
        let event = position / 2;
        let expected = args.components - event - 1;
        arrow.restriction.rank != expected
    }) {
        return Err("kinetic restriction rank differs from the fixture".into());
    }
    let adjacent = trajectory
        .cohomology_events(
            args.dimension,
            1.0,
            args.modulus,
            CohomologyLimits::default(),
        )
        .map_err(|error| error.to_string())?;
    if adjacent.len() != args.components
        || adjacent.iter().enumerate().any(|(position, event)| {
            event.before_rank != args.components - position
                || event.after_rank != args.components - position - 1
        })
    {
        return Err("adjacent event relations differ from the fixture".into());
    }

    let mut build_times = Vec::with_capacity(args.reps);
    let mut adjacent_times = Vec::with_capacity(args.reps);
    let mut checker_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(KineticZigzagArtifact::build(
            &trajectory,
            args.dimension,
            1.0,
            args.modulus,
            limits,
        ))
        .map_err(|error| error.to_string())?;
        build_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(trajectory.cohomology_events(
            args.dimension,
            1.0,
            args.modulus,
            CohomologyLimits::default(),
        ))
        .map_err(|error| error.to_string())?;
        adjacent_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(verify_kinetic_zigzag(&proof, ProofLimits::default()))
            .map_err(|error| error.to_string())?;
        checker_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-kinetic-zigzag-bench-v1 dimension={} components={} modulus={} vertices={} edges={} events={} nodes={} arrows={} interval_spaces={} interval_copies={} initial_rank={} final_rank={} proof_bytes={} build_ns={} adjacent_ns={} checker_ns={} reps={}",
        args.dimension,
        args.components,
        args.modulus,
        trajectory.vertex_count(),
        trajectory.edges().len(),
        args.components,
        zigzag.nodes.len(),
        zigzag.arrows.len(),
        zigzag.barcode.intervals.len(),
        zigzag
            .barcode
            .intervals
            .iter()
            .map(|interval| interval.multiplicity)
            .sum::<usize>(),
        zigzag.nodes[0].rank,
        zigzag.nodes.last().map_or(0, |node| node.rank),
        proof.len(),
        median(build_times),
        median(adjacent_times),
        median(checker_times),
        args.reps,
    );
    Ok(())
}
