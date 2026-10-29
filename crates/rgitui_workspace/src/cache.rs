//! Generic LRU cache with bounded capacity and an optional value-weight budget.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// A bounded Least-Recently-Used cache.
///
/// Evicts the least-recently-used entry when capacity is exceeded on insert.
/// The "most recently used" end is the back of the internal queue; the
/// "least recently used" end is the front.
#[derive(Debug)]
pub struct LruCache<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    cap: usize,
    weight_budget: Option<WeightBudget<V>>,
}

#[derive(Debug)]
struct WeightBudget<V> {
    limit: usize,
    used: usize,
    weigh: fn(&V) -> usize,
}

/// Keep the existing entry limit for text, but bound retained PNG snapshots too.
/// Shared snapshots are counted per reference, conservatively overcounting when
/// entries share an allocation. Decoded images and externally held Arcs are not
/// owned by this cache and are outside this budget.
pub(crate) fn commit_diff_cache() -> LruCache<git2::Oid, std::sync::Arc<rgitui_git::CommitDiff>> {
    LruCache::with_weight_budget(200, 64 * 1024 * 1024, |diff| encoded_image_bytes(diff))
}

fn encoded_image_bytes(diff: &rgitui_git::CommitDiff) -> usize {
    diff.files
        .iter()
        .filter_map(|file| file.image.as_ref())
        .flat_map(|image| [image.before.as_ref(), image.after.as_ref()])
        .flatten()
        .fold(0usize, |total, bytes| total.saturating_add(bytes.len()))
}

impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    /// Create a new cache with the given capacity.
    ///
    /// Capacity must be > 0.  Panics otherwise.
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "LruCache capacity must be > 0");
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
            weight_budget: None,
        }
    }

    /// Add an independent weight limit. The weighing function must return a
    /// stable weight while a value is cached. Zero-weight entries are skipped
    /// when evicting for weight, but still obey the normal entry-count LRU.
    pub fn with_weight_budget(cap: usize, limit: usize, weigh: fn(&V) -> usize) -> Self {
        let mut cache = Self::new(cap);
        cache.weight_budget = Some(WeightBudget {
            limit,
            used: 0,
            weigh,
        });
        cache
    }

    /// Look up a key and mark it as most-recently-used.
    ///
    /// Returns the value if present, otherwise `None`.
    pub fn get(&mut self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        if self.map.contains_key(key) {
            log::debug!("LruCache::get hit: len={}/{}", self.map.len(), self.cap);
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
                self.order.push_back(key.clone());
            }
            self.map.get(key).cloned()
        } else {
            log::debug!("LruCache::get miss: len={}/{}", self.map.len(), self.cap);
            None
        }
    }

    /// Check whether the cache contains a key without updating LRU order.
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Number of entries currently held, for the performance census.
    ///
    /// The cache is bounded by this count, so a report that gives bytes without
    /// it cannot show whether the bound or the entry size is what made the
    /// cache expensive.
    #[cfg(feature = "perf")]
    pub(crate) fn census_len(&self) -> usize {
        self.map.len()
    }

    /// Inspect a cached value without changing its recency.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Insert a key-value pair.
    ///
    /// If the key was already present, its value is updated and the entry
    /// is re-marked as most-recently-used (moved to back of LRU order).
    /// If the key is new and the cache is at capacity, the least-recently-used
    /// entry (oldest) is evicted first. Weight pressure evicts the oldest
    /// nonzero-weight entries. An individually oversized value is not cached;
    /// any previous value for its key is removed to avoid serving stale data,
    /// but unrelated entries are left intact.
    pub fn insert(&mut self, key: K, value: V) {
        let weight = self
            .weight_budget
            .as_ref()
            .map_or(0, |budget| (budget.weigh)(&value));

        if self.map.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
            self.remove_value(&key);
        }

        if self
            .weight_budget
            .as_ref()
            .is_some_and(|budget| weight > budget.limit)
        {
            return;
        }

        // Preserve the usual LRU behavior for the independent entry limit.
        if self.map.len() >= self.cap {
            log::debug!(
                "LruCache::insert eviction: len={}/{}",
                self.map.len(),
                self.cap
            );
            if let Some(evicted) = self.order.pop_front() {
                self.remove_value(&evicted);
            }
        }

        while let Some(budget) = &self.weight_budget {
            // Subtraction avoids overflow even with a usize::MAX limit.
            if budget.used <= budget.limit - weight {
                break;
            }
            let position = self
                .order
                .iter()
                .position(|key| (budget.weigh)(&self.map[key]) > 0)
                .expect("weight pressure implies a nonzero-weight entry");
            let evicted = self.order.remove(position).expect("position is in bounds");
            self.remove_value(&evicted);
        }

        if let Some(budget) = &mut self.weight_budget {
            budget.used += weight;
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    fn remove_value(&mut self, key: &K) {
        if let Some(value) = self.map.remove(key) {
            if let Some(budget) = &mut self.weight_budget {
                budget.used -= (budget.weigh)(&value);
            }
        }
    }
}

