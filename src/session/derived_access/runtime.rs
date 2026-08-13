//! Shared process-local runtime for derived-access facades.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use super::generation::GenerationPublication;
use super::layout::{DerivedStorageDiscovery, DerivedStorageLayout};
use super::lifecycle::{
    CurrentGeneration, DerivedAccessLifecycle, LifecycleControl, LifecycleError,
};
use super::product_contract::{DerivedAccessAvailability, DerivedAccessProfile};
use crate::session::store::backend::StoreBackend;
use crate::session::store::resolution::{ReadStore, opaque_path_identity};

const BACKGROUND_REBUILD_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const BACKGROUND_REBUILD_REQUIRED_CONFIRMATION: Duration = Duration::from_millis(250);
const BACKGROUND_TRUTH_CHANGED_MAX_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(super) struct DerivedAccessMaintenance {
    pub(super) profile: DerivedAccessProfile,
    pub(super) store_root: PathBuf,
    pub(super) store_identity: String,
}

impl DerivedAccessMaintenance {
    pub(super) fn lifecycle(&self) -> Result<DerivedAccessLifecycle, String> {
        DerivedAccessLifecycle::new(self.profile, &self.store_root, self.store_identity.clone())
            .map_err(|error| error.to_string())
    }
}

/// Keeps the active state inline because it is the default, request-scoped hot
/// path. Windows makes `DerivedAccessLifecycle` large enough to trigger
/// Clippy's enum-size heuristic, but boxing it would add an allocation to each
/// active-state construction only to shrink the exceptional `Off` representation.
#[cfg_attr(windows, allow(clippy::large_enum_variant))]
pub(super) enum DerivedAccessMode {
    Off,
    Active {
        lifecycle: DerivedAccessLifecycle,
        current: Mutex<Option<Arc<CurrentGeneration>>>,
        store_identity: String,
        backend: StoreBackend,
    },
}

pub(crate) struct DerivedAccessRuntime {
    mode: DerivedAccessMode,
    maintenance: Option<DerivedAccessMaintenance>,
    background_work_state: Arc<AtomicU8>,
    background_rebuild_cancel: Arc<AtomicBool>,
    background_rebuild_handle: Mutex<Option<JoinHandle<()>>>,
}

pub(super) enum RuntimeCurrentRead {
    Ready(Arc<CurrentGeneration>),
    Unavailable(RuntimeCurrentStatus),
}

pub(super) struct RuntimeCurrentStatus {
    pub(super) availability: DerivedAccessAvailability,
    pub(super) detail: Option<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BackgroundWorkPolicy {
    MaintenanceOnly,
    RebuildWhenRequired,
    RebuildRequested,
}

#[repr(u8)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum BackgroundWorkState {
    Idle,
    Maintenance,
    Rebuild,
}

impl BackgroundWorkPolicy {
    fn initial_state(self) -> BackgroundWorkState {
        match self {
            Self::MaintenanceOnly | Self::RebuildWhenRequired => BackgroundWorkState::Maintenance,
            Self::RebuildRequested => BackgroundWorkState::Rebuild,
        }
    }

    fn allows_rebuild(self) -> bool {
        self != Self::MaintenanceOnly
    }
}

impl DerivedAccessRuntime {
    pub(super) fn from_mode(mode: DerivedAccessMode) -> Arc<Self> {
        Self::new(mode, None)
    }

    pub(super) fn new(
        mode: DerivedAccessMode,
        maintenance: Option<DerivedAccessMaintenance>,
    ) -> Arc<Self> {
        Arc::new(Self {
            mode,
            maintenance,
            background_work_state: Arc::new(AtomicU8::new(BackgroundWorkState::Idle as u8)),
            background_rebuild_cancel: Arc::new(AtomicBool::new(false)),
            background_rebuild_handle: Mutex::new(None),
        })
    }

