//! One-slot, head-marker-keyed projection cache for Inspector history (#255).
//!
//! Server-side `q` search needs the full body-hydrated haystack, so it cannot
//! slice-before-hydrate; without a cache every `/api/history` query would re-read,
//! re-fold, and re-hydrate the whole log. The full build is amortized once per
//! store version: change is detected with the cheap monotonic
//! `event_log_head_marker` and the cached `Arc` is served until the marker
//! moves. Revision collection paging deliberately owns no complete serialized
//! response cache.

use std::sync::{Arc, RwLock};

use pointbreak::session::{BaseHistoryProjection, BaseProjectionConfig};

/// The history base projection cache: one fully-hydrated base per store
/// version and reader configuration. Keyed by [`HistoryCacheKey`], not the
/// bare marker: the base embeds trust-, attribution-, and delegation-dependent
/// rendering, and all three documents can change without moving the marker
/// (#460).
pub(super) type HistoryProjectionCache = MarkerCache<HistoryCacheKey, BaseHistoryProjection>;

/// Cache key for the history base projection: the store version (head marker)
/// plus the WHOLE discovered configuration the build renders with, held by
/// value (the documents are small and structurally comparable). `pointbreak key
/// enroll`, or any edit to the committed allowed-signers / actor-attributes /
/// delegates documents, changes trust-dependent rendering without appending an
/// event, so the key must carry them or enrollment would serve a stale base
/// until an unrelated event moved the marker (#460). Keying on the whole
/// config — not a hand-picked field subset — makes a future discovered input
/// key the cache by construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HistoryCacheKey {
    pub(super) marker: u64,
    pub(super) config: BaseProjectionConfig,
}

/// A single-slot cache of one expensive derivation, keyed by the store version
/// (plus whatever read-side configuration the value depends on).
pub(super) struct MarkerCache<K, T> {
    slot: RwLock<Option<Cached<K, T>>>,
}

struct Cached<K, T> {
    key: K,
    value: Arc<T>,
}

impl<K: PartialEq, T> MarkerCache<K, T> {
    pub(super) fn new() -> Self {
        Self {
            slot: RwLock::new(None),
        }
    }

    /// Return the cached value when `key` matches; otherwise run `build` (which
    /// receives the key, so a build can read the configuration the key carries
    /// without a second clone), store it under `key`, and return it. A build
    /// error caches nothing.
    pub(super) fn get_or_build(
        &self,
        key: K,
        build: impl FnOnce(&K) -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        if let Some(cached) = self.slot.read().unwrap().as_ref()
            && cached.key == key
        {
            return Ok(Arc::clone(&cached.value));
        }
        let mut guard = self.slot.write().unwrap();
        // Re-check under the write lock: another thread may have rebuilt between
        // the read-lock miss and acquiring the write lock.
        if let Some(cached) = guard.as_ref()
            && cached.key == key
        {
            return Ok(Arc::clone(&cached.value));
        }
        let value = Arc::new(build(&key)?);
        *guard = Some(Cached {
            key,
            value: Arc::clone(&value),
        });
        Ok(value)
    }

    /// Return the cached value for `key` only when it is immediately
    /// available. If a background warm is holding the write lock, this
    /// deliberately returns `None` so first-paint paths can avoid waiting for
    /// the full build.
    pub(super) fn try_get(&self, key: &K) -> Option<Arc<T>> {
        let guard = self.slot.try_read().ok()?;
        let cached = guard.as_ref()?;
        (cached.key == *key).then(|| Arc::clone(&cached.value))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn base_stub(tag: &str) -> BaseHistoryProjection {
        BaseHistoryProjection {
            entries: Vec::new(),
            event_set_hash: tag.to_owned(),
            event_count: 0,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn builds_once_and_reuses_on_unchanged_marker() {
        let cache = MarkerCache::<u64, BaseHistoryProjection>::new();
        let builds = AtomicUsize::new(0);
        let build = |_key: &u64| {
            builds.fetch_add(1, Ordering::SeqCst);
            Ok(base_stub("v1"))
        };

        let a = cache.get_or_build(7, build).unwrap();
        let b = cache.get_or_build(7, build).unwrap();
        assert_eq!(
            builds.load(Ordering::SeqCst),
            1,
            "same marker -> built once"
        );
        assert!(Arc::ptr_eq(&a, &b), "same Arc reused");
    }

    #[test]
    fn rebuilds_when_marker_changes() {
        let cache = MarkerCache::<u64, BaseHistoryProjection>::new();
        let builds = AtomicUsize::new(0);

        let v1 = cache
            .get_or_build(7, |_| {
                builds.fetch_add(1, Ordering::SeqCst);
                Ok(base_stub("v1"))
            })
            .unwrap();
        let v2 = cache
            .get_or_build(8, |_| {
                builds.fetch_add(1, Ordering::SeqCst);
                Ok(base_stub("v2"))
            })
            .unwrap();
        assert_eq!(
            builds.load(Ordering::SeqCst),
            2,
            "changed marker -> rebuilt"
        );
        assert!(!Arc::ptr_eq(&v1, &v2));
        assert_eq!(v2.event_set_hash, "v2");
    }

    #[test]
    fn build_error_is_not_cached() {
        let cache = MarkerCache::<u64, BaseHistoryProjection>::new();
        assert!(cache.get_or_build(7, |_| Err("boom".to_owned())).is_err());
        // A subsequent good build at the same marker still runs (the error left no
        // entry behind).
        let ok = cache.get_or_build(7, |_| Ok(base_stub("v1")));
        assert!(ok.is_ok());
    }

    #[test]
    fn try_get_returns_matching_cached_base_without_building() {
        let cache = MarkerCache::<u64, BaseHistoryProjection>::new();
        let built = cache.get_or_build(7, |_| Ok(base_stub("v1"))).unwrap();

        let hit = cache.try_get(&7).expect("matching marker hits");
        assert!(Arc::ptr_eq(&built, &hit));
        assert!(cache.try_get(&8).is_none(), "stale marker misses");
    }
}
