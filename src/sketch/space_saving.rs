use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct SpaceSaving<K> {
    limit: usize,
    counters: HashMap<K, Counter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    pub count: u64,
    pub error: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSavingUpdate<K> {
    Existing,
    Inserted,
    Replaced { evicted: K },
}

impl<K> SpaceSaving<K>
where
    K: Hash + Eq + Clone + Ord,
{
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            counters: HashMap::new(),
        }
    }

    pub fn observe(&mut self, key: K) {
        let _ = self.observe_update(key);
    }

    pub fn observe_update(&mut self, key: K) -> SpaceSavingUpdate<K> {
        if self.limit == 0 {
            return SpaceSavingUpdate::Existing;
        }

        if let Some(counter) = self.counters.get_mut(&key) {
            counter.count += 1;
            return SpaceSavingUpdate::Existing;
        }

        if self.counters.len() < self.limit {
            self.counters.insert(key, Counter { count: 1, error: 0 });
            return SpaceSavingUpdate::Inserted;
        }

        let victim_key = self
            .counters
            .iter()
            .min_by(|(left_key, left), (right_key, right)| {
                left.count
                    .cmp(&right.count)
                    .then_with(|| left_key.cmp(right_key))
            })
            .map(|(key, _)| key.clone())
            .expect("non-empty counters when limit is reached");
        let victim = self
            .counters
            .remove(&victim_key)
            .expect("victim key comes from counters");
        self.counters.insert(
            key,
            Counter {
                count: victim.count + 1,
                error: victim.count,
            },
        );
        SpaceSavingUpdate::Replaced {
            evicted: victim_key,
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.counters.contains_key(key)
    }

    pub fn keys(&self) -> Vec<K> {
        let mut keys: Vec<_> = self.counters.keys().cloned().collect();
        keys.sort();
        keys
    }

    pub fn top(&self) -> Vec<(K, Counter)> {
        let mut rows: Vec<_> = self
            .counters
            .iter()
            .map(|(key, counter)| (key.clone(), counter.clone()))
            .collect();
        rows.sort_by(|left, right| {
            right
                .1
                .count
                .cmp(&left.1.count)
                .then_with(|| left.0.cmp(&right.0))
        });
        rows
    }
}
