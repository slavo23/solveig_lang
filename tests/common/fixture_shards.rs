// Four compiler instances overlap CPU work without multiplying peak test memory excessively.
const FIXTURE_SHARD_COUNT: usize = 4;

pub fn select_fixture_shard<T>(cases: Vec<T>, shard_index: usize) -> Vec<T> {
    cases
        .into_iter()
        .enumerate()
        .filter_map(|(index, case)| (index % FIXTURE_SHARD_COUNT == shard_index).then_some(case))
        .collect()
}
