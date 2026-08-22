use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex, sha256_json_prefixed};
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::model::{
    ActorId, ChangeId, ChangeIdentityDescriptorV1, JournalId, RevisionId, RevisionRefV1,
    current_revisions, replacement_heads_diverge, revision_graph_has_cycle,
};
use crate::session::event::{
    ChangeMembershipAssertedPayload, ChangeRevisionRelationAssertedPayload, EventPayload,
    EventTarget, EventType, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    WriterProducer, build_change_declared, build_membership_asserted,
    build_revision_relation_asserted,
};
use crate::session::store::authority_lock::{STORE_AUTHORITY_LOCK_FILE, StoreAuthorityLock};
#[cfg(any(test, feature = "bench"))]
use crate::session::store::capabilities::route_journal_entries;
use crate::session::store::capabilities::{
    BulkAdoptionCompletionV1, REVIEW_CHANGE_REVISION_COHORT_V1, StoreCapabilityActivationV1,
    build_signed_activation, build_signed_completion, inspect_journal_records,
    publish_control_record,
};
use crate::session::store::resolution::resolve_change_read_store;
use crate::session::store::{BulkAdoptionManifestV1, ReservedCohortRecordV1};
use crate::session::{AuthorityCursorV2, StoreCapabilityStatus, StoreIdentityOptions};
#[cfg(any(test, feature = "bench"))]
use crate::session::{StoreCapabilityInspection, store_capability_for_repo};
use crate::storage::{CreateOutcome, Durability, LocalStorage};

pub const BULK_ADOPTION_DRY_RUN_SCHEMA_V1: &str = "pointbreak.bulk-adoption-dry-run.v1";
pub const BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.bulk-adoption-migration-receipt.v1";
pub const BULK_ADOPTION_MINIMUM_READER_PROFILE_V1: &str = REVIEW_CHANGE_REVISION_COHORT_V1;
pub const BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1: &str = "backup-manifest.json";
pub const BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1: &str = "backup-receipt.json";
pub const BULK_ADOPTION_EXECUTION_PLAN_FILE_V1: &str = "migration-plan.json";
const BULK_ADOPTION_MIGRATION_RECEIPT_FILE_V1: &str = "migration-receipt.json";

const BULK_ADOPTION_BACKUP_MANIFEST_SCHEMA_V1: &str = "pointbreak.bulk-adoption-backup-manifest.v1";
const BULK_ADOPTION_BACKUP_RECEIPT_SCHEMA_V1: &str = "pointbreak.bulk-adoption-backup-receipt.v1";
const BULK_ADOPTION_EXECUTION_PLAN_SCHEMA_V1: &str = "pointbreak.bulk-adoption-execution-plan.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BulkAdoptionDryRunOptions {
    roots: Vec<PathBuf>,
    actor_id: ActorId,
    owner_decisions: Option<BulkAdoptionOwnerDecisionManifestV1>,
}

impl BulkAdoptionDryRunOptions {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            roots: vec![root.as_ref().to_path_buf()],
            actor_id: ActorId::new("actor:bulk-adoption-dry-run"),
            owner_decisions: None,
        }
    }

    pub fn with_root(mut self, root: impl AsRef<Path>) -> Self {
        self.roots.push(root.as_ref().to_path_buf());
        self
    }

    pub fn with_actor_id(mut self, actor_id: ActorId) -> Self {
        self.actor_id = actor_id;
        self
    }

    pub fn with_owner_decisions(mut self, decisions: BulkAdoptionOwnerDecisionManifestV1) -> Self {
        self.owner_decisions = Some(decisions);
        self
    }
}

