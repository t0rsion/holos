use std::hint::black_box;
use std::time::Instant;

use holos_tda::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus, CohomologyLimits, KineticEdge,
    KineticEventKind, KineticFiltration, KineticLimits, RipsParams, SparseDistanceMatrix,
    cohomology_relation, cohomology_space, rips_persistence_sparse,
};
use holos_tda_check::{
    ProofLimits, VerifiedCohomologyInterventionStatus, verify_cohomology_intervention,
};

type WeightedEdge = (usize, usize, f64);

struct Args {
    dimension: usize,
    modulus: u32,
    reps: usize,
}

fn arguments() -> Result<Args, String> {
    let values: Vec<_> = std::env::args().skip(1).collect();
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
        modulus: value("--modulus", "2")
            .parse()
            .map_err(|_| "--modulus must be an integer".to_string())?,
        reps: parse("--reps", "5")?,
    })
}

fn sphere(dimension: usize) -> Result<(SparseDistanceMatrix, Vec<WeightedEdge>), String> {
    let pairs = dimension + 1;
    let vertices = 2 + 2 * pairs;
    let mut edges = Vec::new();
    for u in 2..vertices {
        for v in (u + 1)..vertices {
            if (u - 2) / 2 != (v - 2) / 2 {
                let rank = u * vertices + v;
                edges.push((u, v, 1.0 + rank as f64 / 100_000.0));
            }
        }
    }
    let graph =
        SparseDistanceMatrix::from_triplets(vertices, &edges).map_err(|error| error.to_string())?;
    Ok((graph, edges))
}

