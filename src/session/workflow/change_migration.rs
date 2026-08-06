#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "bulk-adoption execution is qualification-only until public activation"
    )
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
#[cfg(any(test, feature = "bench"))]
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
#[cfg(any(test, feature = "bench"))]
use crate::session::store::capabilities::{
    BulkAdoptionCompletionV1, StoreCapabilityActivationV1, build_signed_activation,
    build_signed_completion, inspect_journal_records, publish_control_record,
};
use crate::session::store::resolution::resolve_change_read_store;
use crate::session::store::{BulkAdoptionManifestV1, ReservedCohortRecordV1};
use crate::session::{AuthorityCursorV2, StoreCapabilityStatus, StoreIdentityOptions};

pub const BULK_ADOPTION_DRY_RUN_SCHEMA_V1: &str = "pointbreak.bulk-adoption-dry-run.v1";

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

/// Fully signed, exact execution input retained by qualification support while
/// it exercises interruption and retry. Public product routes cannot construct
/// or execute this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg(any(test, feature = "bench"))]
pub(crate) struct BulkAdoptionExecutionPlanV1 {
    schema: String,
    root_identity_hash: String,
    manifest_hash: String,
    source_authority_cursor: AuthorityCursorV2,
    events: Vec<ShoreEvent>,
    activation: StoreCapabilityActivationV1,
    completion: BulkAdoptionCompletionV1,
}

#[cfg(any(test, feature = "bench"))]
#[allow(
    clippy::too_many_arguments,
    reason = "the frozen qualification plan names each signed time and identity input explicitly"
)]
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
        schema: "pointbreak.bulk-adoption-execution-plan.v1".to_owned(),
        root_identity_hash: inventory.root_identity_hash,
        manifest_hash,
        source_authority_cursor: inventory.source_authority_cursor,
        events: root.planned_events,
        activation,
        completion,
    })
}

/// Execute a previously frozen plan against one disposable qualification root.
/// `interrupt_after_append` counts activation, each Change event, then
/// completion. Retrying the same plan converges through exclusive-create
/// records and validates the exact M1/L2 authority after every phase.
#[cfg(any(test, feature = "bench"))]
pub(crate) fn execute_bulk_adoption_for_qualification(
    repo: &Path,
    plan: &BulkAdoptionExecutionPlanV1,
    interrupt_after_append: Option<usize>,
) -> Result<()> {
    if plan.schema != "pointbreak.bulk-adoption-execution-plan.v1" {
        return Err(invalid_migration(
            "unsupported bulk-adoption execution plan",
        ));
    }
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

            let error = execute_bulk_adoption_for_qualification(repo.path(), &plan, Some(boundary))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("injected migration interruption")
            );
            execute_bulk_adoption_for_qualification(repo.path(), &plan, None).unwrap();
            assert!(matches!(
                resolve_change_read_store(repo.path()).unwrap().1.status,
                StoreCapabilityStatus::Ready { .. }
            ));
        }
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
