// Four compiler instances overlap CPU work without multiplying peak test memory excessively.
const FIXTURE_SHARD_COUNT: usize = 4;

pub fn select_fixture_shard<T>(cases: Vec<T>, shard_index: usize) -> Vec<T> {
    cases
        .into_iter()
        .enumerate()
        .filter_map(|(index, case)| (index % FIXTURE_SHARD_COUNT == shard_index).then_some(case))
        .collect()
}

pub fn select_weighted_fixture_shard<T>(
    cases: Vec<T>,
    shard_index: usize,
    weight: impl Fn(&T) -> usize,
) -> Vec<T> {
    let mut weighted = cases.into_iter().enumerate().collect::<Vec<_>>();
    weighted.sort_by_key(|(index, case)| (std::cmp::Reverse(weight(case)), *index));

    let mut loads = [0; FIXTURE_SHARD_COUNT];
    let mut selected = Vec::new();
    for (_, case) in weighted {
        let target = loads
            .iter()
            .enumerate()
            .min_by_key(|(index, load)| (**load, *index))
            .map_or(0, |(index, _)| index);
        if let Some(load) = loads.get_mut(target) {
            *load += weight(&case);
        }
        if target == shard_index {
            selected.push(case);
        }
    }

    selected
}

#[test]
fn weighted_fixture_shards_cover_each_case_exactly_once() {
    let weights = [12, 4, 2, 2, 2, 1, 1, 1, 1, 1];
    let mut selected = Vec::new();

    for shard_index in 0..FIXTURE_SHARD_COUNT {
        selected.extend(select_weighted_fixture_shard(
            (0..weights.len()).collect(),
            shard_index,
            |case| weights.get(*case).copied().unwrap_or(1),
        ));
    }

    selected.sort_unstable();
    assert_eq!(selected, (0..weights.len()).collect::<Vec<_>>());
}
