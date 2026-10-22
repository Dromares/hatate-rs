//! A minimal least-recently-used cache (`core/lru_cache.py`).
//!
//! Capped by entry count. Reading an entry with [`LruCache::get`] counts as a use, so an entry
//! that was looked at is not the next one evicted; writing past the cap drops whatever was used
//! least recently. Sizes here are a few hundred to a thousand, so order is kept in a plain list.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct LruCache<K: Eq + Hash + Clone, V: Clone> {
    max_entries: usize,
    data: HashMap<K, V>,
    /// Least recently used first.
    order: Vec<K>,
    pub evictions: usize,
}

impl<K: Eq + Hash + Clone, V: Clone> LruCache<K, V> {
    /// `max_entries` must be at least 1 (`ValueError` in Python).
    pub fn new(max_entries: usize) -> Self {
        assert!(max_entries >= 1, "max_entries must be at least 1");
        LruCache {
            max_entries,
            data: HashMap::new(),
            order: Vec::new(),
            evictions: 0,
        }
    }

    fn touch(&mut self, key: &K) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(at);
            self.order.push(k);
        }
    }

    /// `cache.get(key)`: the value, now the most recently used.
    pub fn get(&mut self, key: &K) -> Option<V> {
        let value = self.data.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    /// `cache[key] = value`.
    pub fn set(&mut self, key: K, value: V) {
        if self.data.contains_key(&key) {
            self.touch(&key);
        } else {
            self.order.push(key.clone());
        }
        self.data.insert(key, value);
        while self.data.len() > self.max_entries {
            let oldest = self.order.remove(0);
            self.data.remove(&oldest);
            self.evictions += 1;
        }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.data.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.order.clear();
    }

    /// The keys, least recently used first.
    pub fn keys(&self) -> &[K] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_used_entry_is_the_one_dropped() {
        let mut c = LruCache::new(2);
        c.set("a", 1);
        c.set("b", 2);
        assert_eq!(c.get(&"a"), Some(1)); // a is now newer than b
        c.set("c", 3);
        assert!(!c.contains(&"b"));
        assert_eq!(c.keys(), ["a", "c"]);
        assert_eq!(c.evictions, 1);
    }

    #[test]
    fn rewriting_a_key_refreshes_it_and_does_not_grow_the_cache() {
        let mut c = LruCache::new(2);
        c.set("a", 1);
        c.set("b", 2);
        c.set("a", 10);
        c.set("c", 3);
        assert_eq!(c.get(&"a"), Some(10));
        assert_eq!(c.get(&"b"), None);
        assert_eq!(c.len(), 2);
    }

    #[test]
    #[should_panic(expected = "at least 1")]
    fn a_cache_of_nothing_is_refused() {
        let _ = LruCache::<u8, u8>::new(0);
    }
}