    /// Construct the one shared runtime from an already-resolved store seam.
    /// Callers choose whether authority classification is appropriate before
    /// supplying the store; runtime ownership stays identical either way.
    pub(super) fn from_read_store(read_store: ReadStore) -> Result<Arc<Self>, String> {
        let profile = read_store.derived_access_profile();
        if profile == DerivedAccessProfile::Off {
            return Ok(Self::from_mode(DerivedAccessMode::Off));
        }
        let store_identity = opaque_path_identity("store", read_store.store_dir())
            .map_err(|error| error.to_string())?;
        let maintenance = DerivedAccessMaintenance {
            profile,
            store_root: read_store.store_dir().to_path_buf(),
            store_identity: store_identity.clone(),
        };
        let mode = match DerivedStorageLayout::discover(read_store.store_dir()) {
            DerivedStorageDiscovery::Conflict { .. } => DerivedAccessMode::Off,
            DerivedStorageDiscovery::Selected(_) => {
                let lifecycle = maintenance.lifecycle()?;
                DerivedAccessMode::Active {
                    lifecycle,
                    current: Mutex::new(None),
                    store_identity,
                    backend: read_store.backend().clone(),
                }
            }
        };
        Ok(Self::new(mode, Some(maintenance)))
    }

    pub(super) fn is_active(&self) -> bool {
        matches!(self.mode, DerivedAccessMode::Active { .. }) || self.maintenance.is_some()
    }

    pub(super) fn active_context(&self) -> Option<(&str, &StoreBackend)> {
        let DerivedAccessMode::Active {
            store_identity,
            backend,
            ..
        } = &self.mode
        else {
            return None;
        };
        Some((store_identity, backend))
    }

    pub(super) fn maintenance(&self) -> Option<&DerivedAccessMaintenance> {
        self.maintenance.as_ref()
    }

    pub(super) fn lifecycle(&self) -> Option<&DerivedAccessLifecycle> {
        let DerivedAccessMode::Active { lifecycle, .. } = &self.mode else {
            return None;
        };
        Some(lifecycle)
    }

    /// Observe only the live completion-last publication record. Namespace
    /// selection is refreshed exactly like `current`, but no generation is
    /// selected, opened, validated, installed, or maintained.
    pub(super) fn current_publication_identity(
        &self,
    ) -> Result<Option<GenerationPublication>, String> {
        let DerivedAccessMode::Active {
            lifecycle: configured_lifecycle,
            ..
        } = &self.mode
        else {
            return Ok(None);
        };
        let refreshed_lifecycle;
        let lifecycle = match &self.maintenance {
            Some(maintenance) => {
                refreshed_lifecycle = maintenance.lifecycle()?;
                &refreshed_lifecycle
            }
            None => configured_lifecycle,
        };
        lifecycle
            .published_generation_identity_read_only()
            .map_err(|error| error.to_string())
    }

    /// Clone the process-local reader already selected by another product
    /// path. This is deliberately a cache observation: it performs no
    /// publication discovery, validation, opening, or maintenance work.
    pub(super) fn cached_current(&self) -> Option<Arc<CurrentGeneration>> {
        let DerivedAccessMode::Active { current, .. } = &self.mode else {
            return None;
        };
        lock(current).as_ref().map(Arc::clone)
    }

    /// Perform the cached reader's bounded native authority recheck without
    /// opening, installing, rebuilding, or maintaining a generation.
    pub(super) fn cached_current_authority_is_stable(
        &self,
        current: &CurrentGeneration,
    ) -> Result<bool, String> {
        let DerivedAccessMode::Active {
            lifecycle: configured_lifecycle,
            ..
        } = &self.mode
        else {
            return Err("derived history is disabled".to_owned());
        };
        let refreshed_lifecycle;
        let lifecycle = match &self.maintenance {
            Some(maintenance) => {
                refreshed_lifecycle = maintenance.lifecycle()?;
                &refreshed_lifecycle
            }
            None => configured_lifecycle,
        };
        lifecycle
            .cached_current_authority_is_stable(current)
            .map_err(|error| error.to_string())
    }

    pub(super) fn rebuild_in_flight(&self) -> bool {
        self.background_work_state.load(Ordering::Acquire) == BackgroundWorkState::Rebuild as u8
    }

    #[cfg(test)]
    pub(super) fn maintenance_in_flight(&self) -> bool {
        self.background_work_state.load(Ordering::Acquire) != BackgroundWorkState::Idle as u8
    }

