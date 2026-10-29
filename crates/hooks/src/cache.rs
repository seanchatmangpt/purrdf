//! Condition cache to eliminate redundant condition checks against identical store snapshots.

use std::collections::BTreeMap;

/// Caches boolean condition results for `(hook_id, store_blake3_digest)`.
#[derive(Debug, Default, Clone)]
pub struct ConditionCache {
    cache: BTreeMap<(String, [u8; 32]), bool>,
}

impl ConditionCache {
    /// Create a new, empty condition cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieve a cached condition result, if present.
    pub fn get(&self, hook_id: &str, store_digest: &[u8; 32]) -> Option<bool> {
        self.cache.get(&(hook_id.to_string(), *store_digest)).copied()
    }

    /// Insert or update a cached condition result.
    pub fn insert(&mut self, hook_id: impl Into<String>, store_digest: [u8; 32], result: bool) {
        self.cache.insert((hook_id.into(), store_digest), result);
    }

    /// Clear all cached results.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// True if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}
