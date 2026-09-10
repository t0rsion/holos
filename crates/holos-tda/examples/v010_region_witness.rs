#![forbid(unsafe_code)]

#[path = "../tests/fixtures/region_witness.rs"]
mod fixture;

use std::fmt::Write;

use holos_tda::{
    Bar, CertificateLimits, CertifiedReductionRegion, CertifiedRegionEvaluation, Diagram,
    PersistenceAtlas, ReductionCertificate, ReductionGuard, RegionViolation, SparseDistanceMatrix,
    TopologyEvent, TopologyEventKind, rips_persistence_sparse,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("v010-region-witness: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let witness = build_witness()?;
    verify_witness(&witness)?;
    println!("{}", render_witness(&witness));
    Ok(())
}

struct Witness {
    initial: SparseDistanceMatrix,
    updated: SparseDistanceMatrix,
    region: CertifiedReductionRegion,
    initial_evaluation: CertifiedRegionEvaluation,
    updated_evaluation: CertifiedRegionEvaluation,
    initial_exact: Diagram,
    updated_exact: Diagram,
    events: Vec<TopologyEvent>,
    initial_violations: Vec<RegionViolation>,
    updated_violations: Vec<RegionViolation>,
    initial_complete_guards_hold: bool,
    updated_complete_guards_hold: bool,
    updated_reduced_guards_hold: bool,
}

fn build_witness() -> Result<Witness, String> {
    let (initial, updated, region, atlas) = build_region()?;
    let (initial_evaluation, updated_evaluation, initial_exact, updated_exact, events) =
        evaluate_region(&initial, &updated, &region, &atlas)?;
    let initial_violations = region.violations(&initial);
    let updated_violations = region.violations(&updated);
    let initial_complete_guards_hold = fixture::guards_hold(&initial, region.complete_guards());
    let updated_complete_guards_hold = fixture::guards_hold(&updated, region.complete_guards());
    let updated_reduced_guards_hold = fixture::guards_hold(&updated, region.guards());
    Ok(Witness {
        initial,
        updated,
        region,
        initial_evaluation,
        updated_evaluation,
        initial_exact,
        updated_exact,
        events,
        initial_violations,
        updated_violations,
        initial_complete_guards_hold,
        updated_complete_guards_hold,
        updated_reduced_guards_hold,
    })
}

fn build_region() -> Result<
    (
        SparseDistanceMatrix,
        SparseDistanceMatrix,
        CertifiedReductionRegion,
        PersistenceAtlas,
    ),
    String,
> {
    let initial = fixture::initial_graph();
    let updated = fixture::updated_graph();
    let params = fixture::params();
    let certificate = ReductionCertificate::build(&initial, &params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let region = certificate
        .compile_region(&initial, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let atlas = PersistenceAtlas::build(&initial, &params).map_err(|error| error.to_string())?;
    Ok((initial, updated, region, atlas))
}

fn evaluate_region(
    initial: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    region: &CertifiedReductionRegion,
    atlas: &PersistenceAtlas,
) -> Result<
    (
        CertifiedRegionEvaluation,
        CertifiedRegionEvaluation,
        Diagram,
        Diagram,
        Vec<TopologyEvent>,
    ),
    String,
> {
    let params = fixture::params();
    let initial_evaluation = region
        .evaluate(initial)
        .map_err(|error| error.to_string())?;
    let updated_evaluation = region
        .evaluate(updated)
        .map_err(|error| error.to_string())?;
    let initial_exact =
        rips_persistence_sparse(initial, &params).map_err(|error| error.to_string())?;
    let updated_exact =
        rips_persistence_sparse(updated, &params).map_err(|error| error.to_string())?;
    let events = atlas.events(updated);
    Ok((
        initial_evaluation,
        updated_evaluation,
        initial_exact,
        updated_exact,
        events,
    ))
}

fn verify_witness(witness: &Witness) -> Result<(), String> {
    verify_diagrams(witness)?;
    verify_guards(witness)?;
    verify_events_and_guard_count(witness)
}

fn verify_diagrams(witness: &Witness) -> Result<(), String> {
    require(
        fixture::diagram_bits_equal(witness.initial_evaluation.diagram(), &witness.initial_exact),
        "initial region evaluation differs from exact reduction",
    )?;
    require(
        fixture::diagram_bits_equal(witness.updated_evaluation.diagram(), &witness.updated_exact),
        "updated region evaluation differs from exact reduction",
    )?;
    Ok(())
}

fn verify_guards(witness: &Witness) -> Result<(), String> {
    require(
        witness.initial_violations.is_empty(),
        "the initial graph violates its reduction-derived guards",
    )?;
    require(
        witness.updated_violations.is_empty(),
        "the fixed update violates a reduction-derived guard",
    )?;
    require(
        witness.initial_complete_guards_hold,
        "the initial graph violates a complete derived guard",
    )?;
    require(
        witness.updated_complete_guards_hold,
        "the fixed update violates a complete derived guard",
    )?;
    require(
        witness.updated_reduced_guards_hold,
        "the fixed update violates a reduced derived guard",
    )?;
    Ok(())
}

fn verify_events_and_guard_count(witness: &Witness) -> Result<(), String> {
    require(
        witness.events.len() == 1 && is_named_order_swap(&witness.events[0]),
        "the complete-order atlas did not report exactly the named reversal",
    )?;
    require(
        !witness.region.complete_guards().is_empty(),
        "the fixed reduction did not derive any guards",
    )?;
    require(
        witness.region.guards().len() <= witness.region.complete_guards().len(),
        "transitive guard removal increased the guard count",
    )
}

fn render_witness(witness: &Witness) -> String {
    let mut record = String::new();
    writeln!(record, "format holos-v010-region-witness-v1").unwrap();
    writeln!(
        record,
        "claim checked region strictly contains initial complete-order chamber"
    )
    .unwrap();
    writeln!(record, "field Z/{}", fixture::MODULUS).unwrap();
    writeln!(record, "maximum_homology_dimension 1").unwrap();
    writeln!(record, "threshold none").unwrap();
    writeln!(record, "vertices {}", fixture::VERTEX_COUNT).unwrap();
    print_graph(&mut record, "initial_graph", &witness.initial);
    print_graph(&mut record, "updated_graph", &witness.updated);
    print_orders(&mut record, &witness.initial, &witness.updated);
    print_events(&mut record, &witness.events);
    print_guards(
        &mut record,
        "complete_guards",
        witness.region.complete_guards(),
    );
    print_guards(&mut record, "reduced_guards", witness.region.guards());
    writeln!(
        record,
        "named_reversal_in_complete_guards {}",
        yes_no(has_named_guard(witness.region.complete_guards()))
    )
    .unwrap();
    writeln!(
        record,
        "named_reversal_in_reduced_guards {}",
        yes_no(has_named_guard(witness.region.guards()))
    )
    .unwrap();
    writeln!(
        record,
        "initial_guard_violations {}",
        witness.initial_violations.len()
    )
    .unwrap();
    writeln!(
        record,
        "updated_guard_violations {}",
        witness.updated_violations.len()
    )
    .unwrap();
    writeln!(
        record,
        "initial_complete_guards_hold {}",
        yes_no(witness.initial_complete_guards_hold)
    )
    .unwrap();
    writeln!(
        record,
        "updated_complete_guards_hold {}",
        yes_no(witness.updated_complete_guards_hold)
    )
    .unwrap();
    writeln!(
        record,
        "updated_reduced_guards_hold {}",
        yes_no(witness.updated_reduced_guards_hold)
    )
    .unwrap();
    print_diagram(
        &mut record,
        "initial_region_diagram",
        witness.initial_evaluation.diagram().bars.as_slice(),
    );
    print_diagram(
        &mut record,
        "initial_exact_diagram",
        &witness.initial_exact.bars,
    );
    print_h1_pairs(&mut record, "initial_h1_pairs", &witness.initial_evaluation);
    print_diagram(
        &mut record,
        "updated_region_diagram",
        witness.updated_evaluation.diagram().bars.as_slice(),
    );
    print_diagram(
        &mut record,
        "updated_exact_diagram",
        &witness.updated_exact.bars,
    );
    print_h1_pairs(&mut record, "updated_h1_pairs", &witness.updated_evaluation);
    record
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.to_string())
}

fn is_named_order_swap(event: &holos_tda::TopologyEvent) -> bool {
    event.kind == TopologyEventKind::OrderSwap
        && event.first.map(|edge| [edge.u, edge.v]) == Some(fixture::REVERSED_FIRST)
        && event.second.map(|edge| [edge.u, edge.v]) == Some(fixture::REVERSED_SECOND)
}

fn print_graph(record: &mut String, label: &str, graph: &SparseDistanceMatrix) {
    writeln!(record, "{label} {}", graph.edges().count()).unwrap();
    for (u, v, weight) in graph.edges() {
        writeln!(record, "  edge [{u},{v}] {}", value(weight)).unwrap();
    }
}

fn print_orders(
    record: &mut String,
    initial: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
) {
    let initial_order = edge_order(initial);
    let updated_order = edge_order(updated);
    writeln!(
        record,
        "initial_complete_edge_order {}",
        initial_order.len()
    )
    .unwrap();
    for (position, (edge, weight)) in initial_order.iter().enumerate() {
        writeln!(record, "  {position} {} {}", simplex(edge), value(*weight)).unwrap();
    }
    writeln!(
        record,
        "updated_complete_edge_order {}",
        updated_order.len()
    )
    .unwrap();
    for (position, (edge, weight)) in updated_order.iter().enumerate() {
        writeln!(record, "  {position} {} {}", simplex(edge), value(*weight)).unwrap();
    }
    let complete_comparisons = initial_order.len() * (initial_order.len() - 1) / 2;
    writeln!(
        record,
        "initial_complete_order_comparisons {complete_comparisons}"
    )
    .unwrap();
    writeln!(
        record,
        "reversed_simplex_pair {} {}",
        simplex(&fixture::REVERSED_FIRST),
        simplex(&fixture::REVERSED_SECOND)
    )
    .unwrap();
    writeln!(
        record,
        "reversed_order initial {} < {} updated {} > {}",
        simplex(&fixture::REVERSED_FIRST),
        simplex(&fixture::REVERSED_SECOND),
        simplex(&fixture::REVERSED_FIRST),
        simplex(&fixture::REVERSED_SECOND)
    )
    .unwrap();
}

fn edge_order(graph: &SparseDistanceMatrix) -> Vec<([usize; 2], f64)> {
    let mut order: Vec<_> = graph
        .edges()
        .map(|(u, v, weight)| ([u, v], weight))
        .collect();
    order.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| edge_rank(right.0).cmp(&edge_rank(left.0)))
    });
    order
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

