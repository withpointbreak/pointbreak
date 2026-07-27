use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::adapter::QualificationDerivedAccessAdapter;
use super::sqlite_cursor::{BootstrapControl, CursorLedgerIdentity, SqliteCursorLedger};
use super::{
    QualificationDerivedAccessCountersV1, QualificationDerivedAccessOperationV1,
    qualification_derived_access_contract_v1,
};
use crate::bench_support::longitudinal::{
    LongitudinalExecutionIdentityV1, LongitudinalMaterializeOptionsV1,
    LongitudinalStoreDataInventoryV1, LongitudinalStrictSemanticReceiptV1, LongitudinalTierV1,
    longitudinal_store_data_inventory_v1, materialize_longitudinal_workload_v1,
    verify_longitudinal_materialization_pair_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
use crate::model::JournalId;
use crate::session::benchmark::{
    LongitudinalRecordShapeV1, LongitudinalRecordSpecV1, prepare_longitudinal_record_v1,
    write_longitudinal_records_v1,
};
use crate::session::derived_access::locator::{ChronologicalWindowRequest, LocatorRead};
use crate::session::derived_access::oracle::strict_bodyless_materialized_snapshot;
use crate::session::event::{
    EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload, Writer,
};
use crate::session::{EventStore, StoreMode, set_store_mode_for_repo, store_dir_for_repo};

pub const QUALIFICATION_DERIVED_ACCESS_D0_MATERIALIZER_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-d0-materializer.v1";
pub const QUALIFICATION_DERIVED_ACCESS_D0_PAIR_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-d0-pair.v1";
pub const QUALIFICATION_DERIVED_ACCESS_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-smoke.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LONGITUDINAL_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-longitudinal-smoke.v1";
pub const QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1: &str =
    "27894b1b25292789e3e33911fa1d3e8ec80a7bc8e39069b09e9bf528a6e4b33c";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0ScheduleEntryV1 {
    pub ordinal: u16,
    pub event_type: String,
    pub family_ordinal: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0ScheduleV1 {
    pub schema: String,
    pub public_seed_hex: String,
    pub stored_events: u16,
    pub revisions: u16,
    pub independently_referenced_objects: u16,
    pub entries: Vec<QualificationDerivedAccessD0ScheduleEntryV1>,
    pub ordered_schedule_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0RootReceiptV1 {
    pub schema: String,
    pub public_seed_hex: String,
    pub coverage_schedule_sha256: String,
    pub ordered_schedule_sha256: String,
    pub ordered_event_identities_sha256: String,
    pub event_count: u16,
    pub revision_count: u16,
    pub independently_referenced_objects: u16,
    pub by_type: BTreeMap<String, u16>,
    pub store_inventory: LongitudinalStoreDataInventoryV1,
    pub strict: LongitudinalStrictSemanticReceiptV1,
    pub receipt_sha256: String,
}

impl QualificationDerivedAccessD0RootReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        let contract = qualification_derived_access_contract_v1();
        let schedule = qualification_derived_access_d0_schedule_v1()?;
        if self.schema != QUALIFICATION_DERIVED_ACCESS_D0_MATERIALIZER_SCHEMA_V1
            || self.public_seed_hex != QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1
            || self.coverage_schedule_sha256 != contract.d0.schedule_sha256
            || self.ordered_schedule_sha256 != schedule.ordered_schedule_sha256
            || self.ordered_event_identities_sha256.len() != 64
            || self.event_count != contract.d0.stored_events
            || self.revision_count != contract.d0.revisions
            || self.independently_referenced_objects != contract.d0.independently_referenced_objects
            || self.receipt_sha256 != self.canonical_sha256()?
        {
            return Err("D0-128 materialization receipt drifted".to_owned());
        }
        self.store_inventory
            .validate()
            .map_err(|error| error.to_string())?;
        for family in &contract.d0.event_families {
            if self.by_type.get(&family.event_type).copied().unwrap_or(0) != family.count {
                return Err(format!(
                    "D0-128 event family drifted: {}",
                    family.event_type
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessD0PairReceiptV1 {
    pub schema: String,
    pub root_a: QualificationDerivedAccessD0RootReceiptV1,
    pub root_b: QualificationDerivedAccessD0RootReceiptV1,
    pub byte_identical: bool,
    pub pair_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessSmokeOperationReceiptV1 {
    pub operation: QualificationDerivedAccessOperationV1,
    pub semantic_receipt_sha256: String,
    pub counters: QualificationDerivedAccessCountersV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessSmokeReceiptV1 {
    pub schema: String,
    pub d0_pair: QualificationDerivedAccessD0PairReceiptV1,
    pub operation_receipts: Vec<QualificationDerivedAccessSmokeOperationReceiptV1>,
    pub counters_captured: bool,
    pub incremental_matches_full_replay: bool,
    pub smoke_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessLongitudinalSmokeReceiptV1 {
    pub schema: String,
    pub tier: super::QualificationDerivedAccessTierV1,
    pub event_count: u64,
    pub revision_count: u64,
    pub root_a_sha256: String,
    pub root_b_sha256: String,
    pub byte_identical: bool,
    pub operation_receipts: Vec<QualificationDerivedAccessSmokeOperationReceiptV1>,
    pub counters_captured: bool,
    pub incremental_matches_full_replay: bool,
    pub smoke_sha256: String,
}

impl QualificationDerivedAccessLongitudinalSmokeReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.smoke_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected_events = match self.tier {
            super::QualificationDerivedAccessTierV1::L1 => 1_024,
            super::QualificationDerivedAccessTierV1::L7 => 7_168,
            _ => {
                return Err("longitudinal derived-access smoke supports only L1 and L7".to_owned());
            }
        };
        if self.schema != QUALIFICATION_DERIVED_ACCESS_LONGITUDINAL_SMOKE_SCHEMA_V1
            || self.event_count != expected_events
            || self.revision_count == 0
            || !self.byte_identical
            || self.root_a_sha256 != self.root_b_sha256
            || !self.incremental_matches_full_replay
            || self
                .operation_receipts
                .iter()
                .map(|row| row.operation)
                .collect::<Vec<_>>()
                != QualificationDerivedAccessOperationV1::ALL
            || self.smoke_sha256 != self.canonical_sha256()?
        {
            return Err("longitudinal derived-access smoke receipt drifted".to_owned());
        }
        Ok(())
    }
}

impl QualificationDerivedAccessSmokeReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.smoke_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.d0_pair.validate()?;
        if self.schema != QUALIFICATION_DERIVED_ACCESS_SMOKE_SCHEMA_V1
            || !self.incremental_matches_full_replay
            || self
                .operation_receipts
                .iter()
                .map(|row| row.operation)
                .collect::<Vec<_>>()
                != QualificationDerivedAccessOperationV1::ALL
            || self
                .operation_receipts
                .iter()
                .any(|row| row.semantic_receipt_sha256.len() != 64)
            || self.smoke_sha256 != self.canonical_sha256()?
        {
            return Err("derived-access non-timing smoke receipt drifted".to_owned());
        }
        Ok(())
    }
}

impl QualificationDerivedAccessD0PairReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.pair_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.root_a.validate()?;
        self.root_b.validate()?;
        if self.schema != QUALIFICATION_DERIVED_ACCESS_D0_PAIR_SCHEMA_V1
            || !self.byte_identical
            || self.root_a != self.root_b
            || self.root_a.store_inventory.inventory_sha256
                != self.root_b.store_inventory.inventory_sha256
            || self.pair_sha256 != self.canonical_sha256()?
        {
            return Err("D0-128 independent roots are not byte-identical".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationDerivedAccessD0MaterializeOptionsV1 {
    pub root: PathBuf,
}

impl QualificationDerivedAccessD0MaterializeOptionsV1 {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }
}

pub fn qualification_derived_access_d0_schedule_v1()
-> Result<QualificationDerivedAccessD0ScheduleV1, String> {
    let contract = qualification_derived_access_contract_v1();
    let mut entries = Vec::with_capacity(contract.d0.stored_events as usize);
    for family in &contract.d0.event_families {
        for family_ordinal in 0..family.count {
            entries.push(QualificationDerivedAccessD0ScheduleEntryV1 {
                ordinal: entries.len() as u16,
                event_type: family.event_type.clone(),
                family_ordinal,
            });
        }
    }
    if entries.len() != contract.d0.stored_events as usize {
        return Err("D0-128 ordered schedule count drifted".to_owned());
    }
    let ordered_schedule_sha256 =
        canonical_sha256(&(QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1, &entries))?;
    Ok(QualificationDerivedAccessD0ScheduleV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_D0_MATERIALIZER_SCHEMA_V1.to_owned(),
        public_seed_hex: QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1.to_owned(),
        stored_events: contract.d0.stored_events,
        revisions: contract.d0.revisions,
        independently_referenced_objects: contract.d0.independently_referenced_objects,
        entries,
        ordered_schedule_sha256,
    })
}

pub fn materialize_qualification_derived_access_d0_v1(
    options: QualificationDerivedAccessD0MaterializeOptionsV1,
) -> Result<QualificationDerivedAccessD0RootReceiptV1, String> {
    initialize_disposable_root(&options.root)?;
    let prepared = prepare_longitudinal_record_v1(LongitudinalRecordSpecV1::new(
        LongitudinalRecordShapeV1::DerivedAccessD0,
        0,
    ))
    .map_err(|error| error.to_string())?;
    let written = write_longitudinal_records_v1(&options.root, &[prepared])
        .map_err(|error| error.to_string())?;
    let schedule = qualification_derived_access_d0_schedule_v1()?;
    let generated_schedule_sha256 = canonical_sha256(
        &written
            .ordered_events
            .iter()
            .enumerate()
            .map(|(ordinal, event)| (ordinal, &event.event_id))
            .collect::<Vec<_>>(),
    )?;
    // The public schedule identifies the ordered family slots; the generated
    // event identity list is additionally bound through the strict and receipt
    // hashes without making source paths part of the contract.
    let contract = qualification_derived_access_contract_v1();
    let by_type = written
        .by_type
        .into_iter()
        .map(|(event_type, count)| {
            u16::try_from(count)
                .map(|count| (event_type, count))
                .map_err(|_| "D0-128 event count exceeds u16".to_owned())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut receipt = QualificationDerivedAccessD0RootReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_D0_MATERIALIZER_SCHEMA_V1.to_owned(),
        public_seed_hex: QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1.to_owned(),
        coverage_schedule_sha256: contract.d0.schedule_sha256,
        ordered_schedule_sha256: schedule.ordered_schedule_sha256,
        ordered_event_identities_sha256: generated_schedule_sha256,
        event_count: u16::try_from(written.event_count)
            .map_err(|_| "D0-128 event count exceeds u16".to_owned())?,
        revision_count: u16::try_from(written.revision_count)
            .map_err(|_| "D0-128 revision count exceeds u16".to_owned())?,
        independently_referenced_objects: u16::try_from(written.object_artifact_count)
            .map_err(|_| "D0-128 object count exceeds u16".to_owned())?,
        by_type,
        store_inventory: longitudinal_store_data_inventory_v1(&options.root)
            .map_err(|error| error.to_string())?,
        strict: written.strict,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

pub fn materialize_qualification_derived_access_d0_pair_v1(
    root_a: impl AsRef<Path>,
    root_b: impl AsRef<Path>,
) -> Result<QualificationDerivedAccessD0PairReceiptV1, String> {
    let root_a = materialize_qualification_derived_access_d0_v1(
        QualificationDerivedAccessD0MaterializeOptionsV1::new(root_a),
    )?;
    let root_b = materialize_qualification_derived_access_d0_v1(
        QualificationDerivedAccessD0MaterializeOptionsV1::new(root_b),
    )?;
    let byte_identical = root_a == root_b
        && root_a.store_inventory.inventory_sha256 == root_b.store_inventory.inventory_sha256;
    let mut receipt = QualificationDerivedAccessD0PairReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_D0_PAIR_SCHEMA_V1.to_owned(),
        root_a,
        root_b,
        byte_identical,
        pair_sha256: String::new(),
    };
    receipt.pair_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

pub fn run_qualification_derived_access_non_timing_smoke_v1()
-> Result<QualificationDerivedAccessSmokeReceiptV1, String> {
    let parent = std::env::temp_dir().join(format!(
        "pointbreak-derived-access-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&parent).map_err(|error| error.to_string())?;
    let result = run_smoke_in_root(&parent);
    let cleanup = std::fs::remove_dir_all(&parent);
    match (result, cleanup) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Ok(_), Err(error)) => Err(format!(
            "derived-access smoke succeeded but cleanup failed: {error}"
        )),
        (Err(error), _) => Err(error),
    }
}

pub fn run_qualification_derived_access_non_timing_smoke_at_v1(
    parent: &Path,
) -> Result<QualificationDerivedAccessSmokeReceiptV1, String> {
    if parent.exists() {
        if parent
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("derived-access smoke root must be absent or empty".to_owned());
        }
    } else {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    run_smoke_in_root(parent)
}

pub fn run_qualification_derived_access_longitudinal_smoke_v1(
    tier: super::QualificationDerivedAccessTierV1,
) -> Result<QualificationDerivedAccessLongitudinalSmokeReceiptV1, String> {
    let parent = std::env::temp_dir().join(format!(
        "pointbreak-derived-access-{tier:?}-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    let result = run_qualification_derived_access_longitudinal_smoke_at_v1(tier, &parent);
    let cleanup = std::fs::remove_dir_all(&parent);
    match (result, cleanup) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Ok(_), Err(error)) => Err(format!(
            "derived-access longitudinal smoke succeeded but cleanup failed: {error}"
        )),
        (Err(error), _) => Err(error),
    }
}

pub fn run_qualification_derived_access_longitudinal_smoke_at_v1(
    tier: super::QualificationDerivedAccessTierV1,
    parent: &Path,
) -> Result<QualificationDerivedAccessLongitudinalSmokeReceiptV1, String> {
    let longitudinal_tier = match tier {
        super::QualificationDerivedAccessTierV1::L1 => LongitudinalTierV1::L1,
        super::QualificationDerivedAccessTierV1::L7 => LongitudinalTierV1::L7,
        _ => return Err("derived-access longitudinal smoke supports only L1 and L7".to_owned()),
    };
    if parent.exists() {
        if parent
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err(
                "derived-access longitudinal smoke root must be absent or empty".to_owned(),
            );
        }
    } else {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let root_a = parent.join("root-a");
    let root_b = parent.join("root-b");
    initialize_disposable_root(&root_a)?;
    initialize_disposable_root(&root_b)?;
    let execution = smoke_longitudinal_execution_identity();
    let receipt_a = materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
        &root_a,
        longitudinal_tier,
        execution.clone(),
    ))
    .map_err(|error| error.to_string())?;
    let receipt_b = materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
        &root_b,
        longitudinal_tier,
        execution,
    ))
    .map_err(|error| error.to_string())?;
    verify_longitudinal_materialization_pair_v1(&receipt_a, &receipt_b)
        .map_err(|error| error.to_string())?;
    let inventory_a =
        longitudinal_store_data_inventory_v1(&root_a).map_err(|error| error.to_string())?;
    let inventory_b =
        longitudinal_store_data_inventory_v1(&root_b).map_err(|error| error.to_string())?;
    let (operation_receipts, incremental_matches_full_replay) =
        exercise_materialized_root(&root_a, &format!("store:derived-access-{tier:?}-smoke"))?;
    let mut receipt = QualificationDerivedAccessLongitudinalSmokeReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_LONGITUDINAL_SMOKE_SCHEMA_V1.to_owned(),
        tier,
        event_count: receipt_a.manifest.event_count,
        revision_count: receipt_a.manifest.revision_count,
        root_a_sha256: inventory_a.inventory_sha256.clone(),
        root_b_sha256: inventory_b.inventory_sha256.clone(),
        byte_identical: inventory_a == inventory_b,
        operation_receipts,
        counters_captured: cfg!(feature = "longitudinal-counting"),
        incremental_matches_full_replay,
        smoke_sha256: String::new(),
    };
    receipt.smoke_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

fn run_smoke_in_root(parent: &Path) -> Result<QualificationDerivedAccessSmokeReceiptV1, String> {
    let root_a = parent.join("root-a");
    let root_b = parent.join("root-b");
    let d0_pair = materialize_qualification_derived_access_d0_pair_v1(&root_a, &root_b)?;
    let (rows, incremental_matches_full_replay) =
        exercise_materialized_root(&root_a, "store:derived-access-d0-smoke")?;
    let mut receipt = QualificationDerivedAccessSmokeReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_SMOKE_SCHEMA_V1.to_owned(),
        d0_pair,
        operation_receipts: rows,
        counters_captured: cfg!(feature = "longitudinal-counting"),
        incremental_matches_full_replay,
        smoke_sha256: String::new(),
    };
    receipt.smoke_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

fn exercise_materialized_root(
    repo_root: &Path,
    store_id: &str,
) -> Result<(Vec<QualificationDerivedAccessSmokeOperationReceiptV1>, bool), String> {
    let store = store_dir_for_repo(repo_root).map_err(|error| error.to_string())?;
    let identity = CursorLedgerIdentity::new(store_id);
    let cursor = SqliteCursorLedger::bootstrap_from_truth(&store, identity.clone(), 1, |_| {
        BootstrapControl::Continue
    })
    .map_err(|error| error.to_string())?;
    drop(cursor);
    let adapter = QualificationDerivedAccessAdapter::open(&store, identity)
        .map_err(|error| error.to_string())?;
    let head = adapter
        .catch_up_to_head(64)
        .map_err(|error| error.to_string())?;
    let events = EventStore::open(&store)
        .list_events()
        .map_err(|error| error.to_string())?;
    let strict =
        strict_bodyless_materialized_snapshot(&events).map_err(|error| error.to_string())?;
    let incremental = match adapter
        .semantic_materialized_audit_snapshot()
        .map_err(|error| error.to_string())?
    {
        LocatorRead::Ready(snapshot) => snapshot,
        LocatorRead::CatchUpRequired { .. } => {
            return Err("D0 smoke remained behind the observed truth head".to_owned());
        }
    };
    let incremental_matches_full_replay = incremental == strict && incremental.as_of == head;

    let first_event = events
        .first()
        .ok_or_else(|| "D0 smoke materialized no events".to_owned())?;
    let (removed_revision, active_revision) = d0_revision_selectors(&events)?;
    let mut rows = Vec::with_capacity(QualificationDerivedAccessOperationV1::ALL.len());
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::SemanticId,
        || match adapter
            .semantic_id(first_event.event_id.as_str())
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(Some(event)) => Ok(event.event_id.as_str().to_owned()),
            _ => Err("D0 semantic-id lookup did not return its event".to_owned()),
        },
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::FreshNoChange,
        || {
            Ok(format!(
                "{:?}",
                adapter.freshness().map_err(|error| error.to_string())?
            ))
        },
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::NewCountZero,
        || {
            Ok(format!(
                "{:?}",
                adapter
                    .new_event_count()
                    .map_err(|error| error.to_string())?
            ))
        },
    )?);
    let head_window =
        capture_smoke_operation(QualificationDerivedAccessOperationV1::WindowHead, || {
            let window = ready_window(
                adapter
                    .chronological_window(ChronologicalWindowRequest::head(10))
                    .map_err(|error| error.to_string())?,
            )?;
            event_ids(&window.events)
        })?;
    let head_for_continuation = ready_window(
        adapter
            .chronological_window(ChronologicalWindowRequest::head(10))
            .map_err(|error| error.to_string())?,
    )?;
    rows.push(head_window);
    let continuation = head_for_continuation
        .continuation
        .ok_or_else(|| "D0 head window did not return a middle continuation".to_owned())?;
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::WindowMiddle,
        || {
            let window = ready_window(
                adapter
                    .chronological_window(ChronologicalWindowRequest::continue_from(
                        continuation.clone(),
                        10,
                    ))
                    .map_err(|error| error.to_string())?,
            )?;
            event_ids(&window.events)
        },
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::WindowTail,
        || {
            let window = ready_window(
                adapter
                    .chronological_window(ChronologicalWindowRequest::tail(10))
                    .map_err(|error| error.to_string())?,
            )?;
            event_ids(&window.events)
        },
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::RevisionDetailActive,
        || revision_detail_receipt(&adapter, &active_revision, false),
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::RevisionDetailRemoved,
        || revision_detail_receipt(&adapter, &removed_revision, true),
    )?);

    let appended = smoke_append_event()?;
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::AppendOne,
        || {
            Ok(format!(
                "{:?}",
                adapter
                    .append_event(&appended, "derived-access-smoke-append")
                    .map_err(|error| error.to_string())?
            ))
        },
    )?);
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::PostOne,
        || {
            let window = ready_window(
                adapter
                    .chronological_window(ChronologicalWindowRequest::tail(10))
                    .map_err(|error| error.to_string())?,
            )?;
            event_ids(&window.events)
        },
    )?);
    drop(adapter);
    let reopened =
        QualificationDerivedAccessAdapter::open(&store, CursorLedgerIdentity::new(store_id))
            .map_err(|error| error.to_string())?;
    rows.push(capture_smoke_operation(
        QualificationDerivedAccessOperationV1::Restart,
        || {
            Ok(format!(
                "{:?}",
                reopened.truth_head().map_err(|error| error.to_string())?
            ))
        },
    )?);

    Ok((rows, incremental_matches_full_replay))
}

