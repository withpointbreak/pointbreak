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

use pointbreak::session::BaseHistoryProjection;

/// The history base projection cache: one fully-hydrated base per store version.
pub(super) type HistoryProjectionCache = MarkerCache<u64, BaseHistoryProjection>;

/// The `/api/revisions` response cache: the endpoint takes no query parameters,
/// so the serialized payload itself is the cacheable unit (#426). Keyed by
/// [`RevisionsCacheKey`], not the bare marker: the payload embeds
/// trust-dependent removal decisions, and trust configuration can change
/// without moving the marker.
pub(super) type RevisionsResponseCache = MarkerCache<RevisionsCacheKey, String>;

/// Cache key for the `/api/revisions` payload: the store version (head marker)
/// plus the reader trust configuration the build read. `shore key enroll` — or
/// any edit to the committed allowed-signers document — changes operative-
/// removal decisions (and with them `file_count`/`row_count` suppression)
/// without appending an event, so the trust fingerprint must key the cache or
/// enrollment would serve stale removal-sensitive counts until an unrelated
/// event moved the marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RevisionsCacheKey {
    pub(super) marker: u64,
    /// The canonical serialization of the trust set the payload was built
    /// with. The allowed-signers document is small and its serialization is
    /// stable (sorted keys), so the string itself is the fingerprint — no
    /// hashing, no collisions.
    pub(super) trust_fingerprint: String,
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

    /// Return the cached value when `key` matches; otherwise run `build`,
    /// store it under `key`, and return it. A build error caches nothing.
    pub(super) fn get_or_build(
        &self,
        key: K,
        build: impl FnOnce() -> Result<T, String>,
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
        let value = Arc::new(build()?);
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
        let cache = HistoryProjectionCache::new();
        let builds = AtomicUsize::new(0);
        let build = || {
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
        let cache = HistoryProjectionCache::new();
        let builds = AtomicUsize::new(0);

        let v1 = cache
            .get_or_build(7, || {
                builds.fetch_add(1, Ordering::SeqCst);
                Ok(base_stub("v1"))
            })
            .unwrap();
        let v2 = cache
            .get_or_build(8, || {
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
        let cache = HistoryProjectionCache::new();
        assert!(cache.get_or_build(7, || Err("boom".to_owned())).is_err());
        // A subsequent good build at the same marker still runs (the error left no
        // entry behind).
        let ok = cache.get_or_build(7, || Ok(base_stub("v1")));
        assert!(ok.is_ok());
    }

    #[test]
    fn try_get_returns_matching_cached_base_without_building() {
        let cache = HistoryProjectionCache::new();
        let built = cache.get_or_build(7, || Ok(base_stub("v1"))).unwrap();

        let hit = cache.try_get(&7).expect("matching marker hits");
        assert!(Arc::ptr_eq(&built, &hit));
        assert!(cache.try_get(&8).is_none(), "stale marker misses");
    }

    fn revisions_key(marker: u64, trust: &str) -> RevisionsCacheKey {
        RevisionsCacheKey {
            marker,
            trust_fingerprint: trust.to_owned(),
        }
    }

    #[test]
    fn revisions_cache_reuses_only_on_identical_marker_and_trust() {
        let cache = RevisionsResponseCache::new();
        let v1 = cache
            .get_or_build(revisions_key(1, "trust-a"), || {
                Ok("{\"entries\":[]}".to_owned())
            })
            .unwrap();
        let hit = cache
            .get_or_build(revisions_key(1, "trust-a"), || {
                panic!("identical key must not rebuild")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&v1, &hit));

        // Same store version, changed trust configuration: MUST rebuild — a
        // key enrollment flips operative-removal decisions without moving the
        // marker.
        let retrusted = cache
            .get_or_build(revisions_key(1, "trust-b"), || {
                Ok("{\"entries\":[\"suppressed\"]}".to_owned())
            })
            .unwrap();
        assert_eq!(*retrusted, "{\"entries\":[\"suppressed\"]}");
        assert!(
            cache.try_get(&revisions_key(1, "trust-a")).is_none(),
            "the single slot now holds the re-trusted payload"
        );

        // Marker move rebuilds as before.
        let v2 = cache
            .get_or_build(revisions_key(2, "trust-b"), || {
                Ok("{\"entries\":[1]}".to_owned())
            })
            .unwrap();
        assert_eq!(*v2, "{\"entries\":[1]}");
    }
}
