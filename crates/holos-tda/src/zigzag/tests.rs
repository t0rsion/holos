use super::*;

fn column(terms: &[(usize, u32)]) -> Vec<ZigzagTerm> {
    terms
        .iter()
        .map(|&(target, coefficient)| ZigzagTerm {
            target,
            coefficient,
        })
        .collect()
}

#[test]
fn identity_chain_is_one_complete_interval() {
    let module = ZigzagModule::new(
        3,
        vec![1, 1, 1],
        vec![
            ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)])]),
            ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)])]),
        ],
        ZigzagLimits::default(),
    )
    .unwrap();
    let barcode = module.decompose().unwrap();
    assert_eq!(barcode.rank(0, 2), Some(1));
    assert_eq!(barcode.intervals.len(), 1);
    assert_eq!(barcode.intervals[0].start, 0);
    assert_eq!(barcode.intervals[0].end, 2);
    assert_eq!(barcode.intervals[0].multiplicity, 1);
}

#[test]
fn zero_map_splits_two_point_intervals() {
    let module = ZigzagModule::new(
        5,
        vec![1, 1],
        vec![ZigzagMap::new(ZigzagDirection::Forward, vec![vec![]])],
        ZigzagLimits::default(),
    )
    .unwrap();
    let intervals = module.decompose().unwrap().intervals;
    assert_eq!(
        intervals
            .iter()
            .map(|interval| (interval.start, interval.end, interval.multiplicity))
            .collect::<Vec<_>>(),
        vec![(0, 0, 1), (1, 1, 1)]
    );
}

#[test]
fn fork_distinguishes_shared_and_independent_event_classes() {
    let shared = ZigzagModule::new(
        3,
        vec![1, 2, 1],
        vec![
            ZigzagMap::new(ZigzagDirection::Backward, vec![column(&[(0, 1)]), vec![]]),
            ZigzagMap::new(ZigzagDirection::Forward, vec![column(&[(0, 1)]), vec![]]),
        ],
        ZigzagLimits::default(),
    )
    .unwrap()
    .decompose()
    .unwrap();
    assert!(
        shared
            .intervals
            .iter()
            .any(|item| item.start == 0 && item.end == 2)
    );

    let independent = ZigzagModule::new(
        3,
        vec![1, 2, 1],
        vec![
            ZigzagMap::new(ZigzagDirection::Backward, vec![column(&[(0, 1)]), vec![]]),
            ZigzagMap::new(ZigzagDirection::Forward, vec![vec![], column(&[(0, 1)])]),
        ],
        ZigzagLimits::default(),
    )
    .unwrap()
    .decompose()
    .unwrap();
    assert_eq!(independent.rank(0, 2), Some(0));
    assert_eq!(
        independent
            .intervals
            .iter()
            .map(|item| (item.start, item.end, item.multiplicity))
            .collect::<Vec<_>>(),
        vec![(0, 1, 1), (1, 2, 1)]
    );
}

#[test]
fn every_orientation_recovers_direct_sum_multiplicities() {
    let expected = vec![(0, 2, 2), (0, 4, 1), (1, 3, 1), (2, 2, 1), (4, 4, 2)];
    let copies = expected
        .iter()
        .flat_map(|&(start, end, multiplicity)| std::iter::repeat_n((start, end), multiplicity))
        .collect::<Vec<_>>();
    let bases = (0..5)
        .map(|node| {
            copies
                .iter()
                .enumerate()
                .filter_map(|(copy, &(start, end))| (start <= node && node <= end).then_some(copy))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let dimensions = bases.iter().map(Vec::len).collect::<Vec<_>>();

    for modulus in [2, 3, 5] {
        for directions in 0..16 {
            let maps = (0..4)
                .map(|position| {
                    let direction = if directions & (1 << position) == 0 {
                        ZigzagDirection::Forward
                    } else {
                        ZigzagDirection::Backward
                    };
                    let (source, target) = match direction {
                        ZigzagDirection::Forward => (&bases[position], &bases[position + 1]),
                        ZigzagDirection::Backward => (&bases[position + 1], &bases[position]),
                    };
                    let columns = source
                        .iter()
                        .map(|copy| {
                            target
                                .iter()
                                .position(|candidate| candidate == copy)
                                .map_or_else(Vec::new, |target| column(&[(target, 1)]))
                        })
                        .collect();
                    ZigzagMap::new(direction, columns)
                })
                .collect();
            let actual =
                ZigzagModule::new(modulus, dimensions.clone(), maps, ZigzagLimits::default())
                    .unwrap()
                    .decompose()
                    .unwrap()
                    .intervals
                    .iter()
                    .map(|interval| (interval.start, interval.end, interval.multiplicity))
                    .collect::<Vec<_>>();
            assert_eq!(actual, expected, "modulus {modulus}, mask {directions}");
        }
    }
}