fn d0_revision_selectors(
    events: &[ShoreEvent],
) -> Result<(crate::model::RevisionId, crate::model::RevisionId), String> {
    let mut revisions = Vec::new();
    let mut removed_hashes = std::collections::BTreeSet::new();
    for event in events {
        if event.event_type == EventType::ArtifactRemoved {
            let hash = event
                .payload
                .get("contentHash")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "D0 removal payload omitted contentHash".to_owned())?;
            removed_hashes.insert(hash.to_owned());
        }
        if event.event_type == EventType::WorkObjectProposed {
            let payload: WorkObjectProposedPayload =
                serde_json::from_value(event.payload.clone()).map_err(|error| error.to_string())?;
            if let WorkObjectProposal::Revision {
                revision,
                object_artifact_content_hash,
                ..
            } = payload.work_object
            {
                revisions.push((
                    revision.id,
                    removed_hashes.contains(&object_artifact_content_hash),
                    object_artifact_content_hash,
                ));
            }
        }
    }
    let removed = revisions
        .iter()
        .find(|(_, _, hash)| removed_hashes.contains(hash))
        .map(|(revision, _, _)| revision.clone())
        .ok_or_else(|| "D0 removed revision is missing".to_owned())?;
    let active = revisions
        .iter()
        .find(|(_, _, hash)| !removed_hashes.contains(hash))
        .map(|(revision, _, _)| revision.clone())
        .ok_or_else(|| "D0 active revision is missing".to_owned())?;
    Ok((removed, active))
}

