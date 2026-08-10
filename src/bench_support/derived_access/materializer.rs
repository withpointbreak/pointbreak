use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(feature = "longitudinal-counting")]
use ed25519_dalek::{Signer as _, SigningKey};
use serde::{Deserialize, Serialize};

use super::adapter::QualificationDerivedAccessAdapter;
use super::sqlite_cursor::{BootstrapControl, CursorLedgerIdentity, SqliteCursorLedger};
use super::{
    QualificationDerivedAccessCountersV1, QualificationDerivedAccessOperationV1,
    QualificationDerivedChangeFixtureV1, QualificationDerivedStorageForbiddenProbeHashesV1,
    qualification_derived_access_contract_v1, qualification_derived_change_storage_probe_hashes_v1,
};
use crate::bench_support::longitudinal::{
    LongitudinalCapacityOwnershipV1, LongitudinalExecutionIdentityV1,
    LongitudinalMaterializeOptionsV1, LongitudinalStoreDataInventoryV1,
    LongitudinalStrictSemanticReceiptV1, LongitudinalTierV1,
    longitudinal_authoritative_store_data_inventory_v1, longitudinal_store_data_inventory_v1,
    materialize_longitudinal_workload_v1, verify_longitudinal_materialization_pair_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::bench_support::longitudinal::{
    LongitudinalCountersV1, LongitudinalCountingScopeV1, capture_longitudinal_process_snapshot_v1,
};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
#[cfg(feature = "longitudinal-counting")]
use crate::crypto::{EventSignatureBytes, SignerId};
use crate::model::JournalId;
#[cfg(feature = "longitudinal-counting")]
use crate::model::{
    ChangeId, ChangeIdentityDescriptorV1, EngagementId, ObjectId, ObservationId, ReviewTargetRef,
    RevisionId, RevisionRefV1, TrackId,
};
use crate::session::benchmark::{
    LongitudinalRecordShapeV1, LongitudinalRecordSpecV1, prepare_longitudinal_record_v1,
    write_longitudinal_records_v1,
};
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
use crate::session::derived_access::locator::{ChronologicalWindowRequest, LocatorRead};
use crate::session::derived_access::oracle::strict_bodyless_materialized_snapshot;
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::product_contract::DerivedAccessProfile;
#[cfg(feature = "longitudinal-counting")]
use crate::session::derived_access::writer::DerivedWriteCoordinator;
#[cfg(feature = "longitudinal-counting")]
use crate::session::event::{
    ArtifactRemovedPayload, BodyContentType, EventSignature, EventSignatureRecordedPayload,
    EventToBeSigned, ReviewObservationRecordedPayload, Revision, build_change_declared,
    build_membership_asserted, build_revision_relation_asserted,
    event_signature_pre_authentication_encoding,
};
use crate::session::event::{
    EventTarget, EventType, ReviewInitializedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload, Writer,
};
use crate::session::{
    ChangeLifecycleV1, ChangeTopologyV1, EventStore, StoreMode, set_store_mode_for_repo,
    store_dir_for_repo,
};
#[cfg(feature = "longitudinal-counting")]
use crate::session::{EventWriteOutcome, opaque_path_identity};

pub const QUALIFICATION_DERIVED_ACCESS_D0_MATERIALIZER_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-d0-materializer.v1";
pub const QUALIFICATION_DERIVED_ACCESS_D0_PAIR_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-d0-pair.v1";
pub const QUALIFICATION_DERIVED_ACCESS_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-smoke.v1";
pub const QUALIFICATION_DERIVED_ACCESS_LONGITUDINAL_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-longitudinal-smoke.v1";
pub const QUALIFICATION_DERIVED_ACCESS_BOOTSTRAP_SMOKE_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-access-bootstrap-smoke.v1";
pub const QUALIFICATION_DERIVED_ACCESS_D0_PUBLIC_SEED_HEX_V1: &str =
    "27894b1b25292789e3e33911fa1d3e8ec80a7bc8e39069b09e9bf528a6e4b33c";
pub const QUALIFICATION_DERIVED_CHANGE_FIXTURE_WITNESS_SCHEMA_V1: &str =
    "pointbreak.qualification-derived-change-fixture-witness.v1";
pub const QUALIFICATION_DERIVED_CHANGE_FIXTURE_MODE_V1: &str =
    "--derived-change-fixture-materialize";
pub const QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1: &str =
    "5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json";
pub const QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1: &str =
    "f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json";
pub const QUALIFICATION_DERIVED_CHANGE_STORAGE_SUMMARY_PROBE_V1: &str =
    "qualification storage summary sentinel v1";
pub const QUALIFICATION_DERIVED_CHANGE_STORAGE_PROSE_PROBE_V1: &str =
    "qualification storage prose sentinel v1";
#[cfg(feature = "longitudinal-counting")]
pub const QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_SHA256_V1: &str =
    "20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf";
#[cfg(feature = "longitudinal-counting")]
const QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_SHA256_V1: &str =
    "b0c6360bd8c90a2e5ae336f3a2caf60aceb205ac3bdf53971bcfcd66bd21041f";

/// A public, disposable fixture shape for Change-reader qualification.
///
/// These shapes intentionally name reader observations, not source-cut claims.
/// They are distinct from D0/L1/L7 workload tiers and never label themselves as
/// native workload evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeFixtureKindV1 {
    DuplicateEqual,
    DuplicateConflicting,
    OperativeRemoval,
    MissingSelectedCarrier,
    MutatedSelectedCarrier,
    WrongFamilySelectedCarrier,
    IncompleteChange,
    CycleConflictedChange,
}

impl QualificationDerivedChangeFixtureKindV1 {
    pub const ALL: [Self; 8] = [
        Self::DuplicateEqual,
        Self::DuplicateConflicting,
        Self::OperativeRemoval,
        Self::MissingSelectedCarrier,
        Self::MutatedSelectedCarrier,
        Self::WrongFamilySelectedCarrier,
        Self::IncompleteChange,
        Self::CycleConflictedChange,
    ];