fn print_events(record: &mut String, events: &[holos_tda::TopologyEvent]) {
    writeln!(record, "complete_order_events {}", events.len()).unwrap();
    for event in events {
        writeln!(
            record,
            "  {:?} {} {} {} {} {} {}",
            event.kind,
            optional_edge(event.first),
            optional_value(event.old_first),
            optional_value(event.new_first),
            optional_edge(event.second),
            optional_value(event.old_second),
            optional_value(event.new_second)
        )
        .unwrap();
    }
}

fn print_guards(record: &mut String, label: &str, guards: &[ReductionGuard]) {
    writeln!(record, "{label} {}", guards.len()).unwrap();
    for guard in guards {
        writeln!(
            record,
            "  {:?} {} <= {}",
            guard.kind(),
            simplex(guard.earlier().vertices()),
            simplex(guard.later().vertices())
        )
        .unwrap();
    }
}

fn has_named_guard(guards: &[ReductionGuard]) -> bool {
    guards.iter().any(|guard| {
        guard.earlier().vertices() == fixture::REVERSED_FIRST
            && guard.later().vertices() == fixture::REVERSED_SECOND
    })
}

fn print_diagram(record: &mut String, label: &str, bars: &[Bar]) {
    writeln!(record, "{label} {}", bars.len()).unwrap();
    for bar in bars {
        writeln!(
            record,
            "  H{} [{},{})",
            bar.dim,
            value(bar.birth),
            value(bar.death)
        )
        .unwrap();
    }
}

