use std::{collections::HashMap, hash::Hash};

enum ScopedEnvEntry<K, V> {
    Insert { key: K, prev_value: Option<V> },
    Marker,
}

struct ScopedEnv<K, V> {
    entries_map: HashMap<K, V>,
    memo: Vec<ScopedEnvEntry<K, V>>,
}

impl<K, V> ScopedEnv<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new() -> ScopedEnv<K, V> {
        ScopedEnv {
            entries_map: HashMap::new(),
            memo: Vec::new(),
        }
    }

    pub fn enter_scope(&mut self) {
        self.memo.push(ScopedEnvEntry::Marker);
    }

    pub fn exit_scope(&mut self) {
        while let Some(entry) = self.memo.pop() {
            match entry {
                ScopedEnvEntry::Marker => break,
                ScopedEnvEntry::Insert { key, prev_value } => match prev_value {
                    Some(some_prev_key) => {
                        self.entries_map.insert(key, some_prev_key);
                    }
                    None => {
                        self.entries_map.remove(&key);
                    }
                },
            }
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries_map.get(key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries_map.contains_key(key)
    }

    pub fn insert(&mut self, key: K, value: V) {
        let prev_value = self.entries_map.insert(key.clone(), value);
        self.memo.push(ScopedEnvEntry::Insert { key, prev_value });
    }
}

#[cfg(test)]
mod env_tests {
    use crate::utils::scoped_env::ScopedEnv;

    #[test]
    fn test_scoped_env() {
        let mut env = ScopedEnv::new();
        let lvl_1_key_1 = "a";
        let lvl_1_key_2 = "b";
        let lvl_2_key_1 = "c";
        let lvl_2_key_2 = "d";
        env.enter_scope();
        env.insert(lvl_1_key_1.clone(), 1);
        env.insert(lvl_1_key_2.clone(), 2);
        env.enter_scope();
        env.insert(lvl_2_key_1.clone(), 3);
        env.insert(lvl_2_key_2.clone(), 4);
        assert!(env.contains_key(&lvl_1_key_1));
        assert!(env.contains_key(&lvl_1_key_2));
        assert!(env.contains_key(&lvl_2_key_1));
        assert!(env.contains_key(&lvl_2_key_2));
        env.exit_scope();
        assert!(!env.contains_key(&lvl_2_key_1));
        assert!(!env.contains_key(&lvl_2_key_2));
        assert!(env.contains_key(&lvl_1_key_1));
        assert!(env.contains_key(&lvl_1_key_2));
    }
}