    pub(super) fn rebuild_paused(&self) -> bool {
        self.background_rebuild_cancel.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(super) fn rebuild_worker_joined(&self) -> bool {
        lock(&self.background_rebuild_handle).is_none()
    }

    pub(super) fn current(&self) -> Result<RuntimeCurrentRead, String> {
        self.current_with_publication_retry(true)
    }

    fn current_with_publication_retry(
        &self,
        retry_current_transition: bool,
    ) -> Result<RuntimeCurrentRead, String> {
        let DerivedAccessMode::Active {
            lifecycle: configured_lifecycle,
            current,
            ..
        } = &self.mode
        else {
            return Err("derived history is disabled".to_owned());
        };
        // A compatible namespace transition may have completed since this
        // long-lived access object was constructed. Resolve a fresh lifecycle
        // before selecting a publication; `open_current` then re-resolves once
        // more after acquiring the generation lease, closing both sides of the
        // transition-vs-reader race.
        let refreshed_lifecycle;
        let lifecycle = match &self.maintenance {
            Some(maintenance) => {
                refreshed_lifecycle = maintenance.lifecycle()?;
                &refreshed_lifecycle
            }
            None => configured_lifecycle,
        };
        let published_generation_id = match lifecycle.published_generation_id() {
            Ok(generation_id) => generation_id,
            Err(error) => {
                self.request_background_rebuild();
                return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                    DerivedAccessAvailability::Unavailable,
                    error.to_string(),
                )));
            }
        };
        let existing = {
            let mut guard = lock(current);
            match guard.as_ref() {
                Some(existing)
                    if published_generation_id.as_deref() == Some(existing.generation_id()) =>
                {
                    Some(Arc::clone(existing))
                }
                Some(_) => {
                    *guard = None;
                    None
                }
                None => None,
            }
        };
        if let Some(existing) = existing {
            let validation = match lifecycle.validate_cached_current(&existing) {
                Ok(validation) => {
                    if validation.authority_maintenance_pending {
                        self.request_background_rebuild();
                    }
                    validation
                }
                Err(LifecycleError::RebuildRequired(detail)) => {
                    self.request_background_rebuild();
                    return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                        DerivedAccessAvailability::RebuildRequired,
                        detail,
                    )));
                }
                Err(error) => {
                    clear_current_if_same(current, &existing);
                    self.request_background_rebuild();
                    return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                        DerivedAccessAvailability::Unavailable,
                        error.to_string(),
                    )));
                }
            };
            let confirmed_generation_id = lifecycle
                .published_generation_id()
                .map_err(|error| error.to_string())?;
            if confirmed_generation_id.as_deref() != Some(existing.generation_id()) {
                clear_current_if_same(current, &existing);
                if retry_current_transition {
                    return self.current_with_publication_retry(false);
                }
                self.request_background_rebuild();
                return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                    DerivedAccessAvailability::Unavailable,
                    "current generation changed while validating the cached reader",
                )));
            }
            if validation.locator_applied == validation.authority.head.cursor {
                return Ok(RuntimeCurrentRead::Ready(existing));
            }
            self.request_background_rebuild();
            return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                DerivedAccessAvailability::CatchingUp,
                "derived history is catching up to authoritative truth",
            )));
        }
        match lifecycle.open_current() {
            Ok(Some(opened)) => {
                let opened = Arc::new(opened);
                let confirmed_generation_id = lifecycle
                    .published_generation_id()
                    .map_err(|error| error.to_string())?;
                if confirmed_generation_id.as_deref() != Some(opened.generation_id()) {
                    if retry_current_transition {
                        return self.current_with_publication_retry(false);
                    }
                    self.request_background_rebuild();
                    return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                        DerivedAccessAvailability::Unavailable,
                        "current generation changed before the reader cache was installed",
                    )));
                }
                let selected = {
                    let mut guard = lock(current);
                    match guard.as_ref() {
                        Some(existing) => Arc::clone(existing),
                        None => {
                            *guard = Some(Arc::clone(&opened));
                            Arc::clone(&opened)
                        }
                    }
                };
                if !Arc::ptr_eq(&selected, &opened) {
                    return self.current_with_publication_retry(retry_current_transition);
                }
                if opened.authority_maintenance_pending() {
                    self.request_background_rebuild();
                }
                if opened.locator_applied() != opened.authority_head() {
                    self.request_background_rebuild();
                    return Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                        DerivedAccessAvailability::CatchingUp,
                        "derived history is catching up to authoritative truth",
                    )));
                }
                Ok(RuntimeCurrentRead::Ready(opened))
            }
            Ok(None) => {
                let observed = lifecycle.status();
                if retry_current_transition
                    && matches!(
                        observed.as_ref(),
                        Ok(status) if status.availability == DerivedAccessAvailability::Current
                    )
                {
                    // Publication completed after `open_current` selected its
                    // input. Retry once so a usable Current generation becomes
                    // a Ready payload, never a 503 carrying "current".
                    return self.current_with_publication_retry(false);
                }
                self.request_background_rebuild();
                Ok(RuntimeCurrentRead::Unavailable(match observed {
                    Ok(observed) => unavailable_lifecycle_status(
                        observed,
                        "current generation was not openable after publication",
                    ),
                    Err(error) => {
                        runtime_status(DerivedAccessAvailability::Unavailable, error.to_string())
                    }
                }))
            }
            Err(error) => {
                let observed = lifecycle.status();
                if retry_current_transition
                    && matches!(
                        observed.as_ref(),
                        Ok(status) if status.availability == DerivedAccessAvailability::Current
                    )
                {
                    return self.current_with_publication_retry(false);
                }
                self.request_background_rebuild();
                match observed {
                    Ok(observed) => Ok(RuntimeCurrentRead::Unavailable(
                        unavailable_lifecycle_status(observed, &error.to_string()),
                    )),
                    Err(status_error) => Ok(RuntimeCurrentRead::Unavailable(runtime_status(
                        DerivedAccessAvailability::Unavailable,
                        format!("{error}; derived status also failed: {status_error}"),
                    ))),
                }
            }
        }
    }

    pub(super) fn start_background_rebuild(&self) -> Result<(), String> {
        let Some(lifecycle) = self.worker_lifecycle()? else {
            return Ok(());
        };
        let policy = if lifecycle
            .change_capability_activated()
            .map_err(|error| error.to_string())?
        {
            BackgroundWorkPolicy::MaintenanceOnly
        } else {
            BackgroundWorkPolicy::RebuildWhenRequired
        };
        self.start_background_worker(lifecycle, policy)
    }

    fn worker_lifecycle(&self) -> Result<Option<DerivedAccessLifecycle>, String> {
        let DerivedAccessMode::Active {
            lifecycle: configured_lifecycle,
            ..
        } = &self.mode
        else {
            return Ok(None);
        };
        match &self.maintenance {
            Some(maintenance) => maintenance.lifecycle().map(Some),
            None => Ok(Some(configured_lifecycle.clone())),
        }
    }

    fn start_background_worker(
        &self,
        lifecycle: DerivedAccessLifecycle,
        policy: BackgroundWorkPolicy,
    ) -> Result<(), String> {
        // The handle mutex is also the worker-control mutex. Holding it through
        // completed-worker join and new-worker publication makes start vs.
        // cancel linearizable: cancel cannot return while an overlapping start
        // leaves an unjoined replacement behind.
        let mut handle_slot = lock(&self.background_rebuild_handle);
        self.start_background_worker_locked(&mut handle_slot, lifecycle, policy)
    }

    fn start_background_worker_locked(
        &self,
        handle_slot: &mut Option<JoinHandle<()>>,
        lifecycle: DerivedAccessLifecycle,
        policy: BackgroundWorkPolicy,
    ) -> Result<(), String> {
        if self.background_rebuild_cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.background_work_state.load(Ordering::Acquire) != BackgroundWorkState::Idle as u8 {
            return Ok(());
        }
        if let Some(prior) = handle_slot.take()
            && prior.join().is_err()
        {
            self.background_work_state
                .store(BackgroundWorkState::Idle as u8, Ordering::Release);
            return Err("prior derived-access rebuild worker panicked".to_owned());
        }
        if self
            .background_work_state
            .compare_exchange(
                BackgroundWorkState::Idle as u8,
                policy.initial_state() as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Ok(());
        }
        let work_state = Arc::clone(&self.background_work_state);
        let cancel = Arc::clone(&self.background_rebuild_cancel);
        let spawned = std::thread::Builder::new()
            .name("pointbreak-derived-rebuild".to_owned())
            .spawn(move || {
                let _guard = BackgroundWorkerGuard(Arc::clone(&work_state));
                background_rebuild(lifecycle, policy, &work_state, cancel)
            });
        match spawned {
            Ok(handle) => {
                *handle_slot = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.background_work_state
                    .store(BackgroundWorkState::Idle as u8, Ordering::Release);
                Err(format!("could not start derived-access rebuild: {error}"))
            }
        }
    }

    pub(super) fn cancel_background_rebuild(&self) -> Result<(), String> {
        let mut handle_slot = lock(&self.background_rebuild_handle);
        self.cancel_background_rebuild_locked(&mut handle_slot)
    }

    fn cancel_background_rebuild_locked(
        &self,
        handle_slot: &mut Option<JoinHandle<()>>,
    ) -> Result<(), String> {
        self.background_rebuild_cancel
            .store(true, Ordering::Release);
        let joined = handle_slot
            .take()
            .map(JoinHandle::join)
            .transpose()
            .map(|_| ())
            .map_err(|_| "derived-access rebuild worker panicked".to_owned());
        self.background_work_state
            .store(BackgroundWorkState::Idle as u8, Ordering::Release);
        joined
    }

    pub(super) fn restart_background_rebuild(&self) -> Result<(), String> {
        let mut handle_slot = lock(&self.background_rebuild_handle);
        self.cancel_background_rebuild_locked(&mut handle_slot)?;
        self.background_rebuild_cancel
            .store(false, Ordering::Release);
        let Some(lifecycle) = self.worker_lifecycle()? else {
            return Ok(());
        };
        self.start_background_worker_locked(
            &mut handle_slot,
            lifecycle,
            BackgroundWorkPolicy::RebuildRequested,
        )
    }

    fn request_background_rebuild(&self) {
        if let Err(error) = self.start_background_rebuild() {
            tracing::warn!(error = %error, "derived_access_background_rebuild_start_failed");
        }
    }
}