fn print_h1_pairs(record: &mut String, label: &str, evaluation: &CertifiedRegionEvaluation) {
    let pairs = evaluation.h1_critical_pairs();
    writeln!(record, "{label} {}", pairs.len()).unwrap();
    for (bar, pair) in pairs {
        let death = pair
            .death
            .as_ref()
            .map(|simplex_value| {
                format!(
                    "{}@{}",
                    simplex(&simplex_value.vertices),
                    value(simplex_value.value)
                )
            })
            .unwrap_or_else(|| "essential".to_string());
        writeln!(
            record,
            "  H1 [{},{}) birth {}@{} death {death}",
            value(bar.birth),
            value(bar.death),
            simplex(&pair.birth.vertices),
            value(pair.birth.value)
        )
        .unwrap();
    }
}

fn simplex(vertices: &[usize]) -> String {
    let labels: Vec<_> = vertices.iter().map(usize::to_string).collect();
    format!("[{}]", labels.join(","))
}

fn optional_edge(edge: Option<holos_tda::EdgeKey>) -> String {
    edge.map(|edge| simplex(&[edge.u, edge.v]))
        .unwrap_or_else(|| "none".to_string())
}

fn optional_value(number: Option<f64>) -> String {
    number.map(value).unwrap_or_else(|| "none".to_string())
}

fn value(number: f64) -> String {
    if number == f64::INFINITY {
        "inf".to_string()
    } else {
        format!("{number:.1}")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
