use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{
    INTERACTION_PERFORMANCE_RECEIPT_SCHEMA_V1, InteractionActorV1, InteractionChildScopeFactV1,
    InteractionLockAcquisitionV1, InteractionLockFactV1, InteractionLockKindV1,
    InteractionLockModeV1, InteractionLockOutcomeV1, InteractionObservedFactsV1,
    InteractionObservedRouteStateV1, InteractionPerformanceExpectedContextV1,
    InteractionPerformanceReceiptV1, InteractionRouteV1, InteractionScopeCoverageV1,
    LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1,
    LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_SCHEMA_V1, LongitudinalCapacityOwnershipV1,
    LongitudinalContractError, LongitudinalCounterReceiptV1, LongitudinalCountersV1,
    LongitudinalTimelineCarrierMismatchKindV1, LongitudinalTimelinePostPinBarrierReceiptV1,
    LongitudinalTimelinePostPinBoundaryV1, interaction_scope_coverage_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

thread_local! {
    static ACTIVE_SCOPES: RefCell<Vec<LongitudinalCountingScopeV1>> =
        const { RefCell::new(Vec::new()) };
    static ACTIVE_DERIVED_ACCESS_PHASES: RefCell<Vec<u16>> =
        const { RefCell::new(Vec::new()) };
    static ACTIVE_INTERACTION_ACTORS: RefCell<Vec<(Arc<Mutex<ObserverState>>, InteractionActorV1)>> =
        const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Default)]
struct ObserverState {
    counters: LongitudinalCountersV1,
    capacity_ownership: LongitudinalCapacityOwnershipV1,
    derived_access_phases: Vec<LongitudinalDerivedAccessPhaseSampleV1>,
    next_phase_ordinal: u16,
    observed_routes: Vec<InteractionRouteV1>,
    observed_route_states: Vec<InteractionObservedRouteStateV1>,
    execution_actors: Vec<InteractionActorV1>,
    outcomes: Vec<(bool, i32)>,
    semantic_result_sha256: Vec<String>,
    child_reservations: Vec<(u16, InteractionActorV1)>,
    child_terminals: Vec<InteractionChildScopeFactV1>,
    next_child_ordinal: u16,
    lock_facts: Vec<InteractionLockFactV1>,
    next_lock_ordinal: u16,
    timeline_post_pin_barrier: Option<LongitudinalTimelinePostPinBarrierStateV1>,
}

pub const LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1: &str =
    "pointbreak.longitudinal-timeline-post-pin-barrier-request.v1";
pub const LONGITUDINAL_TIMELINE_POST_PIN_READY_SCHEMA_V1: &str =
    "pointbreak.longitudinal-timeline-post-pin-ready.v1";
pub const LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1: &str =
    "pointbreak.longitudinal-timeline-post-pin-release.v1";
#[doc(hidden)]
pub const LONGITUDINAL_COUNTING_REQUEST_HEADER_V1: &str = "X-Pointbreak-Longitudinal-Counting";
#[doc(hidden)]
pub const LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1: &str = "X-Pointbreak-Longitudinal-Receipt";
#[doc(hidden)]
pub const LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT_ENV_V1: &str =
    "POINTBREAK_LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT";
#[doc(hidden)]
pub const LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_HEADER_V1: &str =
    "X-Pointbreak-Longitudinal-Timeline-Post-Pin-Barrier-Receipt";

const TIMELINE_POST_PIN_BARRIER_WAIT: Duration = Duration::from_secs(10);
const TIMELINE_POST_PIN_BARRIER_POLL: Duration = Duration::from_millis(5);

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalTimelinePostPinBarrierRequestV1 {
    pub schema: String,
    pub barrier_identity_sha256: String,
    pub expected_carrier_key_digest: String,
    pub clean_carrier_sha256: String,
    pub mutated_carrier_sha256: String,
    pub mutation_recipe_sha256: String,
}

impl LongitudinalTimelinePostPinBarrierRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1 {
            return Err("unsupported Timeline post-pin barrier request schema".to_owned());
        }
        validate_timeline_barrier_hashes([
            (&self.barrier_identity_sha256, "barrier identity"),
            (
                &self.expected_carrier_key_digest,
                "expected carrier key digest",
            ),
            (&self.clean_carrier_sha256, "clean carrier SHA-256"),
            (&self.mutated_carrier_sha256, "mutated carrier SHA-256"),
            (&self.mutation_recipe_sha256, "mutation recipe SHA-256"),
        ])?;
        if self.clean_carrier_sha256 == self.mutated_carrier_sha256 {
            return Err("Timeline post-pin barrier carrier mutation is absent".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalTimelinePostPinReadyV1 {
    pub schema: String,
    pub run_identity: String,
    pub barrier_identity_sha256: String,
    pub boundary: LongitudinalTimelinePostPinBoundaryV1,
    pub carrier_opens_before: u64,
    pub selected_carriers_before: u64,
    pub expected_carrier_key_digest: String,
    pub clean_carrier_sha256: String,
    pub mutated_carrier_sha256: String,
    pub mutation_recipe_sha256: String,
}

impl LongitudinalTimelinePostPinReadyV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_timeline_barrier_sha256(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LONGITUDINAL_TIMELINE_POST_PIN_READY_SCHEMA_V1 {
            return Err("unsupported Timeline post-pin ready schema".to_owned());
        }
        validate_timeline_barrier_hashes([
            (&self.run_identity, "run identity"),
            (&self.barrier_identity_sha256, "barrier identity"),
            (
                &self.expected_carrier_key_digest,
                "expected carrier key digest",
            ),
            (&self.clean_carrier_sha256, "clean carrier SHA-256"),
            (&self.mutated_carrier_sha256, "mutated carrier SHA-256"),
            (&self.mutation_recipe_sha256, "mutation recipe SHA-256"),
        ])?;
        if self.carrier_opens_before != 0 {
            return Err("Timeline post-pin barrier was reached after a carrier open".to_owned());
        }
        if self.selected_carriers_before == 0 {
            return Err("Timeline post-pin barrier selected no carriers".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalTimelinePostPinReleaseV1 {
    pub schema: String,
    pub run_identity: String,
    pub barrier_identity_sha256: String,
    pub ready_receipt_sha256: String,
    pub clean_carrier_sha256: String,
    pub mutated_carrier_sha256: String,
    pub mutation_recipe_sha256: String,
    pub derivative_inventory_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason_sha256: Option<String>,
}

impl LongitudinalTimelinePostPinReleaseV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        canonical_timeline_barrier_sha256(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1 {
            return Err("unsupported Timeline post-pin release schema".to_owned());
        }
        validate_timeline_barrier_hashes([
            (&self.run_identity, "run identity"),
            (&self.barrier_identity_sha256, "barrier identity"),
            (&self.ready_receipt_sha256, "ready receipt SHA-256"),
            (&self.clean_carrier_sha256, "clean carrier SHA-256"),
            (&self.mutated_carrier_sha256, "mutated carrier SHA-256"),
            (&self.mutation_recipe_sha256, "mutation recipe SHA-256"),
            (
                &self.derivative_inventory_sha256,
                "derivative inventory SHA-256",
            ),
        ])?;
        if let Some(abort_reason_sha256) = &self.abort_reason_sha256 {
            validate_timeline_barrier_hashes([(abort_reason_sha256, "abort reason SHA-256")])?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct LongitudinalTimelinePostPinBarrierStateV1 {
    root: PathBuf,
    request: LongitudinalTimelinePostPinBarrierRequestV1,
    ready: Option<LongitudinalTimelinePostPinReadyV1>,
    release: Option<LongitudinalTimelinePostPinReleaseV1>,
    observed_mismatch: Option<(String, LongitudinalTimelineCarrierMismatchKindV1)>,
    error: Option<String>,
}

/// One request/run-local counting scope.
///
/// The active scope is thread-local. A child thread contributes nothing unless
/// its caller explicitly enters a clone of this scope on that thread.
#[derive(Clone)]
pub struct LongitudinalCountingScopeV1 {
    run_identity: String,
    state: Arc<Mutex<ObserverState>>,
}

/// Restores the previously active scope when dropped.
pub struct LongitudinalCountingGuardV1 {
    state: Arc<Mutex<ObserverState>>,
    interaction_actor_entered: bool,
    _not_send: PhantomData<Rc<()>>,
}

pub(crate) struct InteractionActorScopeGuardV1 {
    state: Arc<Mutex<ObserverState>>,
    _not_send: PhantomData<Rc<()>>,
}

#[derive(Clone)]
pub(crate) struct InteractionChildScopeReservationV1 {
    scope: LongitudinalCountingScopeV1,
    ordinal: u16,
    actor: InteractionActorV1,
}

pub(crate) struct InteractionChildScopeExecutionV1 {
    scope: LongitudinalCountingScopeV1,
    ordinal: u16,
    actor: InteractionActorV1,
    incomplete_reason: &'static str,
    completed: bool,
    _actor_guard: InteractionActorScopeGuardV1,
    _scope_guard: LongitudinalCountingGuardV1,
}

#[derive(Debug)]
struct InteractionLockAttemptObservationV1 {
    state: Arc<Mutex<ObserverState>>,
    ordinal: u16,
    actor: InteractionActorV1,
}

#[derive(Debug)]
pub(crate) struct InteractionLockAttemptRecorderV1 {
    observation: Option<InteractionLockAttemptObservationV1>,
    kind: InteractionLockKindV1,
    mode: InteractionLockModeV1,
    started: Instant,
}

#[derive(Debug)]
pub(crate) struct InteractionPhysicalLockHoldRecorderV1 {
    observation: InteractionLockAttemptObservationV1,
    kind: InteractionLockKindV1,
    mode: InteractionLockModeV1,
    wait_nanos: u64,
    acquired_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalDerivedAccessPhaseV1 {
    ChangePageSnapshotAcquisition,
    ChangePageBodylessSelection,
    ChangePageProposalLocatorExpansion,
    ChangePageCarrierHydrationValidation,
    ChangePageExhaustiveProposalSearch,
    ChangePageSupportExpansion,
    ChangePagePresentationProjection,
    RevisionPageSqlSelection,
    RevisionPageEventIdExpansion,
    RevisionPageCarrierHydrationValidation,
    RevisionPageListProjection,
    RevisionPageSupersederSupportExpansion,
    RevisionPageOverviewConstruction,
    RevisionPageSnapshotSummaries,
    BootstrapPopulation,
    BootstrapOracle,
    BootstrapFinalization,
    GovernedWriteAdmission,
    GovernedWriteTruth,
    GovernedWriteCatchUp,
    GovernedWriteAuthorityCursorMaintenance,
    GovernedWriteResponse,
    CliCapabilityPreflightH1,
    WorkflowActivatedCapabilityProbe,
    OrdinaryReadStoreResolutionH2,
    WorkflowChangeReaderReplayH3,
    WorkflowChangeStoreReopenInspection,
    RouteRevisionSelection,
    RouteProjectionFold,
    RouteBodyHydration,
    GitContextResolution,
    SqliteSelection,
    CarrierValidation,
    FactSqliteSelection,
    FactSelectedCarrierHydrationValidation,
    FactSupportCarrierHydrationValidation,
    FactWorkflowProjection,
    SerializationAndOutput,
    CacheAndFallback,
    ReadTransaction,
    CheckpointAndWal,
    GenerationLeaseAndRetention,
}

pub const INTERACTION_FACT_CURRENT_REQUIRED_PHASES_V1: [LongitudinalDerivedAccessPhaseV1; 4] = [
    LongitudinalDerivedAccessPhaseV1::FactSqliteSelection,
    LongitudinalDerivedAccessPhaseV1::FactSelectedCarrierHydrationValidation,
    LongitudinalDerivedAccessPhaseV1::FactSupportCarrierHydrationValidation,
    LongitudinalDerivedAccessPhaseV1::FactWorkflowProjection,
];

pub const INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1: [LongitudinalDerivedAccessPhaseV1; 3] = [
    LongitudinalDerivedAccessPhaseV1::WorkflowChangeReaderReplayH3,
    LongitudinalDerivedAccessPhaseV1::WorkflowChangeStoreReopenInspection,
    LongitudinalDerivedAccessPhaseV1::CacheAndFallback,
];

impl LongitudinalDerivedAccessPhaseV1 {
    pub const fn ownership(self) -> LongitudinalDerivedAccessPhaseOwnershipV1 {
        use LongitudinalDerivedAccessPhaseOwnershipV1 as Ownership;
        match self {
            Self::ChangePageBodylessSelection
            | Self::ChangePageProposalLocatorExpansion
            | Self::RevisionPageSqlSelection
            | Self::RevisionPageEventIdExpansion
            | Self::FactSqliteSelection => Ownership::DerivedAccess,
            Self::ChangePageCarrierHydrationValidation
            | Self::RevisionPageCarrierHydrationValidation
            | Self::FactSelectedCarrierHydrationValidation
            | Self::FactSupportCarrierHydrationValidation
            | Self::GovernedWriteTruth
            | Self::CliCapabilityPreflightH1
            | Self::OrdinaryReadStoreResolutionH2
            | Self::WorkflowChangeReaderReplayH3
            | Self::WorkflowChangeStoreReopenInspection
            | Self::RouteBodyHydration
            | Self::CarrierValidation => Ownership::AuthoritativeTruth,
            Self::ChangePagePresentationProjection
            | Self::ChangePageExhaustiveProposalSearch
            | Self::RevisionPageListProjection
            | Self::RevisionPageOverviewConstruction
            | Self::RevisionPageSnapshotSummaries
            | Self::RouteProjectionFold
            | Self::FactWorkflowProjection
            | Self::GitContextResolution
            | Self::SerializationAndOutput => Ownership::ProductProjection,
            Self::ChangePageSnapshotAcquisition
            | Self::ChangePageSupportExpansion
            | Self::RevisionPageSupersederSupportExpansion
            | Self::BootstrapPopulation
            | Self::BootstrapOracle
            | Self::GovernedWriteCatchUp
            | Self::WorkflowActivatedCapabilityProbe
            | Self::RouteRevisionSelection => Ownership::MixedDerivedAndTruth,
            Self::BootstrapFinalization
            | Self::GovernedWriteAdmission
            | Self::GovernedWriteAuthorityCursorMaintenance
            | Self::GovernedWriteResponse
            | Self::SqliteSelection
            | Self::CacheAndFallback
            | Self::ReadTransaction
            | Self::CheckpointAndWal
            | Self::GenerationLeaseAndRetention => Ownership::DerivedAccess,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LongitudinalDerivedAccessPhaseOwnershipV1 {
    DerivedAccess,
    AuthoritativeTruth,
    ProductProjection,
    MixedDerivedAndTruth,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalDerivedAccessPhaseSampleV1 {
    pub phase: LongitudinalDerivedAccessPhaseV1,
    pub ownership: LongitudinalDerivedAccessPhaseOwnershipV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<InteractionActorV1>,
    pub ordinal: u16,
    pub parent_ordinal: Option<u16>,
    pub wall_nanos: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_cpu_nanos: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_bytes_before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_bytes_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Maximum of the before/after endpoint observations, not a continuous
    /// within-phase peak.
    pub resident_bytes_observed_max: Option<u64>,
    pub counters: LongitudinalCountersV1,
}

/// Completes one coarse, request-scoped derived-access phase on drop.
///
/// The guard records no data without an active [`LongitudinalCountingScopeV1`].
/// Samples are assigned their ordinal on entry, so nested coarse phases retain
/// start order even though their guards necessarily complete in reverse order.
pub struct LongitudinalDerivedAccessPhaseGuardV1 {
    state: Option<Arc<Mutex<ObserverState>>>,
    phase: LongitudinalDerivedAccessPhaseV1,
    ordinal: u16,
    parent_ordinal: Option<u16>,
    actor: Option<InteractionActorV1>,
    started: Instant,
    counters_before: LongitudinalCountersV1,
    process_before: Option<super::LongitudinalProcessSnapshotV1>,
    _not_send: PhantomData<Rc<()>>,
}

/// Tracks one live decoded-event population in the active counting scope.
///
/// Unlike the legacy point-in-time setter, this guard adds its population to
/// the live total and removes exactly that population on drop. Overlapping
/// decoded histories therefore remain observable instead of overwriting one
/// another's ownership count. Callers must not use
/// [`set_retained_decoded_events`] while a guard is live in the same scope: the
/// setter describes an independent point-in-time owner rather than an additive
/// population.
#[derive(Debug)]
pub(crate) struct RetainedDecodedEventsGuardV1 {
    state: Option<Arc<Mutex<ObserverState>>>,
    retained: u64,
}

impl RetainedDecodedEventsGuardV1 {
    pub(crate) fn new(retained: usize) -> Self {
        let retained = retained as u64;
        let state = LongitudinalCountingScopeV1::current().map(|scope| scope.state);
        if let Some(state) = &state {
            add(
                &mut lock_state(state).capacity_ownership.retained_decoded_events,
                retained,
                "retained_decoded_events",
            );
        }
        Self { state, retained }
    }
}

impl Drop for RetainedDecodedEventsGuardV1 {
    fn drop(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        let mut state = lock_state(state);
        state.capacity_ownership.retained_decoded_events = state
            .capacity_ownership
            .retained_decoded_events
            .checked_sub(self.retained)
            .expect("retained decoded-event ownership underflow");
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCountingSnapshotV1 {
    pub run_identity: String,
    pub counters: LongitudinalCountersV1,
    pub capacity_ownership: LongitudinalCapacityOwnershipV1,
    pub derived_access_phases: Vec<LongitudinalDerivedAccessPhaseSampleV1>,
    #[serde(skip)]
    pub(crate) observed_routes: Vec<InteractionRouteV1>,
    #[serde(skip)]
    pub(crate) observed_route_states: Vec<InteractionObservedRouteStateV1>,
    #[serde(skip)]
    pub(crate) execution_actors: Vec<InteractionActorV1>,
    #[serde(skip)]
    pub(crate) outcomes: Vec<(bool, i32)>,
    #[serde(skip)]
    pub(crate) semantic_result_sha256: Vec<String>,
    #[serde(skip)]
    pub(crate) child_reservations: Vec<(u16, InteractionActorV1)>,
    #[serde(skip)]
    pub(crate) child_terminals: Vec<InteractionChildScopeFactV1>,
    #[serde(skip)]
    pub(crate) lock_facts: Vec<InteractionLockFactV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LongitudinalCounterReceiptContextV1 {
    pub root_identity: String,
    pub operation: String,
    pub phase: String,
    pub base_execution_identity_sha256: String,
    pub derivative_execution_identity_sha256: String,
    pub manifest_sha256: String,
    pub schedule_sha256: String,
    pub success: bool,
    pub semantic_result_sha256: String,
    pub include_capacity_ownership: bool,
}

impl LongitudinalCountingScopeV1 {
    pub fn new(run_identity: impl Into<String>) -> Result<Self, String> {
        let run_identity = run_identity.into();
        if run_identity.len() != 64
            || !run_identity
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(
                "longitudinal counting run identity must be 64 hexadecimal characters".to_owned(),
            );
        }
        Ok(Self {
            run_identity,
            state: Arc::new(Mutex::new(ObserverState::default())),
        })
    }

    /// Arms the qualification-only Timeline barrier for this exact request.
    ///
    /// The filesystem location is deliberately not part of the authenticated
    /// request document. The Inspector child receives it through one explicit
    /// feature-gated environment value, while the request binds all semantic
    /// identities exchanged through the barrier.
    #[doc(hidden)]
    pub fn with_timeline_post_pin_barrier(
        self,
        root: impl AsRef<Path>,
        request: LongitudinalTimelinePostPinBarrierRequestV1,
    ) -> Result<Self, String> {
        request.validate()?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|error| format!("canonicalize Timeline post-pin barrier root: {error}"))?;
        if !root.is_dir() {
            return Err("Timeline post-pin barrier root is not a directory".to_owned());
        }
        let mut entries = fs::read_dir(&root)
            .map_err(|error| format!("read Timeline post-pin barrier root: {error}"))?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("inspect Timeline post-pin barrier root entry: {error}"))?
            .is_some()
        {
            return Err("Timeline post-pin barrier root must be empty".to_owned());
        }
        {
            let mut state = lock_state(&self.state);
            if state.timeline_post_pin_barrier.is_some() {
                return Err("Timeline post-pin barrier is already armed".to_owned());
            }
            state.timeline_post_pin_barrier = Some(LongitudinalTimelinePostPinBarrierStateV1 {
                root,
                request,
                ready: None,
                release: None,
                observed_mismatch: None,
                error: None,
            });
        }
        Ok(self)
    }

    #[doc(hidden)]
    pub fn has_timeline_post_pin_barrier(&self) -> bool {
        lock_state(&self.state).timeline_post_pin_barrier.is_some()
    }

    #[doc(hidden)]
    pub fn timeline_post_pin_barrier_identity(&self) -> Option<String> {
        lock_state(&self.state)
            .timeline_post_pin_barrier
            .as_ref()
            .map(|barrier| barrier.request.barrier_identity_sha256.clone())
    }

    #[doc(hidden)]
    pub fn timeline_post_pin_barrier_receipt(
        &self,
    ) -> Result<Option<LongitudinalTimelinePostPinBarrierReceiptV1>, String> {
        let state = lock_state(&self.state);
        let Some(barrier) = &state.timeline_post_pin_barrier else {
            return Ok(None);
        };
        if let Some(error) = &barrier.error {
            return Err(error.clone());
        }
        let ready = barrier
            .ready
            .as_ref()
            .ok_or_else(|| "Timeline post-pin barrier was not reached".to_owned())?;
        let release = barrier
            .release
            .as_ref()
            .ok_or_else(|| "Timeline post-pin barrier was not released".to_owned())?;
        let (observed_mismatch_key_digest, mismatch_kind) = barrier
            .observed_mismatch
            .as_ref()
            .ok_or_else(|| "Timeline post-pin barrier observed no carrier mismatch".to_owned())?;
        let mut receipt = LongitudinalTimelinePostPinBarrierReceiptV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_SCHEMA_V1.to_owned(),
            run_identity: self.run_identity.clone(),
            barrier_identity_sha256: barrier.request.barrier_identity_sha256.clone(),
            boundary: ready.boundary,
            carrier_opens_before: ready.carrier_opens_before,
            selected_carriers_before: ready.selected_carriers_before,
            expected_carrier_key_digest: barrier.request.expected_carrier_key_digest.clone(),
            observed_mismatch_key_digest: observed_mismatch_key_digest.clone(),
            mismatch_kind: *mismatch_kind,
            clean_carrier_sha256: barrier.request.clean_carrier_sha256.clone(),
            mutated_carrier_sha256: release.mutated_carrier_sha256.clone(),
            mutation_recipe_sha256: release.mutation_recipe_sha256.clone(),
            derivative_inventory_sha256: release.derivative_inventory_sha256.clone(),
            ready_receipt_sha256: release.ready_receipt_sha256.clone(),
            release_receipt_sha256: release.canonical_sha256()?,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt
            .canonical_sha256()
            .map_err(|error| error.to_string())?;
        receipt.validate().map_err(|error| error.to_string())?;
        Ok(Some(receipt))
    }

    pub fn enter(&self) -> LongitudinalCountingGuardV1 {
        ACTIVE_SCOPES.with(|scopes| scopes.borrow_mut().push(self.clone()));
        let execution_actor = {
            let state = lock_state(&self.state);
            (state.execution_actors.len() == 1).then(|| state.execution_actors[0])
        };
        if let Some(actor) = execution_actor {
            ACTIVE_INTERACTION_ACTORS.with(|actors| {
                actors.borrow_mut().push((Arc::clone(&self.state), actor));
            });
        }
        LongitudinalCountingGuardV1 {
            state: Arc::clone(&self.state),
            interaction_actor_entered: execution_actor.is_some(),
            _not_send: PhantomData,
        }
    }

    /// Clone the active request scope for explicit propagation to a child
    /// thread. A child remains uncounted unless it enters the returned scope.
    pub fn current() -> Option<Self> {
        ACTIVE_SCOPES.with(|scopes| scopes.borrow().last().cloned())
    }

    pub fn record_observed_route_once(&self, route: InteractionRouteV1) {
        lock_state(&self.state).observed_routes.push(route);
    }

    pub fn record_observed_route_state_once(&self, state: InteractionObservedRouteStateV1) {
        lock_state(&self.state).observed_route_states.push(state);
    }

    pub fn record_execution_actor_once(&self, actor: InteractionActorV1) {
        lock_state(&self.state).execution_actors.push(actor);
    }

    pub fn record_outcome_once(&self, success: bool, exit_code: i32) {
        lock_state(&self.state).outcomes.push((success, exit_code));
    }

    pub fn record_semantic_result_sha256_once(&self, semantic_result_sha256: impl Into<String>) {
        lock_state(&self.state)
            .semantic_result_sha256
            .push(semantic_result_sha256.into());
    }

    pub(crate) fn enter_actor_scope(
        &self,
        actor: InteractionActorV1,
    ) -> InteractionActorScopeGuardV1 {
        assert!(
            Self::current().is_some_and(|active| Arc::ptr_eq(&active.state, &self.state)),
            "interaction actor scope requires its counting scope to be active"
        );
        ACTIVE_INTERACTION_ACTORS.with(|actors| {
            actors.borrow_mut().push((Arc::clone(&self.state), actor));
        });
        InteractionActorScopeGuardV1 {
            state: Arc::clone(&self.state),
            _not_send: PhantomData,
        }
    }

    pub(crate) fn reserve_child_scope(&self, actor: InteractionActorV1) -> u16 {
        let mut state = lock_state(&self.state);
        let ordinal = state.next_child_ordinal;
        state.next_child_ordinal = state
            .next_child_ordinal
            .checked_add(1)
            .expect("interaction child ordinal overflow");
        state.child_reservations.push((ordinal, actor));
        ordinal
    }

    pub(crate) fn record_child_scope_terminal_once(
        &self,
        ordinal: u16,
        actor: InteractionActorV1,
        coverage: InteractionScopeCoverageV1,
    ) {
        lock_state(&self.state)
            .child_terminals
            .push(InteractionChildScopeFactV1 {
                ordinal,
                actor,
                coverage,
            });
    }

    #[cfg(test)]
    pub(crate) fn record_lock_fact(&self, fact: InteractionLockFactV1) {
        let mut state = lock_state(&self.state);
        state.next_lock_ordinal = state.next_lock_ordinal.max(fact.ordinal.saturating_add(1));
        state.lock_facts.push(fact);
    }

    pub fn snapshot(&self) -> LongitudinalCountingSnapshotV1 {
        let state = lock_state(&self.state);
        let mut derived_access_phases = state.derived_access_phases.clone();
        derived_access_phases.sort_by_key(|sample| sample.ordinal);
        let mut lock_facts = state.lock_facts.clone();
        lock_facts.sort_by_key(|fact| fact.ordinal);
        LongitudinalCountingSnapshotV1 {
            run_identity: self.run_identity.clone(),
            counters: state.counters.clone(),
            capacity_ownership: state.capacity_ownership.clone(),
            derived_access_phases,
            observed_routes: state.observed_routes.clone(),
            observed_route_states: state.observed_route_states.clone(),
            execution_actors: state.execution_actors.clone(),
            outcomes: state.outcomes.clone(),
            semantic_result_sha256: state.semantic_result_sha256.clone(),
            child_reservations: state.child_reservations.clone(),
            child_terminals: state.child_terminals.clone(),
            lock_facts,
        }
    }

    pub fn receipt(
        &self,
        context: LongitudinalCounterReceiptContextV1,
    ) -> Result<LongitudinalCounterReceiptV1, LongitudinalContractError> {
        let snapshot = self.snapshot();
        let mut receipt = LongitudinalCounterReceiptV1 {
            schema: LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1.to_owned(),
            run_identity: snapshot.run_identity,
            root_identity: context.root_identity,
            operation: context.operation,
            phase: context.phase,
            base_execution_identity_sha256: context.base_execution_identity_sha256,
            derivative_execution_identity_sha256: context.derivative_execution_identity_sha256,
            manifest_sha256: context.manifest_sha256,
            schedule_sha256: context.schedule_sha256,
            success: context.success,
            semantic_result_sha256: context.semantic_result_sha256,
            counters: snapshot.counters,
            capacity_ownership: context
                .include_capacity_ownership
                .then_some(snapshot.capacity_ownership),
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256()?;
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn interaction_receipt(
        &self,
        expected: InteractionPerformanceExpectedContextV1,
    ) -> Result<InteractionPerformanceReceiptV1, LongitudinalContractError> {
        let snapshot = self.snapshot();
        let route = exactly_one(&snapshot.observed_routes, "interaction observed route")?;
        let route_state = exactly_one(
            &snapshot.observed_route_states,
            "interaction observed route state",
        )?;
        let execution_actor = exactly_one(
            &snapshot.execution_actors,
            "interaction root execution actor",
        )?;
        let (success, exit_code) = exactly_one(&snapshot.outcomes, "interaction outcome")?;
        let semantic_result_sha256 = exactly_one(
            &snapshot.semantic_result_sha256,
            "interaction semantic result SHA-256",
        )?;

        let children =
            reconcile_child_scope_facts(&snapshot.child_reservations, &snapshot.child_terminals)?;
        let scope_coverage = interaction_scope_coverage_v1(&children)?;
        let observed = InteractionObservedFactsV1 {
            route,
            route_state,
            execution_actor,
            success,
            exit_code,
            semantic_result_sha256,
        };
        let mut receipt = InteractionPerformanceReceiptV1 {
            schema: INTERACTION_PERFORMANCE_RECEIPT_SCHEMA_V1.to_owned(),
            expected,
            observed,
            scope_coverage,
            counters: snapshot.counters,
            capacity_ownership: snapshot.capacity_ownership,
            phases: snapshot.derived_access_phases,
            children,
            locks: snapshot.lock_facts,
            receipt_sha256: String::new(),
        };
        receipt.receipt_sha256 = receipt.canonical_sha256()?;
        receipt.validate()?;
        Ok(receipt)
    }
}

pub(crate) fn reserve_interaction_child_scope_v1(
    actor: InteractionActorV1,
) -> Option<InteractionChildScopeReservationV1> {
    let scope = LongitudinalCountingScopeV1::current()?;
    current_interaction_actor_for_state(&scope.state)?;
    let ordinal = scope.reserve_child_scope(actor);
    Some(InteractionChildScopeReservationV1 {
        scope,
        ordinal,
        actor,
    })
}

impl InteractionChildScopeReservationV1 {
    pub(crate) fn enter(self, incomplete_reason: &'static str) -> InteractionChildScopeExecutionV1 {
        let scope_guard = self.scope.enter();
        let actor_guard = self.scope.enter_actor_scope(self.actor);
        InteractionChildScopeExecutionV1 {
            scope: self.scope,
            ordinal: self.ordinal,
            actor: self.actor,
            incomplete_reason,
            completed: false,
            _actor_guard: actor_guard,
            _scope_guard: scope_guard,
        }
    }

    pub(crate) fn record_incomplete(self, reason: impl Into<String>) {
        self.scope.record_child_scope_terminal_once(
            self.ordinal,
            self.actor,
            InteractionScopeCoverageV1::Incomplete {
                reason: reason.into(),
            },
        );
    }
}

impl InteractionChildScopeExecutionV1 {
    pub(crate) fn complete(mut self) {
        self.scope.record_child_scope_terminal_once(
            self.ordinal,
            self.actor,
            InteractionScopeCoverageV1::Complete,
        );
        self.completed = true;
    }
}

impl Drop for InteractionChildScopeExecutionV1 {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.scope.record_child_scope_terminal_once(
            self.ordinal,
            self.actor,
            InteractionScopeCoverageV1::Incomplete {
                reason: self.incomplete_reason.to_owned(),
            },
        );
    }
}

pub(crate) fn begin_interaction_lock_attempt_v1(
    kind: InteractionLockKindV1,
    mode: InteractionLockModeV1,
) -> InteractionLockAttemptRecorderV1 {
    let observation = LongitudinalCountingScopeV1::current().and_then(|scope| {
        let actor = current_interaction_actor_for_state(&scope.state)?;
        let ordinal = {
            let mut state = lock_state(&scope.state);
            let ordinal = state.next_lock_ordinal;
            state.next_lock_ordinal = state
                .next_lock_ordinal
                .checked_add(1)
                .expect("interaction lock ordinal overflow");
            ordinal
        };
        Some(InteractionLockAttemptObservationV1 {
            state: scope.state,
            ordinal,
            actor,
        })
    });
    InteractionLockAttemptRecorderV1 {
        observation,
        kind,
        mode,
        started: Instant::now(),
    }
}

impl InteractionLockAttemptRecorderV1 {
    pub(crate) fn record_reentrant_acquired(self) {
        let Some(observation) = self.observation else {
            return;
        };
        record_interaction_lock_fact(
            observation,
            self.kind,
            self.mode,
            InteractionLockOutcomeV1::Acquired,
            InteractionLockAcquisitionV1::Reentrant,
            0,
            None,
        );
    }

    pub(crate) fn record_not_acquired(self, outcome: InteractionLockOutcomeV1) {
        debug_assert!(matches!(
            outcome,
            InteractionLockOutcomeV1::Busy
                | InteractionLockOutcomeV1::Deferred
                | InteractionLockOutcomeV1::Failed
        ));
        let Some(observation) = self.observation else {
            return;
        };
        record_interaction_lock_fact(
            observation,
            self.kind,
            self.mode,
            outcome,
            InteractionLockAcquisitionV1::NotAcquired,
            elapsed_nanos(self.started),
            None,
        );
    }

    pub(crate) fn record_physical_acquired(self) -> Option<InteractionPhysicalLockHoldRecorderV1> {
        let observation = self.observation?;
        Some(InteractionPhysicalLockHoldRecorderV1 {
            observation,
            kind: self.kind,
            mode: self.mode,
            wait_nanos: elapsed_nanos(self.started),
            acquired_at: Instant::now(),
        })
    }
}

impl Drop for InteractionPhysicalLockHoldRecorderV1 {
    fn drop(&mut self) {
        let observation = InteractionLockAttemptObservationV1 {
            state: Arc::clone(&self.observation.state),
            ordinal: self.observation.ordinal,
            actor: self.observation.actor,
        };
        record_interaction_lock_fact(
            observation,
            self.kind,
            self.mode,
            InteractionLockOutcomeV1::Acquired,
            InteractionLockAcquisitionV1::Physical,
            self.wait_nanos,
            Some(elapsed_nanos(self.acquired_at)),
        );
    }
}

fn record_interaction_lock_fact(
    observation: InteractionLockAttemptObservationV1,
    kind: InteractionLockKindV1,
    mode: InteractionLockModeV1,
    outcome: InteractionLockOutcomeV1,
    acquisition: InteractionLockAcquisitionV1,
    wait_nanos: u64,
    hold_nanos: Option<u64>,
) {
    lock_state(&observation.state)
        .lock_facts
        .push(InteractionLockFactV1 {
            ordinal: observation.ordinal,
            actor: observation.actor,
            kind,
            mode,
            outcome,
            acquisition,
            wait_nanos,
            hold_nanos,
        });
}

fn elapsed_nanos(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn exactly_one<T: Clone>(
    values: &[T],
    field: &'static str,
) -> Result<T, LongitudinalContractError> {
    if values.len() != 1 {
        return Err(LongitudinalContractError::CountMismatch {
            field,
            expected: 1,
            actual: u64::try_from(values.len()).unwrap_or(u64::MAX),
        });
    }
    Ok(values[0].clone())
}

fn reconcile_child_scope_facts(
    reservations: &[(u16, InteractionActorV1)],
    terminals: &[InteractionChildScopeFactV1],
) -> Result<Vec<InteractionChildScopeFactV1>, LongitudinalContractError> {
    let mut children = Vec::with_capacity(reservations.len());
    for (index, (ordinal, actor)) in reservations.iter().copied().enumerate() {
        let expected_ordinal =
            u16::try_from(index).map_err(|_| LongitudinalContractError::ContractDrift {
                field: "interaction child reservation ordinal",
            })?;
        if ordinal != expected_ordinal {
            return Err(LongitudinalContractError::ContractDrift {
                field: "interaction child reservation ordinal",
            });
        }
        let matching = terminals
            .iter()
            .filter(|terminal| terminal.ordinal == ordinal)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(LongitudinalContractError::CountMismatch {
                field: "interaction child terminal",
                expected: 1,
                actual: u64::try_from(matching.len()).unwrap_or(u64::MAX),
            });
        }
        let terminal = matching[0];
        terminal.validate()?;
        if terminal.actor != actor {
            return Err(LongitudinalContractError::PairMismatch);
        }
        children.push(terminal.clone());
    }
    if terminals.len() != reservations.len() {
        return Err(LongitudinalContractError::CountMismatch {
            field: "interaction child terminal count",
            expected: u64::try_from(reservations.len()).unwrap_or(u64::MAX),
            actual: u64::try_from(terminals.len()).unwrap_or(u64::MAX),
        });
    }
    Ok(children)
}

#[doc(hidden)]
pub fn longitudinal_timeline_post_pin_ready_path_v1(
    root: impl AsRef<Path>,
    barrier_identity_sha256: &str,
) -> PathBuf {
    root.as_ref()
        .join(format!("ready-{barrier_identity_sha256}.json"))
}

#[doc(hidden)]
pub fn longitudinal_timeline_post_pin_release_path_v1(
    root: impl AsRef<Path>,
    barrier_identity_sha256: &str,
) -> PathBuf {
    root.as_ref()
        .join(format!("release-{barrier_identity_sha256}.json"))
}

#[doc(hidden)]
pub fn write_longitudinal_timeline_barrier_document_v1<T: Serialize>(
    path: impl AsRef<Path>,
    document: &T,
) -> std::io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            "Timeline post-pin barrier document has no parent",
        )
    })?;
    let value = serde_json::to_value(document)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
    let bytes = canonical_json_bytes(&value)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                ErrorKind::InvalidInput,
                "Timeline post-pin barrier document has no UTF-8 file name",
            )
        })?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let write_result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            return Err(std::io::Error::new(
                ErrorKind::AlreadyExists,
                "Timeline post-pin barrier document already exists",
            ));
        }
        fs::rename(&temporary, path)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

/// Stops an armed request after locator selection and before any carrier is
/// opened, then waits for its controller's authenticated mutation release.
#[doc(hidden)]
pub fn reach_timeline_carrier_locators_selected_v1() -> Result<(), String> {
    let Some(scope) = LongitudinalCountingScopeV1::current() else {
        return Ok(());
    };
    let ready = {
        let mut state = lock_state(&scope.state);
        let counters = state.counters.clone();
        let Some(barrier) = state.timeline_post_pin_barrier.as_mut() else {
            return Ok(());
        };
        if let Some(error) = &barrier.error {
            return Err(error.clone());
        }
        if barrier.ready.is_some() {
            return fail_timeline_barrier(
                barrier,
                "Timeline post-pin barrier was reached more than once",
            );
        }
        let ready = LongitudinalTimelinePostPinReadyV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_READY_SCHEMA_V1.to_owned(),
            run_identity: scope.run_identity.clone(),
            barrier_identity_sha256: barrier.request.barrier_identity_sha256.clone(),
            boundary: LongitudinalTimelinePostPinBoundaryV1::CarrierLocatorsSelected,
            carrier_opens_before: counters.carrier_opens,
            selected_carriers_before: counters.timeline_selected_carriers,
            expected_carrier_key_digest: barrier.request.expected_carrier_key_digest.clone(),
            clean_carrier_sha256: barrier.request.clean_carrier_sha256.clone(),
            mutated_carrier_sha256: barrier.request.mutated_carrier_sha256.clone(),
            mutation_recipe_sha256: barrier.request.mutation_recipe_sha256.clone(),
        };
        if let Err(error) = ready.validate() {
            barrier.error = Some(error.clone());
            return Err(error);
        }
        barrier.ready = Some(ready.clone());
        (barrier.root.clone(), ready)
    };
    let (root, ready) = ready;
    let ready_path =
        longitudinal_timeline_post_pin_ready_path_v1(&root, &ready.barrier_identity_sha256);
    if let Err(error) = write_longitudinal_timeline_barrier_document_v1(&ready_path, &ready) {
        return set_timeline_barrier_error(
            &scope,
            format!("write Timeline post-pin ready document: {error}"),
        );
    }
    let ready_receipt_sha256 = ready.canonical_sha256()?;
    let release_path =
        longitudinal_timeline_post_pin_release_path_v1(&root, &ready.barrier_identity_sha256);
    let deadline = Instant::now() + TIMELINE_POST_PIN_BARRIER_WAIT;
    let release = loop {
        match read_longitudinal_timeline_barrier_document_v1::<LongitudinalTimelinePostPinReleaseV1>(
            &release_path,
        ) {
            Ok(release) => break release,
            Err(error) if error.starts_with("not_found:") => {
                if Instant::now() >= deadline {
                    return set_timeline_barrier_error(
                        &scope,
                        "Timeline post-pin barrier release timed out".to_owned(),
                    );
                }
                std::thread::sleep(TIMELINE_POST_PIN_BARRIER_POLL);
            }
            Err(error) => {
                return set_timeline_barrier_error(
                    &scope,
                    format!("read Timeline post-pin release document: {error}"),
                );
            }
        }
    };
    if let Err(error) = release.validate() {
        return set_timeline_barrier_error(&scope, error);
    }
    if release.run_identity != ready.run_identity
        || release.barrier_identity_sha256 != ready.barrier_identity_sha256
        || release.ready_receipt_sha256 != ready_receipt_sha256
        || release.clean_carrier_sha256 != ready.clean_carrier_sha256
        || release.mutated_carrier_sha256 != ready.mutated_carrier_sha256
        || release.mutation_recipe_sha256 != ready.mutation_recipe_sha256
    {
        return set_timeline_barrier_error(
            &scope,
            "Timeline post-pin release does not match its ready document".to_owned(),
        );
    }
    if let Some(abort_reason_sha256) = &release.abort_reason_sha256 {
        return set_timeline_barrier_error(
            &scope,
            format!("Timeline post-pin barrier aborted: {abort_reason_sha256}"),
        );
    }
    lock_state(&scope.state)
        .timeline_post_pin_barrier
        .as_mut()
        .expect("armed Timeline barrier remains present")
        .release = Some(release);
    Ok(())
}

/// Records the exact carrier rejection reached after an armed post-pin
/// release. Ordinary requests and counting scopes without a barrier are no-ops.
#[doc(hidden)]
pub fn record_timeline_carrier_mismatch_v1(
    logical_reread_key_digest: &str,
    kind: LongitudinalTimelineCarrierMismatchKindV1,
) -> Result<(), String> {
    let Some(scope) = LongitudinalCountingScopeV1::current() else {
        return Ok(());
    };
    let mut state = lock_state(&scope.state);
    let Some(barrier) = state.timeline_post_pin_barrier.as_mut() else {
        return Ok(());
    };
    if let Some(error) = &barrier.error {
        return Err(error.clone());
    }
    if logical_reread_key_digest != barrier.request.expected_carrier_key_digest {
        return fail_timeline_barrier(
            barrier,
            "Timeline post-pin barrier rejected a different carrier",
        );
    }
    if barrier.release.is_none() {
        return fail_timeline_barrier(
            barrier,
            "Timeline carrier mismatch was observed before barrier release",
        );
    }
    if barrier.observed_mismatch.is_some() {
        return fail_timeline_barrier(
            barrier,
            "Timeline post-pin barrier observed more than one carrier mismatch",
        );
    }
    barrier.observed_mismatch = Some((logical_reread_key_digest.to_owned(), kind));
    Ok(())
}

#[doc(hidden)]
pub fn canonical_longitudinal_response_semantic_sha256_v1(
    response_body: &[u8],
) -> Result<String, String> {
    let document: serde_json::Value = serde_json::from_slice(response_body)
        .map_err(|error| format!("counted response body is not JSON: {error}"))?;
    let bytes = canonical_json_bytes(&document).map_err(|error| error.to_string())?;
    Ok(sha256_bytes_hex(&bytes))
}

fn fail_timeline_barrier<T>(
    barrier: &mut LongitudinalTimelinePostPinBarrierStateV1,
    message: &str,
) -> Result<T, String> {
    barrier.error = Some(message.to_owned());
    Err(message.to_owned())
}

fn set_timeline_barrier_error<T>(
    scope: &LongitudinalCountingScopeV1,
    error: String,
) -> Result<T, String> {
    if let Some(barrier) = lock_state(&scope.state).timeline_post_pin_barrier.as_mut() {
        barrier.error = Some(error.clone());
    }
    Err(error)
}

#[doc(hidden)]
pub fn read_longitudinal_timeline_barrier_document_v1<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            format!("not_found:{error}")
        } else {
            error.to_string()
        }
    })?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let canonical = canonical_json_bytes(&value).map_err(|error| error.to_string())?;
    if canonical != bytes {
        return Err("barrier document is not canonical JSON".to_owned());
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn canonical_timeline_barrier_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    let bytes = canonical_json_bytes(&value).map_err(|error| error.to_string())?;
    Ok(sha256_bytes_hex(&bytes))
}

fn validate_timeline_barrier_hashes<'a>(
    hashes: impl IntoIterator<Item = (&'a String, &'static str)>,
) -> Result<(), String> {
    for (hash, field) in hashes {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "{field} must be 64 lowercase hexadecimal characters"
            ));
        }
    }
    Ok(())
}