fn filled_graph(vertices: usize, edges: &[WeightedEdge]) -> Result<SparseDistanceMatrix, String> {
    let mut filled = edges.to_vec();
    filled.push((2, 3, 1.5));
    SparseDistanceMatrix::from_triplets(vertices, &filled).map_err(|error| error.to_string())
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), String> {
    let args = arguments()?;
    if args.reps < 5 || !(1..=3).contains(&args.dimension) {
        return Err("reps must be at least 5, and dimension must be 1, 2, or 3".into());
    }
    let scale = 2.0;
    let limits = CohomologyLimits::default();
    let intervention_limits = CohomologyInterventionLimits::default();
    let (graph, edges) = sphere(args.dimension)?;
    let filled = filled_graph(graph.len(), &edges)?;
    let space = cohomology_space(&graph, args.dimension, scale, args.modulus, limits)
        .map_err(|error| error.to_string())?;
    let next = cohomology_space(&filled, args.dimension, scale, args.modulus, limits)
        .map_err(|error| error.to_string())?;
    let relation = cohomology_relation(&graph, &space, &filled, &next, limits)
        .map_err(|error| error.to_string())?;
    let identity = cohomology_relation(&graph, &space, &graph, &space, limits)
        .map_err(|error| error.to_string())?;
    if space.rank() != 1 || next.rank() != 0 || relation.relation_rank != 0 {
        return Err("cross-polytope sphere ranks or fill relation differ from the fixture".into());
    }
    if !identity.is_isomorphism() || relation.contains_old_class(space.basis()[0].id) {
        return Err("cohomology relation differs from its exact restriction contract".into());
    }

    let candidates = [
        CohomologyInterventionCandidate::new(0, 1, 1),
        CohomologyInterventionCandidate::new(2, 3, 1),
    ];
    let scenario = CohomologyInterventionScenario::from_graph(&graph, scale, 0)
        .map_err(|error| error.to_string())?;
    let artifact = CohomologyInterventionArtifact::build(
        graph.len(),
        args.dimension,
        scale,
        args.modulus,
        std::slice::from_ref(&scenario),
        &candidates,
        1,
        intervention_limits,
    )
    .map_err(|error| error.to_string())?;
    let proof = artifact
        .encode(intervention_limits)
        .map_err(|error| error.to_string())?;
    let checked = verify_cohomology_intervention(&proof, ProofLimits::default())
        .map_err(|error| error.to_string())?;
    if artifact.status() != CohomologyInterventionStatus::Optimal
        || artifact.edits() != [CohomologyInterventionCandidate::new(2, 3, 1)]
        || artifact.oracle_calls() == 0
        || checked.status != VerifiedCohomologyInterventionStatus::Optimal
        || checked.edits != 1
    {
        return Err("finite-candidate intervention or independent replay differs".into());
    }

    let mut affine = edges
        .iter()
        .map(|&(u, v, intercept)| KineticEdge {
            u,
            v,
            intercept,
            velocity: 0.0,
        })
        .collect::<Vec<_>>();
    affine.push(KineticEdge {
        u: 2,
        v: 3,
        intercept: 3.0,
        velocity: -2.0,
    });
    let kinetic = KineticFiltration::new(graph.len(), affine, 0.25, 0.75, KineticLimits::default())
        .map_err(|error| error.to_string())?;
    let schedule = kinetic
        .events(Some(scale))
        .map_err(|error| error.to_string())?;
    let class_events = kinetic
        .cohomology_events(args.dimension, scale, args.modulus, limits)
        .map_err(|error| error.to_string())?;
    let threshold_events = schedule
        .events
        .iter()
        .flat_map(|event| &event.kinds)
        .filter(|kind| matches!(kind, KineticEventKind::ThresholdCrossing { .. }))
        .count();
    if threshold_events != 1
        || class_events.len() != 1
        || class_events[0].before_rank != 1
        || class_events[0].after_rank != 0
        || class_events[0].relation.relation_rank != 0
    {
        return Err("affine event schedule differs from the sphere fill".into());
    }

    let persistence = rips_persistence_sparse(
        &graph,
        &RipsParams::new(args.dimension).with_modulus(args.modulus),
    )
    .map_err(|error| error.to_string())?;
    let persistence_rank = persistence.in_dim(args.dimension).count();
    if persistence_rank != 1 {
        return Err("fixed-scale rank differs from persistent homology".into());
    }

    let mut cohomology_times = Vec::with_capacity(args.reps);
    let mut relation_times = Vec::with_capacity(args.reps);
    let mut kinetic_times = Vec::with_capacity(args.reps);
    let mut intervention_times = Vec::with_capacity(args.reps);
    let mut checker_times = Vec::with_capacity(args.reps);
    let mut persistence_times = Vec::with_capacity(args.reps);
    for _ in 0..args.reps {
        let start = Instant::now();
        black_box(cohomology_space(
            &graph,
            args.dimension,
            scale,
            args.modulus,
            limits,
        ))
        .map_err(|error| error.to_string())?;
        cohomology_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(cohomology_relation(&graph, &space, &filled, &next, limits))
            .map_err(|error| error.to_string())?;
        relation_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(kinetic.cohomology_events(args.dimension, scale, args.modulus, limits))
            .map_err(|error| error.to_string())?;
        kinetic_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(CohomologyInterventionArtifact::build(
            graph.len(),
            args.dimension,
            scale,
            args.modulus,
            std::slice::from_ref(&scenario),
            &candidates,
            1,
            intervention_limits,
        ))
        .map_err(|error| error.to_string())?;
        intervention_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(verify_cohomology_intervention(
            &proof,
            ProofLimits::default(),
        ))
        .map_err(|error| error.to_string())?;
        checker_times.push(start.elapsed().as_nanos());

        let start = Instant::now();
        black_box(rips_persistence_sparse(
            &graph,
            &RipsParams::new(args.dimension).with_modulus(args.modulus),
        ))
        .map_err(|error| error.to_string())?;
        persistence_times.push(start.elapsed().as_nanos());
    }
    println!(
        "format=holos-cohomology-bench-v2 dimension={} modulus={} vertices={} active_edges={} candidates={} reps={} rank={} filled_rank={} relation_rank={} identity_isomorphism={} threshold_events={} event_before_rank={} event_after_rank={} persistence_rank={} intervention_status={} edits={} oracle_calls={} proof_bytes={} cohomology_ns={} relation_ns={} kinetic_ns={} intervention_ns={} checker_ns={} persistence_ns={}",
        args.dimension,
        args.modulus,
        graph.len(),
        edges.len(),
        candidates.len(),
        args.reps,
        space.rank(),
        next.rank(),
        relation.relation_rank,
        identity.is_isomorphism(),
        threshold_events,
        class_events[0].before_rank,
        class_events[0].after_rank,
        persistence_rank,
        artifact.status(),
        artifact.edits().len(),
        artifact.oracle_calls(),
        proof.len(),
        median(cohomology_times),
        median(relation_times),
        median(kinetic_times),
        median(intervention_times),
        median(checker_times),
        median(persistence_times),
    );
    Ok(())
}