impl Drop for DerivedAccessRuntime {
    fn drop(&mut self) {
        if let Err(error) = self.cancel_background_rebuild() {
            tracing::warn!(error = %error, "derived_access_background_rebuild_join_failed");
        }
    }
}

struct BackgroundWorkerGuard(Arc<AtomicU8>);

impl Drop for BackgroundWorkerGuard {
    fn drop(&mut self) {
        self.0
            .store(BackgroundWorkState::Idle as u8, Ordering::Release);
    }
}

fn background_rebuild(
    lifecycle: DerivedAccessLifecycle,
    policy: BackgroundWorkPolicy,
    work_state: &AtomicU8,
    cancel: Arc<AtomicBool>,
) {
    let mut truth_changed_retry_interval = BACKGROUND_REBUILD_RETRY_INTERVAL;
    let mut rebuild_required_confirmed = false;
    loop {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        if !policy.allows_rebuild() {
            match lifecycle.maintain_current_generation() {
                Ok(true) => return,
                Ok(false) => {
                    if wait_or_cancel(&cancel, BACKGROUND_REBUILD_RETRY_INTERVAL) {
                        return;
                    }
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "derived_access_background_current_maintenance_failed"
                    );
                    return;
                }
            }
        }
        match lifecycle.status() {
            Ok(status)
                if matches!(
                    status.availability,
                    DerivedAccessAvailability::Current | DerivedAccessAvailability::CatchingUp
                ) =>
            {
                match lifecycle.maintain_current_generation() {
                    Ok(true) => return,
                    Ok(false) => {
                        if wait_or_cancel(&cancel, BACKGROUND_REBUILD_RETRY_INTERVAL) {
                            return;
                        }
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "derived_access_background_current_maintenance_failed"
                        );
                        return;
                    }
                }
            }
            Ok(status)
                if status.availability == DerivedAccessAvailability::RebuildRequired
                    && !rebuild_required_confirmed =>
            {
                rebuild_required_confirmed = true;
                if wait_or_cancel(&cancel, BACKGROUND_REBUILD_REQUIRED_CONFIRMATION) {
                    return;
                }
                continue;
            }
            Ok(status) if status.availability == DerivedAccessAvailability::RebuildRequired => {
                match lifecycle.rebuild_required_while_writer_idle() {
                    Ok(true) => {}
                    Ok(false) => {
                        if wait_or_cancel(&cancel, BACKGROUND_REBUILD_REQUIRED_CONFIRMATION) {
                            return;
                        }
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "derived_access_background_rebuild_confirmation_failed"
                        );
                        return;
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error = %error, "derived_access_background_status_failed");
                return;
            }
        }
        work_state.store(BackgroundWorkState::Rebuild as u8, Ordering::Release);
        let progress = |_| {
            if cancel.load(Ordering::Acquire) {
                LifecycleControl::Cancel
            } else {
                LifecycleControl::Continue
            }
        };
        let rebuild = match policy {
            BackgroundWorkPolicy::RebuildWhenRequired => {
                lifecycle.try_automatic_legacy_rebuild(progress)
            }
            BackgroundWorkPolicy::RebuildRequested => {
                lifecycle.try_explicit_background_rebuild(progress)
            }
            BackgroundWorkPolicy::MaintenanceOnly => {
                unreachable!("maintenance-only background work returns before rebuild admission")
            }
        };
        match rebuild {
            Ok(_) => return,
            Err(LifecycleError::RebuildBusy) => {
                if wait_or_cancel(&cancel, BACKGROUND_REBUILD_RETRY_INTERVAL) {
                    return;
                }
            }
            Err(LifecycleError::TruthChanged) => {
                if wait_or_cancel(&cancel, truth_changed_retry_interval) {
                    return;
                }
                truth_changed_retry_interval = truth_changed_retry_interval
                    .saturating_mul(2)
                    .min(BACKGROUND_TRUTH_CHANGED_MAX_INTERVAL);
            }
            Err(LifecycleError::Cancelled) => return,
            Err(LifecycleError::AutomaticRebuildSuppressed) => return,
            Err(error) => {
                tracing::warn!(error = %error, "derived_access_background_rebuild_failed");
                return;
            }
        }
    }
}