/// Enter one coarse derived-access phase in the current counting scope.
///
/// Process CPU/RSS data is optional because the existing normalized process
/// snapshot is currently available only on native macOS. Missing capture stays
/// `None`; it is never translated to zero.
pub fn enter_derived_access_phase_v1(
    phase: LongitudinalDerivedAccessPhaseV1,
) -> LongitudinalDerivedAccessPhaseGuardV1 {
    let active = LongitudinalCountingScopeV1::current();
    let (state, ordinal, counters_before) = match active {
        Some(scope) => {
            let mut state = lock_state(&scope.state);
            let ordinal = state.next_phase_ordinal;
            state.next_phase_ordinal = state
                .next_phase_ordinal
                .checked_add(1)
                .expect("derived-access phase ordinal overflow");
            let counters = state.counters.clone();
            (Some(Arc::clone(&scope.state)), ordinal, counters)
        }
        None => (None, 0, LongitudinalCountersV1::default()),
    };
    let parent_ordinal = state.as_ref().and_then(|_| {
        ACTIVE_DERIVED_ACCESS_PHASES.with(|phases| {
            let mut phases = phases.borrow_mut();
            let parent = phases.last().copied();
            phases.push(ordinal);
            parent
        })
    });
    let actor = state.as_ref().and_then(current_interaction_actor_for_state);
    let process_before = state
        .as_ref()
        .and_then(|_| super::capture_longitudinal_process_snapshot_v1(std::process::id()).ok());
    LongitudinalDerivedAccessPhaseGuardV1 {
        state,
        phase,
        ordinal,
        parent_ordinal,
        actor,
        started: Instant::now(),
        counters_before,
        process_before,
        _not_send: PhantomData,
    }
}