fn revision_detail_receipt(
    adapter: &QualificationDerivedAccessAdapter,
    revision_id: &crate::model::RevisionId,
    expected_removed: bool,
) -> Result<String, String> {
    match adapter
        .revision_detail(revision_id)
        .map_err(|error| error.to_string())?
    {
        LocatorRead::Ready(Some(detail)) if detail.object_content_removed == expected_removed => {
            canonical_sha256(&(
                detail.revision_id.as_str(),
                detail.object_content_hash,
                detail.object_content_removed,
                event_ids(&detail.authoritative_events)?,
            ))
        }
        _ => Err(format!(
            "D0 revision detail did not match removal state {expected_removed}"
        )),
    }
}

fn ready_window(
    read: LocatorRead<crate::session::derived_access::locator::HydratedWindow>,
) -> Result<crate::session::derived_access::locator::HydratedWindow, String> {
    match read {
        LocatorRead::Ready(window) => Ok(window),
        LocatorRead::CatchUpRequired { .. } => {
            Err("D0 window remained behind the observed truth head".to_owned())
        }
    }
}

fn event_ids(events: &[ShoreEvent]) -> Result<String, String> {
    canonical_sha256(
        &events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
    )
}

fn capture_smoke_operation(
    operation: QualificationDerivedAccessOperationV1,
    operation_fn: impl FnOnce() -> Result<String, String>,
) -> Result<QualificationDerivedAccessSmokeOperationReceiptV1, String> {
    #[cfg(feature = "longitudinal-counting")]
    let scope = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
        canonical_sha256(&("derived-access-smoke", operation))?,
    )?;
    #[cfg(feature = "longitudinal-counting")]
    let guard = scope.enter();
    let value = operation_fn()?;
    #[cfg(feature = "longitudinal-counting")]
    drop(guard);
    #[cfg(feature = "longitudinal-counting")]
    let counters = {
        let observed = scope.snapshot().counters;
        QualificationDerivedAccessCountersV1 {
            directory_entries_walked: observed.directory_entries_walked,
            carrier_opens: observed.carrier_opens,
            carrier_bytes_read: observed.carrier_bytes_read,
            event_decodes: observed.event_decodes,
            event_validations: observed.event_validations,
            event_folds: observed.event_folds,
            chronological_sort_items: observed.chronological_sort_items,
            body_artifact_reads: observed.body_artifact_reads,
            body_bytes_read: observed.body_bytes_read,
            object_artifact_reads: observed.object_artifact_reads,
            object_bytes_read: observed.object_bytes_read,
            unselected_body_artifact_reads: 0,
            unselected_object_artifact_reads: 0,
            projection_rebuilds: observed.projection_rebuilds,
            state_rebuilds: observed.state_rebuilds,
            response_bytes: value.len() as u64,
        }
    };
    #[cfg(not(feature = "longitudinal-counting"))]
    let counters = QualificationDerivedAccessCountersV1 {
        directory_entries_walked: 0,
        carrier_opens: 0,
        carrier_bytes_read: 0,
        event_decodes: 0,
        event_validations: 0,
        event_folds: 0,
        chronological_sort_items: 0,
        body_artifact_reads: 0,
        body_bytes_read: 0,
        object_artifact_reads: 0,
        object_bytes_read: 0,
        unselected_body_artifact_reads: 0,
        unselected_object_artifact_reads: 0,
        projection_rebuilds: 0,
        state_rebuilds: 0,
        response_bytes: value.len() as u64,
    };
    Ok(QualificationDerivedAccessSmokeOperationReceiptV1 {
        operation,
        semantic_receipt_sha256: canonical_sha256(&(operation, &value))?,
        counters,
    })
}