fn wait_or_cancel(cancel: &AtomicBool, duration: Duration) -> bool {
    const POLL: Duration = Duration::from_millis(25);
    let mut remaining = duration;
    while !remaining.is_zero() {
        if cancel.load(Ordering::Acquire) {
            return true;
        }
        let wait = remaining.min(POLL);
        std::thread::sleep(wait);
        remaining = remaining.saturating_sub(wait);
    }
    cancel.load(Ordering::Acquire)
}

fn unavailable_lifecycle_status(
    observed: super::lifecycle::LifecycleStatus,
    fallback_detail: &str,
) -> RuntimeCurrentStatus {
    RuntimeCurrentStatus {
        availability: if observed.availability == DerivedAccessAvailability::Current {
            DerivedAccessAvailability::Unavailable
        } else {
            observed.availability
        },
        detail: observed.detail.or_else(|| Some(fallback_detail.to_owned())),
    }
}

fn runtime_status(
    availability: DerivedAccessAvailability,
    detail: impl Into<String>,
) -> RuntimeCurrentStatus {
    RuntimeCurrentStatus {
        availability,
        detail: Some(detail.into()),
    }
}

fn clear_current_if_same(
    current: &Mutex<Option<Arc<CurrentGeneration>>>,
    expected: &Arc<CurrentGeneration>,
) {
    let mut guard = lock(current);
    if guard
        .as_ref()
        .is_some_and(|cached| Arc::ptr_eq(cached, expected))
    {
        *guard = None;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