impl Drop for LongitudinalCountingGuardV1 {
    fn drop(&mut self) {
        if self.interaction_actor_entered {
            remove_active_interaction_actor(&self.state);
        }
        ACTIVE_SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            if scopes
                .last()
                .is_some_and(|active| Arc::ptr_eq(&active.state, &self.state))
            {
                scopes.pop();
                return;
            }
            if let Some(index) = scopes
                .iter()
                .rposition(|active| Arc::ptr_eq(&active.state, &self.state))
            {
                scopes.remove(index);
            }
        });
    }
}

impl Drop for InteractionActorScopeGuardV1 {
    fn drop(&mut self) {
        remove_active_interaction_actor(&self.state);
    }
}

impl Drop for LongitudinalDerivedAccessPhaseGuardV1 {
    fn drop(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        ACTIVE_DERIVED_ACCESS_PHASES.with(|phases| {
            assert_eq!(
                phases.borrow_mut().pop(),
                Some(self.ordinal),
                "derived-access phases must complete in nesting order"
            );
        });
        let wall_nanos = u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let process_after =
            super::capture_longitudinal_process_snapshot_v1(std::process::id()).ok();
        let (process_cpu_nanos, resident_bytes_before, resident_bytes_after) =
            match (self.process_before, process_after) {
                (Some(before), Some(after)) => {
                    let before_cpu = before.user_cpu_nanos.checked_add(before.system_cpu_nanos);
                    let after_cpu = after.user_cpu_nanos.checked_add(after.system_cpu_nanos);
                    (
                        before_cpu
                            .zip(after_cpu)
                            .and_then(|(before, after)| after.checked_sub(before)),
                        Some(before.resident_bytes),
                        Some(after.resident_bytes),
                    )
                }
                _ => (None, None, None),
            };
        let resident_bytes_observed_max = resident_bytes_before
            .zip(resident_bytes_after)
            .map(|(before, after)| before.max(after));
        let mut state = lock_state(state);
        let counters = counter_delta(&self.counters_before, &state.counters);
        state
            .derived_access_phases
            .push(LongitudinalDerivedAccessPhaseSampleV1 {
                phase: self.phase,
                ownership: self.phase.ownership(),
                actor: self.actor,
                ordinal: self.ordinal,
                parent_ordinal: self.parent_ordinal,
                wall_nanos,
                process_cpu_nanos,
                resident_bytes_before,
                resident_bytes_after,
                resident_bytes_observed_max,
                counters,
            });
    }
}

fn current_interaction_actor_for_state(
    state: &Arc<Mutex<ObserverState>>,
) -> Option<InteractionActorV1> {
    ACTIVE_INTERACTION_ACTORS.with(|actors| {
        actors
            .borrow()
            .iter()
            .rev()
            .find_map(|(active_state, actor)| Arc::ptr_eq(active_state, state).then_some(*actor))
    })
}

fn remove_active_interaction_actor(state: &Arc<Mutex<ObserverState>>) {
    ACTIVE_INTERACTION_ACTORS.with(|actors| {
        let mut actors = actors.borrow_mut();
        if actors
            .last()
            .is_some_and(|(active_state, _)| Arc::ptr_eq(active_state, state))
        {
            actors.pop();
            return;
        }
        if let Some(index) = actors
            .iter()
            .rposition(|(active_state, _)| Arc::ptr_eq(active_state, state))
        {
            actors.remove(index);
        }
    });
}