pub const BULK_ADOPTION_OWNER_DECISIONS_SCHEMA_V1: &str =
    "pointbreak.bulk-adoption-owner-decisions.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum BulkAdoptionOverlapIdentityDecisionV1 {
    Shared {
        identity_descriptor: ChangeIdentityDescriptorV1,
    },
    Distinct,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionRetainedAllocationV1 {
    pub change_id: ChangeId,
    pub identity_descriptor: ChangeIdentityDescriptorV1,
    pub members: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionRetainedManifestV1 {
    pub manifest_hash: String,
    pub allocations: Vec<BulkAdoptionRetainedAllocationV1>,
}

/// Owner-authored amendment to a dry-run proposal. It carries identities and
/// anomaly acknowledgements only—never paths or corpus bytes—and is itself
/// hashed into the amended dry-run document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionOwnerDecisionManifestV1 {
    pub schema: String,
    #[serde(default)]
    pub approved_anomaly_ids: BTreeSet<String>,
    #[serde(default)]
    pub overlap_identity: BTreeMap<RevisionId, BulkAdoptionOverlapIdentityDecisionV1>,
    #[serde(default)]
    pub retained_manifests: Vec<BulkAdoptionRetainedManifestV1>,
    pub claim_occurred_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionDryRunRootV1 {
    pub root_identity_hash: String,
    pub source_authority_cursor: AuthorityCursorV2,
    pub revision_count: usize,
    pub proposed_changes: Vec<BulkAdoptionDryRunChangeV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohort_manifest_hash: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionDryRunChangeV1 {
    pub change_id: ChangeId,
    pub members: BTreeSet<RevisionId>,
    pub admitted_relations: BTreeSet<(RevisionId, RevisionId)>,
    pub membership_claim_ids: BTreeSet<String>,
    pub relation_claim_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionDryRunAnomalyV1 {
    pub anomaly_id: String,
    pub code: String,
    pub root_identity_hashes: BTreeSet<String>,
    pub revision_ids: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionDryRunDocumentV1 {
    pub schema: String,
    pub manifest_hash: String,
    pub roots: Vec<BulkAdoptionDryRunRootV1>,
    pub anomalies: Vec<BulkAdoptionDryRunAnomalyV1>,
    pub requires_owner_decision: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_decision_manifest_hash: Option<String>,
    pub writer: Writer,
    pub claim_occurred_at: String,
    pub signature_policy: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRevisionV1 {
    revision: RevisionRefV1,
    legacy_group: String,
    supersedes: Vec<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRootInventoryV1 {
    root_identity_hash: String,
    journal_id: JournalId,
    source_authority_cursor: AuthorityCursorV2,
    revisions: Vec<LegacyRevisionV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum OverlapIdentityDecisionV1 {
    Shared {
        identity_descriptor: ChangeIdentityDescriptorV1,
    },
    Distinct,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedChangeAllocationV1 {
    change_id: ChangeId,
    identity_descriptor: ChangeIdentityDescriptorV1,
    members: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedAdoptionManifestV1 {
    manifest_hash: String,
    allocations: Vec<RetainedChangeAllocationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnerManifestDecisionsV1 {
    approved_anomaly_ids: BTreeSet<String>,
    overlap_identity: BTreeMap<RevisionId, OverlapIdentityDecisionV1>,
    writer: Writer,
    occurred_at: String,
}

impl Default for OwnerManifestDecisionsV1 {
    fn default() -> Self {
        Self {
            approved_anomaly_ids: BTreeSet::new(),
            overlap_identity: BTreeMap::new(),
            writer: Writer::shore_local("bulk-adoption-proposal-v1"),
            occurred_at: "1970-01-01T00:00:00Z".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationAnomalyV1 {
    anomaly_id: String,
    code: String,
    root_identity_hashes: BTreeSet<String>,
    revision_ids: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdoptionGroupProposalV1 {
    change_id: ChangeId,
    members: BTreeSet<RevisionId>,
    admitted_relations: BTreeSet<(RevisionId, RevisionId)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RootAdoptionProposalV1 {
    root_identity_hash: String,
    covered_revisions: BTreeSet<RevisionId>,
    groups: Vec<AdoptionGroupProposalV1>,
    planned_events: Vec<ShoreEvent>,
    manifest: Option<BulkAdoptionManifestV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BulkAdoptionProposalV1 {
    roots: Vec<RootAdoptionProposalV1>,
    anomalies: Vec<MigrationAnomalyV1>,
    requires_owner_decision: bool,
}

/// Produce a path-private, deterministic migration proposal without mutating
/// any supplied root. Every root must still be L0; M1/L2 input is refused
/// before the planner reads legacy Revision payloads.
pub fn dry_run_bulk_adoption(
    options: BulkAdoptionDryRunOptions,
) -> Result<BulkAdoptionDryRunDocumentV1> {
    if options.roots.is_empty() {
        return Err(invalid_migration("at least one L0 root is required"));
    }
    let mut inventories = options
        .roots
        .iter()
        .map(|root| legacy_inventory_for_repo(root))
        .collect::<Result<Vec<_>>>()?;
    inventories.sort_by(|left, right| left.root_identity_hash.cmp(&right.root_identity_hash));
    if inventories
        .windows(2)
        .any(|pair| pair[0].root_identity_hash == pair[1].root_identity_hash)
    {
        return Err(invalid_migration(
            "the same resolved store was supplied more than once",
        ));
    }
    let writer = Writer {
        actor_id: options.actor_id,
        producer: WriterProducer {
            name: "pointbreak".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    };
    let owner_decision_manifest_hash = options
        .owner_decisions
        .as_ref()
        .map(validate_owner_decisions)
        .transpose()?;
    let retained_manifests = options
        .owner_decisions
        .as_ref()
        .map(|manifest| {
            manifest
                .retained_manifests
                .iter()
                .map(|retained| RetainedAdoptionManifestV1 {
                    manifest_hash: retained.manifest_hash.clone(),
                    allocations: retained
                        .allocations
                        .iter()
                        .map(|allocation| RetainedChangeAllocationV1 {
                            change_id: allocation.change_id.clone(),
                            identity_descriptor: allocation.identity_descriptor.clone(),
                            members: allocation.members.clone(),
                        })
                        .collect(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let decisions = options
        .owner_decisions
        .as_ref()
        .map(|manifest| OwnerManifestDecisionsV1 {
            approved_anomaly_ids: manifest.approved_anomaly_ids.clone(),
            overlap_identity: manifest
                .overlap_identity
                .iter()
                .map(|(revision, decision)| {
                    let decision = match decision {
                        BulkAdoptionOverlapIdentityDecisionV1::Shared {
                            identity_descriptor,
                        } => OverlapIdentityDecisionV1::Shared {
                            identity_descriptor: identity_descriptor.clone(),
                        },
                        BulkAdoptionOverlapIdentityDecisionV1::Distinct => {
                            OverlapIdentityDecisionV1::Distinct
                        }
                    };
                    (revision.clone(), decision)
                })
                .collect(),
            writer: writer.clone(),
            occurred_at: manifest.claim_occurred_at.clone(),
        })
        .unwrap_or_else(|| OwnerManifestDecisionsV1 {
            writer: writer.clone(),
            ..OwnerManifestDecisionsV1::default()
        });
    let proposal = plan_bulk_adoption(&inventories, &retained_manifests, &decisions)?;
    let mut roots = Vec::with_capacity(proposal.roots.len());
    for root in &proposal.roots {
        let inventory = inventories
            .iter()
            .find(|inventory| inventory.root_identity_hash == root.root_identity_hash)
            .expect("proposal root came from the supplied inventory");
        let mut proposed_changes = Vec::with_capacity(root.groups.len());
        for group in &root.groups {
            let mut membership_claim_ids = BTreeSet::new();
            let mut relation_claim_ids = BTreeSet::new();
            for event in &root.planned_events {
                match event.event_type {
                    EventType::ChangeMembershipAsserted => {
                        let payload: ChangeMembershipAssertedPayload =
                            serde_json::from_value(event.payload.clone())?;
                        if payload.change_id == group.change_id
                            && group.members.contains(&payload.revision_id)
                        {
                            membership_claim_ids
                                .insert(payload.membership_claim_id.as_str().to_owned());
                        }
                    }
                    EventType::ChangeRevisionRelationAsserted => {
                        let payload: ChangeRevisionRelationAssertedPayload =
                            serde_json::from_value(event.payload.clone())?;
                        if payload.change_id == group.change_id {
                            relation_claim_ids
                                .insert(payload.relation_claim_id.as_str().to_owned());
                        }
                    }
                    _ => {}
                }
            }
            proposed_changes.push(BulkAdoptionDryRunChangeV1 {
                change_id: group.change_id.clone(),
                members: group.members.clone(),
                admitted_relations: group.admitted_relations.clone(),
                membership_claim_ids,
                relation_claim_ids,
            });
        }
        proposed_changes.sort_by(|left, right| left.change_id.cmp(&right.change_id));
        roots.push(BulkAdoptionDryRunRootV1 {
            root_identity_hash: root.root_identity_hash.clone(),
            source_authority_cursor: inventory.source_authority_cursor.clone(),
            revision_count: root.covered_revisions.len(),
            proposed_changes,
            cohort_manifest_hash: root
                .manifest
                .as_ref()
                .map(BulkAdoptionManifestV1::canonical_hash)
                .transpose()?,
        });
    }
    let anomalies = proposal
        .anomalies
        .into_iter()
        .map(|anomaly| BulkAdoptionDryRunAnomalyV1 {
            anomaly_id: anomaly.anomaly_id,
            code: anomaly.code,
            root_identity_hashes: anomaly.root_identity_hashes,
            revision_ids: anomaly.revision_ids,
        })
        .collect::<Vec<_>>();
    let material = serde_json::json!({
        "schema": BULK_ADOPTION_DRY_RUN_SCHEMA_V1,
        "roots": &roots,
        "anomalies": &anomalies,
        "requiresOwnerDecision": proposal.requires_owner_decision,
        "ownerDecisionManifestHash": &owner_decision_manifest_hash,
        "writer": &writer,
        "claimOccurredAt": &decisions.occurred_at,
        "signaturePolicy": "unsigned_change_claims_v1",
    });
    Ok(BulkAdoptionDryRunDocumentV1 {
        schema: BULK_ADOPTION_DRY_RUN_SCHEMA_V1.to_owned(),
        manifest_hash: sha256_json_prefixed(&material)?,
        roots,
        anomalies,
        requires_owner_decision: proposal.requires_owner_decision,
        owner_decision_manifest_hash,
        writer,
        claim_occurred_at: decisions.occurred_at,
        signature_policy: "unsigned_change_claims_v1".to_owned(),
    })
}

fn validate_owner_decisions(manifest: &BulkAdoptionOwnerDecisionManifestV1) -> Result<String> {
    if manifest.schema != BULK_ADOPTION_OWNER_DECISIONS_SCHEMA_V1
        || manifest.claim_occurred_at.trim().is_empty()
        || manifest
            .approved_anomaly_ids
            .iter()
            .any(|identity| !is_prefixed_sha256(identity))
    {
        return Err(invalid_migration(
            "owner decision manifest has an unsupported schema or invalid identity",
        ));
    }
    sha256_json_prefixed(&serde_json::to_value(manifest)?)
}

fn legacy_inventory_for_repo(repo: &Path) -> Result<LegacyRootInventoryV1> {
    let (_store, inspection) = resolve_change_read_store(repo)?;
    if !matches!(inspection.status, StoreCapabilityStatus::MigrationRequired) {
        return Err(invalid_migration(
            "bulk-adoption dry run requires an untouched L0 store",
        ));
    }
    let identity = crate::session::store_identity(StoreIdentityOptions::new(repo))?;
    let events = inspection
        .event_entries
        .into_iter()
        .map(|entry| {
            crate::session::EventStore::decode_qualification_entry(entry.key_digest, entry.bytes)
        })
        .collect::<Result<Vec<_>>>()?;
    let journal_id = events
        .first()
        .map(|event| event.target.journal_id.clone())
        .unwrap_or_else(|| JournalId::new("journal:default"));
    if events
        .iter()
        .any(|event| event.target.journal_id != journal_id)
    {
        return Err(invalid_migration(
            "legacy root contains more than one Journal identity",
        ));
    }
    let mut revisions = Vec::new();
    for event in &events {
        if event.event_type != EventType::WorkObjectProposed {
            continue;
        }
        let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
        if let WorkObjectProposal::Revision {
            revision,
            object_artifact_content_hash,
            supersedes,
            ..
        } = payload.work_object
        {
            revisions.push(LegacyRevisionV1 {
                revision: RevisionRefV1::new(revision.id, object_artifact_content_hash)?,
                legacy_group: payload.engagement_id.as_str().to_owned(),
                supersedes,
            });
        }
    }
    Ok(LegacyRootInventoryV1 {
        root_identity_hash: identity.store_identity,
        journal_id,
        source_authority_cursor: inspection.cursor,
        revisions,
    })
}

fn invalid_migration(reason: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: reason.into(),
    }
}

fn minimum_reader_profile_v1() -> String {
    REVIEW_CHANGE_REVISION_COHORT_V1.to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkAdoptionMigrationDispositionV1 {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionMigrationReceiptV1 {
    pub schema: String,
    pub approved_dry_run_hash: String,
    pub cohort_manifest_hash: String,
    pub minimum_reader_profile: String,
    pub activation_id: String,
    pub completion_id: String,
    pub backup_manifest_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_generation_id: Option<String>,
    pub disposition: BulkAdoptionMigrationDispositionV1,
}

pub const BULK_ADOPTION_BACKUP_RESTORE_RECEIPT_SCHEMA_V1: &str =
    "pointbreak.bulk-adoption-backup-restore-receipt.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkAdoptionBackupRestoreDispositionV1 {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkAdoptionBackupRestoreReceiptV1 {
    pub schema: String,
    pub backup_manifest_hash: String,
    pub restored_store_identity: String,
    pub restored_authority_cursor: AuthorityCursorV2,
    pub file_count: u64,
    pub total_bytes: u64,
    pub disposition: BulkAdoptionBackupRestoreDispositionV1,
}

pub struct BulkAdoptionMigrationOptions {
    repo: PathBuf,
    dry_run: BulkAdoptionDryRunDocumentV1,
    acknowledged_dry_run_hash: String,
    acknowledged_cohort_manifest_hash: String,
    backup_root: PathBuf,
    operation_id: String,
    minimum_reader_ack: Option<String>,
    legacy_reader_unsupported_ack: bool,
    owner_decisions: Option<BulkAdoptionOwnerDecisionManifestV1>,
    signer: Option<Box<dyn EventSigner + Send + Sync>>,
    fixed_occurred_at: Option<String>,
    derived_enabled: bool,
    interruption_after_append: Option<usize>,
}

impl BulkAdoptionMigrationOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        dry_run: BulkAdoptionDryRunDocumentV1,
        acknowledged_dry_run_hash: impl Into<String>,
        acknowledged_cohort_manifest_hash: impl Into<String>,
        backup_root: impl AsRef<Path>,
        operation_id: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            dry_run,
            acknowledged_dry_run_hash: acknowledged_dry_run_hash.into(),
            acknowledged_cohort_manifest_hash: acknowledged_cohort_manifest_hash.into(),
            backup_root: backup_root.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            minimum_reader_ack: None,
            legacy_reader_unsupported_ack: false,
            owner_decisions: None,
            signer: None,
            fixed_occurred_at: None,
            derived_enabled: true,
            interruption_after_append: None,
        }
    }

    pub fn with_minimum_reader_ack(mut self, profile: impl Into<String>) -> Self {
        self.minimum_reader_ack = Some(profile.into());
        self
    }

    pub fn with_legacy_reader_unsupported_ack(mut self) -> Self {
        self.legacy_reader_unsupported_ack = true;
        self
    }

    pub fn with_owner_decisions(mut self, decisions: BulkAdoptionOwnerDecisionManifestV1) -> Self {
        self.owner_decisions = Some(decisions);
        self
    }

    pub fn sign_with<S>(mut self, signer: S) -> Self
    where
        S: EventSigner + Send + Sync + 'static,
    {
        self.signer = Some(Box::new(signer));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_fixed_occurred_at(mut self, occurred_at: impl Into<String>) -> Self {
        self.fixed_occurred_at = Some(occurred_at.into());
        self
    }

    pub fn with_derived_enabled(mut self, enabled: bool) -> Self {
        self.derived_enabled = enabled;
        self
    }

    #[cfg(test)]
    fn with_interruption_after_append(mut self, append: usize) -> Self {
        self.interruption_after_append = Some(append);
        self
    }
}

/// Restore a verified pre-activation backup into one empty, separately
/// identified repository store. This never rolls an activated store backward;
/// the restored L0 root has its own placement identity and must receive a fresh
/// dry run before any later activation.
pub fn restore_bulk_adoption_backup(
    backup_root: impl AsRef<Path>,
    target_repo: impl AsRef<Path>,
) -> Result<BulkAdoptionBackupRestoreReceiptV1> {
    let backup_root = backup_root.as_ref();
    let target_repo = target_repo.as_ref();
    let manifest = verify_backup(backup_root)?;
    let (target, _) = resolve_change_read_store(target_repo)?;
    let target_root = target.store_dir().to_path_buf();
    ensure_external_backup_path(&target_root, backup_root)?;
    let _authority = StoreAuthorityLock::acquire(&target_root)?;
    let (_, initial) = resolve_change_read_store(target_repo)?;
    let target_identity = crate::session::store_identity(StoreIdentityOptions::new(target_repo))?;
    if target_identity.store_identity == manifest.source_store_identity {
        return Err(invalid_migration(
            "backup restore requires a separately identified destination store",
        ));
    }
    if !matches!(initial.status, StoreCapabilityStatus::MigrationRequired) {
        return Err(invalid_migration(
            "backup restore requires an empty untouched L0 destination",
        ));
    }
    let existing = inventory_store_files(&target_root)?;
    let disposition = if existing == manifest.entries {
        BulkAdoptionBackupRestoreDispositionV1::Existing
    } else {
        if !existing.is_empty() {
            return Err(invalid_migration(
                "backup restore destination contains different durable store files",
            ));
        }
        let storage = LocalStorage::new(&target_root);
        for entry in &manifest.entries {
            let source = backup_root.join("store").join(&entry.path);
            let bytes = fs::read(&source).map_err(|error| {
                invalid_migration(format!(
                    "could not read verified backup file {}: {error}",
                    entry.path
                ))
            })?;
            storage.create_file_exclusive(Path::new(&entry.path), &bytes, Durability::Durable)?;
        }
        BulkAdoptionBackupRestoreDispositionV1::Created
    };
    let (_, restored) = resolve_change_read_store(target_repo)?;
    if !matches!(restored.status, StoreCapabilityStatus::MigrationRequired)
        || restored.cursor != manifest.source_authority_cursor
    {
        return Err(invalid_migration(
            "restored backup did not reproduce the verified L0 authority cursor",
        ));
    }
    Ok(BulkAdoptionBackupRestoreReceiptV1 {
        schema: BULK_ADOPTION_BACKUP_RESTORE_RECEIPT_SCHEMA_V1.to_owned(),
        backup_manifest_hash: manifest.manifest_hash,
        restored_store_identity: target_identity.store_identity,
        restored_authority_cursor: restored.cursor,
        file_count: manifest.entries.len() as u64,
        total_bytes: manifest.entries.iter().map(|entry| entry.bytes).sum(),
        disposition,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BulkAdoptionBackupEntryV1 {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BulkAdoptionBackupManifestV1 {
    schema: String,
    source_store_identity: String,
    source_authority_cursor: AuthorityCursorV2,
    entries: Vec<BulkAdoptionBackupEntryV1>,
    manifest_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BulkAdoptionBackupReceiptV1 {
    schema: String,
    source_store_identity: String,
    source_authority_cursor: AuthorityCursorV2,
    manifest_hash: String,
    file_count: u64,
    total_bytes: u64,
}

pub fn migrate_bulk_adoption(
    options: BulkAdoptionMigrationOptions,
) -> Result<BulkAdoptionMigrationReceiptV1> {
    validate_migration_options(&options)?;
    let (resolved, _) = resolve_change_read_store(&options.repo)?;
    let store_root = resolved.store_dir().to_path_buf();
    ensure_external_backup_path(&store_root, &options.backup_root)?;
    let _authority = StoreAuthorityLock::acquire(&store_root)?;

    let (_, initial) = resolve_change_read_store(&options.repo)?;
    let current_identity =
        crate::session::store_identity(StoreIdentityOptions::new(&options.repo))?;
    let approved_root = approved_root_for_identity(&options, &current_identity.store_identity)?;
    if matches!(&initial.status, StoreCapabilityStatus::MigrationRequired) {
        if initial.cursor != approved_root.source_authority_cursor {
            return Err(invalid_migration(
                "current L0 authority does not match the approved dry-run cursor",
            ));
        }
        if !options
            .backup_root
            .join(BULK_ADOPTION_EXECUTION_PLAN_FILE_V1)
            .is_file()
            && options.signer.is_none()
        {
            return Err(invalid_migration(
                "initial activation requires an available strict signing key",
            ));
        }
    }
    let backup = match &initial.status {
        StoreCapabilityStatus::MigrationRequired => ensure_backup(
            &store_root,
            &options.backup_root,
            &approved_root.root_identity_hash,
            &initial.cursor,
        )?,
        StoreCapabilityStatus::MigrationInProgress { .. } | StoreCapabilityStatus::Ready { .. } => {
            verify_backup(&options.backup_root)?
        }
    };
    if backup.source_store_identity != approved_root.root_identity_hash
        || backup.source_authority_cursor != approved_root.source_authority_cursor
    {
        return Err(invalid_migration(
            "verified backup does not match the approved pre-activation authority",
        ));
    }

    let plan_path = options
        .backup_root
        .join(BULK_ADOPTION_EXECUTION_PLAN_FILE_V1);
    let plan = if plan_path.is_file() {
        let plan: BulkAdoptionExecutionPlanV1 =
            serde_json::from_slice(&fs::read(&plan_path).map_err(|error| {
                invalid_migration(format!("could not read retained migration plan: {error}"))
            })?)?;
        validate_retained_plan(&plan, &options, approved_root)?;
        plan
    } else {
        if !matches!(&initial.status, StoreCapabilityStatus::MigrationRequired) {
            return Err(invalid_migration(
                "M1/L2 recovery requires the retained pre-activation execution plan",
            ));
        }
        let signer = options.signer.as_ref().ok_or_else(|| {
            invalid_migration("initial activation requires an available strict signing key")
        })?;
        let plan = prepare_approved_bulk_adoption(&options, approved_root, signer.as_ref())?;
        let bytes = canonical_json_bytes(&serde_json::to_value(&plan)?)?;
        match LocalStorage::new(&options.backup_root).create_file_exclusive(
            Path::new(BULK_ADOPTION_EXECUTION_PLAN_FILE_V1),
            &bytes,
            Durability::Durable,
        )? {
            CreateOutcome::Created => {}
            CreateOutcome::AlreadyExists => {
                let retained = fs::read(&plan_path).map_err(|error| {
                    invalid_migration(format!("could not verify retained migration plan: {error}"))
                })?;
                if retained != bytes {
                    return Err(invalid_migration(
                        "retained migration plan conflicts with the approved execution plan",
                    ));
                }
            }
        }
        plan
    };

    let final_receipt_path = options
        .backup_root
        .join(BULK_ADOPTION_MIGRATION_RECEIPT_FILE_V1);
    if final_receipt_path.is_file() {
        if !matches!(&initial.status, StoreCapabilityStatus::Ready { .. }) {
            return Err(invalid_migration(
                "a retained completion receipt exists but the store is not L2",
            ));
        }
        execute_bulk_adoption_plan(&options.repo, &plan, None)?;
        let mut receipt: BulkAdoptionMigrationReceiptV1 =
            serde_json::from_slice(&fs::read(&final_receipt_path).map_err(|error| {
                invalid_migration(format!(
                    "could not read retained migration receipt: {error}"
                ))
            })?)?;
        validate_migration_receipt(&receipt, &plan, &backup)?;
        receipt.disposition = BulkAdoptionMigrationDispositionV1::Existing;
        return Ok(receipt);
    }

    execute_bulk_adoption_plan(&options.repo, &plan, options.interruption_after_append)?;
    let derived_generation_id = if options.derived_enabled {
        // Publish through the product lifecycle so the descriptor, SQLite
        // database, locator, and store identity form one validated generation.
        // The bodyless Change generator is qualification-only and cannot stand
        // in for this complete post-L2 product generation.
        let access =
            crate::session::derived_access::history::DerivedHistoryAccess::resolve(&options.repo)
                .map_err(ShoreError::Message)?;
        access
            .build(|_| crate::session::derived_access::history::DerivedHistoryControl::Continue)
            .map_err(ShoreError::Message)?
            .generation_id
    } else {
        None
    };
    let receipt = BulkAdoptionMigrationReceiptV1 {
        schema: BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1.to_owned(),
        approved_dry_run_hash: plan.approved_dry_run_hash.clone(),
        cohort_manifest_hash: plan.manifest_hash.clone(),
        minimum_reader_profile: plan.minimum_reader_profile.clone(),
        activation_id: plan.activation.activation_id().to_owned(),
        completion_id: plan.completion.completion_id().to_owned(),
        backup_manifest_hash: backup.manifest_hash,
        derived_generation_id,
        disposition: BulkAdoptionMigrationDispositionV1::Created,
    };
    let receipt_bytes = canonical_json_bytes(&serde_json::to_value(&receipt)?)?;
    let storage = LocalStorage::new(&options.backup_root);
    match storage.create_file_exclusive(
        Path::new(BULK_ADOPTION_MIGRATION_RECEIPT_FILE_V1),
        &receipt_bytes,
        Durability::Durable,
    )? {
        CreateOutcome::Created => {}
        CreateOutcome::AlreadyExists => {
            if storage.read_bytes(Path::new(BULK_ADOPTION_MIGRATION_RECEIPT_FILE_V1))?
                != receipt_bytes
            {
                return Err(invalid_migration(
                    "retained migration completion receipt conflicts with this execution",
                ));
            }
        }
    }
    Ok(receipt)
}

fn validate_migration_receipt(
    receipt: &BulkAdoptionMigrationReceiptV1,
    plan: &BulkAdoptionExecutionPlanV1,
    backup: &BulkAdoptionBackupManifestV1,
) -> Result<()> {
    if receipt.schema != BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1
        || receipt.approved_dry_run_hash != plan.approved_dry_run_hash
        || receipt.cohort_manifest_hash != plan.manifest_hash
        || receipt.minimum_reader_profile != plan.minimum_reader_profile
        || receipt.activation_id != plan.activation.activation_id()
        || receipt.completion_id != plan.completion.completion_id()
        || receipt.backup_manifest_hash != backup.manifest_hash
        || receipt.disposition != BulkAdoptionMigrationDispositionV1::Created
    {
        return Err(invalid_migration(
            "retained migration receipt does not match the completed execution plan",
        ));
    }
    Ok(())
}

fn validate_migration_options(options: &BulkAdoptionMigrationOptions) -> Result<()> {
    validate_dry_run_document(&options.dry_run)?;
    if options.acknowledged_dry_run_hash != options.dry_run.manifest_hash {
        return Err(invalid_migration(
            "acknowledged dry-run hash does not match the exact dry-run document",
        ));
    }
    if options.dry_run.requires_owner_decision {
        return Err(invalid_migration(
            "approved dry run still requires an explicit owner decision",
        ));
    }
    if options.operation_id.trim().is_empty() {
        return Err(invalid_migration("migration operation id cannot be empty"));
    }
    if options.minimum_reader_ack.as_deref() != Some(REVIEW_CHANGE_REVISION_COHORT_V1) {
        return Err(invalid_migration(format!(
            "minimum reader acknowledgement must be {REVIEW_CHANGE_REVISION_COHORT_V1}",
        )));
    }
    if !options.legacy_reader_unsupported_ack {
        return Err(invalid_migration(
            "activation requires explicit acknowledgement that v0.9 readers become unsupported",
        ));
    }
    Ok(())
}

fn validate_dry_run_document(document: &BulkAdoptionDryRunDocumentV1) -> Result<()> {
    if document.schema != BULK_ADOPTION_DRY_RUN_SCHEMA_V1 {
        return Err(invalid_migration(
            "unsupported bulk-adoption dry-run schema",
        ));
    }
    if document.roots.is_empty()
        || document
            .roots
            .windows(2)
            .any(|pair| pair[0].root_identity_hash >= pair[1].root_identity_hash)
    {
        return Err(invalid_migration(
            "bulk-adoption dry run has a non-canonical root set",
        ));
    }
    let material = serde_json::json!({
        "schema": document.schema,
        "roots": document.roots,
        "anomalies": document.anomalies,
        "requiresOwnerDecision": document.requires_owner_decision,
        "ownerDecisionManifestHash": document.owner_decision_manifest_hash,
        "writer": document.writer,
        "claimOccurredAt": document.claim_occurred_at,
        "signaturePolicy": document.signature_policy,
    });
    if sha256_json_prefixed(&material)? != document.manifest_hash {
        return Err(invalid_migration(
            "bulk-adoption dry-run self-hash mismatch",
        ));
    }
    Ok(())
}

fn prepare_approved_bulk_adoption(
    options: &BulkAdoptionMigrationOptions,
    approved_root: &BulkAdoptionDryRunRootV1,
    signer: &(dyn EventSigner + Send + Sync),
) -> Result<BulkAdoptionExecutionPlanV1> {
    let inventory = legacy_inventory_for_repo(&options.repo)?;
    if inventory.root_identity_hash != approved_root.root_identity_hash
        || inventory.source_authority_cursor != approved_root.source_authority_cursor
    {
        return Err(invalid_migration(
            "current L0 authority does not match the approved dry-run cursor",
        ));
    }
    let decisions = decisions_for_execution(options)?;
    let retained = options
        .owner_decisions
        .as_ref()
        .map(|manifest| {
            manifest
                .retained_manifests
                .iter()
                .map(|retained| RetainedAdoptionManifestV1 {
                    manifest_hash: retained.manifest_hash.clone(),
                    allocations: retained
                        .allocations
                        .iter()
                        .map(|allocation| RetainedChangeAllocationV1 {
                            change_id: allocation.change_id.clone(),
                            identity_descriptor: allocation.identity_descriptor.clone(),
                            members: allocation.members.clone(),
                        })
                        .collect(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let proposal = plan_bulk_adoption(std::slice::from_ref(&inventory), &retained, &decisions)?;
    if proposal.requires_owner_decision {
        return Err(invalid_migration(
            "exact activation re-plan still requires an owner decision",
        ));
    }
    let root = proposal
        .roots
        .into_iter()
        .next()
        .ok_or_else(|| invalid_migration("approved activation has no root"))?;
    let manifest = root
        .manifest
        .ok_or_else(|| invalid_migration("approved activation has no executable manifest"))?;
    let manifest_hash = manifest.canonical_hash()?;
    if manifest_hash != options.acknowledged_cohort_manifest_hash {
        return Err(invalid_migration(
            "exact activation re-plan changed the acknowledged cohort manifest",
        ));
    }
    let occurred_at = options
        .fixed_occurred_at
        .clone()
        .unwrap_or_else(crate::session::current_timestamp);
    let nonce = sha256_json_prefixed(&serde_json::json!({
        "operationId": options.operation_id,
        "rootIdentityHash": inventory.root_identity_hash,
        "approvedDryRunHash": options.acknowledged_dry_run_hash,
    }))?;
    let activation = build_signed_activation(
        signer,
        manifest,
        nonce,
        options.dry_run.writer.clone(),
        occurred_at.clone(),
    )?;
    let completion = build_signed_completion(
        signer,
        &activation,
        options.dry_run.writer.clone(),
        occurred_at,
    )?;
    Ok(BulkAdoptionExecutionPlanV1 {
        schema: BULK_ADOPTION_EXECUTION_PLAN_SCHEMA_V1.to_owned(),
        operation_id: options.operation_id.clone(),
        approved_dry_run_hash: options.acknowledged_dry_run_hash.clone(),
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        root_identity_hash: inventory.root_identity_hash,
        manifest_hash,
        source_authority_cursor: inventory.source_authority_cursor,
        events: root.planned_events,
        activation,
        completion,
    })
}

fn decisions_for_execution(
    options: &BulkAdoptionMigrationOptions,
) -> Result<OwnerManifestDecisionsV1> {
    let Some(manifest) = options.owner_decisions.as_ref() else {
        return Ok(OwnerManifestDecisionsV1 {
            writer: options.dry_run.writer.clone(),
            occurred_at: options.dry_run.claim_occurred_at.clone(),
            ..OwnerManifestDecisionsV1::default()
        });
    };
    let hash = validate_owner_decisions(manifest)?;
    if options.dry_run.owner_decision_manifest_hash.as_deref() != Some(hash.as_str()) {
        return Err(invalid_migration(
            "owner decisions do not match the approved dry-run document",
        ));
    }
    Ok(OwnerManifestDecisionsV1 {
        approved_anomaly_ids: manifest.approved_anomaly_ids.clone(),
        overlap_identity: manifest
            .overlap_identity
            .iter()
            .map(|(revision, decision)| {
                let decision = match decision {
                    BulkAdoptionOverlapIdentityDecisionV1::Shared {
                        identity_descriptor,
                    } => OverlapIdentityDecisionV1::Shared {
                        identity_descriptor: identity_descriptor.clone(),
                    },
                    BulkAdoptionOverlapIdentityDecisionV1::Distinct => {
                        OverlapIdentityDecisionV1::Distinct
                    }
                };
                (revision.clone(), decision)
            })
            .collect(),
        writer: options.dry_run.writer.clone(),
        occurred_at: manifest.claim_occurred_at.clone(),
    })
}

fn validate_retained_plan(
    plan: &BulkAdoptionExecutionPlanV1,
    options: &BulkAdoptionMigrationOptions,
    root: &BulkAdoptionDryRunRootV1,
) -> Result<()> {
    validate_execution_plan(plan)?;
    if plan.operation_id != options.operation_id
        || plan.approved_dry_run_hash != options.acknowledged_dry_run_hash
        || plan.root_identity_hash != root.root_identity_hash
        || plan.manifest_hash != options.acknowledged_cohort_manifest_hash
        || plan.source_authority_cursor != root.source_authority_cursor
    {
        return Err(invalid_migration(
            "retained execution plan does not match the approved migration inputs",
        ));
    }
    Ok(())
}

fn validate_execution_plan(plan: &BulkAdoptionExecutionPlanV1) -> Result<()> {
    if plan.schema != BULK_ADOPTION_EXECUTION_PLAN_SCHEMA_V1
        || plan.minimum_reader_profile != REVIEW_CHANGE_REVISION_COHORT_V1
    {
        return Err(invalid_migration(
            "unsupported bulk-adoption execution plan",
        ));
    }
    plan.activation.validate_for_execution()?;
    plan.completion.validate_for_execution()?;
    let manifest = plan.activation.manifest();
    if manifest.canonical_hash()? != plan.manifest_hash
        || manifest.source_authority_cursor != plan.source_authority_cursor
    {
        return Err(invalid_migration(
            "execution plan control authority does not match its frozen manifest",
        ));
    }
    let mut reservations = plan
        .events
        .iter()
        .map(reserved_record)
        .collect::<Result<Vec<_>>>()?;
    reservations.sort_by(|left, right| left.logical_key.cmp(&right.logical_key));
    if reservations != manifest.reserved_records {
        return Err(invalid_migration(
            "execution plan event bytes do not match the signed reservation manifest",
        ));
    }
    Ok(())
}

fn approved_root_for_identity<'a>(
    options: &'a BulkAdoptionMigrationOptions,
    store_identity: &str,
) -> Result<&'a BulkAdoptionDryRunRootV1> {
    let root = options
        .dry_run
        .roots
        .iter()
        .find(|root| root.root_identity_hash == store_identity)
        .ok_or_else(|| {
            invalid_migration("resolved store is absent from the approved dry-run document")
        })?;
    if root.cohort_manifest_hash.as_deref()
        != Some(options.acknowledged_cohort_manifest_hash.as_str())
    {
        return Err(invalid_migration(
            "acknowledged cohort manifest does not match the approved root",
        ));
    }
    Ok(root)
}

fn ensure_external_backup_path(store_root: &Path, backup_root: &Path) -> Result<()> {
    let store = absolute_path(store_root)?;
    let backup = absolute_path(backup_root)?;
    if backup.starts_with(&store) || store.starts_with(&backup) {
        return Err(invalid_migration(
            "migration backup must be outside the authoritative store root",
        ));
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                invalid_migration(format!("could not resolve current directory: {error}"))
            })?
            .join(path)
    };
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.try_exists().map_err(|error| {
        invalid_migration(format!(
            "could not inspect migration path boundary: {error}"
        ))
    })? {
        suffix.push(
            existing
                .file_name()
                .ok_or_else(|| invalid_migration("migration path has no existing ancestor"))?
                .to_os_string(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| invalid_migration("migration path has no existing ancestor"))?;
    }
    let mut resolved = fs::canonicalize(existing).map_err(|error| {
        invalid_migration(format!(
            "could not canonicalize migration path boundary: {error}"
        ))
    })?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn ensure_backup(
    store_root: &Path,
    backup_root: &Path,
    store_identity: &str,
    source_cursor: &AuthorityCursorV2,
) -> Result<BulkAdoptionBackupManifestV1> {
    if backup_root
        .join(BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1)
        .is_file()
    {
        let manifest = verify_backup(backup_root)?;
        if manifest.source_store_identity != store_identity
            || manifest.source_authority_cursor != *source_cursor
        {
            return Err(invalid_migration(
                "existing backup belongs to a different source authority",
            ));
        }
        return Ok(manifest);
    }
    fs::create_dir_all(backup_root.join("store")).map_err(|error| {
        invalid_migration(format!(
            "could not create external backup directory: {error}"
        ))
    })?;
    let before = inventory_store_files(store_root)?;
    validate_partial_backup(backup_root, &before)?;
    let storage = LocalStorage::new(backup_root);
    for entry in &before {
        let bytes = fs::read(store_root.join(&entry.path)).map_err(|error| {
            invalid_migration(format!(
                "could not read source backup file {}: {error}",
                entry.path
            ))
        })?;
        let target = Path::new("store").join(&entry.path);
        match storage.create_file_exclusive(&target, &bytes, Durability::Durable)? {
            CreateOutcome::Created => {}
            CreateOutcome::AlreadyExists => {
                if storage.read_bytes(&target)? != bytes {
                    return Err(invalid_migration(format!(
                        "backup file conflicts with source authority: {}",
                        entry.path
                    )));
                }
            }
        }
    }
    let after = inventory_store_files(store_root)?;
    if before != after {
        return Err(invalid_migration(
            "source authority changed while the external backup was being written",
        ));
    }
    let material = serde_json::json!({
        "schema": BULK_ADOPTION_BACKUP_MANIFEST_SCHEMA_V1,
        "sourceStoreIdentity": store_identity,
        "sourceAuthorityCursor": source_cursor,
        "entries": before,
    });
    let manifest = BulkAdoptionBackupManifestV1 {
        schema: BULK_ADOPTION_BACKUP_MANIFEST_SCHEMA_V1.to_owned(),
        source_store_identity: store_identity.to_owned(),
        source_authority_cursor: source_cursor.clone(),
        entries: before,
        manifest_hash: sha256_json_prefixed(&material)?,
    };
    let manifest_bytes = canonical_json_bytes(&serde_json::to_value(&manifest)?)?;
    match storage.create_file_exclusive(
        Path::new(BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1),
        &manifest_bytes,
        Durability::Durable,
    )? {
        CreateOutcome::Created => {}
        CreateOutcome::AlreadyExists => {
            if storage.read_bytes(Path::new(BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1))?
                != manifest_bytes
            {
                return Err(invalid_migration(
                    "partial backup carries a conflicting manifest",
                ));
            }
        }
    }
    let receipt = BulkAdoptionBackupReceiptV1 {
        schema: BULK_ADOPTION_BACKUP_RECEIPT_SCHEMA_V1.to_owned(),
        source_store_identity: store_identity.to_owned(),
        source_authority_cursor: source_cursor.clone(),
        manifest_hash: manifest.manifest_hash.clone(),
        file_count: manifest.entries.len() as u64,
        total_bytes: manifest.entries.iter().map(|entry| entry.bytes).sum(),
    };
    let receipt_bytes = canonical_json_bytes(&serde_json::to_value(receipt)?)?;
    storage.create_file_exclusive(
        Path::new(BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1),
        &receipt_bytes,
        Durability::Durable,
    )?;
    Ok(manifest)
}

fn validate_partial_backup(
    backup_root: &Path,
    source_entries: &[BulkAdoptionBackupEntryV1],
) -> Result<()> {
    let entries = fs::read_dir(backup_root)
        .map_err(|error| invalid_migration(format!("could not inspect partial backup: {error}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| invalid_migration(format!("could not inspect partial backup: {error}")))?;
    for entry in entries {
        let name = entry.file_name();
        let kind = entry.file_type().map_err(|error| {
            invalid_migration(format!("could not inspect partial backup entry: {error}"))
        })?;
        let allowed = (name == "store" && kind.is_dir())
            || (name == BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1 && kind.is_file())
            || backup_path_is_disposable(Path::new(&name));
        if !allowed {
            return Err(invalid_migration(format!(
                "incomplete backup contains an unexpected entry: {}",
                entry.path().display()
            )));
        }
    }
    for existing in inventory_store_files(&backup_root.join("store"))? {
        if !source_entries.iter().any(|source| source == &existing) {
            return Err(invalid_migration(format!(
                "partial backup conflicts with current source authority: {}",
                existing.path
            )));
        }
    }
    Ok(())
}

fn verify_backup(backup_root: &Path) -> Result<BulkAdoptionBackupManifestV1> {
    let manifest: BulkAdoptionBackupManifestV1 = serde_json::from_slice(
        &fs::read(backup_root.join(BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1)).map_err(|error| {
            invalid_migration(format!("could not read backup manifest: {error}"))
        })?,
    )?;
    let receipt: BulkAdoptionBackupReceiptV1 = serde_json::from_slice(
        &fs::read(backup_root.join(BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1)).map_err(|error| {
            invalid_migration(format!("could not read complete backup receipt: {error}"))
        })?,
    )?;
    if manifest.schema != BULK_ADOPTION_BACKUP_MANIFEST_SCHEMA_V1
        || receipt.schema != BULK_ADOPTION_BACKUP_RECEIPT_SCHEMA_V1
        || receipt.source_store_identity != manifest.source_store_identity
        || receipt.source_authority_cursor != manifest.source_authority_cursor
        || receipt.manifest_hash != manifest.manifest_hash
        || receipt.file_count != manifest.entries.len() as u64
        || receipt.total_bytes
            != manifest
                .entries
                .iter()
                .map(|entry| entry.bytes)
                .sum::<u64>()
    {
        return Err(invalid_migration("backup manifest/receipt mismatch"));
    }
    if manifest
        .entries
        .windows(2)
        .any(|pair| pair[0].path >= pair[1].path)
        || manifest
            .entries
            .iter()
            .any(|entry| !backup_entry_path_is_safe(&entry.path))
    {
        return Err(invalid_migration(
            "backup manifest contains a non-canonical or unsafe relative path",
        ));
    }
    let material = serde_json::json!({
        "schema": manifest.schema,
        "sourceStoreIdentity": manifest.source_store_identity,
        "sourceAuthorityCursor": manifest.source_authority_cursor,
        "entries": manifest.entries,
    });
    if sha256_json_prefixed(&material)? != manifest.manifest_hash {
        return Err(invalid_migration("backup manifest self-hash mismatch"));
    }
    for entry in &manifest.entries {
        let bytes = fs::read(backup_root.join("store").join(&entry.path)).map_err(|error| {
            invalid_migration(format!(
                "could not read retained backup file {}: {error}",
                entry.path
            ))
        })?;
        if bytes.len() as u64 != entry.bytes || sha256_bytes_hex(&bytes) != entry.sha256 {
            return Err(invalid_migration(format!(
                "retained backup file failed verification: {}",
                entry.path
            )));
        }
    }
    Ok(manifest)
}

fn backup_entry_path_is_safe(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && !value.is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
        && !backup_path_is_disposable(path)
}

fn inventory_store_files(store_root: &Path) -> Result<Vec<BulkAdoptionBackupEntryV1>> {
    let mut paths = Vec::new();
    collect_store_files(store_root, store_root, &mut paths)?;
    paths.sort();
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = path
            .strip_prefix(store_root)
            .map_err(|_| invalid_migration("backup inventory escaped the source store"))?;
        let relative = relative.to_str().ok_or_else(|| {
            invalid_migration("backup inventory requires Unicode store-relative paths")
        })?;
        let relative = relative.replace('\\', "/");
        let bytes = fs::read(&path).map_err(|error| {
            invalid_migration(format!(
                "could not inventory source file {relative}: {error}"
            ))
        })?;
        entries.push(BulkAdoptionBackupEntryV1 {
            path: relative,
            bytes: bytes.len() as u64,
            sha256: sha256_bytes_hex(&bytes),
        });
    }
    Ok(entries)
}

fn collect_store_files(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            invalid_migration(format!("could not inventory store directory: {error}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| invalid_migration(format!("could not inventory store entry: {error}")))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid_migration("backup inventory escaped the source store"))?;
        if backup_path_is_disposable(relative) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            invalid_migration(format!("could not inspect backup source entry: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_migration(format!(
                "refusing symlink in authoritative backup inventory: {}",
                relative.display()
            )));
        }
        if metadata.is_dir() {
            collect_store_files(root, &path, paths)?;
        } else if metadata.is_file() {
            paths.push(path);
        } else {
            return Err(invalid_migration(format!(
                "refusing non-file store entry in backup inventory: {}",
                relative.display()
            )));
        }
    }
    Ok(())
}

fn backup_path_is_disposable(path: &Path) -> bool {
    let first = path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    first == Some("derived")
        || name == STORE_AUTHORITY_LOCK_FILE
        || name == "derived.writer.lock"
        || name == "derived.rebuild.lock"
        || (name.starts_with(".shore-write.") && name.ends_with(".tmp"))
}

/// Fully signed, exact execution input retained outside the authoritative
/// store before activation. M1 recovery reads this document and never attempts
/// to reconstruct an L0 graph from partially migrated authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct BulkAdoptionExecutionPlanV1 {
    schema: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    approved_dry_run_hash: String,
    #[serde(default = "minimum_reader_profile_v1")]
    minimum_reader_profile: String,
    root_identity_hash: String,
    manifest_hash: String,
    source_authority_cursor: AuthorityCursorV2,
    events: Vec<ShoreEvent>,
    activation: StoreCapabilityActivationV1,
    completion: BulkAdoptionCompletionV1,
}

#[allow(
    clippy::too_many_arguments,
    dead_code,
    reason = "the frozen qualification plan remains available to the feature-built evidence harness"
)]
#[cfg(any(test, feature = "bench"))]
pub(crate) fn prepare_bulk_adoption_for_qualification(
    repo: &Path,
    acknowledged_manifest_hash: &str,
    writer: Writer,
    claim_occurred_at: String,
    activation_nonce: String,
    activation_occurred_at: String,
    completion_occurred_at: String,
    signer: &impl EventSigner,
) -> Result<BulkAdoptionExecutionPlanV1> {
    let inventory = legacy_inventory_for_repo(repo)?;
    let decisions = OwnerManifestDecisionsV1 {
        writer: writer.clone(),
        occurred_at: claim_occurred_at,
        ..OwnerManifestDecisionsV1::default()
    };
    let proposal = plan_bulk_adoption(std::slice::from_ref(&inventory), &[], &decisions)?;
    if proposal.requires_owner_decision {
        return Err(invalid_migration(
            "bulk-adoption proposal still requires explicit anomaly decisions",
        ));
    }
    let root = proposal
        .roots
        .into_iter()
        .next()
        .ok_or_else(|| invalid_migration("bulk-adoption proposal has no root"))?;
    let manifest = root
        .manifest
        .ok_or_else(|| invalid_migration("bulk-adoption proposal has no executable manifest"))?;
    let manifest_hash = manifest.canonical_hash()?;
    if manifest_hash != acknowledged_manifest_hash {
        return Err(invalid_migration(
            "acknowledged bulk-adoption manifest hash does not match the current L0 authority",
        ));
    }
    let activation = build_signed_activation(
        signer,
        manifest,
        activation_nonce,
        writer.clone(),
        activation_occurred_at,
    )?;
    let completion = build_signed_completion(signer, &activation, writer, completion_occurred_at)?;
    Ok(BulkAdoptionExecutionPlanV1 {
        schema: BULK_ADOPTION_EXECUTION_PLAN_SCHEMA_V1.to_owned(),
        operation_id: String::new(),
        approved_dry_run_hash: String::new(),
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        root_identity_hash: inventory.root_identity_hash,
        manifest_hash,
        source_authority_cursor: inventory.source_authority_cursor,
        events: root.planned_events,
        activation,
        completion,
    })
}

/// Activate one empty disposable qualification root through the production
/// capability machinery without introducing the backup/key-management boundary.
///
/// The signed activation/completion records, append ordering, and final strict
/// capability fold are the production implementations. Requiring an empty root
/// keeps synthetic current-data fixtures out of the legacy adoption contract.
#[cfg(any(test, feature = "bench"))]
pub(crate) fn activate_empty_store_for_qualification(
    repo: &Path,
    activation_nonce: String,
    activation_occurred_at: String,
    completion_occurred_at: String,
    signer: &impl EventSigner,
) -> Result<StoreCapabilityInspection> {
    let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo))?;
    if dry_run.requires_owner_decision || dry_run.roots.len() != 1 || !dry_run.anomalies.is_empty()
    {
        return Err(invalid_migration(
            "qualification capability activation requires one anomaly-free L0 root",
        ));
    }
    let root = &dry_run.roots[0];
    if root.source_authority_cursor.event_count != 0
        || root.source_authority_cursor.journal_record_count != 0
        || root.revision_count != 0
        || !root.proposed_changes.is_empty()
    {
        return Err(invalid_migration(
            "qualification capability activation requires an empty store",
        ));
    }
    let acknowledged_manifest_hash = root
        .cohort_manifest_hash
        .as_deref()
        .ok_or_else(|| invalid_migration("qualification root omitted its cohort manifest"))?;
    let plan = prepare_bulk_adoption_for_qualification(
        repo,
        acknowledged_manifest_hash,
        dry_run.writer.clone(),
        dry_run.claim_occurred_at,
        activation_nonce,
        activation_occurred_at,
        completion_occurred_at,
        signer,
    )?;
    if !plan.events.is_empty() {
        return Err(invalid_migration(
            "empty qualification activation unexpectedly planned Change events",
        ));
    }
    execute_bulk_adoption_plan(repo, &plan, None)?;
    let final_authority = store_capability_for_repo(repo)?;
    if !matches!(final_authority.status, StoreCapabilityStatus::Ready { .. })
        || final_authority.minimum_reader_profile.as_deref()
            != Some(REVIEW_CHANGE_REVISION_COHORT_V1)
        || final_authority.cursor.event_count != 0
        || final_authority.cursor.journal_record_count != 2
    {
        return Err(invalid_migration(
            "qualification activation did not produce exact empty L2 authority",
        ));
    }
    Ok(final_authority)
}

/// Derive the exact capability identities produced by the empty qualification
/// activation without opening or mutating a store. Resume uses this to reject a
/// different, otherwise valid L2 authority before it writes anything.
#[cfg(any(test, feature = "bench"))]
#[allow(
    dead_code,
    reason = "the exact identity oracle is consumed by the bench-gated resume executor"
)]
pub(crate) fn expected_empty_store_qualification_status(
    activation_nonce: String,
    activation_occurred_at: String,
    completion_occurred_at: String,
    signer: &impl EventSigner,
) -> Result<StoreCapabilityStatus> {
    let source = route_journal_entries(Vec::new())?.cursor;
    let manifest = BulkAdoptionManifestV1::from_reserved_records(source, Vec::new())?;
    let writer = Writer {
        actor_id: ActorId::new("actor:bulk-adoption-dry-run"),
        producer: WriterProducer {
            name: "pointbreak".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    };
    let activation = build_signed_activation(
        signer,
        manifest,
        activation_nonce,
        writer.clone(),
        activation_occurred_at,
    )?;
    let completion = build_signed_completion(signer, &activation, writer, completion_occurred_at)?;
    Ok(StoreCapabilityStatus::Ready {
        activation_id: activation.activation_id().to_owned(),
        manifest_hash: activation.manifest_hash().to_owned(),
        completion_id: completion.completion_id().to_owned(),
    })
}

/// Execute a previously frozen migration plan against one resolved store.
/// `interrupt_after_append` counts activation, each Change event, then
/// completion. Retrying the same plan converges through exclusive-create
/// records and validates the exact M1/L2 authority after every phase.
fn execute_bulk_adoption_plan(
    repo: &Path,
    plan: &BulkAdoptionExecutionPlanV1,
    interrupt_after_append: Option<usize>,
) -> Result<()> {
    validate_execution_plan(plan)?;
    let identity = crate::session::store_identity(StoreIdentityOptions::new(repo))?;
    if identity.store_identity != plan.root_identity_hash {
        return Err(invalid_migration(
            "bulk-adoption execution plan names a different resolved store",
        ));
    }
    let (store, inspection) = resolve_change_read_store(repo)?;
    let journal = store.backend().journal();
    match &inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            if inspection.cursor != plan.source_authority_cursor {
                return Err(invalid_migration(
                    "L0 authority changed after the migration plan was frozen",
                ));
            }
            publish_control_record(
                journal.as_ref(),
                &plan.activation.logical_key(),
                &plan.activation,
            )?;
        }
        StoreCapabilityStatus::MigrationInProgress {
            activation_id,
            manifest_hash,
        }
        | StoreCapabilityStatus::Ready {
            activation_id,
            manifest_hash,
            ..
        } if activation_id == plan.activation.activation_id()
            && manifest_hash == plan.activation.manifest_hash() => {}
        _ => {
            return Err(invalid_migration(
                "store capability state does not match the frozen migration plan",
            ));
        }
    }
    if interrupt_after_append == Some(1) {
        return Err(invalid_migration(
            "injected migration interruption after append 1",
        ));
    }

    let after_activation = inspect_journal_records(journal.as_ref())?;
    if !matches!(
        after_activation.status,
        StoreCapabilityStatus::MigrationInProgress { .. } | StoreCapabilityStatus::Ready { .. }
    ) {
        return Err(invalid_migration(
            "migration activation did not produce a validated M1 authority",
        ));
    }
    for (index, event) in plan.events.iter().enumerate() {
        let bytes = serde_json::to_vec(event)?;
        journal.create_record_once(&event.idempotency_key, &bytes)?;
        if interrupt_after_append == Some(index + 2) {
            return Err(invalid_migration(format!(
                "injected migration interruption after append {}",
                index + 2
            )));
        }
    }
    let completion_append = plan.events.len() + 2;
    publish_control_record(
        journal.as_ref(),
        &plan.completion.logical_key(),
        &plan.completion,
    )?;
    if interrupt_after_append == Some(completion_append) {
        return Err(invalid_migration(format!(
            "injected migration interruption after append {completion_append}"
        )));
    }
    let complete = inspect_journal_records(journal.as_ref())?;
    match complete.status {
        StoreCapabilityStatus::Ready {
            activation_id,
            manifest_hash,
            completion_id,
        } if activation_id == plan.activation.activation_id()
            && manifest_hash == plan.manifest_hash
            && completion_id == plan.completion.completion_id() =>
        {
            Ok(())
        }
        _ => Err(invalid_migration(
            "bulk-adoption completion did not produce the frozen L2 authority",
        )),
    }
}

fn plan_bulk_adoption(
    roots: &[LegacyRootInventoryV1],
    retained_manifests: &[RetainedAdoptionManifestV1],
    decisions: &OwnerManifestDecisionsV1,
) -> Result<BulkAdoptionProposalV1> {
    let mut roots = roots.to_vec();
    roots.sort_by(|left, right| left.root_identity_hash.cmp(&right.root_identity_hash));
    for root in &mut roots {
        for revision in &mut root.revisions {
            revision.supersedes.sort();
            revision.supersedes.dedup();
        }
        root.revisions.sort_by(|left, right| {
            (
                &left.revision.revision_id,
                &left.revision.object_artifact_content_hash,
                &left.legacy_group,
                &left.supersedes,
            )
                .cmp(&(
                    &right.revision.revision_id,
                    &right.revision.object_artifact_content_hash,
                    &right.legacy_group,
                    &right.supersedes,
                ))
        });
    }
    let mut retained_manifests = retained_manifests.to_vec();
    retained_manifests.sort_by(|left, right| left.manifest_hash.cmp(&right.manifest_hash));
    for manifest in &mut retained_manifests {
        if !is_prefixed_sha256(&manifest.manifest_hash) {
            return Err(crate::error::ShoreError::Message(
                "retained adoption manifest requires a prefixed SHA-256 identity".to_owned(),
            ));
        }
        manifest.allocations.sort_by(|left, right| {
            (&left.change_id, &left.members).cmp(&(&right.change_id, &right.members))
        });
        for allocation in &manifest.allocations {
            if crate::model::derive_change_id(&allocation.identity_descriptor)?
                != allocation.change_id
            {
                return Err(crate::error::ShoreError::Message(
                    "retained adoption allocation has a mismatched Change identity".to_owned(),
                ));
            }
        }
    }

    let mut anomalies = Vec::new();
    let mut locations: BTreeMap<RevisionId, BTreeSet<String>> = BTreeMap::new();
    let mut root_revisions: BTreeMap<
        RevisionId,
        Vec<(String, RevisionRefV1, BTreeSet<RevisionId>)>,
    > = BTreeMap::new();
    let mut retained_allocations: BTreeMap<RevisionId, Vec<RetainedChangeAllocationV1>> =
        BTreeMap::new();
    for root in &roots {
        for revision in &root.revisions {
            locations
                .entry(revision.revision.revision_id.clone())
                .or_default()
                .insert(root.root_identity_hash.clone());
            root_revisions
                .entry(revision.revision.revision_id.clone())
                .or_default()
                .push((
                    root.root_identity_hash.clone(),
                    revision.revision.clone(),
                    revision.supersedes.iter().cloned().collect(),
                ));
        }
    }
    for manifest in &retained_manifests {
        let source = format!("retained-manifest:{}", manifest.manifest_hash);
        for allocation in &manifest.allocations {
            for revision_id in &allocation.members {
                locations
                    .entry(revision_id.clone())
                    .or_default()
                    .insert(source.clone());
                retained_allocations
                    .entry(revision_id.clone())
                    .or_default()
                    .push(allocation.clone());
            }
        }
    }
    for (revision_id, root_hashes) in locations.iter().filter(|(_, roots)| roots.len() > 1) {
        anomalies.push(anomaly(
            "cross_root_revision_overlap",
            root_hashes.clone(),
            [revision_id.clone()].into(),
        )?);
        let occurrences = root_revisions
            .get(revision_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let artifact_hashes: BTreeSet<_> = occurrences
            .iter()
            .map(|(_, revision, _)| revision.object_artifact_content_hash.clone())
            .collect();
        if artifact_hashes.len() > 1 {
            anomalies.push(anomaly(
                "cross_root_revision_disagreement",
                root_hashes.clone(),
                [revision_id.clone()].into(),
            )?);
        }
        let relation_sets: BTreeSet<_> = occurrences
            .iter()
            .map(|(_, _, supersedes)| supersedes.clone())
            .collect();
        if relation_sets.len() > 1 {
            let mut implicated = [revision_id.clone()].into_iter().collect::<BTreeSet<_>>();
            implicated.extend(
                occurrences
                    .iter()
                    .flat_map(|(_, _, supersedes)| supersedes.iter().cloned()),
            );
            anomalies.push(anomaly(
                "cross_root_relation_disagreement",
                root_hashes.clone(),
                implicated,
            )?);
        }
        if let Some(OverlapIdentityDecisionV1::Shared {
            identity_descriptor,
        }) = decisions.overlap_identity.get(revision_id)
            && retained_allocations
                .get(revision_id)
                .is_some_and(|allocations| {
                    !allocations
                        .iter()
                        .any(|allocation| allocation.identity_descriptor == *identity_descriptor)
                })
        {
            anomalies.push(anomaly(
                "retained_identity_mismatch",
                root_hashes.clone(),
                [revision_id.clone()].into(),
            )?);
        }
    }

    let mut root_components = Vec::new();
    for root in &roots {
        let mut by_id = BTreeMap::new();
        for revision in &root.revisions {
            if let Some(existing) =
                by_id.insert(revision.revision.revision_id.clone(), revision.clone())
                && existing.revision != revision.revision
            {
                anomalies.push(anomaly(
                    "revision_identity_collision",
                    [root.root_identity_hash.clone()].into(),
                    [revision.revision.revision_id.clone()].into(),
                )?);
            }
        }
        let mut adjacency: BTreeMap<RevisionId, BTreeSet<RevisionId>> = by_id
            .keys()
            .cloned()
            .map(|revision| (revision, BTreeSet::new()))
            .collect();
        let mut admitted = BTreeSet::new();
        for revision in by_id.values() {
            for predecessor in &revision.supersedes {
                if by_id.contains_key(predecessor) {
                    adjacency
                        .get_mut(&revision.revision.revision_id)
                        .expect("known revision")
                        .insert(predecessor.clone());
                    adjacency
                        .get_mut(predecessor)
                        .expect("known predecessor")
                        .insert(revision.revision.revision_id.clone());
                    admitted.insert((revision.revision.revision_id.clone(), predecessor.clone()));
                } else {
                    anomalies.push(anomaly(
                        "dangling_relation",
                        [root.root_identity_hash.clone()].into(),
                        [revision.revision.revision_id.clone(), predecessor.clone()].into(),
                    )?);
                }
            }
        }
        let components = connected_components(&adjacency);
        for members in &components {
            let groups: BTreeSet<_> = members
                .iter()
                .filter_map(|revision| by_id.get(revision))
                .map(|revision| revision.legacy_group.clone())
                .collect();
            if groups.len() > 1 {
                anomalies.push(anomaly(
                    "legacy_group_bridge",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let component_edges: BTreeSet<_> = admitted
                .iter()
                .filter(|(successor, predecessor)| {
                    members.contains(successor) && members.contains(predecessor)
                })
                .cloned()
                .collect();
            if revision_graph_has_cycle(members, &component_edges) {
                anomalies.push(anomaly(
                    "relation_cycle",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let current = current_revisions(members, &component_edges);
            if replacement_heads_diverge(&current, &component_edges) {
                anomalies.push(anomaly(
                    "replacement_divergent_group",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let shared_descriptors: BTreeSet<_> = members
                .iter()
                .filter_map(|revision| match decisions.overlap_identity.get(revision) {
                    Some(OverlapIdentityDecisionV1::Shared {
                        identity_descriptor,
                    }) => serde_json::to_string(identity_descriptor).ok(),
                    Some(OverlapIdentityDecisionV1::Distinct) | None => None,
                })
                .collect();
            if shared_descriptors.len() > 1 {
                anomalies.push(anomaly(
                    "overlap_identity_decision_conflict",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
        }
        root_components.push((root.clone(), by_id, components, admitted));
    }

    anomalies.sort_by(|left, right| left.anomaly_id.cmp(&right.anomaly_id));
    anomalies.dedup_by(|left, right| left.anomaly_id == right.anomaly_id);
    let unresolved = anomalies.iter().any(|item| {
        if matches!(
            item.code.as_str(),
            "overlap_identity_decision_conflict" | "retained_identity_mismatch"
        ) {
            true
        } else if item.code == "cross_root_revision_overlap" {
            item.revision_ids
                .iter()
                .any(|revision| !decisions.overlap_identity.contains_key(revision))
        } else {
            !decisions.approved_anomaly_ids.contains(&item.anomaly_id)
        }
    });

    let mut proposals = Vec::new();
    for (root, by_id, components, admitted) in root_components {
        let mut groups = Vec::new();
        let mut planned_events = Vec::new();
        for members in components {
            let shared_descriptor = members.iter().find_map(|revision| {
                match decisions.overlap_identity.get(revision) {
                    Some(OverlapIdentityDecisionV1::Shared {
                        identity_descriptor,
                    }) => Some(identity_descriptor.clone()),
                    Some(OverlapIdentityDecisionV1::Distinct) | None => None,
                }
            });
            let descriptor = if let Some(descriptor) = shared_descriptor {
                descriptor
            } else {
                ChangeIdentityDescriptorV1::opaque_nonce(deterministic_nonce(&serde_json::json!({
                    "rootIdentityHash": root.root_identity_hash,
                    "members": members,
                }))?)
            };
            let declaration = build_change_declared(
                descriptor,
                deterministic_nonce(
                    &serde_json::json!({"claim": "declaration", "members": members}),
                )?,
            )?;
            let change_id = declaration.change_id.clone();
            planned_events.push(planned_event(
                &root,
                &decisions.writer,
                &decisions.occurred_at,
                declaration,
            )?);
            for revision_id in &members {
                planned_events.push(planned_event(
                    &root,
                    &decisions.writer,
                    &decisions.occurred_at,
                    build_membership_asserted(
                        &change_id,
                        revision_id,
                        deterministic_nonce(&serde_json::json!({
                            "claim": "membership",
                            "changeId": change_id,
                            "revisionId": revision_id,
                        }))?,
                    )?,
                )?);
            }
            let relations: BTreeSet<_> = admitted
                .iter()
                .filter(|(successor, predecessor)| {
                    members.contains(successor) && members.contains(predecessor)
                })
                .cloned()
                .collect();
            for (successor, predecessor) in &relations {
                planned_events.push(planned_event(
                    &root,
                    &decisions.writer,
                    &decisions.occurred_at,
                    build_revision_relation_asserted(
                        &change_id,
                        by_id
                            .get(successor)
                            .expect("component successor")
                            .revision
                            .clone(),
                        by_id
                            .get(predecessor)
                            .expect("component predecessor")
                            .revision
                            .clone(),
                        deterministic_nonce(&serde_json::json!({
                            "claim": "relation",
                            "changeId": change_id,
                            "successor": successor,
                            "predecessor": predecessor,
                        }))?,
                    )?,
                )?);
            }
            groups.push(AdoptionGroupProposalV1 {
                change_id,
                members,
                admitted_relations: relations,
            });
        }
        planned_events.sort_by(|left, right| left.idempotency_key.cmp(&right.idempotency_key));
        let manifest = if unresolved {
            None
        } else {
            let records = planned_events
                .iter()
                .map(reserved_record)
                .collect::<Result<Vec<_>>>()?;
            Some(BulkAdoptionManifestV1::from_reserved_records(
                root.source_authority_cursor.clone(),
                records,
            )?)
        };
        proposals.push(RootAdoptionProposalV1 {
            root_identity_hash: root.root_identity_hash,
            covered_revisions: by_id.into_keys().collect(),
            groups,
            planned_events,
            manifest,
        });
    }
    Ok(BulkAdoptionProposalV1 {
        roots: proposals,
        anomalies,
        requires_owner_decision: unresolved,
    })
}

fn planned_event<P: EventPayload>(
    root: &LegacyRootInventoryV1,
    writer: &Writer,
    occurred_at: &str,
    payload: P,
) -> Result<ShoreEvent> {
    let payload_value = serde_json::to_value(&payload)?;
    let payload_hash = sha256_json_prefixed(&payload_value)?;
    ShoreEvent::new(
        payload.event_type(),
        format!(
            "bulk-adoption:{}:{payload_hash}",
            payload.event_type().as_str()
        ),
        EventTarget::for_journal(root.journal_id.clone()),
        writer.clone(),
        payload,
        occurred_at,
    )
}

fn reserved_record(event: &ShoreEvent) -> Result<ReservedCohortRecordV1> {
    // EventStore persists serde's exact compact struct encoding. Reservations
    // bind those bytes, not a separately canonicalized representation.
    let bytes = serde_json::to_vec(event)?;
    let family = match event.event_type {
        crate::session::event::EventType::ChangeDeclared => "change_declared_v1",
        crate::session::event::EventType::ChangeMembershipAsserted => {
            "change_membership_asserted_v1"
        }
        crate::session::event::EventType::ChangeRevisionRelationAsserted => {
            "change_revision_relation_asserted_v1"
        }
        _ => {
            return Err(crate::error::ShoreError::Message(
                "bulk-adoption planner emitted an unsupported event family".to_owned(),
            ));
        }
    };
    Ok(ReservedCohortRecordV1 {
        logical_key: event.idempotency_key.clone(),
        record_family: family.to_owned(),
        record_hash: format!("sha256:{}", sha256_bytes_hex(&bytes)),
    })
}

fn deterministic_nonce(value: &serde_json::Value) -> Result<[u8; 32]> {
    let hash = sha256_json_prefixed(value)?;
    let hex = hash.strip_prefix("sha256:").expect("canonical hash prefix");
    let mut nonce = [0_u8; 32];
    for (index, byte) in nonce.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .expect("canonical hash is lowercase hexadecimal");
    }
    Ok(nonce)
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn anomaly(
    code: &str,
    root_identity_hashes: BTreeSet<String>,
    revision_ids: BTreeSet<RevisionId>,
) -> Result<MigrationAnomalyV1> {
    let anomaly_id = sha256_json_prefixed(&serde_json::json!({
        "code": code,
        "rootIdentityHashes": root_identity_hashes,
        "revisionIds": revision_ids,
    }))?;
    Ok(MigrationAnomalyV1 {
        anomaly_id,
        code: code.to_owned(),
        root_identity_hashes,
        revision_ids,
    })
}

fn connected_components(
    adjacency: &BTreeMap<RevisionId, BTreeSet<RevisionId>>,
) -> Vec<BTreeSet<RevisionId>> {
    let mut remaining: BTreeSet<_> = adjacency.keys().cloned().collect();
    let mut result = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut component = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            if !component.insert(node.clone()) {
                continue;
            }
            remaining.remove(&node);
            pending.extend(adjacency.get(&node).into_iter().flatten().cloned());
        }
        result.push(component);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JournalId, RevisionId, RevisionRefV1};
    use crate::session::AuthorityCursorV2;

    fn cursor() -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: "pointbreak.authority-cursor.v2".to_owned(),
            journal_record_count: 3,
            event_count: 3,
            journal_record_set_hash: format!("sha256:{}", "1".repeat(64)),
            event_set_hash: format!("sha256:{}", "2".repeat(64)),
            capability_set_hash: format!("sha256:{}", "3".repeat(64)),
        }
    }

    fn revision(name: &str, byte: char, supersedes: &[&str]) -> LegacyRevisionV1 {
        LegacyRevisionV1 {
            revision: RevisionRefV1::new(
                RevisionId::new(format!("rev:sha256:{name}")),
                format!("sha256:{}", byte.to_string().repeat(64)),
            )
            .unwrap(),
            legacy_group: "engagement:one".to_owned(),
            supersedes: supersedes
                .iter()
                .map(|name| RevisionId::new(format!("rev:sha256:{name}")))
                .collect(),
        }
    }

    fn root(revisions: Vec<LegacyRevisionV1>) -> LegacyRootInventoryV1 {
        LegacyRootInventoryV1 {
            root_identity_hash: format!("sha256:{}", "4".repeat(64)),
            journal_id: JournalId::new("journal:test"),
            source_authority_cursor: cursor(),
            revisions,
        }
    }

    #[test]
    fn component_and_singleton_proposal_is_permutation_stable_and_covers_every_revision() {
        let input = root(vec![
            revision("a", 'a', &[]),
            revision("b", 'b', &["a"]),
            revision("c", 'c', &[]),
        ]);
        let first = plan_bulk_adoption(
            std::slice::from_ref(&input),
            &[],
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        let mut reversed = input;
        reversed.revisions.reverse();
        let second =
            plan_bulk_adoption(&[reversed], &[], &OwnerManifestDecisionsV1::default()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.roots[0].covered_revisions.len(), 3);
        assert_eq!(first.roots[0].groups.len(), 2);
        assert!(first.roots[0].manifest.is_some());
    }

    #[test]
    fn dangling_relation_and_cross_root_overlap_require_explicit_owner_decisions() {
        let dangling = root(vec![revision("a", 'a', &["missing"])]);
        let mut other = root(vec![revision("a", 'a', &[])]);
        other.root_identity_hash = format!("sha256:{}", "5".repeat(64));
        other.journal_id = JournalId::new("journal:other");
        let proposal = plan_bulk_adoption(
            &[dangling.clone(), other.clone()],
            &[],
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        assert!(proposal.requires_owner_decision);
        assert!(proposal.roots.iter().all(|root| root.manifest.is_none()));
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "dangling_relation")
        );
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "cross_root_revision_overlap")
        );
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "cross_root_relation_disagreement")
        );

        let decisions = OwnerManifestDecisionsV1 {
            approved_anomaly_ids: proposal
                .anomalies
                .iter()
                .filter(|item| item.code != "cross_root_revision_overlap")
                .map(|item| item.anomaly_id.clone())
                .collect(),
            overlap_identity: [(
                RevisionId::new("rev:sha256:a"),
                OverlapIdentityDecisionV1::Shared {
                    identity_descriptor: ChangeIdentityDescriptorV1::root_revision(
                        RevisionId::new("rev:sha256:a"),
                    ),
                },
            )]
            .into(),
            ..OwnerManifestDecisionsV1::default()
        };
        let approved = plan_bulk_adoption(&[dangling, other], &[], &decisions).unwrap();
        assert!(!approved.requires_owner_decision);
        assert!(approved.roots.iter().all(|root| root.manifest.is_some()));
        assert_eq!(
            approved.roots[0].groups[0].change_id,
            approved.roots[1].groups[0].change_id
        );
    }

    #[test]
    fn retained_manifest_overlap_requires_and_reuses_an_explicit_identity_decision() {
        let revision_id = RevisionId::new("rev:sha256:a");
        let descriptor = ChangeIdentityDescriptorV1::root_revision(revision_id.clone());
        let retained_change_id = crate::model::derive_change_id(&descriptor).unwrap();
        let retained = RetainedAdoptionManifestV1 {
            manifest_hash: format!("sha256:{}", "9".repeat(64)),
            allocations: vec![RetainedChangeAllocationV1 {
                change_id: retained_change_id.clone(),
                identity_descriptor: descriptor.clone(),
                members: [revision_id.clone()].into(),
            }],
        };
        let input = root(vec![revision("a", 'a', &[])]);

        let unresolved = plan_bulk_adoption(
            std::slice::from_ref(&input),
            std::slice::from_ref(&retained),
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        assert!(unresolved.requires_owner_decision);
        assert!(unresolved.roots[0].manifest.is_none());

        let decisions = OwnerManifestDecisionsV1 {
            overlap_identity: [(
                revision_id,
                OverlapIdentityDecisionV1::Shared {
                    identity_descriptor: descriptor,
                },
            )]
            .into(),
            ..OwnerManifestDecisionsV1::default()
        };
        let resolved = plan_bulk_adoption(&[input], &[retained], &decisions).unwrap();
        assert!(!resolved.requires_owner_decision);
        assert!(resolved.roots[0].manifest.is_some());
        assert_eq!(resolved.roots[0].groups[0].change_id, retained_change_id);
    }

    #[test]
    fn real_l0_dry_run_is_stable_path_private_and_mutation_free() {
        let repo = real_l0_repo();

        let before = resolve_change_read_store(repo.path()).unwrap().1.cursor;
        let first = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let second = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let after = resolve_change_read_store(repo.path()).unwrap().1.cursor;

        assert_eq!(first, second);
        assert_eq!(before, after);
        assert_eq!(first.roots.len(), 1);
        assert_eq!(first.roots[0].revision_count, 2);
        assert_eq!(first.roots[0].proposed_changes.len(), 1);
        assert!(!first.requires_owner_decision);
        assert!(first.roots[0].cohort_manifest_hash.is_some());
        let json = serde_json::to_string(&first).unwrap();
        assert!(!json.contains(&repo.path().display().to_string()));
        assert!(!json.contains("private first bytes"));
        assert!(!json.contains("private second bytes"));
    }

    #[test]
    fn private_executor_retries_every_append_boundary_to_exact_l2() {
        use crate::crypto::TestEd25519Signer;

        for boundary in 1..=6 {
            let repo = real_l0_repo();
            let dry_run =
                dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
            let manifest_hash = dry_run.roots[0].cohort_manifest_hash.as_deref().unwrap();
            let signer = TestEd25519Signer::from_seed([0x41; 32]);
            let plan = prepare_bulk_adoption_for_qualification(
                repo.path(),
                manifest_hash,
                dry_run.writer.clone(),
                dry_run.claim_occurred_at.clone(),
                format!("qualification-activation-{boundary:02}"),
                "2026-08-06T00:00:00Z".to_owned(),
                "2026-08-06T00:00:01Z".to_owned(),
                &signer,
            )
            .unwrap();
            let expected_boundaries = plan.events.len() + 2;
            assert_eq!(expected_boundaries, 6);

            let error = execute_bulk_adoption_plan(repo.path(), &plan, Some(boundary)).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected migration interruption")
            );
            execute_bulk_adoption_plan(repo.path(), &plan, None).unwrap();
            assert!(matches!(
                resolve_change_read_store(repo.path()).unwrap().1.status,
                StoreCapabilityStatus::Ready { .. }
            ));
        }
    }

    #[test]
    fn qualification_activation_requires_and_produces_an_empty_l2_store() {
        use crate::crypto::TestEd25519Signer;

        let repo = empty_repo();
        let authority = activate_empty_store_for_qualification(
            repo.path(),
            "qualification-l2-capacity-v1".to_owned(),
            "2026-08-22T00:00:00Z".to_owned(),
            "2026-08-22T00:00:01Z".to_owned(),
            &TestEd25519Signer::from_seed([0x62; 32]),
        )
        .unwrap();

        assert_eq!(authority.cursor.event_count, 0);
        assert_eq!(authority.cursor.journal_record_count, 2);
        assert_eq!(
            authority.minimum_reader_profile.as_deref(),
            Some("review_change_revision_v1")
        );
        assert!(matches!(
            authority.status,
            StoreCapabilityStatus::Ready { .. }
        ));

        let nonempty = real_l0_repo();
        let before = resolve_change_read_store(nonempty.path()).unwrap().1;
        let error = activate_empty_store_for_qualification(
            nonempty.path(),
            "qualification-must-stay-empty".to_owned(),
            "2026-08-22T00:00:00Z".to_owned(),
            "2026-08-22T00:00:01Z".to_owned(),
            &TestEd25519Signer::from_seed([0x62; 32]),
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires an empty store"));
        let after = resolve_change_read_store(nonempty.path()).unwrap().1;
        assert_eq!(after.status, before.status);
        assert_eq!(after.cursor, before.cursor);
    }

    #[test]
    fn execution_plan_tampering_fails_before_activation() {
        use crate::crypto::TestEd25519Signer;

        let repo = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let mut plan = prepare_bulk_adoption_for_qualification(
            repo.path(),
            dry_run.roots[0].cohort_manifest_hash.as_deref().unwrap(),
            dry_run.writer.clone(),
            dry_run.claim_occurred_at.clone(),
            "tamper-proof".to_owned(),
            "2026-08-06T18:19:00Z".to_owned(),
            "2026-08-06T18:19:01Z".to_owned(),
            &TestEd25519Signer::from_seed([0x50; 32]),
        )
        .unwrap();
        plan.events[0].payload["changeId"] =
            serde_json::Value::String("change:tampered".to_owned());
        let error = execute_bulk_adoption_plan(repo.path(), &plan, None).unwrap_err();
        assert!(error.to_string().contains("reservation manifest"));
        assert!(matches!(
            resolve_change_read_store(repo.path()).unwrap().1.status,
            StoreCapabilityStatus::MigrationRequired
        ));
    }

    #[test]
    fn production_migration_requires_exact_acks_and_publishes_a_verified_external_backup() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let repo = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let cohort_hash = dry_run.roots[0].cohort_manifest_hash.clone().unwrap();
        let backup_parent = tempfile::tempdir().unwrap();
        let backup = backup_parent.path().join("pre-activation");

        let missing_reader_ack = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash.clone(),
                cohort_hash.clone(),
                &backup,
                "migration-one",
            )
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x51; 32]))
            .with_fixed_occurred_at("2026-08-06T18:20:00Z")
            .with_derived_enabled(false),
        )
        .unwrap_err();
        assert!(missing_reader_ack.to_string().contains("minimum reader"));
        assert!(!backup.exists());

        let receipt = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash.clone(),
                cohort_hash,
                &backup,
                "migration-one",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x51; 32]))
            .with_fixed_occurred_at("2026-08-06T18:20:00Z")
            .with_derived_enabled(false),
        )
        .unwrap();

        assert_eq!(receipt.schema, BULK_ADOPTION_MIGRATION_RECEIPT_SCHEMA_V1);
        assert_eq!(receipt.approved_dry_run_hash, dry_run.manifest_hash);
        assert_eq!(
            receipt.minimum_reader_profile,
            REVIEW_CHANGE_REVISION_COHORT_V1
        );
        assert!(backup.join("store/events").is_dir());
        assert!(backup.join(BULK_ADOPTION_BACKUP_MANIFEST_FILE_V1).is_file());
        assert!(backup.join(BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1).is_file());
        assert!(backup.join(BULK_ADOPTION_EXECUTION_PLAN_FILE_V1).is_file());
        assert!(matches!(
            resolve_change_read_store(repo.path()).unwrap().1.status,
            StoreCapabilityStatus::Ready { .. }
        ));

        let repeated = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash,
                dry_run.roots[0].cohort_manifest_hash.clone().unwrap(),
                &backup,
                "migration-one",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .with_derived_enabled(false),
        )
        .unwrap();
        assert_eq!(
            repeated.disposition,
            BulkAdoptionMigrationDispositionV1::Existing
        );
        assert_eq!(repeated.completion_id, receipt.completion_id);
    }

    #[test]
    fn production_migration_resumes_each_append_boundary_from_the_retained_plan() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        for boundary in 1..=6 {
            let repo = real_l0_repo();
            let dry_run =
                dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
            let cohort_hash = dry_run.roots[0].cohort_manifest_hash.clone().unwrap();
            let backup_parent = tempfile::tempdir().unwrap();
            let backup = backup_parent
                .path()
                .join(format!("pre-activation-{boundary}"));
            let base = BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash.clone(),
                cohort_hash.clone(),
                &backup,
                format!("migration-{boundary}"),
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .with_fixed_occurred_at("2026-08-06T18:21:00Z")
            .with_derived_enabled(false);

            let interrupted = migrate_bulk_adoption(
                base.sign_with(TestEd25519Signer::from_seed([0x52; 32]))
                    .with_interruption_after_append(boundary),
            )
            .unwrap_err();
            assert!(interrupted.to_string().contains("interruption"));
            assert!(backup.join(BULK_ADOPTION_EXECUTION_PLAN_FILE_V1).is_file());

            let resumed = migrate_bulk_adoption(
                BulkAdoptionMigrationOptions::new(
                    repo.path(),
                    dry_run.clone(),
                    dry_run.manifest_hash.clone(),
                    cohort_hash,
                    &backup,
                    format!("migration-{boundary}"),
                )
                .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
                .with_legacy_reader_unsupported_ack()
                .with_fixed_occurred_at("2026-08-06T18:21:00Z")
                .with_derived_enabled(false),
            )
            .unwrap();
            assert!(matches!(
                resolve_change_read_store(repo.path()).unwrap().1.status,
                StoreCapabilityStatus::Ready { .. }
            ));
            assert_eq!(
                resumed.disposition,
                BulkAdoptionMigrationDispositionV1::Created
            );
        }
    }

    #[test]
    fn production_migration_refuses_wrong_acknowledgements_and_authority_drift_before_backup() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let repo = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let cohort_hash = dry_run.roots[0].cohort_manifest_hash.clone().unwrap();
        let backups = tempfile::tempdir().unwrap();

        for (name, dry_ack, cohort_ack) in [
            ("wrong-dry", "sha256:wrong".to_owned(), cohort_hash.clone()),
            (
                "wrong-cohort",
                dry_run.manifest_hash.clone(),
                "sha256:wrong".to_owned(),
            ),
        ] {
            let backup = backups.path().join(name);
            let error = migrate_bulk_adoption(
                BulkAdoptionMigrationOptions::new(
                    repo.path(),
                    dry_run.clone(),
                    dry_ack,
                    cohort_ack,
                    &backup,
                    name,
                )
                .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
                .with_legacy_reader_unsupported_ack()
                .sign_with(TestEd25519Signer::from_seed([0x61; 32]))
                .with_derived_enabled(false),
            )
            .unwrap_err();
            assert!(error.to_string().contains("acknowledged"));
            assert!(!backup.exists());
        }

        std::fs::write(repo.path().join("sample.txt"), "authority moved\n").unwrap();
        crate::session::capture_review(
            crate::session::CaptureOptions::new(repo.path()).with_summary("authority moved"),
        )
        .unwrap();
        let drift_backup = backups.path().join("authority-drift");
        let error = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash,
                cohort_hash,
                &drift_backup,
                "authority-drift",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x62; 32]))
            .with_derived_enabled(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("approved dry-run cursor"));
        assert!(!drift_backup.exists());
    }

    #[test]
    fn production_migration_refuses_a_damaged_backup() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let damaged_repo = real_l0_repo();
        let damaged_dry =
            dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(damaged_repo.path())).unwrap();
        let damaged_backup_parent = tempfile::tempdir().unwrap();
        let damaged_backup = damaged_backup_parent.path().join("damaged");
        let interrupted = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                damaged_repo.path(),
                damaged_dry.clone(),
                damaged_dry.manifest_hash.clone(),
                damaged_dry.roots[0].cohort_manifest_hash.clone().unwrap(),
                &damaged_backup,
                "damaged-backup",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x63; 32]))
            .with_fixed_occurred_at("2026-08-06T18:22:00Z")
            .with_derived_enabled(false)
            .with_interruption_after_append(1),
        )
        .unwrap_err();
        assert!(interrupted.to_string().contains("interruption"));
        let event = std::fs::read_dir(damaged_backup.join("store/events"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(event, b"damaged").unwrap();
        let error = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                damaged_repo.path(),
                damaged_dry.clone(),
                damaged_dry.manifest_hash,
                damaged_dry.roots[0].cohort_manifest_hash.clone().unwrap(),
                &damaged_backup,
                "damaged-backup",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .with_derived_enabled(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("failed verification"));
    }

    #[test]
    fn production_migration_default_publishes_derived_only_after_l2() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let repo = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let backup_parent = tempfile::tempdir().unwrap();
        let receipt = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash,
                dry_run.roots[0].cohort_manifest_hash.clone().unwrap(),
                backup_parent.path().join("derived"),
                "derived-after-l2",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x64; 32]))
            // Intentionally do not override `derived_enabled`: this proves the
            // production default while the authority lock spans activation.
            .with_fixed_occurred_at("2026-08-06T18:23:00Z"),
        )
        .unwrap();
        assert!(receipt.derived_generation_id.is_some());
        let (store, inspection) = resolve_change_read_store(repo.path()).unwrap();
        assert!(matches!(
            inspection.status,
            StoreCapabilityStatus::Ready { .. }
        ));
        assert!(
            std::fs::read_dir(store.store_dir().join("derived/publications"))
                .unwrap()
                .next()
                .is_some()
        );
        let status =
            crate::session::derived_access::history::DerivedHistoryAccess::resolve(repo.path())
                .unwrap()
                .lifecycle_status();
        assert_eq!(
            status.availability,
            crate::session::derived_access::history::DerivedHistoryAvailability::Current,
            "{status:?}"
        );
        assert_eq!(receipt.derived_generation_id, status.generation_id);
    }

    #[test]
    fn verified_backup_restores_only_as_a_separately_identified_l0_fork() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let source = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(source.path())).unwrap();
        let backup_parent = tempfile::tempdir().unwrap();
        let backup = backup_parent.path().join("restore-source");
        migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                source.path(),
                dry_run.clone(),
                dry_run.manifest_hash,
                dry_run.roots[0].cohort_manifest_hash.clone().unwrap(),
                &backup,
                "restore-source",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x65; 32]))
            .with_fixed_occurred_at("2026-08-06T18:24:00Z")
            .with_derived_enabled(false),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        git(target.path(), &["init", "--quiet"]);
        git(target.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            target.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        let restored = restore_bulk_adoption_backup(&backup, target.path()).unwrap();
        assert_eq!(
            restored.disposition,
            BulkAdoptionBackupRestoreDispositionV1::Created
        );
        assert_ne!(
            restored.restored_store_identity,
            dry_run.roots[0].root_identity_hash
        );
        let restored_dry =
            dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(target.path())).unwrap();
        assert_eq!(restored_dry.roots[0].revision_count, 2);
        assert!(matches!(
            resolve_change_read_store(target.path()).unwrap().1.status,
            StoreCapabilityStatus::MigrationRequired
        ));
        assert_eq!(
            restore_bulk_adoption_backup(&backup, target.path())
                .unwrap()
                .disposition,
            BulkAdoptionBackupRestoreDispositionV1::Existing
        );
    }

    #[test]
    fn production_migration_repairs_an_exact_interrupted_backup_copy() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let repo = real_l0_repo();
        let dry_run = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(repo.path())).unwrap();
        let (store, _) = resolve_change_read_store(repo.path()).unwrap();
        let source_entries = inventory_store_files(store.store_dir()).unwrap();
        let first = source_entries.first().unwrap();
        let backup_parent = tempfile::tempdir().unwrap();
        let backup = backup_parent.path().join("partial");
        let partial_target = backup.join("store").join(&first.path);
        std::fs::create_dir_all(partial_target.parent().unwrap()).unwrap();
        std::fs::copy(store.store_dir().join(&first.path), &partial_target).unwrap();

        let receipt = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                repo.path(),
                dry_run.clone(),
                dry_run.manifest_hash,
                dry_run.roots[0].cohort_manifest_hash.clone().unwrap(),
                &backup,
                "partial-backup",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x66; 32]))
            .with_fixed_occurred_at("2026-08-06T18:25:00Z")
            .with_derived_enabled(false),
        )
        .unwrap();
        assert_eq!(
            receipt.disposition,
            BulkAdoptionMigrationDispositionV1::Created
        );
        assert!(backup.join(BULK_ADOPTION_BACKUP_RECEIPT_FILE_V1).is_file());
    }

    #[test]
    fn production_migration_selects_its_exact_root_from_a_multi_root_approval() {
        use crate::crypto::TestEd25519Signer;
        use crate::session::store::capabilities::REVIEW_CHANGE_REVISION_COHORT_V1;

        let first = real_l0_repo();
        let second = real_l0_repo();
        let mut approved =
            dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(first.path())).unwrap();
        let other = dry_run_bulk_adoption(BulkAdoptionDryRunOptions::new(second.path())).unwrap();
        approved.roots.push(other.roots[0].clone());
        approved
            .roots
            .sort_by(|left, right| left.root_identity_hash.cmp(&right.root_identity_hash));
        let material = serde_json::json!({
            "schema": approved.schema,
            "roots": approved.roots,
            "anomalies": approved.anomalies,
            "requiresOwnerDecision": approved.requires_owner_decision,
            "ownerDecisionManifestHash": approved.owner_decision_manifest_hash,
            "writer": approved.writer,
            "claimOccurredAt": approved.claim_occurred_at,
            "signaturePolicy": approved.signature_policy,
        });
        approved.manifest_hash = sha256_json_prefixed(&material).unwrap();
        let first_identity =
            crate::session::store_identity(StoreIdentityOptions::new(first.path()))
                .unwrap()
                .store_identity;
        let cohort = approved
            .roots
            .iter()
            .find(|root| root.root_identity_hash == first_identity)
            .unwrap()
            .cohort_manifest_hash
            .clone()
            .unwrap();
        let backup_parent = tempfile::tempdir().unwrap();
        let backup = backup_parent.path().join("multi-root");
        let receipt = migrate_bulk_adoption(
            BulkAdoptionMigrationOptions::new(
                first.path(),
                approved.clone(),
                approved.manifest_hash.clone(),
                cohort,
                &backup,
                "multi-root",
            )
            .with_minimum_reader_ack(REVIEW_CHANGE_REVISION_COHORT_V1)
            .with_legacy_reader_unsupported_ack()
            .sign_with(TestEd25519Signer::from_seed([0x67; 32]))
            .with_fixed_occurred_at("2026-08-06T18:26:00Z")
            .with_derived_enabled(false),
        )
        .unwrap();
        assert_eq!(receipt.approved_dry_run_hash, approved.manifest_hash);
    }

    fn real_l0_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        git(repo.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            repo.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        git(repo.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.path().join("sample.txt"), "base\n").unwrap();
        git(repo.path(), &["add", "sample.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "base"]);
        std::fs::write(repo.path().join("sample.txt"), "private first bytes\n").unwrap();
        let first = crate::session::capture_review(
            crate::session::CaptureOptions::new(repo.path()).with_summary("first"),
        )
        .unwrap();
        std::fs::write(repo.path().join("sample.txt"), "private second bytes\n").unwrap();
        crate::session::capture_review(
            crate::session::CaptureOptions::new(repo.path())
                .with_summary("second")
                .with_supersedes(vec![first.revision_id]),
        )
        .unwrap();
        repo
    }

    fn empty_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        repo
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
