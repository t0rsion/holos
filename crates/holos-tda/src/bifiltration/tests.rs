use super::*;

fn path() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 2, 1.0)]).unwrap()
}

#[test]
fn degree_rips_records_multicritical_vertex_births() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&path(), DegreeRipsParams::default()).unwrap();
    let vertices = &degree_rips.bifiltration().simplices()[0];
    assert_eq!(
        vertices[0].births().grades(),
        &[Bigrade::new(0, 2), Bigrade::new(1, 1)]
    );
    assert_eq!(
        vertices[1].births().grades(),
        &[Bigrade::new(0, 2), Bigrade::new(1, 0)]
    );
    assert_eq!(
        degree_rips.bifiltration().simplices()[1][0]
            .births()
            .grades(),
        &[Bigrade::new(1, 1)]
    );
}

#[test]
fn every_slice_matches_the_degree_rips_definition() {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 0.5), (1, 2, 1.0), (0, 2, 1.0), (2, 3, 2.0)],
    )
    .unwrap();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let bifiltration = degree_rips.bifiltration();
    for scale_index in 0..bifiltration.scale_bits.len() {
        let scale = f64::from_bits(bifiltration.scale_bits[scale_index]);
        let mut degrees = vec![0usize; graph.len()];
        for (u, v, value) in graph.edges() {
            if value <= scale {
                degrees[u] += 1;
                degrees[v] += 1;
            }
        }
        for density_index in 0..bifiltration.minimum_degrees.len() {
            let minimum = bifiltration.minimum_degrees[density_index];
            let grade = Bigrade::new(scale_index, density_index);
            let slice = bifiltration.slice(grade).unwrap();
            let expected_vertices = (0..graph.len())
                .filter(|&vertex| degrees[vertex] >= minimum)
                .collect::<Vec<_>>();
            assert_eq!(slice.active_vertices(), expected_vertices);
            for dimension in 1..slice.simplices().len() {
                for simplex in &slice.simplices()[dimension] {
                    assert!(simplex.iter().all(|&vertex| degrees[vertex] >= minimum));
                    for left in 0..simplex.len() {
                        for right in left + 1..simplex.len() {
                            assert!(graph.get(simplex[left], simplex[right]) <= scale);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn rejects_comparable_births() {
    let error = BirthAntichain::new(vec![Bigrade::new(0, 0), Bigrade::new(1, 1)]).unwrap_err();
    assert!(error.to_string().contains("pairwise incomparable"));
}

#[test]
fn rejects_face_support_violation() {
    let vertex_birth = BirthAntichain::new(vec![Bigrade::new(1, 1)]).unwrap();
    let edge_birth = BirthAntichain::new(vec![Bigrade::new(0, 0)]).unwrap();
    let error = MulticriticalBifiltration::new(
        2,
        vec![0.0, 1.0],
        vec![1, 0],
        vec![
            vec![
                MulticriticalSimplex::new(vec![0], vertex_birth.clone()),
                MulticriticalSimplex::new(vec![1], vertex_birth),
            ],
            vec![MulticriticalSimplex::new(vec![0, 1], edge_birth)],
        ],
        BifiltrationLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("appears after its coface"));
}

#[test]
fn h1_slice_graph_omits_inactive_edges() {
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&path(), DegreeRipsParams::default()).unwrap();
    let slice = degree_rips
        .bifiltration()
        .slice(Bigrade::new(1, 0))
        .unwrap();
    let graph = slice.h1_graph(3).unwrap();
    assert_eq!(slice.active_vertices(), &[1]);
    assert_eq!(graph.num_edges(), 0);
}

#[test]
fn declared_grid_keeps_exact_slices_and_omits_intermediate_values() {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 0.5), (1, 2, 0.75), (2, 3, 1.0), (0, 3, 1.25)],
    )
    .unwrap();
    let degree_rips = DegreeRipsBifiltration::from_graph_on_grid(
        &graph,
        vec![0.75, 1.25],
        vec![2, 0],
        DegreeRipsParams {
            threshold: Some(1.25),
            ..DegreeRipsParams::default()
        },
    )
    .unwrap();
    assert_eq!(
        degree_rips.bifiltration().scales().collect::<Vec<_>>(),
        vec![0.75, 1.25]
    );
    assert_eq!(degree_rips.bifiltration().minimum_degrees(), &[2, 0]);
    let first = degree_rips
        .bifiltration()
        .slice(Bigrade::new(0, 1))
        .unwrap();
    assert_eq!(first.simplices()[1], vec![vec![0, 1], vec![1, 2]]);
    let last = degree_rips
        .bifiltration()
        .slice(Bigrade::new(1, 1))
        .unwrap();
    assert_eq!(last.simplices()[1].len(), 4);
}

#[test]
fn declared_grid_requires_its_terminal_parameters() {
    let error = DegreeRipsBifiltration::from_graph_on_grid(
        &path(),
        vec![1.0],
        vec![1],
        DegreeRipsParams::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("minimum degree zero"));
    let error = DegreeRipsBifiltration::from_graph_on_grid(
        &path(),
        vec![1.0],
        vec![0],
        DegreeRipsParams {
            threshold: Some(2.0),
            ..DegreeRipsParams::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("must equal its last scale"));
}