fn counter_delta(
    before: &LongitudinalCountersV1,
    after: &LongitudinalCountersV1,
) -> LongitudinalCountersV1 {
    macro_rules! delta {
        ($field:ident) => {
            after
                .$field
                .checked_sub(before.$field)
                .expect("longitudinal counter decreased within a phase")
        };
    }
    LongitudinalCountersV1 {
        directory_entries_walked: delta!(directory_entries_walked),
        carrier_opens: delta!(carrier_opens),
        carrier_bytes_read: delta!(carrier_bytes_read),
        authority_identity_rows_scanned: delta!(authority_identity_rows_scanned),
        strict_journal_inspections: delta!(strict_journal_inspections),
        fact_sqlite_rows_selected: delta!(fact_sqlite_rows_selected),
        change_semantic_constructions: delta!(change_semantic_constructions),
        change_projection_constructions: delta!(change_projection_constructions),
        change_candidates: delta!(change_candidates),
        change_candidate_current_revisions: delta!(change_candidate_current_revisions),
        change_capability_carriers_opened: delta!(change_capability_carriers_opened),
        change_proposal_carriers_opened: delta!(change_proposal_carriers_opened),
        change_proposal_carriers_validated: delta!(change_proposal_carriers_validated),
        change_support_carriers_opened: delta!(change_support_carriers_opened),
        change_matches: delta!(change_matches),
        change_rows_emitted: delta!(change_rows_emitted),
        timeline_sqlite_candidates: delta!(timeline_sqlite_candidates),
        timeline_sqlite_window_rows: delta!(timeline_sqlite_window_rows),
        timeline_sqlite_facet_rows: delta!(timeline_sqlite_facet_rows),
        timeline_selected_carriers: delta!(timeline_selected_carriers),
        timeline_revision_candidate_carriers: delta!(timeline_revision_candidate_carriers),
        timeline_removal_support_carriers: delta!(timeline_removal_support_carriers),
        timeline_signature_support_carriers: delta!(timeline_signature_support_carriers),
        timeline_correlation_support_carriers: delta!(timeline_correlation_support_carriers),
        timeline_trust_support_carriers: delta!(timeline_trust_support_carriers),
        timeline_exhaustive_candidates: delta!(timeline_exhaustive_candidates),
        timeline_entries_emitted: delta!(timeline_entries_emitted),
        authoritative_fallbacks: delta!(authoritative_fallbacks),
        full_history_fallbacks: delta!(full_history_fallbacks),
        event_decodes: delta!(event_decodes),
        event_validations: delta!(event_validations),
        event_folds: delta!(event_folds),
        chronological_sort_items: delta!(chronological_sort_items),
        body_artifact_reads: delta!(body_artifact_reads),
        body_bytes_read: delta!(body_bytes_read),
        object_artifact_reads: delta!(object_artifact_reads),
        object_bytes_read: delta!(object_bytes_read),
        projection_rebuilds: delta!(projection_rebuilds),
        state_rebuilds: delta!(state_rebuilds),
        response_bytes: delta!(response_bytes),
    }
}

fn lock_state(state: &Arc<Mutex<ObserverState>>) -> MutexGuard<'_, ObserverState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with_active(mut update: impl FnMut(&mut ObserverState)) {
    let active = LongitudinalCountingScopeV1::current();
    if let Some(active) = active {
        update(&mut lock_state(&active.state));
    }
}

fn add(value: &mut u64, amount: u64, field: &'static str) {
    *value = value
        .checked_add(amount)
        .unwrap_or_else(|| panic!("longitudinal counter overflow: {field}"));
}

pub fn record_directory_entries_walked(count: usize) {
    with_active(|state| {
        add(
            &mut state.counters.directory_entries_walked,
            count as u64,
            "directory_entries_walked",
        );
    });
}

pub fn record_carrier_read(bytes: usize) {
    record_carrier_open();
    record_carrier_bytes(bytes);
}

pub fn record_carrier_open() {
    with_active(|state| {
        add(&mut state.counters.carrier_opens, 1, "carrier_opens");
    });
}

pub fn record_carrier_bytes(bytes: usize) {
    with_active(|state| {
        add(
            &mut state.counters.carrier_bytes_read,
            bytes as u64,
            "carrier_bytes_read",
        );
    });
}

pub fn record_authority_identity_rows_scanned(count: usize) {
    with_active(|state| {
        add(
            &mut state.counters.authority_identity_rows_scanned,
            count as u64,
            "authority_identity_rows_scanned",
        );
    });
}

pub fn record_strict_journal_inspection() {
    with_active(|state| {
        add(
            &mut state.counters.strict_journal_inspections,
            1,
            "strict_journal_inspections",
        );
    });
}

macro_rules! change_counter {
    ($name:ident, $field:ident) => {
        pub fn $name(count: usize) {
            with_active(|state| {
                add(&mut state.counters.$field, count as u64, stringify!($field));
            });
        }
    };
}

change_counter!(record_change_candidates, change_candidates);
change_counter!(record_fact_sqlite_rows_selected, fact_sqlite_rows_selected);
change_counter!(
    record_change_candidate_current_revisions,
    change_candidate_current_revisions
);
change_counter!(
    record_change_capability_carriers_opened,
    change_capability_carriers_opened
);
change_counter!(
    record_change_proposal_carriers_opened,
    change_proposal_carriers_opened
);
change_counter!(
    record_change_proposal_carriers_validated,
    change_proposal_carriers_validated
);
change_counter!(
    record_change_support_carriers_opened,
    change_support_carriers_opened
);
change_counter!(record_change_matches, change_matches);
change_counter!(record_change_rows_emitted, change_rows_emitted);
change_counter!(
    record_timeline_sqlite_candidates,
    timeline_sqlite_candidates
);
change_counter!(
    record_timeline_sqlite_window_rows,
    timeline_sqlite_window_rows
);
change_counter!(
    record_timeline_sqlite_facet_rows,
    timeline_sqlite_facet_rows
);
change_counter!(
    record_timeline_selected_carriers,
    timeline_selected_carriers
);
change_counter!(
    record_timeline_revision_candidate_carriers,
    timeline_revision_candidate_carriers
);
change_counter!(
    record_timeline_removal_support_carriers,
    timeline_removal_support_carriers
);
change_counter!(
    record_timeline_signature_support_carriers,
    timeline_signature_support_carriers
);
change_counter!(
    record_timeline_correlation_support_carriers,
    timeline_correlation_support_carriers
);
change_counter!(
    record_timeline_trust_support_carriers,
    timeline_trust_support_carriers
);
change_counter!(
    record_timeline_exhaustive_candidates,
    timeline_exhaustive_candidates
);
change_counter!(record_timeline_entries_emitted, timeline_entries_emitted);

pub fn record_change_semantic_construction() {
    with_active(|state| {
        add(
            &mut state.counters.change_semantic_constructions,
            1,
            "change_semantic_constructions",
        );
    });
}

pub fn record_change_projection_construction() {
    with_active(|state| {
        add(
            &mut state.counters.change_projection_constructions,
            1,
            "change_projection_constructions",
        );
    });
}

pub fn record_authoritative_fallback() {
    with_active(|state| {
        add(
            &mut state.counters.authoritative_fallbacks,
            1,
            "authoritative_fallbacks",
        );
    });
}

pub fn record_full_history_fallback() {
    with_active(|state| {
        add(
            &mut state.counters.full_history_fallbacks,
            1,
            "full_history_fallbacks",
        );
    });
}

pub fn record_event_decode() {
    with_active(|state| add(&mut state.counters.event_decodes, 1, "event_decodes"));
}

pub fn record_event_validation() {
    with_active(|state| {
        add(
            &mut state.counters.event_validations,
            1,
            "event_validations",
        );
    });
}

pub fn record_event_folds(count: usize) {
    with_active(|state| {
        add(&mut state.counters.event_folds, count as u64, "event_folds");
    });
}

pub fn record_chronological_sort_items(count: usize) {
    with_active(|state| {
        add(
            &mut state.counters.chronological_sort_items,
            count as u64,
            "chronological_sort_items",
        );
    });
}

pub fn record_body_artifact_read(bytes: Option<usize>) {
    record_body_artifact_read_attempt();
    if let Some(bytes) = bytes {
        record_body_artifact_bytes(bytes);
    }
}

pub fn record_body_artifact_read_attempt() {
    with_active(|state| {
        add(
            &mut state.counters.body_artifact_reads,
            1,
            "body_artifact_reads",
        );
    });
}

pub fn record_body_artifact_bytes(bytes: usize) {
    with_active(|state| {
        add(
            &mut state.counters.body_bytes_read,
            bytes as u64,
            "body_bytes_read",
        );
    });
}

pub fn record_object_artifact_read(bytes: Option<usize>) {
    record_object_artifact_read_attempt();
    if let Some(bytes) = bytes {
        record_object_artifact_bytes(bytes);
    }
}

pub fn record_object_artifact_read_attempt() {
    with_active(|state| {
        add(
            &mut state.counters.object_artifact_reads,
            1,
            "object_artifact_reads",
        );
    });
}

pub fn record_object_artifact_bytes(bytes: usize) {
    with_active(|state| {
        add(
            &mut state.counters.object_bytes_read,
            bytes as u64,
            "object_bytes_read",
        );
    });
}

pub fn record_projection_rebuild() {
    with_active(|state| {
        add(
            &mut state.counters.projection_rebuilds,
            1,
            "projection_rebuilds",
        );
    });
}

pub fn record_state_rebuild() {
    with_active(|state| add(&mut state.counters.state_rebuilds, 1, "state_rebuilds"));
}

pub fn record_response_bytes(bytes: usize) {
    with_active(|state| {
        add(
            &mut state.counters.response_bytes,
            bytes as u64,
            "response_bytes",
        );
    });
}

macro_rules! ownership_setter {
    ($name:ident, $field:ident) => {
        pub fn $name(value: usize) {
            with_active(|state| state.capacity_ownership.$field = value as u64);
        }
    };
}

