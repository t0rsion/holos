use super::*;

fn hitting_oracle(groups: Vec<Vec<usize>>) -> impl FnMut(&[usize]) -> Result<bool> {
    move |selected| {
        Ok(groups.iter().any(|group| {
            group
                .iter()
                .all(|candidate| selected.binary_search(candidate).is_err())
        }))
    }
}

fn exhaustive(costs: &[u64], max_selected: usize, groups: &[Vec<usize>]) -> Option<u64> {
    (0usize..1usize << costs.len())
        .filter(|mask| mask.count_ones() as usize <= max_selected)
        .filter(|mask| {
            groups
                .iter()
                .all(|group| group.iter().any(|candidate| mask & (1 << candidate) != 0))
        })
        .map(|mask| {
            costs
                .iter()
                .enumerate()
                .filter_map(|(candidate, cost)| (mask & (1 << candidate) != 0).then_some(cost))
                .sum()
        })
        .min()
}

#[test]
fn weighted_blocker_search_matches_exhaustive_hitting_sets() {
    let limits = SearchLimits {
        oracle_calls: 100_000,
        search_nodes: 100_000,
    };
    for seed in 0..64usize {
        let costs = (0..8)
            .map(|candidate| 1 + ((candidate * 7 + seed * 3) % 11) as u64)
            .collect::<Vec<_>>();
        let groups = (0..4)
            .map(|group| {
                (0..8)
                    .filter(|candidate| (candidate * 5 + group * 3 + seed) % 7 < 3)
                    .collect::<Vec<_>>()
            })
            .filter(|group| !group.is_empty())
            .collect::<Vec<_>>();
        let expected = exhaustive(&costs, 5, &groups);
        let actual = minimize_antitone(&costs, 5, limits, hitting_oracle(groups.clone())).unwrap();
        assert_eq!(actual.upper_bound, expected, "seed {seed}");
        assert_eq!(
            actual.status,
            if expected.is_some() {
                SearchStatus::Optimal
            } else {
                SearchStatus::Infeasible
            }
        );
    }
}

#[test]
fn necessary_set_prunes_a_wide_candidate_family() {
    let mut groups = vec![vec![0]];
    groups.push((1..128).collect());
    let result = minimize_antitone(
        &vec![1; 128],
        2,
        SearchLimits {
            oracle_calls: 2_000,
            search_nodes: 2_000,
        },
        hitting_oracle(groups),
    )
    .unwrap();
    assert_eq!(result.status, SearchStatus::Optimal);
    assert_eq!(result.upper_bound, Some(2));
    assert!(result.oracle_calls < 1_000);
    assert_eq!(result.root_blocker_bound, 2);
}

#[test]
fn work_limit_keeps_a_checked_bound_and_incumbent() {
    let result = minimize_antitone(
        &[4, 2, 7, 1, 9, 3],
        3,
        SearchLimits {
            oracle_calls: 12,
            search_nodes: 2,
        },
        hitting_oracle(vec![vec![0, 1], vec![2, 3], vec![4, 5]]),
    )
    .unwrap();
    assert!(matches!(
        result.status,
        SearchStatus::Incomplete | SearchStatus::Optimal
    ));
    if let (Some(lower), Some(upper)) = (result.lower_bound, result.upper_bound) {
        assert!(lower <= upper);
    }
}