    pub fn fixture_id(self) -> &'static str {
        match self {
            Self::DuplicateEqual => "duplicate-equal-v1",
            Self::DuplicateConflicting => "duplicate-conflict-v1",
            Self::OperativeRemoval => "removal-v1",
            Self::MissingSelectedCarrier => "missing-carrier-v1",
            Self::MutatedSelectedCarrier => "mutated-carrier-v1",
            Self::WrongFamilySelectedCarrier => "wrong-family-carrier-v1",
            Self::IncompleteChange => "incomplete-v1",
            Self::CycleConflictedChange => "cycle-conflicted-v1",
        }
    }

    fn contract_fixture(self) -> QualificationDerivedChangeFixtureV1 {
        match self {
            Self::DuplicateEqual => QualificationDerivedChangeFixtureV1::DuplicateEqualV1,
            Self::DuplicateConflicting => QualificationDerivedChangeFixtureV1::DuplicateConflictV1,
            Self::OperativeRemoval => QualificationDerivedChangeFixtureV1::RemovalV1,
            Self::MissingSelectedCarrier => QualificationDerivedChangeFixtureV1::MissingCarrierV1,
            Self::MutatedSelectedCarrier => QualificationDerivedChangeFixtureV1::MutatedCarrierV1,
            Self::WrongFamilySelectedCarrier => {
                QualificationDerivedChangeFixtureV1::WrongFamilyCarrierV1
            }
            Self::IncompleteChange => QualificationDerivedChangeFixtureV1::IncompleteV1,
            Self::CycleConflictedChange => QualificationDerivedChangeFixtureV1::CycleConflictedV1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeFixtureRequestV1 {
    pub source_checkout: PathBuf,
    pub root: PathBuf,
    pub kind: QualificationDerivedChangeFixtureKindV1,
}

impl QualificationDerivedChangeFixtureRequestV1 {
    pub fn new(root: impl Into<PathBuf>, kind: QualificationDerivedChangeFixtureKindV1) -> Self {
        Self {
            source_checkout: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            root: root.into(),
            kind,
        }
    }

    pub fn with_source_checkout(mut self, source_checkout: impl Into<PathBuf>) -> Self {
        self.source_checkout = source_checkout.into();
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeFixtureCarrierRoleV1 {
    Primary,
    EqualDuplicate,
    ConflictingDuplicate,
    Selected,
    RemovalSupport,
    SignatureSupport,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeFixtureCarrierStateV1 {
    Present,
    Missing,
    Mutated,
    WrongFamily,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeFixtureCarrierV1 {
    pub role: QualificationDerivedChangeFixtureCarrierRoleV1,
    pub state: QualificationDerivedChangeFixtureCarrierStateV1,
    pub idempotency_key_sha256: String,
    pub payload_sha256: String,
    pub event_record_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeFixtureTopologyV1 {
    pub change_id_sha256: String,
    pub expected_topology: ChangeTopologyV1,
    pub expected_lifecycle: ChangeLifecycleV1,
    pub current_revision_ref_sha256: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDerivedChangeFixtureExpectedOutcomeV1 {
    Ready,
    ProjectionInvalid,
    ProjectionRebuildRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedChangeFixtureWitnessV1 {
    pub schema: String,
    pub fixture_id: String,
    pub kind: QualificationDerivedChangeFixtureKindV1,
    pub authoritative_inventory_sha256: String,
    pub topology: QualificationDerivedChangeFixtureTopologyV1,
    pub carriers: Vec<QualificationDerivedChangeFixtureCarrierV1>,
    pub expected_outcome: QualificationDerivedChangeFixtureExpectedOutcomeV1,
    pub storage_forbidden_probe_hashes: QualificationDerivedStorageForbiddenProbeHashesV1,
    pub witness_sha256: String,
}

impl QualificationDerivedChangeFixtureWitnessV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.witness_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn refresh_sha256(&mut self) -> Result<(), String> {
        self.witness_sha256 = self.canonical_sha256()?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALIFICATION_DERIVED_CHANGE_FIXTURE_WITNESS_SCHEMA_V1
            || self.fixture_id != self.kind.fixture_id()
            || self.carriers.is_empty()
                && self.kind != QualificationDerivedChangeFixtureKindV1::IncompleteChange
            || !is_sha256_unprefixed(&self.authoritative_inventory_sha256)
            || !is_sha256_unprefixed(&self.witness_sha256)
            || !is_sha256_unprefixed(&self.topology.change_id_sha256)
            || self.storage_forbidden_probe_hashes.validate().is_err()
            || self.storage_forbidden_probe_hashes
                != qualification_derived_change_storage_probe_hashes_v1(
                    self.kind.contract_fixture(),
                )
            || self
                .topology
                .current_revision_ref_sha256
                .iter()
                .any(|hash| !is_sha256_unprefixed(hash))
            || self.witness_sha256 != self.canonical_sha256()?
        {
            return Err("derived Change fixture witness drifted".to_owned());
        }
        for carrier in &self.carriers {
            if !is_sha256_prefixed(&carrier.payload_sha256)
                || !is_sha256_prefixed(&carrier.event_record_sha256)
                || !is_sha256_unprefixed(&carrier.idempotency_key_sha256)
            {
                return Err("derived Change fixture carrier witness drifted".to_owned());
            }
        }
        let (topology, lifecycle, current_count, outcome, carrier_shape) = match self.kind {
            QualificationDerivedChangeFixtureKindV1::DuplicateEqual => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                vec![
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::EqualDuplicate,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                ],
            ),
            QualificationDerivedChangeFixtureKindV1::DuplicateConflicting => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                vec![
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::ConflictingDuplicate,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                ],
            ),
            QualificationDerivedChangeFixtureKindV1::OperativeRemoval => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                vec![
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::RemovalSupport,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::SignatureSupport,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                ],
            ),
            QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionRebuildRequired,
                vec![(
                    QualificationDerivedChangeFixtureCarrierRoleV1::Selected,
                    QualificationDerivedChangeFixtureCarrierStateV1::Missing,
                )],
            ),
            QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                vec![(
                    QualificationDerivedChangeFixtureCarrierRoleV1::Selected,
                    QualificationDerivedChangeFixtureCarrierStateV1::Mutated,
                )],
            ),
            QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier => (
                ChangeTopologyV1::Initial,
                ChangeLifecycleV1::InProgress,
                1,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                vec![(
                    QualificationDerivedChangeFixtureCarrierRoleV1::Selected,
                    QualificationDerivedChangeFixtureCarrierStateV1::WrongFamily,
                )],
            ),
            QualificationDerivedChangeFixtureKindV1::IncompleteChange => (
                ChangeTopologyV1::Incomplete,
                ChangeLifecycleV1::Incomplete,
                0,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                Vec::new(),
            ),
            QualificationDerivedChangeFixtureKindV1::CycleConflictedChange => (
                ChangeTopologyV1::CycleConflicted,
                ChangeLifecycleV1::Conflicted,
                0,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                vec![
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                    (
                        QualificationDerivedChangeFixtureCarrierRoleV1::Selected,
                        QualificationDerivedChangeFixtureCarrierStateV1::Present,
                    ),
                ],
            ),
        };
        let observed_carrier_shape = self
            .carriers
            .iter()
            .map(|carrier| (carrier.role, carrier.state))
            .collect::<Vec<_>>();
        if self.topology.expected_topology != topology
            || self.topology.expected_lifecycle != lifecycle
            || self.topology.current_revision_ref_sha256.len() != current_count
            || self.expected_outcome != outcome
            || observed_carrier_shape != carrier_shape
        {
            return Err("derived Change fixture witness shape drifted".to_owned());
        }
        Ok(())
    }
}

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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessBootstrapProgressV1 {
    pub phase: String,
    pub completed: u64,
    pub total: u64,
    pub bytes_processed: u64,
    pub elapsed_ms: u64,
    pub estimated_remaining_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualificationDerivedAccessBootstrapSmokeReceiptV1 {
    pub schema: String,
    pub tier: super::QualificationDerivedAccessTierV1,
    pub event_count: u64,
    pub head_sequence: u64,
    pub progress_updates: u64,
    pub phases: Vec<QualificationDerivedAccessBootstrapProgressV1>,
    pub counters: QualificationDerivedAccessCountersV1,
    pub capacity_ownership: LongitudinalCapacityOwnershipV1,
    pub rss_observed: bool,
    pub baseline_rss_bytes: Option<u64>,
    pub peak_observed_rss_bytes: Option<u64>,
    pub steady_rss_bytes: Option<u64>,
    pub semantic_receipt_sha256: String,
    pub receipt_sha256: String,
}

impl QualificationDerivedAccessBootstrapSmokeReceiptV1 {
    pub fn canonical_sha256(&self) -> Result<String, String> {
        let mut preimage = self.clone();
        preimage.receipt_sha256.clear();
        canonical_sha256(&preimage)
    }

    pub fn validate(&self) -> Result<(), String> {
        let expected_events = match self.tier {
            super::QualificationDerivedAccessTierV1::D0_128 => 128,
            super::QualificationDerivedAccessTierV1::L1 => 1_024,
            super::QualificationDerivedAccessTierV1::L7 => 7_168,
            _ => return Err("bootstrap smoke supports only D0-128, L1, and L7".to_owned()),
        };
        let expected_phases = [
            "cursor_population",
            "projection_population",
            "strict_verification",
            "finalizing",
        ];
        if self.schema != QUALIFICATION_DERIVED_ACCESS_BOOTSTRAP_SMOKE_SCHEMA_V1 {
            return Err("bounded bootstrap smoke schema drifted".to_owned());
        }
        if self.event_count != expected_events || self.head_sequence != expected_events {
            return Err(format!(
                "bounded bootstrap event/head count drifted: events={}, head={}, expected={expected_events}",
                self.event_count, self.head_sequence
            ));
        }
        if self.progress_updates == 0
            || self.phases.len() != expected_phases.len()
            || self
                .phases
                .iter()
                .map(|progress| progress.phase.as_str())
                .ne(expected_phases)
            || self
                .phases
                .iter()
                .any(|progress| progress.completed != progress.total)
        {
            return Err("bounded bootstrap progress receipt drifted".to_owned());
        }
        let minimum_carrier_work = expected_events.saturating_mul(2);
        let maximum_carrier_work = expected_events.saturating_mul(3);
        if self.counters.carrier_opens < minimum_carrier_work
            || self.counters.carrier_opens > maximum_carrier_work
            || self.counters.event_decodes != self.counters.carrier_opens
            || self.counters.event_validations != self.counters.carrier_opens
        {
            return Err(format!(
                "bounded bootstrap carrier work drifted: opens={}, decodes={}, validations={}, expected={minimum_carrier_work}..={maximum_carrier_work}",
                self.counters.carrier_opens,
                self.counters.event_decodes,
                self.counters.event_validations
            ));
        }
        if self.capacity_ownership.retained_decoded_events != 0 {
            return Err(format!(
                "bounded bootstrap retained {} decoded events",
                self.capacity_ownership.retained_decoded_events
            ));
        }
        if self.semantic_receipt_sha256.len() != 64 {
            return Err("bounded bootstrap semantic receipt is not a SHA-256".to_owned());
        }
        if self.receipt_sha256 != self.canonical_sha256()? {
            return Err("bounded bootstrap receipt hash drifted".to_owned());
        }
        let rss_values = (
            self.baseline_rss_bytes,
            self.peak_observed_rss_bytes,
            self.steady_rss_bytes,
        );
        match rss_values {
            (Some(baseline), Some(peak), Some(steady)) if self.rss_observed => {
                if peak < baseline || peak < steady {
                    return Err("bounded bootstrap RSS observations are inconsistent".to_owned());
                }
            }
            (None, None, None) if !self.rss_observed => {}
            _ => {
                return Err("bounded bootstrap RSS observation status is inconsistent".to_owned());
            }
        }
        Ok(())
    }
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
        store_inventory: longitudinal_authoritative_store_data_inventory_v1(&options.root)
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

#[cfg(feature = "longitudinal-counting")]
pub fn run_qualification_derived_access_bootstrap_smoke_v1(
    tier: super::QualificationDerivedAccessTierV1,
) -> Result<QualificationDerivedAccessBootstrapSmokeReceiptV1, String> {
    let parent = std::env::temp_dir().join(format!(
        "pointbreak-derived-bootstrap-{tier:?}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&parent).map_err(|error| error.to_string())?;
    let result = run_bootstrap_smoke_in_root(tier, &parent);
    let cleanup = std::fs::remove_dir_all(&parent);
    match (result, cleanup) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Ok(_), Err(error)) => Err(format!(
            "bounded bootstrap smoke succeeded but cleanup failed: {error}"
        )),
        (Err(error), _) => Err(error),
    }
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn run_qualification_derived_access_bootstrap_smoke_v1(
    _tier: super::QualificationDerivedAccessTierV1,
) -> Result<QualificationDerivedAccessBootstrapSmokeReceiptV1, String> {
    Err("bounded bootstrap smoke requires --features longitudinal-counting".to_owned())
}

#[cfg(feature = "longitudinal-counting")]
fn run_bootstrap_smoke_in_root(
    tier: super::QualificationDerivedAccessTierV1,
    parent: &Path,
) -> Result<QualificationDerivedAccessBootstrapSmokeReceiptV1, String> {
    let root = parent.join("root");
    let event_count = match tier {
        super::QualificationDerivedAccessTierV1::D0_128 => u64::from(
            materialize_qualification_derived_access_d0_v1(
                QualificationDerivedAccessD0MaterializeOptionsV1::new(&root),
            )?
            .event_count,
        ),
        super::QualificationDerivedAccessTierV1::L1
        | super::QualificationDerivedAccessTierV1::L7 => {
            initialize_disposable_root(&root)?;
            let longitudinal_tier = if tier == super::QualificationDerivedAccessTierV1::L1 {
                LongitudinalTierV1::L1
            } else {
                LongitudinalTierV1::L7
            };
            materialize_longitudinal_workload_v1(LongitudinalMaterializeOptionsV1::new(
                &root,
                longitudinal_tier,
                smoke_longitudinal_execution_identity(),
            ))
            .map_err(|error| error.to_string())?
            .manifest
            .event_count
        }
        _ => return Err("bootstrap smoke supports only D0-128, L1, and L7".to_owned()),
    };
    let store = store_dir_for_repo(&root).map_err(|error| error.to_string())?;
    let lifecycle = DerivedAccessLifecycle::new(
        DerivedAccessProfile::SqliteWalBodylessV1,
        &store,
        format!("store:derived-bootstrap-{tier:?}"),
    )
    .map_err(|error| error.to_string())?;
    let scope = LongitudinalCountingScopeV1::new(canonical_sha256(&(
        "derived-access-bootstrap-smoke",
        tier,
    ))?)?;
    let baseline_rss_bytes = current_process_rss();
    let mut peak_observed_rss_bytes = baseline_rss_bytes;
    let mut progress_updates = 0_u64;
    let mut phase_progress = BTreeMap::new();
    let guard = scope.enter();
    let lifecycle_receipt = lifecycle
        .rebuild(|progress| {
            progress_updates = progress_updates.saturating_add(1);
            if let Some(observed) = current_process_rss() {
                peak_observed_rss_bytes =
                    Some(peak_observed_rss_bytes.unwrap_or_default().max(observed));
            }
            phase_progress.insert(
                progress.phase.as_str().to_owned(),
                QualificationDerivedAccessBootstrapProgressV1 {
                    phase: progress.phase.as_str().to_owned(),
                    completed: u64::try_from(progress.completed).unwrap_or(u64::MAX),
                    total: u64::try_from(progress.total).unwrap_or(u64::MAX),
                    bytes_processed: progress.bytes_processed,
                    elapsed_ms: progress.elapsed_ms,
                    estimated_remaining_ms: progress.estimated_remaining_ms,
                },
            );
            LifecycleControl::Continue
        })
        .map_err(|error| error.to_string())?;
    drop(guard);
    let observed = scope.snapshot();
    let steady_rss_bytes = current_process_rss();
    if let Some(steady) = steady_rss_bytes {
        peak_observed_rss_bytes = Some(peak_observed_rss_bytes.unwrap_or_default().max(steady));
    }
    let phases = [
        "cursor_population",
        "projection_population",
        "strict_verification",
        "finalizing",
    ]
    .into_iter()
    .map(|phase| {
        phase_progress
            .remove(phase)
            .ok_or_else(|| format!("bootstrap did not report {phase}"))
    })
    .collect::<Result<Vec<_>, _>>()?;
    if !phase_progress.is_empty() {
        return Err("bootstrap reported an unknown phase".to_owned());
    }
    let semantic_receipt = lifecycle_receipt
        .semantic_receipt
        .ok_or_else(|| "bootstrap did not produce a semantic receipt".to_owned())?;
    let semantic_receipt_sha256 = semantic_receipt
        .strip_prefix("sha256:")
        .ok_or_else(|| "bootstrap semantic receipt omitted its SHA-256 algorithm".to_owned())?
        .to_owned();
    let current = lifecycle
        .open_current()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "bootstrap did not publish a current generation".to_owned())?;
    if current
        .service()
        .truth_head()
        .map_err(|error| error.to_string())?
        .cursor
        .sequence
        != event_count
    {
        return Err("published bootstrap head differs from materialized truth".to_owned());
    }
    let mut receipt = QualificationDerivedAccessBootstrapSmokeReceiptV1 {
        schema: QUALIFICATION_DERIVED_ACCESS_BOOTSTRAP_SMOKE_SCHEMA_V1.to_owned(),
        tier,
        event_count,
        head_sequence: lifecycle_receipt.head_sequence,
        progress_updates,
        phases,
        counters: qualification_smoke_counters(&observed.counters, 0),
        capacity_ownership: observed.capacity_ownership,
        rss_observed: baseline_rss_bytes.is_some()
            && peak_observed_rss_bytes.is_some()
            && steady_rss_bytes.is_some(),
        baseline_rss_bytes,
        peak_observed_rss_bytes,
        steady_rss_bytes,
        semantic_receipt_sha256,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = receipt.canonical_sha256()?;
    receipt.validate()?;
    Ok(receipt)
}

#[cfg(feature = "longitudinal-counting")]
fn current_process_rss() -> Option<u64> {
    capture_longitudinal_process_snapshot_v1(std::process::id())
        .ok()
        .map(|snapshot| snapshot.resident_bytes)
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
        qualification_smoke_counters(&observed, value.len() as u64)
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

#[cfg(feature = "longitudinal-counting")]
fn qualification_smoke_counters(
    observed: &LongitudinalCountersV1,
    response_bytes: u64,
) -> QualificationDerivedAccessCountersV1 {
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
        response_bytes,
    }
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

/// Materialize one deterministic, disposable public Change fixture and return
/// only its bodyless authority witness. The root is always an ephemeral store;
/// no owner store, source path, proposal summary, or event payload document is
/// present in the witness.
#[cfg(feature = "longitudinal-counting")]
pub fn materialize_qualification_derived_change_fixture_v1(
    request: QualificationDerivedChangeFixtureRequestV1,
) -> Result<QualificationDerivedChangeFixtureWitnessV1, String> {
    validate_change_fixture_request(&request)?;
    initialize_disposable_change_fixture_root(&request.root)?;
    let store_root = store_dir_for_repo(&request.root).map_err(|error| error.to_string())?;
    copy_change_ready_fixture_records(&request.source_checkout, &store_root)?;
    let store_identity =
        opaque_path_identity("store", &store_root).map_err(|error| error.to_string())?;
    let lifecycle = DerivedAccessLifecycle::new(
        DerivedAccessProfile::SqliteWalBodylessV1,
        &store_root,
        store_identity,
    )
    .map_err(|error| error.to_string())?;
    lifecycle
        .rebuild(|_| LifecycleControl::Continue)
        .map_err(|error| error.to_string())?;
    let coordinator = DerivedWriteCoordinator::new(lifecycle).map_err(|error| error.to_string())?;
    let store = EventStore::open(&store_root).with_coordinator(coordinator);
    let fixture = ChangeFixtureEvents::new(request.kind)?;

    record_fixture_event(&store, fixture.declaration.clone())?;
    for event in &fixture.proposals {
        record_fixture_event(&store, event.clone())?;
    }
    for event in &fixture.memberships {
        record_fixture_event(&store, event.clone())?;
    }
    for event in &fixture.relations {
        record_fixture_event(&store, event.clone())?;
    }
    if let Some(event) = &fixture.removal {
        record_fixture_event(&store, event.clone())?;
    }
    if let Some(event) = &fixture.signature {
        record_fixture_event(&store, event.clone())?;
    }
    for event in &fixture.storage_probe_events {
        record_fixture_event(&store, event.clone())?;
    }

    let mut carriers = fixture
        .proposals
        .iter()
        .enumerate()
        .map(|(index, event)| {
            carrier_witness(
                event,
                match request.kind {
                    QualificationDerivedChangeFixtureKindV1::DuplicateEqual if index == 1 => {
                        QualificationDerivedChangeFixtureCarrierRoleV1::EqualDuplicate
                    }
                    QualificationDerivedChangeFixtureKindV1::DuplicateConflicting if index == 1 => {
                        QualificationDerivedChangeFixtureCarrierRoleV1::ConflictingDuplicate
                    }
                    _ if index == 0 => QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                    _ => QualificationDerivedChangeFixtureCarrierRoleV1::Selected,
                },
                QualificationDerivedChangeFixtureCarrierStateV1::Present,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(event) = &fixture.removal {
        carriers.push(carrier_witness(
            event,
            QualificationDerivedChangeFixtureCarrierRoleV1::RemovalSupport,
            QualificationDerivedChangeFixtureCarrierStateV1::Present,
        )?);
    }
    if let Some(event) = &fixture.signature {
        carriers.push(carrier_witness(
            event,
            QualificationDerivedChangeFixtureCarrierRoleV1::SignatureSupport,
            QualificationDerivedChangeFixtureCarrierStateV1::Present,
        )?);
    }

    match request.kind {
        QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier => {
            let selected = fixture.proposals.first().ok_or_else(|| {
                "missing-carrier fixture omitted its selected proposal".to_owned()
            })?;
            std::fs::remove_file(store.event_path_for_idempotency_key(&selected.idempotency_key))
                .map_err(|error| error.to_string())?;
            carriers[0].role = QualificationDerivedChangeFixtureCarrierRoleV1::Selected;
            carriers[0].state = QualificationDerivedChangeFixtureCarrierStateV1::Missing;
        }
        QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier => {
            let selected = fixture.proposals.first().ok_or_else(|| {
                "mutated-carrier fixture omitted its selected proposal".to_owned()
            })?;
            let mut mutated = selected.clone();
            mutated.occurred_at = "2026-08-10T04:00:01Z".to_owned();
            std::fs::write(
                store.event_path_for_idempotency_key(&selected.idempotency_key),
                serde_json::to_vec(&mutated).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            carriers[0].role = QualificationDerivedChangeFixtureCarrierRoleV1::Selected;
            carriers[0].state = QualificationDerivedChangeFixtureCarrierStateV1::Mutated;
        }
        QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier => {
            let selected = fixture
                .proposals
                .first()
                .ok_or_else(|| "wrong-family fixture omitted its selected proposal".to_owned())?;
            let replacement = ShoreEvent::new(
                EventType::ReviewInitialized,
                selected.idempotency_key.clone(),
                EventTarget::for_journal(JournalId::new("journal:qualification-change-fixture")),
                Writer::shore_local("qualification-fixture"),
                ReviewInitializedPayload {},
                selected.occurred_at.clone(),
            )
            .map_err(|error| error.to_string())?;
            std::fs::write(
                store.event_path_for_idempotency_key(&selected.idempotency_key),
                serde_json::to_vec(&replacement).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            carriers[0].role = QualificationDerivedChangeFixtureCarrierRoleV1::Selected;
            carriers[0].state = QualificationDerivedChangeFixtureCarrierStateV1::WrongFamily;
        }
        _ => {}
    }

    let inventory = longitudinal_authoritative_store_data_inventory_v1(&request.root)
        .map_err(|error| error.to_string())?;
    let mut witness = QualificationDerivedChangeFixtureWitnessV1 {
        schema: QUALIFICATION_DERIVED_CHANGE_FIXTURE_WITNESS_SCHEMA_V1.to_owned(),
        fixture_id: request.kind.fixture_id().to_owned(),
        kind: request.kind,
        authoritative_inventory_sha256: inventory.inventory_sha256,
        topology: fixture.topology,
        carriers,
        expected_outcome: fixture.expected_outcome,
        storage_forbidden_probe_hashes: qualification_derived_change_storage_probe_hashes_v1(
            request.kind.contract_fixture(),
        ),
        witness_sha256: String::new(),
    };
    witness.refresh_sha256()?;
    witness.validate()?;
    Ok(witness)
}

#[cfg(feature = "longitudinal-counting")]
pub fn materialize_qualification_derived_change_fixture_from_request_v1(
    request_path: &Path,
) -> Result<QualificationDerivedChangeFixtureWitnessV1, String> {
    let file = std::fs::File::open(request_path).map_err(|error| error.to_string())?;
    let request = serde_json::from_reader(file).map_err(|error| error.to_string())?;
    materialize_qualification_derived_change_fixture_v1(request)
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn materialize_qualification_derived_change_fixture_v1(
    _request: QualificationDerivedChangeFixtureRequestV1,
) -> Result<QualificationDerivedChangeFixtureWitnessV1, String> {
    Err(
        "derived Change fixture materialization requires --features longitudinal-counting"
            .to_owned(),
    )
}

#[cfg(not(feature = "longitudinal-counting"))]
pub fn materialize_qualification_derived_change_fixture_from_request_v1(
    _request_path: &Path,
) -> Result<QualificationDerivedChangeFixtureWitnessV1, String> {
    Err(
        "derived Change fixture materialization requires --features longitudinal-counting"
            .to_owned(),
    )
}

#[cfg(feature = "longitudinal-counting")]
struct ChangeFixtureEvents {
    declaration: ShoreEvent,
    proposals: Vec<ShoreEvent>,
    memberships: Vec<ShoreEvent>,
    relations: Vec<ShoreEvent>,
    removal: Option<ShoreEvent>,
    signature: Option<ShoreEvent>,
    storage_probe_events: Vec<ShoreEvent>,
    topology: QualificationDerivedChangeFixtureTopologyV1,
    expected_outcome: QualificationDerivedChangeFixtureExpectedOutcomeV1,
}

#[cfg(feature = "longitudinal-counting")]
impl ChangeFixtureEvents {
    fn new(kind: QualificationDerivedChangeFixtureKindV1) -> Result<Self, String> {
        let descriptor = ChangeIdentityDescriptorV1::opaque_nonce([0x51; 32]);
        let declaration_payload =
            build_change_declared(descriptor, [0x52; 32]).map_err(|error| error.to_string())?;
        let change_id = declaration_payload.change_id.clone();
        let declaration = fixture_event(
            declaration_payload,
            "change-fixture:declared",
            "2026-08-10T04:00:00Z",
        )?;

        let (left, left_proposal) =
            fixture_revision_proposal("left", 'a', Some("fixture-left"), 0)?;
        let (right, right_proposal) =
            fixture_revision_proposal("right", 'b', Some("fixture-right"), 1)?;
        let left_membership = fixture_event(
            build_membership_asserted(&change_id, &left.revision_id, [0x53; 32])
                .map_err(|error| error.to_string())?,
            "change-fixture:membership:left",
            "2026-08-10T04:00:03Z",
        )?;
        let right_membership = fixture_event(
            build_membership_asserted(&change_id, &right.revision_id, [0x54; 32])
                .map_err(|error| error.to_string())?,
            "change-fixture:membership:right",
            "2026-08-10T04:00:04Z",
        )?;

        let (proposals, memberships, relations, removal, topology, expected_outcome) = match kind {
            QualificationDerivedChangeFixtureKindV1::DuplicateEqual => {
                let duplicate = fixture_revision_proposal("left", 'a', Some("fixture-left"), 1)?.1;
                (
                    vec![left_proposal, duplicate],
                    vec![left_membership],
                    Vec::new(),
                    None,
                    fixture_topology(
                        change_id,
                        ChangeTopologyV1::Initial,
                        ChangeLifecycleV1::InProgress,
                        vec![left],
                    )?,
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                )
            }
            QualificationDerivedChangeFixtureKindV1::DuplicateConflicting => {
                let conflict = fixture_revision_proposal("left", 'a', None, 1)?.1;
                (
                    vec![left_proposal, conflict],
                    vec![left_membership],
                    Vec::new(),
                    None,
                    fixture_topology(
                        change_id,
                        ChangeTopologyV1::Initial,
                        ChangeLifecycleV1::InProgress,
                        vec![left],
                    )?,
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                )
            }
            QualificationDerivedChangeFixtureKindV1::OperativeRemoval => {
                let removal = ShoreEvent::new(
                    EventType::ArtifactRemoved,
                    ArtifactRemovedPayload::idempotency_key(&left.object_artifact_content_hash),
                    EventTarget::for_journal(JournalId::new(
                        "journal:qualification-change-fixture",
                    )),
                    Writer::shore_local("qualification-fixture"),
                    ArtifactRemovedPayload {
                        content_hash: left.object_artifact_content_hash.clone(),
                    },
                    "2026-08-10T04:00:05Z",
                )
                .map_err(|error| error.to_string())?;
                (
                    vec![left_proposal],
                    vec![left_membership],
                    Vec::new(),
                    Some(removal),
                    fixture_topology(
                        change_id,
                        ChangeTopologyV1::Initial,
                        ChangeLifecycleV1::InProgress,
                        vec![left],
                    )?,
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                )
            }
            QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier
            | QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier
            | QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier => {
                let expected = if kind
                    == QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier
                {
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionRebuildRequired
                } else {
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid
                };
                (
                    vec![left_proposal],
                    vec![left_membership],
                    Vec::new(),
                    None,
                    fixture_topology(
                        change_id,
                        ChangeTopologyV1::Initial,
                        ChangeLifecycleV1::InProgress,
                        vec![left],
                    )?,
                    expected,
                )
            }
            QualificationDerivedChangeFixtureKindV1::IncompleteChange => (
                Vec::new(),
                vec![left_membership],
                Vec::new(),
                None,
                fixture_topology(
                    change_id,
                    ChangeTopologyV1::Incomplete,
                    ChangeLifecycleV1::Incomplete,
                    Vec::new(),
                )?,
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
            ),
            QualificationDerivedChangeFixtureKindV1::CycleConflictedChange => {
                let left_to_right = fixture_event(
                    build_revision_relation_asserted(
                        &change_id,
                        left.clone(),
                        right.clone(),
                        [0x55; 32],
                    )
                    .map_err(|error| error.to_string())?,
                    "change-fixture:relation:left-right",
                    "2026-08-10T04:00:05Z",
                )?;
                let right_to_left = fixture_event(
                    build_revision_relation_asserted(
                        &change_id,
                        right.clone(),
                        left.clone(),
                        [0x56; 32],
                    )
                    .map_err(|error| error.to_string())?,
                    "change-fixture:relation:right-left",
                    "2026-08-10T04:00:06Z",
                )?;
                (
                    vec![left_proposal, right_proposal],
                    vec![left_membership, right_membership],
                    vec![left_to_right, right_to_left],
                    None,
                    fixture_topology(
                        change_id,
                        ChangeTopologyV1::CycleConflicted,
                        ChangeLifecycleV1::Conflicted,
                        Vec::new(),
                    )?,
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                )
            }
        };
        let signature = removal
            .as_ref()
            .map(fixture_detached_signature)
            .transpose()?;
        let (storage_probe_revision, storage_probe_proposal) = fixture_revision_proposal(
            "storage-probe",
            'e',
            Some(QUALIFICATION_DERIVED_CHANGE_STORAGE_SUMMARY_PROBE_V1),
            7,
        )?;
        let storage_probe_body = QUALIFICATION_DERIVED_CHANGE_STORAGE_PROSE_PROBE_V1.to_owned();
        let storage_probe_observation = ShoreEvent::new(
            EventType::ReviewObservationRecorded,
            ReviewObservationRecordedPayload::idempotency_key(
                &storage_probe_revision.revision_id,
                &TrackId::new("agent:qualification-storage-probe"),
                "storage-probe-v1",
            ),
            EventTarget::for_revision(
                JournalId::new("journal:qualification-change-fixture"),
                storage_probe_revision.revision_id.clone(),
                Some(TrackId::new("agent:qualification-storage-probe")),
            )
            .map_err(|error| error.to_string())?,
            Writer::shore_local("qualification-fixture"),
            ReviewObservationRecordedPayload {
                observation_id: ObservationId::new(
                    "obs:sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                ),
                target: ReviewTargetRef::Revision {
                    revision_id: storage_probe_revision.revision_id,
                },
                title: "Qualification storage probe".to_owned(),
                body: Some(storage_probe_body.clone()),
                body_content_type: BodyContentType::TextPlain,
                body_artifact_path: None,
                body_byte_size: Some(storage_probe_body.len() as u64),
                body_content_hash: Some(format!(
                    "sha256:{}",
                    sha256_bytes_hex(storage_probe_body.as_bytes())
                )),
                tags: Vec::new(),
                confidence: None,
                supersedes_observation_ids: Vec::new(),
                responds_to_observation_ids: Vec::new(),
            },
            "2026-08-10T04:00:07Z",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            declaration,
            proposals,
            memberships,
            relations,
            removal,
            signature,
            storage_probe_events: vec![storage_probe_proposal, storage_probe_observation],
            topology,
            expected_outcome,
        })
    }
}

#[cfg(feature = "longitudinal-counting")]
fn fixture_detached_signature(target: &ShoreEvent) -> Result<ShoreEvent, String> {
    let signing_key = SigningKey::from_bytes(&[0x57; 32]);
    let signer_id = SignerId::from_ed25519_public_key(signing_key.verifying_key().to_bytes());
    let to_be_signed =
        EventToBeSigned::from_event(target, &signer_id).map_err(|error| error.to_string())?;
    let message = event_signature_pre_authentication_encoding(&to_be_signed)
        .map_err(|error| error.to_string())?;
    let signature = EventSignatureBytes::from_bytes(&signing_key.sign(&message).to_bytes());
    let payload = EventSignatureRecordedPayload {
        target_event_id: target.event_id.clone(),
        target_event_record_hash: target
            .event_record_hash()
            .map_err(|error| error.to_string())?,
        attesting_signer: signer_id,
        attestation: EventSignature::ed25519_v1(signature),
        inclusion_proof: None,
    };
    ShoreEvent::new(
        EventType::EventSignatureRecorded,
        EventSignatureRecordedPayload::idempotency_key(
            &payload.target_event_record_hash,
            &payload.attesting_signer,
            payload.attestation.sig.as_str(),
        ),
        EventTarget::for_journal(JournalId::new("journal:qualification-change-fixture")),
        Writer::shore_local("qualification-fixture"),
        payload,
        "2026-08-10T04:00:06Z",
    )
    .map_err(|error| error.to_string())
}

#[cfg(feature = "longitudinal-counting")]
fn fixture_revision_proposal(
    name: &str,
    marker: char,
    summary: Option<&str>,
    duplicate: usize,
) -> Result<(RevisionRefV1, ShoreEvent), String> {
    let revision_id = RevisionId::new(format!("rev:sha256:{}", marker.to_string().repeat(64)));
    let artifact = format!("sha256:{}", marker.to_string().repeat(64));
    let exact = RevisionRefV1::new(revision_id.clone(), artifact.clone())
        .map_err(|error| error.to_string())?;
    let event = ShoreEvent::new(
        EventType::WorkObjectProposed,
        format!("change-fixture:proposal:{name}:{duplicate}"),
        EventTarget::for_revision(
            JournalId::new("journal:qualification-change-fixture"),
            revision_id.clone(),
            None,
        )
        .map_err(|error| error.to_string())?,
        Writer::shore_local("qualification-fixture"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(format!(
                "engagement:sha256:{}",
                marker.to_string().repeat(64)
            )),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id,
                    object_id: ObjectId::new(format!(
                        "obj:sha256:{}",
                        marker.to_string().repeat(64)
                    )),
                    git_provenance: None,
                },
                summary: summary.map(str::to_owned),
                object_artifact_content_hash: artifact,
                supersedes: Vec::new(),
            },
        },
        format!("2026-08-10T04:00:{duplicate:02}Z"),
    )
    .map_err(|error| error.to_string())?;
    Ok((exact, event))
}

#[cfg(feature = "longitudinal-counting")]
fn fixture_event<P: crate::session::event::EventPayload>(
    payload: P,
    idempotency_key: &str,
    occurred_at: &str,
) -> Result<ShoreEvent, String> {
    ShoreEvent::new(
        payload.event_type(),
        idempotency_key,
        EventTarget::for_journal(JournalId::new("journal:qualification-change-fixture")),
        Writer::shore_local("qualification-fixture"),
        payload,
        occurred_at,
    )
    .map_err(|error| error.to_string())
}

#[cfg(feature = "longitudinal-counting")]
fn fixture_topology(
    change_id: ChangeId,
    expected_topology: ChangeTopologyV1,
    expected_lifecycle: ChangeLifecycleV1,
    current_revisions: Vec<RevisionRefV1>,
) -> Result<QualificationDerivedChangeFixtureTopologyV1, String> {
    Ok(QualificationDerivedChangeFixtureTopologyV1 {
        change_id_sha256: sha256_bytes_hex(change_id.as_str().as_bytes()),
        expected_topology,
        expected_lifecycle,
        current_revision_ref_sha256: current_revisions
            .iter()
            .map(canonical_sha256)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

#[cfg(feature = "longitudinal-counting")]
fn record_fixture_event(store: &EventStore, event: ShoreEvent) -> Result<(), String> {
    match store
        .record_event_once(&event)
        .map_err(|error| error.to_string())?
    {
        EventWriteOutcome::Created => Ok(()),
        EventWriteOutcome::Existing | EventWriteOutcome::ExistingDivergentSignature => {
            Err("public Change fixture generated a duplicate idempotency key".to_owned())
        }
    }
}

#[cfg(feature = "longitudinal-counting")]
fn carrier_witness(
    event: &ShoreEvent,
    role: QualificationDerivedChangeFixtureCarrierRoleV1,
    state: QualificationDerivedChangeFixtureCarrierStateV1,
) -> Result<QualificationDerivedChangeFixtureCarrierV1, String> {
    Ok(QualificationDerivedChangeFixtureCarrierV1 {
        role,
        state,
        idempotency_key_sha256: sha256_bytes_hex(event.idempotency_key.as_bytes()),
        payload_sha256: event.payload_hash.clone(),
        event_record_sha256: event
            .event_record_hash()
            .map_err(|error| error.to_string())?,
    })
}

fn is_sha256_prefixed(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(is_sha256_unprefixed)
}

fn is_sha256_unprefixed(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(feature = "longitudinal-counting")]
fn validate_change_fixture_request(
    request: &QualificationDerivedChangeFixtureRequestV1,
) -> Result<(), String> {
    if !request.root.is_absolute()
        || !request.source_checkout.is_absolute()
        || request.root == request.source_checkout
        || request.root.starts_with(&request.source_checkout)
        || request.source_checkout.starts_with(&request.root)
        || !request.source_checkout.join("Cargo.toml").is_file()
    {
        return Err("invalid derived Change fixture request".to_owned());
    }
    Ok(())
}

#[cfg(feature = "longitudinal-counting")]
fn initialize_disposable_change_fixture_root(root: &Path) -> Result<(), String> {
    if root.exists() {
        if root
            .read_dir()
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("derived Change fixture root must be absent or empty".to_owned());
        }
    } else {
        std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    }
    run_git(root, &["init", "--quiet"])?;
    run_git(root, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    run_git(root, &["config", "user.name", "Pointbreak Matrix"])?;
    run_git(
        root,
        &["config", "user.email", "pointbreak-matrix@example.com"],
    )?;
    run_git(root, &["config", "commit.gpgsign", "false"])?;
    std::fs::create_dir_all(root.join(".git/pointbreak-home"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "longitudinal-counting")]
fn run_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(())
}

#[cfg(feature = "longitudinal-counting")]
fn copy_change_ready_fixture_records(
    source_checkout: &Path,
    store_root: &Path,
) -> Result<(), String> {
    let source = source_checkout.join("tests/support/assets/change-ready-store");
    let destination = store_root.join("events");
    std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    for fixture in [
        QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1,
        QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_V1,
    ] {
        let source_path = source.join(fixture);
        let bytes = std::fs::read(&source_path).map_err(|error| error.to_string())?;
        let expected_sha256 = if fixture == QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_V1 {
            QUALIFICATION_DERIVED_CHANGE_ACTIVATION_FIXTURE_SHA256_V1
        } else {
            QUALIFICATION_DERIVED_CHANGE_COMPLETION_FIXTURE_SHA256_V1
        };
        if sha256_bytes_hex(&bytes) != expected_sha256 {
            return Err("derived Change activation fixture bytes drifted".to_owned());
        }
        std::fs::write(destination.join(fixture), bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
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

#[cfg(all(test, feature = "longitudinal-counting"))]
mod change_fixture_tests {
    use super::*;
    use crate::session::{
        DerivedChangeAccess, DerivedChangeOutcomeV1, DerivedChangePageRequestV1,
        DerivedProjectionFailureCodeV1,
    };

    fn assert_declared_outcome<T: std::fmt::Debug>(
        kind: QualificationDerivedChangeFixtureKindV1,
        expected: QualificationDerivedChangeFixtureExpectedOutcomeV1,
        observed: DerivedChangeOutcomeV1<T>,
    ) {
        match (expected, observed) {
            (
                QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                DerivedChangeOutcomeV1::Ready(_),
            ) => {}
            (
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                DerivedChangeOutcomeV1::ProjectionUnavailable(document),
            ) => assert_eq!(
                document.code(),
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "fixture {kind:?} typed code drifted"
            ),
            (
                QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionRebuildRequired,
                DerivedChangeOutcomeV1::ProjectionUnavailable(document),
            ) => assert_eq!(
                document.code(),
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                "fixture {kind:?} typed code drifted"
            ),
            (expected, observed) => {
                panic!("fixture {kind:?} outcome drifted: expected {expected:?}, got {observed:?}")
            }
        }
    }

    #[test]
    fn change_fixture_witnesses_are_deterministic_and_bind_public_authority() {
        for kind in QualificationDerivedChangeFixtureKindV1::ALL {
            let roots = tempfile::tempdir().expect("fixture parent");
            let first = materialize_qualification_derived_change_fixture_v1(
                QualificationDerivedChangeFixtureRequestV1::new(roots.path().join("first"), kind),
            )
            .expect("materialize first fixture");
            let second = materialize_qualification_derived_change_fixture_v1(
                QualificationDerivedChangeFixtureRequestV1::new(roots.path().join("second"), kind),
            )
            .expect("materialize second fixture");

            first.validate().expect("validate first fixture witness");
            second.validate().expect("validate second fixture witness");
            assert_eq!(
                first, second,
                "fixture witness must not depend on root path"
            );
            assert_eq!(first.fixture_id, kind.fixture_id());
            assert!(
                !first
                    .authoritative_inventory_sha256
                    .contains(roots.path().to_str().unwrap())
            );
        }
    }

    #[test]
    fn change_fixture_witnesses_name_duplicate_removal_and_fault_carriers_without_prose() {
        let parent = tempfile::tempdir().expect("fixture parent");
        let equal = materialize_qualification_derived_change_fixture_v1(
            QualificationDerivedChangeFixtureRequestV1::new(
                parent.path().join("equal"),
                QualificationDerivedChangeFixtureKindV1::DuplicateEqual,
            ),
        )
        .expect("materialize equal fixture");
        assert_eq!(
            equal
                .carriers
                .iter()
                .map(|carrier| carrier.role)
                .collect::<Vec<_>>(),
            vec![
                QualificationDerivedChangeFixtureCarrierRoleV1::Primary,
                QualificationDerivedChangeFixtureCarrierRoleV1::EqualDuplicate,
            ]
        );

        let removal = materialize_qualification_derived_change_fixture_v1(
            QualificationDerivedChangeFixtureRequestV1::new(
                parent.path().join("removal"),
                QualificationDerivedChangeFixtureKindV1::OperativeRemoval,
            ),
        )
        .expect("materialize removal fixture");
        assert!(removal.carriers.iter().any(|carrier| {
            carrier.role == QualificationDerivedChangeFixtureCarrierRoleV1::RemovalSupport
        }));
        assert!(removal.carriers.iter().any(|carrier| {
            carrier.role == QualificationDerivedChangeFixtureCarrierRoleV1::SignatureSupport
        }));

        for kind in [
            QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier,
            QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier,
            QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier,
        ] {
            let witness = materialize_qualification_derived_change_fixture_v1(
                QualificationDerivedChangeFixtureRequestV1::new(
                    parent.path().join(kind.fixture_id()),
                    kind,
                ),
            )
            .expect("materialize fault fixture");
            assert_eq!(
                witness.expected_outcome,
                if kind == QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier {
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionRebuildRequired
                } else {
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid
                }
            );
            assert_eq!(
                witness.carriers[0].state,
                match kind {
                    QualificationDerivedChangeFixtureKindV1::MissingSelectedCarrier => {
                        QualificationDerivedChangeFixtureCarrierStateV1::Missing
                    }
                    QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier => {
                        QualificationDerivedChangeFixtureCarrierStateV1::Mutated
                    }
                    QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier => {
                        QualificationDerivedChangeFixtureCarrierStateV1::WrongFamily
                    }
                    _ => unreachable!(),
                }
            );
            let json = serde_json::to_string(&witness).expect("serialize witness");
            assert!(!json.contains("fixture-left"));
            assert!(!json.contains("\"payload\":"));
            assert!(!json.contains("rev:sha256:"));
            assert!(!json.contains(parent.path().to_str().unwrap()));
        }
    }

    #[test]
    fn incomplete_and_cyclic_fixtures_have_their_declared_change_shapes() {
        let parent = tempfile::tempdir().expect("fixture parent");
        let incomplete = materialize_qualification_derived_change_fixture_v1(
            QualificationDerivedChangeFixtureRequestV1::new(
                parent.path().join("incomplete"),
                QualificationDerivedChangeFixtureKindV1::IncompleteChange,
            ),
        )
        .expect("materialize incomplete fixture");
        assert_eq!(
            incomplete.topology.expected_topology,
            ChangeTopologyV1::Incomplete
        );
        assert!(incomplete.topology.current_revision_ref_sha256.is_empty());

        let cyclic = materialize_qualification_derived_change_fixture_v1(
            QualificationDerivedChangeFixtureRequestV1::new(
                parent.path().join("cyclic"),
                QualificationDerivedChangeFixtureKindV1::CycleConflictedChange,
            ),
        )
        .expect("materialize cyclic fixture");
        assert_eq!(
            cyclic.topology.expected_topology,
            ChangeTopologyV1::CycleConflicted
        );
        assert_eq!(
            cyclic.topology.expected_lifecycle,
            ChangeLifecycleV1::Conflicted
        );
    }

    #[test]
    fn change_fixtures_exercise_their_declared_derived_outcomes() {
        let parent = tempfile::tempdir().expect("fixture parent");
        for kind in QualificationDerivedChangeFixtureKindV1::ALL {
            let root = parent.path().join(kind.fixture_id());
            let witness = materialize_qualification_derived_change_fixture_v1(
                QualificationDerivedChangeFixtureRequestV1::new(&root, kind),
            )
            .expect("materialize Change fixture");
            let access = DerivedChangeAccess::resolve_for_inspector(&root)
                .expect("resolve fixture derived access");
            assert_declared_outcome(
                kind,
                if matches!(
                    kind,
                    QualificationDerivedChangeFixtureKindV1::DuplicateConflicting
                        | QualificationDerivedChangeFixtureKindV1::MutatedSelectedCarrier
                        | QualificationDerivedChangeFixtureKindV1::WrongFamilySelectedCarrier
                ) {
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready
                } else {
                    witness.expected_outcome
                },
                access.profile().expect("read fixture Profile"),
            );
            assert_declared_outcome(
                kind,
                witness.expected_outcome,
                access
                    .attention(&DerivedChangePageRequestV1::Bare)
                    .expect("read fixture Attention"),
            );
            let outcome = access
                .changes(&DerivedChangePageRequestV1::Bare)
                .expect("read fixture Changes");
            match (witness.expected_outcome, outcome) {
                (
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::Ready,
                    DerivedChangeOutcomeV1::Ready(page),
                ) => {
                    let value = serde_json::to_value(page.document)
                        .expect("serialize fixture Change document");
                    let change = value["changes"]
                        .as_array()
                        .and_then(|changes| changes.first())
                        .expect("fixture emits one Change");
                    assert_eq!(
                        sha256_bytes_hex(
                            change["changeId"]
                                .as_str()
                                .expect("fixture Change id")
                                .as_bytes()
                        ),
                        witness.topology.change_id_sha256
                    );
                    assert_eq!(
                        change["topology"],
                        serde_json::to_value(witness.topology.expected_topology)
                            .expect("serialize expected topology")
                    );
                    assert_eq!(
                        change["lifecycle"],
                        serde_json::to_value(witness.topology.expected_lifecycle)
                            .expect("serialize expected lifecycle")
                    );
                    let observed_current = change["currentRevisionRefs"]
                        .as_array()
                        .expect("fixture current exact Revisions")
                        .iter()
                        .map(canonical_sha256)
                        .collect::<Result<std::collections::BTreeSet<_>, _>>()
                        .expect("hash fixture current exact Revisions");
                    assert_eq!(
                        observed_current,
                        witness
                            .topology
                            .current_revision_ref_sha256
                            .iter()
                            .cloned()
                            .collect()
                    );
                }
                (
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionInvalid,
                    DerivedChangeOutcomeV1::ProjectionUnavailable(document),
                ) => assert_eq!(
                    document.code(),
                    DerivedProjectionFailureCodeV1::ProjectionInvalid,
                    "fixture {kind:?} typed code drifted"
                ),
                (
                    QualificationDerivedChangeFixtureExpectedOutcomeV1::ProjectionRebuildRequired,
                    DerivedChangeOutcomeV1::ProjectionUnavailable(document),
                ) => assert_eq!(
                    document.code(),
                    DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                    "fixture {kind:?} typed code drifted"
                ),
                (expected, observed) => {
                    panic!(
                        "fixture {kind:?} outcome drifted: expected {expected:?}, got {observed:?}"
                    )
                }
            }
        }
    }
}