ownership_setter!(set_retained_decoded_events, retained_decoded_events);
ownership_setter!(
    set_retained_hydrated_history_entries,
    retained_hydrated_history_entries
);
ownership_setter!(
    set_retained_hydrated_body_bytes,
    retained_hydrated_body_bytes
);
ownership_setter!(
    set_retained_search_record_strings,
    retained_search_record_strings
);
ownership_setter!(
    set_retained_search_record_field_bytes,
    retained_search_record_field_bytes
);
ownership_setter!(
    set_retained_serialized_response_cache_bytes,
    retained_serialized_response_cache_bytes
);
ownership_setter!(
    set_retained_snapshot_highlight_entries,
    retained_snapshot_highlight_entries
);
ownership_setter!(
    set_retained_snapshot_highlight_bytes,
    retained_snapshot_highlight_bytes
);

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::super::{
        InteractionExecutionIdentityV1, InteractionLockAcquisitionV1, InteractionLockKindV1,
        InteractionLockModeV1, InteractionLockOutcomeV1, InteractionSetupExpectationV1,
    };
    use super::*;

    fn hash(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn interaction_context(
        route: InteractionRouteV1,
        setup_expectation: InteractionSetupExpectationV1,
    ) -> InteractionPerformanceExpectedContextV1 {
        let is_version = route == InteractionRouteV1::VersionJson;
        let requires_track = matches!(
            route,
            InteractionRouteV1::AssessmentCurrentResult
                | InteractionRouteV1::AssessmentCurrentSummary
                | InteractionRouteV1::ObservationReviewerList
                | InteractionRouteV1::ValidationReviewerList
        );
        InteractionPerformanceExpectedContextV1 {
            execution: InteractionExecutionIdentityV1 {
                source_commit: "a".repeat(40),
                source_tree: "b".repeat(40),
                cargo_lock_sha256: hash('c'),
                binary_path: std::env::current_exe()
                    .expect("current interaction counter test binary")
                    .display()
                    .to_string(),
                binary_sha256: hash('d'),
                build_profile: "debug".to_owned(),
                rustc_version: "rustc test".to_owned(),
                features: vec!["gix".to_owned(), "longitudinal-counting".to_owned()],
            },
            route,
            arguments: if is_version {
                vec![
                    "version".to_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                ]
            } else {
                vec!["fixture-route".to_owned()]
            },
            setup_expectation,
            fixture_identity_sha256: (!is_version).then(|| hash('e')),
            revision: (!is_version).then(|| format!("rev:sha256:{}", hash('f'))),
            track: requires_track.then(|| "agent:reviewer".to_owned()),
            domain_actor: (!is_version).then(|| "actor:agent:claude-code".to_owned()),
            expected_child_actors: std::collections::BTreeMap::new(),
        }
    }

    fn record_minimal_interaction_facts(
        scope: &LongitudinalCountingScopeV1,
        route: InteractionRouteV1,
        observed_state: InteractionObservedRouteStateV1,
    ) {
        scope.record_observed_route_once(route);
        scope.record_observed_route_state_once(observed_state);
        scope.record_execution_actor_once(InteractionActorV1::RequestReader);
        scope.record_outcome_once(true, 0);
        scope.record_semantic_result_sha256_once(hash('9'));
    }

    #[test]
    fn interaction_receipt_is_additive_actor_qualified_and_self_authenticating() {
        let scope = LongitudinalCountingScopeV1::new(hash('1')).expect("valid scope");
        let _scope_guard = scope.enter();
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentSummary,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );

        let _actor = scope.enter_actor_scope(InteractionActorV1::RequestReader);
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
            );
            record_event_folds(7);
        }
        let child_ordinal = scope.reserve_child_scope(InteractionActorV1::BackgroundMaintenance);
        scope.record_child_scope_terminal_once(
            child_ordinal,
            InteractionActorV1::BackgroundMaintenance,
            InteractionScopeCoverageV1::Complete,
        );
        scope.record_lock_fact(InteractionLockFactV1 {
            ordinal: 0,
            actor: InteractionActorV1::RequestReader,
            kind: InteractionLockKindV1::Derived,
            mode: InteractionLockModeV1::Try,
            outcome: InteractionLockOutcomeV1::Busy,
            acquisition: InteractionLockAcquisitionV1::NotAcquired,
            wait_nanos: 3,
            hold_nanos: None,
        });

        let mut expected = interaction_context(
            InteractionRouteV1::AssessmentCurrentSummary,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        expected
            .expected_child_actors
            .insert(InteractionActorV1::BackgroundMaintenance, 1);
        let receipt = scope
            .interaction_receipt(expected)
            .expect("valid interaction receipt");
        receipt.validate().expect("interaction receipt validates");
        assert_eq!(receipt.observed.semantic_result_sha256, hash('9'));
        assert_eq!(receipt.scope_coverage, InteractionScopeCoverageV1::Complete);
        assert_eq!(receipt.phases.len(), 1);
        assert_eq!(
            receipt.phases[0].actor,
            Some(InteractionActorV1::RequestReader)
        );
        assert_eq!(receipt.children.len(), 1);
        assert_eq!(receipt.locks.len(), 1);
        assert_eq!(receipt.counters.event_folds, 7);
        assert_eq!(
            receipt.receipt_sha256,
            receipt.canonical_sha256().expect("canonical receipt hash")
        );
        let receipt_json = serde_json::to_value(&receipt).expect("interaction receipt JSON");
        let round_trip =
            serde_json::from_value::<InteractionPerformanceReceiptV1>(receipt_json.clone())
                .expect("interaction receipt round trip");
        assert_eq!(round_trip, receipt);
        round_trip.validate().expect("round-trip receipt validates");
        let mut unknown = receipt_json;
        unknown
            .as_object_mut()
            .expect("interaction receipt object")
            .insert("latencyMillis".to_owned(), serde_json::json!(1));
        assert!(serde_json::from_value::<InteractionPerformanceReceiptV1>(unknown).is_err());

        let v1 = scope
            .receipt(LongitudinalCounterReceiptContextV1 {
                root_identity: hash('2'),
                operation: "WARM_HEAD".to_owned(),
                phase: "warm".to_owned(),
                base_execution_identity_sha256: hash('3'),
                derivative_execution_identity_sha256: hash('4'),
                manifest_sha256: hash('5'),
                schedule_sha256: hash('6'),
                success: true,
                semantic_result_sha256: hash('7'),
                include_capacity_ownership: true,
            })
            .expect("legacy V1 receipt");
        let v1_json = serde_json::to_value(v1).expect("legacy receipt JSON");
        assert!(v1_json.get("phases").is_none());
        assert!(v1_json.get("observed").is_none());
    }

    #[test]
    fn interaction_receipt_enforces_the_exact_route_state_matrix() {
        let valid = [
            (
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::AssessmentCurrentSummary,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::InputRequestOpenAllTracks,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::ObservationReviewerList,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::ValidationReviewerList,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::VersionJson,
                InteractionSetupExpectationV1::NotApplicable,
                InteractionObservedRouteStateV1::NotApplicable,
            ),
            (
                InteractionRouteV1::AttentionCurrentOrFallback,
                InteractionSetupExpectationV1::AttentionDerivedCurrent,
                InteractionObservedRouteStateV1::DerivedCurrent,
            ),
            (
                InteractionRouteV1::AttentionCurrentOrFallback,
                InteractionSetupExpectationV1::AttentionColdInactive,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::AttentionCurrentOrFallback,
                InteractionSetupExpectationV1::AttentionActiveUnavailable,
                InteractionObservedRouteStateV1::LabeledFallbackToAuthoritative,
            ),
        ];
        for (ordinal, (route, setup, observed)) in valid.into_iter().enumerate() {
            let scope =
                LongitudinalCountingScopeV1::new(format!("{ordinal:064x}")).expect("valid scope");
            record_minimal_interaction_facts(&scope, route, observed);
            scope
                .interaction_receipt(interaction_context(route, setup))
                .expect("valid route/state pair");
        }

        let invalid = [
            (
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionSetupExpectationV1::AuthoritativeReplay,
                InteractionObservedRouteStateV1::DerivedCurrent,
            ),
            (
                InteractionRouteV1::VersionJson,
                InteractionSetupExpectationV1::NotApplicable,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                InteractionRouteV1::AttentionCurrentOrFallback,
                InteractionSetupExpectationV1::AttentionColdInactive,
                InteractionObservedRouteStateV1::DerivedCurrent,
            ),
        ];
        for (ordinal, (route, setup, observed)) in invalid.into_iter().enumerate() {
            let scope = LongitudinalCountingScopeV1::new(format!("{:064x}", ordinal + 32))
                .expect("valid scope");
            record_minimal_interaction_facts(&scope, route, observed);
            assert!(
                scope
                    .interaction_receipt(interaction_context(route, setup))
                    .is_err(),
                "invalid route/state pair was accepted"
            );
        }
    }

    #[test]
    fn interaction_receipt_rejects_duplicate_or_incomplete_source_facts() {
        let scope = LongitudinalCountingScopeV1::new(hash('2')).expect("valid scope");
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
        scope
            .record_observed_route_state_once(InteractionObservedRouteStateV1::AuthoritativeReplay);
        assert!(
            scope
                .interaction_receipt(interaction_context(
                    InteractionRouteV1::AssessmentCurrentResult,
                    InteractionSetupExpectationV1::AuthoritativeReplay,
                ))
                .is_err()
        );

        let scope = LongitudinalCountingScopeV1::new(hash('3')).expect("valid scope");
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
        scope.reserve_child_scope(InteractionActorV1::BackgroundMaintenance);
        let mut expected = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        expected
            .expected_child_actors
            .insert(InteractionActorV1::BackgroundMaintenance, 1);
        assert!(scope.interaction_receipt(expected).is_err());

        let scope = LongitudinalCountingScopeV1::new(hash('4')).expect("valid scope");
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
        let ordinal = scope.reserve_child_scope(InteractionActorV1::BackgroundMaintenance);
        scope.record_child_scope_terminal_once(
            ordinal,
            InteractionActorV1::BackgroundMaintenance,
            InteractionScopeCoverageV1::Incomplete {
                reason: " ".to_owned(),
            },
        );
        let mut expected = interaction_context(
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionSetupExpectationV1::AuthoritativeReplay,
        );
        expected
            .expected_child_actors
            .insert(InteractionActorV1::BackgroundMaintenance, 1);
        assert!(scope.interaction_receipt(expected).is_err());
    }

    #[test]
    fn child_execution_drop_records_a_source_owned_incomplete_terminal() {
        let scope = LongitudinalCountingScopeV1::new(hash('b')).expect("valid scope");
        scope.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _scope_guard = scope.enter();
        let child = reserve_interaction_child_scope_v1(InteractionActorV1::BackgroundMaintenance)
            .expect("active interaction reserves a child");
        drop(child.enter("background maintenance exited before completion"));

        let snapshot = scope.snapshot();
        assert_eq!(
            snapshot.child_reservations,
            vec![(0, InteractionActorV1::BackgroundMaintenance)]
        );
        assert_eq!(snapshot.child_terminals.len(), 1);
        assert_eq!(snapshot.child_terminals[0].ordinal, 0);
        assert_eq!(
            snapshot.child_terminals[0].actor,
            InteractionActorV1::BackgroundMaintenance
        );
        assert_eq!(
            snapshot.child_terminals[0].coverage,
            InteractionScopeCoverageV1::Incomplete {
                reason: "background maintenance exited before completion".to_owned(),
            }
        );
    }

    #[test]
    fn interaction_receipt_rejects_every_missing_or_duplicate_once_only_fact() {
        for field in 0..5 {
            let scope = LongitudinalCountingScopeV1::new(format!("{:064x}", field + 64))
                .expect("valid scope");
            record_minimal_interaction_facts(
                &scope,
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            );
            let mut state = lock_state(&scope.state);
            match field {
                0 => state.observed_routes.clear(),
                1 => state.observed_route_states.clear(),
                2 => state.execution_actors.clear(),
                3 => state.outcomes.clear(),
                4 => state.semantic_result_sha256.clear(),
                _ => unreachable!(),
            }
            drop(state);
            assert!(
                scope
                    .interaction_receipt(interaction_context(
                        InteractionRouteV1::AssessmentCurrentResult,
                        InteractionSetupExpectationV1::AuthoritativeReplay,
                    ))
                    .is_err(),
                "missing once-only field {field} was accepted"
            );
        }

        for field in 0..5 {
            let scope = LongitudinalCountingScopeV1::new(format!("{:064x}", field + 96))
                .expect("valid scope");
            record_minimal_interaction_facts(
                &scope,
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            );
            match field {
                0 => scope.record_observed_route_once(InteractionRouteV1::AssessmentCurrentResult),
                1 => scope.record_observed_route_state_once(
                    InteractionObservedRouteStateV1::AuthoritativeReplay,
                ),
                2 => scope.record_execution_actor_once(InteractionActorV1::RequestReader),
                3 => scope.record_outcome_once(true, 0),
                4 => scope.record_semantic_result_sha256_once(hash('9')),
                _ => unreachable!(),
            }
            assert!(
                scope
                    .interaction_receipt(interaction_context(
                        InteractionRouteV1::AssessmentCurrentResult,
                        InteractionSetupExpectationV1::AuthoritativeReplay,
                    ))
                    .is_err(),
                "duplicate once-only field {field} was accepted"
            );
        }
    }

    #[test]
    fn interaction_receipt_rejects_missing_phase_actor_and_child_terminal_drift() {
        let scope = LongitudinalCountingScopeV1::new(hash('5')).expect("valid scope");
        let _guard = scope.enter();
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
            );
        }
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
        assert!(
            scope
                .interaction_receipt(interaction_context(
                    InteractionRouteV1::AssessmentCurrentResult,
                    InteractionSetupExpectationV1::AuthoritativeReplay,
                ))
                .is_err()
        );

        for case in 0..3 {
            let scope = LongitudinalCountingScopeV1::new(format!("{:064x}", case + 128))
                .expect("valid scope");
            record_minimal_interaction_facts(
                &scope,
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            );
            let ordinal = scope.reserve_child_scope(InteractionActorV1::BackgroundMaintenance);
            scope.record_child_scope_terminal_once(
                ordinal,
                if case == 1 {
                    InteractionActorV1::BackgroundRebuild
                } else {
                    InteractionActorV1::BackgroundMaintenance
                },
                InteractionScopeCoverageV1::Complete,
            );
            if case == 0 {
                scope.record_child_scope_terminal_once(
                    ordinal,
                    InteractionActorV1::BackgroundMaintenance,
                    InteractionScopeCoverageV1::Complete,
                );
            }
            if case == 2 {
                scope.record_child_scope_terminal_once(
                    ordinal + 1,
                    InteractionActorV1::BackgroundMaintenance,
                    InteractionScopeCoverageV1::Complete,
                );
            }
            let mut expected = interaction_context(
                InteractionRouteV1::AssessmentCurrentResult,
                InteractionSetupExpectationV1::AuthoritativeReplay,
            );
            expected
                .expected_child_actors
                .insert(InteractionActorV1::BackgroundMaintenance, 1);
            assert!(
                scope.interaction_receipt(expected).is_err(),
                "child terminal drift case {case} was accepted"
            );
        }

        let scope = LongitudinalCountingScopeV1::new(hash('6')).expect("valid scope");
        record_minimal_interaction_facts(
            &scope,
            InteractionRouteV1::AssessmentCurrentResult,
            InteractionObservedRouteStateV1::AuthoritativeReplay,
        );
        let ordinal = scope.reserve_child_scope(InteractionActorV1::BackgroundMaintenance);
        scope.record_child_scope_terminal_once(
            ordinal,
            InteractionActorV1::BackgroundMaintenance,
            InteractionScopeCoverageV1::Complete,
        );
        assert!(
            scope
                .interaction_receipt(interaction_context(
                    InteractionRouteV1::AssessmentCurrentResult,
                    InteractionSetupExpectationV1::AuthoritativeReplay,
                ))
                .is_err(),
            "unexpected source child was silently omitted"
        );
    }

    #[test]
    fn interaction_phases_have_stable_spellings_and_ownership() {
        let phases = [
            LongitudinalDerivedAccessPhaseV1::CliCapabilityPreflightH1,
            LongitudinalDerivedAccessPhaseV1::WorkflowActivatedCapabilityProbe,
            LongitudinalDerivedAccessPhaseV1::OrdinaryReadStoreResolutionH2,
            LongitudinalDerivedAccessPhaseV1::WorkflowChangeReaderReplayH3,
            LongitudinalDerivedAccessPhaseV1::WorkflowChangeStoreReopenInspection,
            LongitudinalDerivedAccessPhaseV1::RouteRevisionSelection,
            LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
            LongitudinalDerivedAccessPhaseV1::RouteBodyHydration,
            LongitudinalDerivedAccessPhaseV1::GitContextResolution,
            LongitudinalDerivedAccessPhaseV1::SqliteSelection,
            LongitudinalDerivedAccessPhaseV1::CarrierValidation,
            LongitudinalDerivedAccessPhaseV1::SerializationAndOutput,
            LongitudinalDerivedAccessPhaseV1::CacheAndFallback,
            LongitudinalDerivedAccessPhaseV1::ReadTransaction,
            LongitudinalDerivedAccessPhaseV1::CheckpointAndWal,
            LongitudinalDerivedAccessPhaseV1::GenerationLeaseAndRetention,
            LongitudinalDerivedAccessPhaseV1::FactSqliteSelection,
            LongitudinalDerivedAccessPhaseV1::FactSelectedCarrierHydrationValidation,
            LongitudinalDerivedAccessPhaseV1::FactSupportCarrierHydrationValidation,
            LongitudinalDerivedAccessPhaseV1::FactWorkflowProjection,
        ];
        let spellings = phases
            .iter()
            .map(|phase| serde_json::to_value(phase).expect("phase JSON"))
            .collect::<Vec<_>>();
        assert_eq!(
            spellings,
            [
                "cli_capability_preflight_h1",
                "workflow_activated_capability_probe",
                "ordinary_read_store_resolution_h2",
                "workflow_change_reader_replay_h3",
                "workflow_change_store_reopen_inspection",
                "route_revision_selection",
                "route_projection_fold",
                "route_body_hydration",
                "git_context_resolution",
                "sqlite_selection",
                "carrier_validation",
                "serialization_and_output",
                "cache_and_fallback",
                "read_transaction",
                "checkpoint_and_wal",
                "generation_lease_and_retention",
                "fact_sqlite_selection",
                "fact_selected_carrier_hydration_validation",
                "fact_support_carrier_hydration_validation",
                "fact_workflow_projection",
            ]
            .into_iter()
            .map(serde_json::Value::from)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            phases.map(LongitudinalDerivedAccessPhaseV1::ownership),
            [
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::MixedDerivedAndTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::MixedDerivedAndTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::ProductProjection,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::ProductProjection,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::ProductProjection,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::DerivedAccess,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::AuthoritativeTruth,
                LongitudinalDerivedAccessPhaseOwnershipV1::ProductProjection,
            ]
        );
    }

    #[test]
    fn fact_route_counters_name_bounded_work_and_forbidden_change_construction() {
        let scope = LongitudinalCountingScopeV1::new(hash('8')).expect("valid scope");
        let _guard = scope.enter();

        record_strict_journal_inspection();
        record_fact_sqlite_rows_selected(2);
        record_change_semantic_construction();
        record_change_projection_construction();

        let counters = scope.snapshot().counters;
        assert_eq!(counters.strict_journal_inspections, 1);
        assert_eq!(counters.fact_sqlite_rows_selected, 2);
        assert_eq!(counters.change_semantic_constructions, 1);
        assert_eq!(counters.change_projection_constructions, 1);
    }

    #[test]
    fn no_scope_has_zero_effect_and_new_scope_starts_empty() {
        let phase = enter_derived_access_phase_v1(
            LongitudinalDerivedAccessPhaseV1::RevisionPageSqlSelection,
        );
        record_directory_entries_walked(9);
        record_carrier_read(11);
        record_authority_identity_rows_scanned(12);
        record_authoritative_fallback();
        record_full_history_fallback();
        record_event_decode();
        record_event_validation();
        record_event_folds(13);
        record_chronological_sort_items(17);
        record_body_artifact_read(Some(19));
        record_object_artifact_read(Some(23));
        record_projection_rebuild();
        record_state_rebuild();
        record_response_bytes(29);
        drop(phase);

        let scope = LongitudinalCountingScopeV1::new(hash('1')).expect("valid scope");
        let _guard = scope.enter();
        assert_eq!(scope.snapshot().counters, LongitudinalCountersV1::default());
        assert!(scope.snapshot().derived_access_phases.is_empty());
        assert_eq!(
            scope.snapshot().capacity_ownership,
            LongitudinalCapacityOwnershipV1::default()
        );
    }

    #[test]
    fn phase_scope_records_ordered_counter_and_resource_deltas() {
        let scope = LongitudinalCountingScopeV1::new(hash('8')).expect("valid scope");
        let _scope_guard = scope.enter();

        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::RevisionPageSqlSelection,
            );
            record_carrier_read(13);
            record_authoritative_fallback();
            record_event_decode();
        }
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::RevisionPageEventIdExpansion,
            );
            record_full_history_fallback();
            record_event_folds(17);
        }

        let phases = scope.snapshot().derived_access_phases;
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].ordinal, 0);
        assert_eq!(phases[1].ordinal, 1);
        assert!(phases.iter().all(|phase| phase.parent_ordinal.is_none()));
        assert_eq!(
            phases.iter().map(|phase| phase.phase).collect::<Vec<_>>(),
            vec![
                LongitudinalDerivedAccessPhaseV1::RevisionPageSqlSelection,
                LongitudinalDerivedAccessPhaseV1::RevisionPageEventIdExpansion,
            ]
        );
        assert_eq!(phases[0].counters.carrier_opens, 1);
        assert_eq!(phases[0].counters.carrier_bytes_read, 13);
        assert_eq!(phases[0].counters.authoritative_fallbacks, 1);
        assert_eq!(phases[0].counters.event_decodes, 1);
        assert_eq!(phases[1].counters.full_history_fallbacks, 1);
        assert_eq!(phases[1].counters.event_folds, 17);
        assert!(phases.iter().all(|phase| phase.wall_nanos < u64::MAX));
        assert!(phases.iter().all(|phase| {
            phase
                .resident_bytes_observed_max
                .is_none_or(|observed_max| {
                    phase
                        .resident_bytes_before
                        .is_some_and(|before| observed_max >= before)
                        && phase
                            .resident_bytes_after
                            .is_some_and(|after| observed_max >= after)
                })
        }));
    }

    #[test]
    fn change_page_counters_stay_attributed_to_their_exact_work_phase() {
        let scope = LongitudinalCountingScopeV1::new(hash('6')).expect("valid scope");
        let _scope_guard = scope.enter();

        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::ChangePageBodylessSelection,
            );
            record_change_candidates(3);
            record_change_candidate_current_revisions(5);
        }
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::ChangePageCarrierHydrationValidation,
            );
            record_change_proposal_carriers_opened(7);
            record_change_proposal_carriers_validated(11);
        }
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::ChangePageExhaustiveProposalSearch,
            );
            record_change_matches(13);
        }
        {
            let _phase = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::ChangePageSupportExpansion,
            );
            record_change_support_carriers_opened(17);
        }
        record_change_rows_emitted(19);

        let snapshot = scope.snapshot();
        let phases = &snapshot.derived_access_phases;
        assert_eq!(phases.len(), 4);
        assert_eq!(phases[0].counters.change_candidates, 3);
        assert_eq!(phases[0].counters.change_candidate_current_revisions, 5);
        assert_eq!(phases[1].counters.change_proposal_carriers_opened, 7);
        assert_eq!(phases[1].counters.change_proposal_carriers_validated, 11);
        assert_eq!(phases[2].counters.change_matches, 13);
        assert_eq!(phases[3].counters.change_support_carriers_opened, 17);
        assert!(
            phases
                .iter()
                .all(|phase| phase.counters.change_rows_emitted == 0)
        );
        assert_eq!(snapshot.counters.change_rows_emitted, 19);
    }

    #[test]
    fn phase_scope_marks_nested_samples_with_their_parent_ordinal() {
        let scope = LongitudinalCountingScopeV1::new(hash('9')).expect("valid scope");
        let _scope_guard = scope.enter();

        let outer = enter_derived_access_phase_v1(
            LongitudinalDerivedAccessPhaseV1::RevisionPageOverviewConstruction,
        );
        {
            let _inner = enter_derived_access_phase_v1(
                LongitudinalDerivedAccessPhaseV1::RevisionPageSnapshotSummaries,
            );
            record_object_artifact_read(Some(13));
        }
        drop(outer);

        let phases = scope.snapshot().derived_access_phases;
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].ordinal, 0);
        assert_eq!(phases[0].parent_ordinal, None);
        assert_eq!(phases[1].ordinal, 1);
        assert_eq!(phases[1].parent_ordinal, Some(0));
        assert_eq!(phases[0].counters.object_artifact_reads, 1);
        assert_eq!(phases[1].counters.object_artifact_reads, 1);
    }

    #[test]
    fn scope_records_every_frozen_counter_and_ownership_field_exactly() {
        let scope = LongitudinalCountingScopeV1::new(hash('2')).expect("valid scope");
        let _guard = scope.enter();

        record_directory_entries_walked(2);
        record_carrier_read(3);
        record_carrier_read(5);
        record_authority_identity_rows_scanned(6);
        record_strict_journal_inspection();
        record_fact_sqlite_rows_selected(59);
        record_change_semantic_construction();
        record_change_projection_construction();
        record_change_candidates(71);
        record_change_candidate_current_revisions(73);
        record_change_capability_carriers_opened(79);
        record_change_proposal_carriers_opened(83);
        record_change_proposal_carriers_validated(89);
        record_change_support_carriers_opened(97);
        record_change_matches(101);
        record_change_rows_emitted(103);
        record_timeline_sqlite_candidates(107);
        record_timeline_sqlite_window_rows(109);
        record_timeline_sqlite_facet_rows(113);
        record_timeline_selected_carriers(127);
        record_timeline_revision_candidate_carriers(129);
        record_timeline_removal_support_carriers(131);
        record_timeline_signature_support_carriers(137);
        record_timeline_correlation_support_carriers(139);
        record_timeline_trust_support_carriers(149);
        record_timeline_exhaustive_candidates(151);
        record_timeline_entries_emitted(157);
        record_authoritative_fallback();
        record_full_history_fallback();
        record_event_decode();
        record_event_validation();
        record_event_folds(7);
        record_chronological_sort_items(11);
        record_body_artifact_read(None);
        record_body_artifact_read(Some(13));
        record_object_artifact_read(None);
        record_object_artifact_read(Some(17));
        record_projection_rebuild();
        record_state_rebuild();
        record_response_bytes(19);

        set_retained_decoded_events(23);
        set_retained_hydrated_history_entries(29);
        set_retained_hydrated_body_bytes(31);
        set_retained_search_record_strings(37);
        set_retained_search_record_field_bytes(41);
        set_retained_serialized_response_cache_bytes(43);
        set_retained_snapshot_highlight_entries(47);
        set_retained_snapshot_highlight_bytes(53);

        let snapshot = scope.snapshot();
        assert_eq!(
            snapshot.counters,
            LongitudinalCountersV1 {
                directory_entries_walked: 2,
                carrier_opens: 2,
                carrier_bytes_read: 8,
                authority_identity_rows_scanned: 6,
                strict_journal_inspections: 1,
                fact_sqlite_rows_selected: 59,
                change_semantic_constructions: 1,
                change_projection_constructions: 1,
                change_candidates: 71,
                change_candidate_current_revisions: 73,
                change_capability_carriers_opened: 79,
                change_proposal_carriers_opened: 83,
                change_proposal_carriers_validated: 89,
                change_support_carriers_opened: 97,
                change_matches: 101,
                change_rows_emitted: 103,
                timeline_sqlite_candidates: 107,
                timeline_sqlite_window_rows: 109,
                timeline_sqlite_facet_rows: 113,
                timeline_selected_carriers: 127,
                timeline_revision_candidate_carriers: 129,
                timeline_removal_support_carriers: 131,
                timeline_signature_support_carriers: 137,
                timeline_correlation_support_carriers: 139,
                timeline_trust_support_carriers: 149,
                timeline_exhaustive_candidates: 151,
                timeline_entries_emitted: 157,
                authoritative_fallbacks: 1,
                full_history_fallbacks: 1,
                event_decodes: 1,
                event_validations: 1,
                event_folds: 7,
                chronological_sort_items: 11,
                body_artifact_reads: 2,
                body_bytes_read: 13,
                object_artifact_reads: 2,
                object_bytes_read: 17,
                projection_rebuilds: 1,
                state_rebuilds: 1,
                response_bytes: 19,
            }
        );
        assert_eq!(
            snapshot.capacity_ownership,
            LongitudinalCapacityOwnershipV1 {
                retained_decoded_events: 23,
                retained_hydrated_history_entries: 29,
                retained_hydrated_body_bytes: 31,
                retained_search_record_strings: 37,
                retained_search_record_field_bytes: 41,
                retained_serialized_response_cache_bytes: 43,
                retained_snapshot_highlight_entries: 47,
                retained_snapshot_highlight_bytes: 53,
            }
        );
    }

    #[test]
    fn decoded_event_guards_add_and_release_their_own_populations() {
        let scope = LongitudinalCountingScopeV1::new(hash('7')).expect("valid scope");
        let _scope_guard = scope.enter();

        let first = RetainedDecodedEventsGuardV1::new(2);
        assert_eq!(
            scope.snapshot().capacity_ownership.retained_decoded_events,
            2
        );

        let second = RetainedDecodedEventsGuardV1::new(3);
        assert_eq!(
            scope.snapshot().capacity_ownership.retained_decoded_events,
            5
        );

        drop(first);
        assert_eq!(
            scope.snapshot().capacity_ownership.retained_decoded_events,
            3
        );

        drop(second);
        assert_eq!(
            scope.snapshot().capacity_ownership.retained_decoded_events,
            0
        );
    }

    #[test]
    fn nested_scopes_do_not_merge_their_active_intervals() {
        let outer = LongitudinalCountingScopeV1::new(hash('3')).expect("valid outer scope");
        let inner = LongitudinalCountingScopeV1::new(hash('4')).expect("valid inner scope");
        let _outer_guard = outer.enter();
        record_event_folds(2);
        {
            let _inner_guard = inner.enter();
            record_event_folds(5);
        }
        record_event_folds(7);

        assert_eq!(outer.snapshot().counters.event_folds, 9);
        assert_eq!(inner.snapshot().counters.event_folds, 5);
    }

    #[test]
    fn concurrent_scopes_are_request_local() {
        let barrier = Arc::new(Barrier::new(3));
        let mut joins = Vec::new();
        for (run, folds) in [(hash('5'), 11), (hash('6'), 17)] {
            let barrier = Arc::clone(&barrier);
            joins.push(thread::spawn(move || {
                let scope = LongitudinalCountingScopeV1::new(run).expect("valid scope");
                let _guard = scope.enter();
                barrier.wait();
                record_event_folds(folds);
                barrier.wait();
                scope.snapshot()
            }));
        }
        barrier.wait();
        barrier.wait();

        let left = joins.remove(0).join().expect("left scope");
        let right = joins.remove(0).join().expect("right scope");
        assert_eq!(left.counters.event_folds, 11);
        assert_eq!(right.counters.event_folds, 17);
    }

    #[test]
    fn receipt_transport_binds_lineage_and_rejects_metric_fields() {
        let scope = LongitudinalCountingScopeV1::new(hash('7')).expect("valid scope");
        let _guard = scope.enter();
        record_carrier_read(101);
        record_response_bytes(103);

        let receipt = scope
            .receipt(LongitudinalCounterReceiptContextV1 {
                root_identity: hash('8'),
                operation: "WARM_HEAD".to_owned(),
                phase: "warm".to_owned(),
                base_execution_identity_sha256: hash('9'),
                derivative_execution_identity_sha256: hash('a'),
                manifest_sha256: hash('b'),
                schedule_sha256: hash('c'),
                success: true,
                semantic_result_sha256: hash('d'),
                include_capacity_ownership: true,
            })
            .expect("valid receipt");
        receipt.validate().expect("receipt validates");
        assert_eq!(receipt.run_identity, hash('7'));
        assert_eq!(receipt.counters.carrier_opens, 1);
        assert_eq!(receipt.counters.carrier_bytes_read, 101);
        assert_eq!(receipt.counters.response_bytes, 103);
        assert!(receipt.capacity_ownership.is_some());

        let mut value = serde_json::to_value(receipt).expect("receipt JSON");
        value
            .as_object_mut()
            .expect("receipt object")
            .insert("wallNanos".to_owned(), serde_json::json!(1));
        assert!(serde_json::from_value::<LongitudinalCounterReceiptV1>(value).is_err());
    }

    #[test]
    fn timeline_post_pin_barrier_binds_exact_request_boundary_and_carrier_mismatch() {
        let root = tempfile::tempdir().expect("barrier root");
        let request = LongitudinalTimelinePostPinBarrierRequestV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1.to_owned(),
            barrier_identity_sha256: hash('b'),
            expected_carrier_key_digest: hash('c'),
            clean_carrier_sha256: hash('d'),
            mutated_carrier_sha256: hash('e'),
            mutation_recipe_sha256: hash('f'),
        };
        let scope = LongitudinalCountingScopeV1::new(hash('a'))
            .expect("valid scope")
            .with_timeline_post_pin_barrier(root.path(), request.clone())
            .expect("valid barrier");
        let ready_path = longitudinal_timeline_post_pin_ready_path_v1(
            root.path(),
            &request.barrier_identity_sha256,
        );
        let release_path = longitudinal_timeline_post_pin_release_path_v1(
            root.path(),
            &request.barrier_identity_sha256,
        );
        let controller = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let ready: LongitudinalTimelinePostPinReadyV1 = loop {
                match std::fs::read(&ready_path) {
                    Ok(bytes) => break serde_json::from_slice(&bytes).expect("ready document"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        assert!(Instant::now() < deadline, "ready document timed out");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("read ready document: {error}"),
                }
            };
            ready.validate().expect("valid ready document");
            let release = LongitudinalTimelinePostPinReleaseV1 {
                schema: LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1.to_owned(),
                run_identity: ready.run_identity.clone(),
                barrier_identity_sha256: ready.barrier_identity_sha256.clone(),
                ready_receipt_sha256: ready.canonical_sha256().expect("ready identity"),
                clean_carrier_sha256: hash('d'),
                mutated_carrier_sha256: hash('e'),
                mutation_recipe_sha256: hash('f'),
                derivative_inventory_sha256: hash('1'),
                abort_reason_sha256: None,
            };
            write_longitudinal_timeline_barrier_document_v1(&release_path, &release)
                .expect("write release");
        });

        let _guard = scope.enter();
        record_timeline_selected_carriers(1);
        reach_timeline_carrier_locators_selected_v1().expect("release post-pin barrier");
        record_timeline_carrier_mismatch_v1(
            &hash('c'),
            LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
        )
        .expect("record exact mismatch");
        controller.join().expect("barrier controller");

        let receipt = scope
            .timeline_post_pin_barrier_receipt()
            .expect("complete barrier")
            .expect("barrier receipt");
        receipt.validate().expect("valid barrier receipt");
        assert_eq!(receipt.run_identity, hash('a'));
        assert_eq!(receipt.carrier_opens_before, 0);
        assert_eq!(receipt.selected_carriers_before, 1);
        assert_eq!(receipt.expected_carrier_key_digest, hash('c'));
        assert_eq!(receipt.observed_mismatch_key_digest, hash('c'));
        assert_eq!(
            receipt.mismatch_kind,
            LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness
        );
        assert_eq!(receipt.derivative_inventory_sha256, hash('1'));
    }

    #[test]
    fn timeline_post_pin_observations_are_noops_without_an_armed_request() {
        assert!(reach_timeline_carrier_locators_selected_v1().is_ok());
        assert!(
            record_timeline_carrier_mismatch_v1(
                "not-a-digest",
                LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
            )
            .is_ok()
        );

        let scope = LongitudinalCountingScopeV1::new(hash('a')).expect("valid scope");
        let _guard = scope.enter();
        assert!(reach_timeline_carrier_locators_selected_v1().is_ok());
        assert!(
            record_timeline_carrier_mismatch_v1(
                "not-a-digest",
                LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
            )
            .is_ok()
        );
        assert_eq!(
            scope
                .timeline_post_pin_barrier_receipt()
                .expect("unarmed receipt lookup"),
            None
        );
    }

    #[test]
    fn barrier_response_semantic_is_canonical_json() {
        let left = canonical_longitudinal_response_semantic_sha256_v1(br#"{"b":2,"a":1}"#)
            .expect("left semantic");
        let right = canonical_longitudinal_response_semantic_sha256_v1(br#"{"a":1,"b":2}"#)
            .expect("right semantic");

        assert_eq!(left, right);
    }

    #[test]
    fn timeline_post_pin_barrier_rejects_a_different_carrier_mismatch() {
        let root = tempfile::tempdir().expect("barrier root");
        let request = LongitudinalTimelinePostPinBarrierRequestV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1.to_owned(),
            barrier_identity_sha256: hash('b'),
            expected_carrier_key_digest: hash('c'),
            clean_carrier_sha256: hash('d'),
            mutated_carrier_sha256: hash('e'),
            mutation_recipe_sha256: hash('f'),
        };
        let scope = LongitudinalCountingScopeV1::new(hash('a'))
            .expect("valid scope")
            .with_timeline_post_pin_barrier(root.path(), request)
            .expect("valid barrier");
        let _guard = scope.enter();
        assert_eq!(
            record_timeline_carrier_mismatch_v1(
                &hash('9'),
                LongitudinalTimelineCarrierMismatchKindV1::ValidationWitness,
            ),
            Err("Timeline post-pin barrier rejected a different carrier".to_owned())
        );
        assert!(scope.timeline_post_pin_barrier_receipt().is_err());
    }

    #[test]
    fn timeline_post_pin_barrier_abort_release_unblocks_and_fails_closed() {
        let root = tempfile::tempdir().expect("barrier root");
        let request = LongitudinalTimelinePostPinBarrierRequestV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1.to_owned(),
            barrier_identity_sha256: hash('b'),
            expected_carrier_key_digest: hash('c'),
            clean_carrier_sha256: hash('d'),
            mutated_carrier_sha256: hash('e'),
            mutation_recipe_sha256: hash('f'),
        };
        let scope = LongitudinalCountingScopeV1::new(hash('a'))
            .expect("valid scope")
            .with_timeline_post_pin_barrier(root.path(), request.clone())
            .expect("valid barrier");
        let ready_path = longitudinal_timeline_post_pin_ready_path_v1(
            root.path(),
            &request.barrier_identity_sha256,
        );
        let release_path = longitudinal_timeline_post_pin_release_path_v1(
            root.path(),
            &request.barrier_identity_sha256,
        );
        let controller = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let ready: LongitudinalTimelinePostPinReadyV1 = loop {
                match read_longitudinal_timeline_barrier_document_v1(&ready_path) {
                    Ok(ready) => break ready,
                    Err(error) if error.starts_with("not_found:") => {
                        assert!(Instant::now() < deadline, "ready document timed out");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("read ready document: {error}"),
                }
            };
            let release = LongitudinalTimelinePostPinReleaseV1 {
                schema: LONGITUDINAL_TIMELINE_POST_PIN_RELEASE_SCHEMA_V1.to_owned(),
                run_identity: ready.run_identity.clone(),
                barrier_identity_sha256: ready.barrier_identity_sha256.clone(),
                ready_receipt_sha256: ready.canonical_sha256().expect("ready identity"),
                clean_carrier_sha256: request.clean_carrier_sha256,
                mutated_carrier_sha256: request.mutated_carrier_sha256,
                mutation_recipe_sha256: request.mutation_recipe_sha256,
                derivative_inventory_sha256: hash('1'),
                abort_reason_sha256: Some(hash('2')),
            };
            write_longitudinal_timeline_barrier_document_v1(&release_path, &release)
                .expect("write abort release");
        });

        let _guard = scope.enter();
        record_timeline_selected_carriers(1);
        let error =
            reach_timeline_carrier_locators_selected_v1().expect_err("abort release fails closed");
        controller.join().expect("barrier controller");

        assert_eq!(
            error,
            format!("Timeline post-pin barrier aborted: {}", hash('2'))
        );
        assert!(scope.timeline_post_pin_barrier_receipt().is_err());
    }
}
