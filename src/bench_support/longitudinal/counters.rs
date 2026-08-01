use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

use super::{
    LONGITUDINAL_COUNTER_RECEIPT_SCHEMA_V1, LongitudinalCapacityOwnershipV1,
    LongitudinalContractError, LongitudinalCounterReceiptV1, LongitudinalCountersV1,
};

thread_local! {
    static ACTIVE_SCOPES: RefCell<Vec<LongitudinalCountingScopeV1>> =
        const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Default)]
struct ObserverState {
    counters: LongitudinalCountersV1,
    capacity_ownership: LongitudinalCapacityOwnershipV1,
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
    _not_send: PhantomData<Rc<()>>,
}

/// Tracks one live decoded-event population in the active counting scope.
///
/// Unlike the legacy point-in-time setter, this guard adds its population to
/// the live total and removes exactly that population on drop. Overlapping
/// decoded histories therefore remain observable instead of overwriting one
/// another's ownership count.
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

    pub fn enter(&self) -> LongitudinalCountingGuardV1 {
        ACTIVE_SCOPES.with(|scopes| scopes.borrow_mut().push(self.clone()));
        LongitudinalCountingGuardV1 {
            state: Arc::clone(&self.state),
            _not_send: PhantomData,
        }
    }

    /// Clone the active request scope for explicit propagation to a child
    /// thread. A child remains uncounted unless it enters the returned scope.
    pub fn current() -> Option<Self> {
        ACTIVE_SCOPES.with(|scopes| scopes.borrow().last().cloned())
    }

    pub fn snapshot(&self) -> LongitudinalCountingSnapshotV1 {
        let state = lock_state(&self.state);
        LongitudinalCountingSnapshotV1 {
            run_identity: self.run_identity.clone(),
            counters: state.counters.clone(),
            capacity_ownership: state.capacity_ownership.clone(),
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
}

impl Drop for LongitudinalCountingGuardV1 {
    fn drop(&mut self) {
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

    use super::*;

    fn hash(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    #[test]
    fn no_scope_has_zero_effect_and_new_scope_starts_empty() {
        record_directory_entries_walked(9);
        record_carrier_read(11);
        record_event_decode();
        record_event_validation();
        record_event_folds(13);
        record_chronological_sort_items(17);
        record_body_artifact_read(Some(19));
        record_object_artifact_read(Some(23));
        record_projection_rebuild();
        record_state_rebuild();
        record_response_bytes(29);

        let scope = LongitudinalCountingScopeV1::new(hash('1')).expect("valid scope");
        let _guard = scope.enter();
        assert_eq!(scope.snapshot().counters, LongitudinalCountersV1::default());
        assert_eq!(
            scope.snapshot().capacity_ownership,
            LongitudinalCapacityOwnershipV1::default()
        );
    }

    #[test]
    fn scope_records_every_frozen_counter_and_ownership_field_exactly() {
        let scope = LongitudinalCountingScopeV1::new(hash('2')).expect("valid scope");
        let _guard = scope.enter();

        record_directory_entries_walked(2);
        record_carrier_read(3);
        record_carrier_read(5);
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
}
