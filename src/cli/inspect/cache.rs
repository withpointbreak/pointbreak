//! One-slot, head-marker-keyed projection caches for the inspect server (#255,
//! #426).
//!
//! Server-side `q` search needs the full body-hydrated haystack, so it cannot
//! slice-before-hydrate; without a cache every `/api/history` query would re-read,
//! re-fold, and re-hydrate the whole log. `/api/revisions` has the same shape:
//! every request rebuilds every revision's overview. Both amortize the full
//! build to once per store version: change is detected with the cheap monotonic
//! `event_log_head_marker` (plan 0090, no event-byte decode) and the cached
//! `Arc` value is served until the marker moves, then dropped and rebuilt.
//! Single-slot (one store version), read-side lazy, no store-dir lock (INV-5).
//! This is ADR-0024 D4's detect-vs-confirm model applied WITHOUT building the
//! deferred redb index.

use std::sync::{Arc, RwLock};

use pointbreak::session::{BaseHistoryProjection, BaseProjectionConfig, TrustSet};

/// The history base projection cache: one fully-hydrated base per store
/// version and reader configuration. Keyed by [`HistoryCacheKey`], not the
/// bare marker: the base embeds trust-, attribution-, and delegation-dependent
/// rendering, and all three documents can change without moving the marker
/// (#460).
pub(super) type HistoryProjectionCache = MarkerCache<HistoryCacheKey, BaseHistoryProjection>;

/// Cache key for the history base projection: the store version (head marker)
/// plus the WHOLE discovered configuration the build renders with, held by
/// value (the documents are small and structurally comparable). `shore key
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

/// The `/api/revisions` response cache: the endpoint takes no query parameters,
/// so the serialized payload itself is the cacheable unit (#426). Keyed by
/// [`RevisionsCacheKey`], not the bare marker: the payload embeds
/// trust-dependent removal decisions and git-derived merge statuses, and both
/// inputs can change without moving the marker.
pub(super) type RevisionsResponseCache = MarkerCache<RevisionsCacheKey, String>;

/// Cache key for the `/api/revisions` payload: the store version (head marker)
/// plus the two non-event inputs the build reads. The trust set (held by
/// value — the allowed-signers document is small and structurally comparable)
/// covers `shore key enroll` and allowed-signers edits, which change
/// operative-removal decisions without appending an event (#426). The
/// commit-graph stamp covers pure-git ref moves — most importantly the landing
/// itself, a fast-forward that flips `mergeStatus` open→merged with no shore
/// event — which would otherwise serve stale until an unrelated event moved
/// the marker (#467).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RevisionsCacheKey {
    pub(super) marker: u64,
    pub(super) trust_set: TrustSet,
    pub(super) commit_graph_stamp: String,
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

    fn revisions_key(marker: u64, stamp: &str) -> RevisionsCacheKey {
        RevisionsCacheKey {
            marker,
            trust_set: TrustSet::default(),
            commit_graph_stamp: stamp.to_owned(),
        }
    }

    #[test]
    fn revisions_cache_reuses_only_on_identical_marker_and_ref_state() {
        let cache = RevisionsResponseCache::new();
        let v1 = cache
            .get_or_build(revisions_key(1, "stamp-a"), |_| {
                Ok("{\"entries\":[]}".to_owned())
            })
            .unwrap();
        let hit = cache
            .get_or_build(revisions_key(1, "stamp-a"), |_| {
                panic!("identical key must not rebuild")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&v1, &hit));

        // Same store version, moved commit-graph stamp: MUST rebuild — a
        // pure-git landing flips merge statuses without moving the marker
        // (#467). Trust changes rebuild the same way (structural key
        // inequality; covered end-to-end in the api tests).
        let restamped = cache
            .get_or_build(revisions_key(1, "stamp-b"), |_| {
                Ok("{\"entries\":[\"landed\"]}".to_owned())
            })
            .unwrap();
        assert_eq!(*restamped, "{\"entries\":[\"landed\"]}");
        assert!(
            cache.try_get(&revisions_key(1, "stamp-a")).is_none(),
            "the single slot now holds the re-stamped payload"
        );

        // Marker move rebuilds as before.
        let v2 = cache
            .get_or_build(revisions_key(2, "stamp-b"), |_| {
                Ok("{\"entries\":[1]}".to_owned())
            })
            .unwrap();
        assert_eq!(*v2, "{\"entries\":[1]}");
    }
}