fn smoke_append_event() -> Result<ShoreEvent, String> {
    let journal_id = JournalId::new("journal:derived-access-d0-smoke-append");
    ShoreEvent::new(
        EventType::ReviewInitialized,
        ReviewInitializedPayload::idempotency_key(&journal_id),
        EventTarget::for_journal(journal_id),
        Writer::shore_local(env!("CARGO_PKG_VERSION")),
        ReviewInitializedPayload {},
        "2026-07-27T00:10:00.000Z",
    )
    .map_err(|error| error.to_string())
}

fn smoke_longitudinal_execution_identity() -> LongitudinalExecutionIdentityV1 {
    LongitudinalExecutionIdentityV1 {
        source_commit: "0".repeat(40),
        source_tree: "1".repeat(40),
        cargo_lock_sha256: "2".repeat(64),
        runner_sha256: "3".repeat(64),
        build_profile: "derived-access-non-timing-smoke".to_owned(),
        operating_system: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        filesystem: "disposable".to_owned(),
        parent_commit: None,
    }
}

fn initialize_disposable_root(root: &Path) -> Result<(), String> {
    if root.exists() {
        if root
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("D0-128 root must be absent or empty".to_owned());
        }
    } else {
        std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    }
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .arg(root)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    set_store_mode_for_repo(root, StoreMode::Ephemeral).map_err(|error| error.to_string())
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    canonical_json_bytes(&value)
        .map(|bytes| sha256_bytes_hex(&bytes))
        .map_err(|error| error.to_string())
}
