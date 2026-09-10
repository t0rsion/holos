use super::*;

#[test]
fn recursive_proof_excludes_every_cheaper_hitting_set() {
    let costs = [1, 1, 1];
    let groups = [vec![0, 1], vec![1, 2], vec![0, 2]];
    let mut oracle = |selected: &[usize]| {
        Ok(groups.iter().any(|group| {
            group
                .iter()
                .all(|candidate| selected.binary_search(candidate).is_err())
        }))
    };
    let limits = ProofLimits {
        nodes: 100,
        depth: 10,
        terms: 100,
        checks: 100,
    };
    let (proof, built) = build_proof(&costs, 2, Some(2), limits, &mut oracle).unwrap();
    assert!(built.nodes > 1);
    let checked = verify_proof(&proof, &costs, 2, Some(2), limits, &mut oracle).unwrap();
    assert_eq!(checked.checks, proof_topology_checks(&proof));
}