/// Measuring a cache needs more of its keys and values than using one does, so
/// the census lives in its own impl block rather than adding `HeapSize` to the
/// bounds every caller has to satisfy.
#[cfg(feature = "perf")]
impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Clone + rgitui_perf::HeapSize,
    V: rgitui_perf::HeapSize,
{
    /// Heap bytes held by the entries.
    ///
    /// A capacity bound counted in entries says nothing about bytes, and these
    /// caches hold values whose size varies by orders of magnitude — a
    /// `CommitDiff` for a typo and one for a vendored dependency bump are both
    /// "one entry". This is what turns that bound into a number.
    ///
    /// The recency queue is charged too. It holds a second copy of every key,
    /// which for a `ViewCacheKey` means a second repository path and file path
    /// per entry — small individually, and not nothing across 200 of them.
    pub(crate) fn census_entries(&self, census: &mut rgitui_perf::Census) -> usize {
        use rgitui_perf::HeapSize as _;

        let mut total = self.map.heap_size(census);
        total += self.order.capacity() * std::mem::size_of::<K>();
        total += self
            .order
            .iter()
            .map(|key| key.heap_size(census))
            .sum::<usize>();
        total
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_with_cap<K, V>(cap: usize) -> LruCache<K, V>
    where
        K: Hash + Eq + Clone,
    {
        LruCache::new(cap)
    }

    // ── Basic ops ────────────────────────────────────────────────────────────

    #[test]
    fn insert_and_get() {
        let mut c = cache_with_cap(3);
        c.insert(1, "a");
        assert_eq!(c.get(&1), Some("a"));
        assert!(c.contains(&1));
        assert!(!c.contains(&2));
    }

    #[test]
    fn get_updates_lru_order() {
        let mut c = cache_with_cap(3);
        c.insert(1, "a");
        c.insert(2, "b");
        c.insert(3, "c");

        // Access 1, making it most-recently-used.
        assert_eq!(c.get(&1), Some("a"));

        // Insert 4 — should evict 2 (LRU after 1 was touched).
        c.insert(4, "d");

        assert!(!c.contains(&2), "2 should be evicted");
        assert!(c.contains(&1), "1 should NOT be evicted");
        assert!(c.contains(&3), "3 should NOT be evicted");
        assert!(c.contains(&4), "4 should be present");
    }

    #[test]
    fn update_existing_key_does_not_evict() {
        let mut c = cache_with_cap(2);
        c.insert(1, "a");
        c.insert(2, "b");

        // Update key that is already in cache — should not affect LRU list
        // and should not cause eviction of other entries.
        c.insert(1, "a-updated");

        assert_eq!(c.get(&1), Some("a-updated"));
        assert_eq!(c.get(&2), Some("b"));
    }

    #[test]
    fn lru_eviction_order() {
        let mut c = cache_with_cap(3);
        c.insert(1, "a");
        c.insert(2, "b");
        c.insert(3, "c");

        // Evict LRU: 1 is oldest.
        c.insert(4, "d");

        assert!(!c.contains(&1));
        assert!(c.contains(&2));
        assert!(c.contains(&3));
        assert!(c.contains(&4));

        // Evict next: 2 is now oldest.
        c.insert(5, "e");

        assert!(!c.contains(&2));
        assert!(c.contains(&3));
        assert!(c.contains(&4));
        assert!(c.contains(&5));
    }

    #[test]
    fn reinsert_evicted_key_at_end() {
        let mut c = cache_with_cap(2);
        c.insert(1, "a");
        c.insert(2, "b");

        // 1 is evicted when 3 is inserted.
        c.insert(3, "c");
        assert!(!c.contains(&1));
        assert!(c.contains(&2));
        assert!(c.contains(&3));

        // Re-insert 1 — it becomes MRU (back of order).
        c.insert(1, "a-again");
        assert!(c.contains(&1));
        // 3 is now LRU (it was inserted before 1).
        assert!(c.contains(&3));

        // 4 is new and cache is full → 3 (LRU) is evicted.
        c.insert(4, "d");
        assert!(c.contains(&1), "1 should be back in cache");
        assert!(!c.contains(&2), "2 was already evicted earlier");
        assert!(
            !c.contains(&3),
            "3 should be evicted (it became LRU after 1 was reinserted)"
        );
        assert!(c.contains(&4));
    }

    #[test]
    fn full_cache_at_capacity() {
        let mut c = cache_with_cap(3);
        c.insert(1, "a");
        c.insert(2, "b");
        c.insert(3, "c");
        assert!(c.contains(&1));
        assert!(c.contains(&2));
        assert!(c.contains(&3));

        // Adding one more evicts the LRU entry (1, the oldest).
        c.insert(4, "d");
        assert!(!c.contains(&1));
        assert!(c.contains(&2));
        assert!(c.contains(&3));
        assert!(c.contains(&4));
    }

    #[test]
    fn single_insert_then_evict() {
        let mut c: LruCache<i32, &str> = LruCache::new(2);
        c.insert(1, "one");
        c.insert(2, "two");
        c.insert(3, "three");
        // 1 should be evicted (oldest).
        assert!(!c.contains(&1));
        assert!(c.contains(&2));
        assert!(c.contains(&3));
    }

    #[test]
    fn contains_after_get() {
        let mut c = cache_with_cap(3);
        c.insert(1, "a");
        c.insert(2, "b");

        c.get(&1);

        c.insert(3, "c");
        c.insert(4, "d");

        // 1 was touched and is therefore NOT LRU — 2 should be evicted.
        assert!(!c.contains(&2));
        assert!(c.contains(&1));
    }

    #[test]
    fn peek_returns_value_without_refreshing_lru_order() {
        let mut c = cache_with_cap(2);
        c.insert(1, "one");
        c.insert(2, "two");

        assert_eq!(c.peek(&1), Some(&"one"));
        c.insert(3, "three");

        assert!(!c.contains(&1), "peek must not make an entry most-recent");
        assert!(c.contains(&2));
        assert!(c.contains(&3));
    }

    #[test]
    fn weight_eviction_skips_text_and_respects_recency() {
        let mut c = LruCache::with_weight_budget(8, 10, |weight: &usize| *weight);
        c.insert("text", 0);
        c.insert("a", 3);
        c.insert("b", 3);
        c.insert("c", 4);
        c.get(&"a");
        c.insert("d", 7);

        assert_eq!(c.order, VecDeque::from(["text", "a", "d"]));
        assert!(!c.contains(&"b"));
        assert!(!c.contains(&"c"));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);
    }

    #[test]
    fn weighted_replacement_releases_old_weight_and_can_evict() {
        let mut c = LruCache::with_weight_budget(3, 10, |weight: &usize| *weight);
        c.insert(1, 2);
        c.insert(2, 4);
        c.insert(3, 4);
        c.insert(1, 6);
        assert_eq!(c.order, VecDeque::from([3, 1]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);

        c.insert(1, 1);
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 5);
        c.insert(3, 0);
        c.insert(4, 9);
        assert_eq!(c.order, VecDeque::from([1, 3, 4]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);
    }

    #[test]
    fn oversized_insert_does_not_flush_cache_or_change_recency() {
        let mut c = LruCache::with_weight_budget(2, 10, |weight: &usize| *weight);
        c.insert(1, 4);
        c.insert(2, 6);
        c.insert(3, 11);
        assert_eq!(c.order, VecDeque::from([1, 2]));
        assert!(!c.contains(&3));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);

        // An oversized replacement must not leave the old value behind.
        c.insert(1, 11);
        assert!(!c.contains(&1));
        assert_eq!(c.order, VecDeque::from([2]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 6);
        c.insert(3, 4);
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);
    }

    #[test]
    fn entry_eviction_also_releases_weight() {
        let mut c = LruCache::with_weight_budget(2, 10, |weight: &usize| *weight);
        c.insert(1, 10);
        c.insert(2, 0);
        c.insert(3, 0);
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 0);
        c.insert(4, 10);
        assert_eq!(c.order, VecDeque::from([3, 4]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 10);
    }

    #[test]
    fn zero_budget_retains_zero_weight_entries_up_to_entry_limit() {
        let mut c = LruCache::with_weight_budget(2, 0, |weight: &usize| *weight);
        c.insert(1, 0);
        c.insert(2, 0);
        c.insert(3, 1);
        assert_eq!(c.order, VecDeque::from([1, 2]));
        c.insert(4, 0);
        assert_eq!(c.order, VecDeque::from([2, 4]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 0);
    }

    #[test]
    fn weight_pressure_does_not_overflow() {
        let mut c = LruCache::with_weight_budget(2, usize::MAX, |weight: &usize| *weight);
        c.insert(1, usize::MAX);
        c.insert(2, 1);
        assert_eq!(c.order, VecDeque::from([2]));
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 1);
    }

    #[test]
    fn commit_diff_budget_counts_both_sides_and_all_files() {
        use rgitui_git::{CommitDiff, FileChangeKind, FileDiff, ImageDiff};
        use std::sync::Arc;

        let file = |image| FileDiff {
            path: "image.png".into(),
            image,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            kind: FileChangeKind::Modified,
        };
        let shared: Arc<[u8]> = Arc::from(&b"abc"[..]);
        let diff = Arc::new(CommitDiff {
            files: vec![
                file(Some(ImageDiff {
                    before: Some(shared.clone()),
                    after: Some(Arc::from(&b"12345"[..])),
                    error: None,
                })),
                file(Some(ImageDiff {
                    before: None,
                    after: Some(shared),
                    error: None,
                })),
                file(Some(ImageDiff {
                    before: None,
                    after: None,
                    error: Some("preview unavailable".into()),
                })),
                file(None),
            ],
            total_additions: 0,
            total_deletions: 0,
        });
        let mut c = commit_diff_cache();
        assert_eq!(c.cap, 200);
        assert_eq!(c.weight_budget.as_ref().unwrap().limit, 64 * 1024 * 1024);
        assert_eq!(encoded_image_bytes(&diff), 11);
        c.insert(git2::Oid::zero(), diff);
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 11);
    }

    #[test]
    fn commit_diff_budget_preserves_text_entry_capacity() {
        let mut c = commit_diff_cache();
        let text = std::sync::Arc::new(rgitui_git::CommitDiff {
            files: Vec::new(),
            total_additions: 0,
            total_deletions: 0,
        });
        let oid = |n: u8| git2::Oid::from_bytes(&[n; 20]).unwrap();
        for n in 0..200 {
            c.insert(oid(n), text.clone());
        }
        assert_eq!(c.map.len(), 200);
        c.get(&oid(0));
        c.insert(oid(200), text);
        assert!(c.contains(&oid(0)));
        assert!(!c.contains(&oid(1)));
        assert_eq!(c.map.len(), 200);
        assert_eq!(c.weight_budget.as_ref().unwrap().used, 0);
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn new_with_zero_capacity_panics() {
        let _: LruCache<i32, &str> = LruCache::new(0);
    }
}
