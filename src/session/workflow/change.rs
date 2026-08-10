//! Retryable Change mutation workflows.
//!
//! A Change operation plans immutable events from a caller-retained operation
//! id before its first append. Retrying the same plan reuses every claim nonce
//! and idempotency key; the local operation receipt is recovery state only and
//! never participates in Change projection authority.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::model::{
    ActorId, ChangeId, ChangeIdentityDescriptorV1, ChangeMembershipClaimId,
    ChangeRevisionRelationClaimId, JournalId, ObjectId, ReviewEndpoint, RevisionId, RevisionRefV1,
};
use crate::session::event::{
    ChangeLinkRelationV1, EventPayload, EventTarget, ShoreEvent, build_change_declared,
    build_change_link_asserted, build_membership_asserted, build_membership_withdrawn,
    build_revision_relation_asserted, build_revision_relation_withdrawn,
};
use crate::session::store::capabilities::preflight_change_writer;
use crate::session::store::resolution::resolve_change_write_store;
use crate::session::{
    BestEffortSkipSink, EventSigningOptions, EventWriteOutcome, current_timestamp,
    sign_event_if_requested, writer_from_options,
};

pub const CHANGE_OPERATION_SCHEMA_V1: &str = "pointbreak.change-operation.v1";
const CHANGE_CAPTURE_CHECKPOINT_SCHEMA_V1: &str = "pointbreak.change-capture-checkpoint.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOperationEventOutcomeV1 {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeOperationEventReceiptV1 {
    pub event_type: String,
    pub idempotency_key: String,
    pub event_id: crate::model::EventId,
    pub outcome: ChangeOperationEventOutcomeV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeOperationReceiptV1 {
    pub schema: String,
    pub operation_id: String,
    pub change_id: ChangeId,
    pub events: Vec<ChangeOperationEventReceiptV1>,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeOperationPlanV1 {
    schema: String,
    operation_id: String,
    request_hash: String,
    change_id: ChangeId,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_revision: Option<RevisionRefV1>,
    graph_preconditions: Vec<ChangeGraphPreconditionV1>,
    events: Vec<ShoreEvent>,
}

/// Durable recovery binding written before a Change capture proposal. It is
/// deliberately not semantic authority: the journal remains authoritative,
/// while this file prevents an operation retry from silently adopting a
/// different source snapshot after an interrupted proposal append.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeCaptureCheckpointV1 {
    schema: String,
    operation_id: String,
    request_hash: String,
    change_id: ChangeId,
    revision: RevisionRefV1,
    graph_preconditions: Vec<ChangeGraphPreconditionV1>,
    predecessors: Vec<RevisionRefV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeGraphPreconditionV1 {
    change_id: ChangeId,
    graph_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeCreateOptions {
    repo: PathBuf,
    operation_id: String,
    identity_descriptor: ChangeIdentityDescriptorV1,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl ChangeCreateOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        identity_descriptor: ChangeIdentityDescriptorV1,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            identity_descriptor,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }

    pub fn with_actor_id(mut self, actor_id: ActorId) -> Self {
        self.actor_id = Some(actor_id);
        self
    }
}

macro_rules! impl_change_signing_options {
    ($($ty:ty),+ $(,)?) => {$(
        impl $ty {
            pub fn sign_with<S>(mut self, signer: S) -> Self
            where
                S: EventSigner + Send + Sync + 'static,
            {
                self.signing = EventSigningOptions::sign_with(signer);
                self
            }

            pub fn sign_with_best_effort<S>(
                mut self,
                signer: S,
                skip_sink: BestEffortSkipSink,
            ) -> Self
            where
                S: EventSigner + Send + Sync + 'static,
            {
                self.signing = EventSigningOptions::sign_with_best_effort(signer, skip_sink);
                self
            }
        }
    )+};
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeMembershipOptions {
    repo: PathBuf,
    operation_id: String,
    change_id: ChangeId,
    revision_id: RevisionId,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl ChangeMembershipOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        change_id: ChangeId,
        revision_id: RevisionId,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            change_id,
            revision_id,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeMembershipWithdrawalOptions {
    repo: PathBuf,
    operation_id: String,
    membership_claim_id: ChangeMembershipClaimId,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl ChangeMembershipWithdrawalOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        membership_claim_id: ChangeMembershipClaimId,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            membership_claim_id,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRelationOptions {
    repo: PathBuf,
    operation_id: String,
    change_id: ChangeId,
    successor: RevisionRefV1,
    predecessor: RevisionRefV1,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl ChangeRelationOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        change_id: ChangeId,
        successor: RevisionRefV1,
        predecessor: RevisionRefV1,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            change_id,
            successor,
            predecessor,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRelationWithdrawalOptions {
    repo: PathBuf,
    operation_id: String,
    relation_claim_id: ChangeRevisionRelationClaimId,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl ChangeRelationWithdrawalOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        relation_claim_id: ChangeRevisionRelationClaimId,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            relation_claim_id,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeLinkOptions {
    repo: PathBuf,
    operation_id: String,
    first_change_id: ChangeId,
    second_change_id: ChangeId,
    relation: ChangeLinkRelationV1,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAdvanceV1 {
    Replace,
    Parallel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChangeCaptureTransitionV1 {
    Initial {
        identity_descriptor: ChangeIdentityDescriptorV1,
    },
    Advance {
        review_cursor: String,
        advance: ChangeAdvanceV1,
        additional_predecessors: Vec<RevisionRefV1>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeCaptureOptions {
    operation_id: String,
    capture: super::capture::CaptureOptions,
    transition: ChangeCaptureTransitionV1,
    #[cfg(test)]
    interruption_after_append: Option<usize>,
}

impl ChangeCaptureOptions {
    pub fn initial(
        operation_id: impl Into<String>,
        capture: super::capture::CaptureOptions,
        identity_descriptor: ChangeIdentityDescriptorV1,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            capture,
            transition: ChangeCaptureTransitionV1::Initial {
                identity_descriptor,
            },
            #[cfg(test)]
            interruption_after_append: None,
        }
    }

    pub fn advance(
        operation_id: impl Into<String>,
        capture: super::capture::CaptureOptions,
        review_cursor: impl Into<String>,
        advance: ChangeAdvanceV1,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            capture,
            transition: ChangeCaptureTransitionV1::Advance {
                review_cursor: review_cursor.into(),
                advance,
                additional_predecessors: Vec::new(),
            },
            #[cfg(test)]
            interruption_after_append: None,
        }
    }

    pub fn with_additional_predecessor(mut self, predecessor: RevisionRefV1) -> Self {
        if let ChangeCaptureTransitionV1::Advance {
            additional_predecessors,
            ..
        } = &mut self.transition
        {
            additional_predecessors.push(predecessor);
        }
        self
    }

    #[cfg(test)]
    fn with_interruption_after_append(mut self, append_count: usize) -> Self {
        self.interruption_after_append = Some(append_count);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeCaptureReceiptV1 {
    pub schema: String,
    pub version: u32,
    pub operation_id: String,
    pub change_id: ChangeId,
    pub revision: ChangeCaptureRevisionV1,
    pub review_cursor: crate::session::ReviewCursorSelectionV1,
    pub diffstat: crate::session::CaptureDiffstat,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<crate::session::ProjectionDiagnostic>,
    pub revision_events_created: usize,
    pub revision_events_existing: usize,
    pub change_events: Vec<ChangeOperationEventReceiptV1>,
    pub complete: bool,
}

/// Exact Revision reference plus the capture metadata useful in the immediate
/// author receipt. The duplicate `id` is an output-only discovery alias; the
/// canonical `RevisionRefV1` wire and every persisted identity stay unchanged.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeCaptureRevisionV1 {
    pub id: RevisionId,
    pub revision_id: RevisionId,
    pub object_id: ObjectId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<ReviewEndpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReviewEndpoint>,
    pub object_artifact_content_hash: String,
}

impl ChangeCaptureRevisionV1 {
    pub fn exact_ref(&self) -> RevisionRefV1 {
        RevisionRefV1::new(
            self.revision_id.clone(),
            self.object_artifact_content_hash.clone(),
        )
        .expect("a capture receipt is built from a validated exact Revision")
    }
}

impl ChangeLinkOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        operation_id: impl Into<String>,
        first_change_id: ChangeId,
        second_change_id: ChangeId,
        relation: ChangeLinkRelationV1,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            operation_id: operation_id.into(),
            first_change_id,
            second_change_id,
            relation,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }
}

impl_change_signing_options!(
    ChangeCreateOptions,
    ChangeMembershipOptions,
    ChangeMembershipWithdrawalOptions,
    ChangeRelationOptions,
    ChangeRelationWithdrawalOptions,
    ChangeLinkOptions,
);

pub fn create_change(options: ChangeCreateOptions) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let write_store = resolve_change_write_store(&options.repo)?;
    preflight_change_writer(write_store.backend().journal().as_ref())?;

    let request_hash = sha256_json_prefixed(&serde_json::json!({
        "operation": "create_change_v1",
        "operationId": options.operation_id,
        "identityDescriptor": options.identity_descriptor,
    }))?;
    let plan_path = operation_plan_path(write_store.store_dir(), &options.operation_id);
    let plan = if plan_path.exists() {
        let bytes = fs::read(&plan_path)
            .map_err(|error| io_error("read Change operation plan", &plan_path, error))?;
        let plan: ChangeOperationPlanV1 = serde_json::from_slice(&bytes)?;
        if plan.schema != CHANGE_OPERATION_SCHEMA_V1
            || plan.operation_id != options.operation_id
            || plan.request_hash != request_hash
        {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "operation id is already bound to different Change inputs".to_owned(),
            });
        }
        plan
    } else {
        let claim_nonce = operation_nonce(&options.operation_id, "change-declaration")?;
        let payload = build_change_declared(options.identity_descriptor, claim_nonce)?;
        let change_id = payload.change_id.clone();
        let writer = writer_from_options(&options.repo, options.actor_id.as_ref());
        let event = operation_event(
            &options.operation_id,
            "change-declaration",
            payload,
            writer,
            current_timestamp(),
            &options.signing,
        )?;
        let plan = ChangeOperationPlanV1 {
            schema: CHANGE_OPERATION_SCHEMA_V1.to_owned(),
            operation_id: options.operation_id.clone(),
            request_hash,
            change_id,
            capture_revision: None,
            graph_preconditions: Vec::new(),
            events: vec![event],
        };
        persist_operation_plan(&plan_path, &plan)?;
        plan
    };

    // The plan is durable before the first append. Recheck the capability at
    // the append boundary so an L0/M1 transition cannot be crossed by a stale
    // workflow object.
    preflight_change_writer(write_store.backend().journal().as_ref())?;
    let event_store = write_store.event_store()?;
    let mut receipts = Vec::with_capacity(plan.events.len());
    for event in &plan.events {
        let outcome = event_store.record_change_event_once(event)?;
        receipts.push(event_receipt(event, outcome));
    }
    Ok(ChangeOperationReceiptV1 {
        schema: CHANGE_OPERATION_SCHEMA_V1.to_owned(),
        operation_id: plan.operation_id,
        change_id: plan.change_id,
        events: receipts,
        complete: true,
    })
}

pub fn join_revision_to_change(
    options: ChangeMembershipOptions,
) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let ready = ready_for_mutation(&options.repo)?;
    let change = change_for_mutation(&ready, &options.change_id)?;
    require_exact_revision_id(&ready, &options.revision_id)?;
    let graph_preconditions = vec![graph_precondition(change)?];
    let request_hash = operation_request_hash(
        "join_revision_to_change_v1",
        &options.operation_id,
        &serde_json::json!({
            "changeId": options.change_id,
            "revisionId": options.revision_id,
        }),
    )?;
    let payload = build_membership_asserted(
        &options.change_id,
        &options.revision_id,
        operation_nonce(&options.operation_id, "membership-assertion")?,
    )?;
    execute_single_event_operation(
        &options.repo,
        &options.operation_id,
        request_hash,
        options.change_id,
        graph_preconditions,
        "membership-assertion",
        payload,
        options.actor_id.as_ref(),
        &options.signing,
    )
}

pub fn withdraw_revision_from_change(
    options: ChangeMembershipWithdrawalOptions,
) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let ready = ready_for_mutation(&options.repo)?;
    let claim = ready
        .document_projection
        .membership_claims
        .iter()
        .find(|claim| claim.claim_id == options.membership_claim_id && claim.active)
        .ok_or_else(|| invalid_input("active Change membership claim is unavailable"))?;
    let change = change_for_mutation(&ready, &claim.change_id)?;
    let request_hash = operation_request_hash(
        "withdraw_revision_from_change_v1",
        &options.operation_id,
        &serde_json::json!({"membershipClaimId": options.membership_claim_id}),
    )?;
    let payload = build_membership_withdrawn(
        &options.membership_claim_id,
        operation_nonce(&options.operation_id, "membership-withdrawal")?,
    )?;
    execute_single_event_operation(
        &options.repo,
        &options.operation_id,
        request_hash,
        claim.change_id.clone(),
        vec![graph_precondition(change)?],
        "membership-withdrawal",
        payload,
        options.actor_id.as_ref(),
        &options.signing,
    )
}

pub fn assert_change_revision_relation(
    options: ChangeRelationOptions,
) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let ready = ready_for_mutation(&options.repo)?;
    let change = change_for_single_state_mutation(&ready, &options.change_id)?;
    for revision in [&options.successor, &options.predecessor] {
        require_exact_revision(&ready, revision)?;
        if !change.members.contains(&revision.revision_id) {
            return Err(invalid_input(
                "Change relation endpoints must both be exact active members",
            ));
        }
    }
    if !change
        .current_revisions
        .contains(&options.successor.revision_id)
    {
        return Err(invalid_input(
            "Change relation successor must be an exact current member",
        ));
    }
    if options.successor == options.predecessor {
        return Err(invalid_input("Change relation endpoints must be distinct"));
    }
    let request_hash = operation_request_hash(
        "assert_change_revision_relation_v1",
        &options.operation_id,
        &serde_json::json!({
            "changeId": options.change_id,
            "successor": options.successor,
            "predecessor": options.predecessor,
        }),
    )?;
    let payload = build_revision_relation_asserted(
        &options.change_id,
        options.successor,
        options.predecessor,
        operation_nonce(&options.operation_id, "relation-assertion")?,
    )?;
    execute_single_event_operation(
        &options.repo,
        &options.operation_id,
        request_hash,
        options.change_id,
        vec![graph_precondition(change)?],
        "relation-assertion",
        payload,
        options.actor_id.as_ref(),
        &options.signing,
    )
}

pub fn withdraw_change_revision_relation(
    options: ChangeRelationWithdrawalOptions,
) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let ready = ready_for_mutation(&options.repo)?;
    let claim = ready
        .document_projection
        .relation_claims
        .iter()
        .find(|claim| claim.claim_id == options.relation_claim_id && claim.active)
        .ok_or_else(|| invalid_input("active Change relation claim is unavailable"))?;
    let change = change_for_mutation(&ready, &claim.change_id)?;
    let request_hash = operation_request_hash(
        "withdraw_change_revision_relation_v1",
        &options.operation_id,
        &serde_json::json!({"relationClaimId": options.relation_claim_id}),
    )?;
    let payload = build_revision_relation_withdrawn(
        &options.relation_claim_id,
        operation_nonce(&options.operation_id, "relation-withdrawal")?,
    )?;
    execute_single_event_operation(
        &options.repo,
        &options.operation_id,
        request_hash,
        claim.change_id.clone(),
        vec![graph_precondition(change)?],
        "relation-withdrawal",
        payload,
        options.actor_id.as_ref(),
        &options.signing,
    )
}

pub fn link_changes(options: ChangeLinkOptions) -> Result<ChangeOperationReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let ready = ready_for_mutation(&options.repo)?;
    let first = change_for_mutation(&ready, &options.first_change_id)?;
    let second = change_for_mutation(&ready, &options.second_change_id)?;
    let request_hash = operation_request_hash(
        "link_changes_v1",
        &options.operation_id,
        &serde_json::json!({
            "firstChangeId": options.first_change_id,
            "secondChangeId": options.second_change_id,
            "relation": options.relation,
        }),
    )?;
    let payload = build_change_link_asserted(
        &options.first_change_id,
        &options.second_change_id,
        options.relation,
        operation_nonce(&options.operation_id, "change-link")?,
    )?;
    execute_single_event_operation(
        &options.repo,
        &options.operation_id,
        request_hash,
        options.first_change_id,
        vec![graph_precondition(first)?, graph_precondition(second)?],
        "change-link",
        payload,
        options.actor_id.as_ref(),
        &options.signing,
    )
}

pub fn capture_change_revision(options: ChangeCaptureOptions) -> Result<ChangeCaptureReceiptV1> {
    validate_operation_id(&options.operation_id)?;
    let repo = options.capture.repo().to_path_buf();
    let write_store = resolve_change_write_store(&repo)?;
    preflight_change_writer(write_store.backend().journal().as_ref())?;
    let transition_material = match &options.transition {
        ChangeCaptureTransitionV1::Initial {
            identity_descriptor,
        } => serde_json::json!({
            "kind": "initial",
            "identityDescriptor": identity_descriptor,
        }),
        ChangeCaptureTransitionV1::Advance {
            review_cursor,
            advance,
            additional_predecessors,
        } => serde_json::json!({
            "kind": "advance",
            "reviewCursor": review_cursor,
            "advance": advance,
            "additionalPredecessors": additional_predecessors,
        }),
    };
    let request_hash = operation_request_hash(
        "capture_change_revision_v1",
        &options.operation_id,
        &serde_json::json!({
            "capture": options.capture.operation_material(),
            "transition": transition_material,
        }),
    )?;
    let plan_path = operation_plan_path(write_store.store_dir(), &options.operation_id);
    if plan_path.exists() {
        let plan = read_matching_plan(&plan_path, &options.operation_id, &request_hash)?;
        let revision = plan
            .capture_revision
            .clone()
            .ok_or_else(|| invalid_input("capture operation plan has no exact Revision"))?;
        let operation = execute_operation_plan(&repo, &write_store, plan)?;
        return capture_receipt(&repo, revision, None, 0, 1, operation);
    }

    let ready = ready_for_mutation(&repo)?;
    let checkpoint_path = capture_checkpoint_path(write_store.store_dir(), &options.operation_id);
    let existing_checkpoint = if checkpoint_path.exists() {
        Some(read_matching_checkpoint(
            &checkpoint_path,
            &options.operation_id,
            &request_hash,
        )?)
    } else {
        None
    };
    let (change_id, graph_preconditions, predecessors) =
        if let Some(checkpoint) = &existing_checkpoint {
            validate_graph_precondition_values(&repo, &checkpoint.graph_preconditions, &[])?;
            (
                checkpoint.change_id.clone(),
                checkpoint.graph_preconditions.clone(),
                checkpoint.predecessors.clone(),
            )
        } else {
            capture_transition_inputs(&options.transition, &ready)?
        };

    let mut prepared_checkpoint = None;
    let capture = super::capture::capture_change_review_with_preappend(
        options.capture.clone(),
        |revision| {
            let checkpoint = ChangeCaptureCheckpointV1 {
                schema: CHANGE_CAPTURE_CHECKPOINT_SCHEMA_V1.to_owned(),
                operation_id: options.operation_id.clone(),
                request_hash: request_hash.clone(),
                change_id: change_id.clone(),
                revision: revision.clone(),
                graph_preconditions: graph_preconditions.clone(),
                predecessors: predecessors.clone(),
            };
            if let Some(expected) = &existing_checkpoint
                && expected != &checkpoint
            {
                return Err(invalid_input(
                    "capture source changed after the operation was prepared; restore the original source or use a new operation id",
                ));
            }
            persist_capture_checkpoint(&checkpoint_path, &checkpoint)?;
            prepared_checkpoint = Some(checkpoint);
            Ok(())
        },
    )?;
    let checkpoint = prepared_checkpoint
        .ok_or_else(|| invalid_input("capture did not bind an exact Revision checkpoint"))?;
    let revision = checkpoint.revision;
    if ready
        .projection
        .changes
        .get(&checkpoint.change_id)
        .is_some_and(|change| change.members.contains(&revision.revision_id))
    {
        return Err(invalid_input(
            "capture did not create a new Revision for this Change; keep the existing exact cursor",
        ));
    }
    if checkpoint.predecessors.contains(&revision) {
        return Err(invalid_input(
            "capture produced no new Revision; keep the existing exact cursor",
        ));
    }
    let writer = writer_from_options(&repo, options.capture.actor_id());
    let occurred_at = current_timestamp();
    #[cfg(test)]
    if options.interruption_after_append == Some(1) {
        return Err(invalid_input("injected interruption after append 1"));
    }

    let mut events = Vec::new();
    if let ChangeCaptureTransitionV1::Initial {
        identity_descriptor,
    } = &options.transition
    {
        events.push(operation_event(
            &options.operation_id,
            "change-declaration",
            build_change_declared(
                identity_descriptor.clone(),
                operation_nonce(&options.operation_id, "change-declaration")?,
            )?,
            writer.clone(),
            occurred_at.clone(),
            options.capture.signing(),
        )?);
    }
    events.push(operation_event(
        &options.operation_id,
        "membership-assertion",
        build_membership_asserted(
            &checkpoint.change_id,
            &revision.revision_id,
            operation_nonce(&options.operation_id, "membership-assertion")?,
        )?,
        writer.clone(),
        occurred_at.clone(),
        options.capture.signing(),
    )?);
    for (index, predecessor) in checkpoint.predecessors.iter().cloned().enumerate() {
        let step = format!("relation-assertion-{index}");
        events.push(operation_event(
            &options.operation_id,
            &step,
            build_revision_relation_asserted(
                &checkpoint.change_id,
                revision.clone(),
                predecessor,
                operation_nonce(&options.operation_id, &step)?,
            )?,
            writer.clone(),
            occurred_at.clone(),
            options.capture.signing(),
        )?);
    }
    let plan = ChangeOperationPlanV1 {
        schema: CHANGE_OPERATION_SCHEMA_V1.to_owned(),
        operation_id: options.operation_id,
        request_hash,
        change_id: checkpoint.change_id,
        capture_revision: Some(revision.clone()),
        graph_preconditions: checkpoint.graph_preconditions,
        events,
    };
    persist_operation_plan(&plan_path, &plan)?;
    let revision_events_created = capture.events_created;
    let revision_events_existing = capture.events_existing;
    #[cfg(test)]
    let operation = execute_operation_plan_with_limit(
        &repo,
        &write_store,
        plan,
        options
            .interruption_after_append
            .map(|append_count| append_count.saturating_sub(1)),
    )?;
    #[cfg(not(test))]
    let operation = execute_operation_plan(&repo, &write_store, plan)?;
    capture_receipt(
        &repo,
        revision,
        Some(capture.clone()),
        revision_events_created,
        revision_events_existing,
        operation,
    )
}

fn capture_transition_inputs(
    transition: &ChangeCaptureTransitionV1,
    ready: &crate::session::ChangeReaderReadyV1,
) -> Result<(ChangeId, Vec<ChangeGraphPreconditionV1>, Vec<RevisionRefV1>)> {
    match transition {
        ChangeCaptureTransitionV1::Initial {
            identity_descriptor,
        } => Ok((
            crate::model::derive_change_id(identity_descriptor)?,
            Vec::new(),
            Vec::new(),
        )),
        ChangeCaptureTransitionV1::Advance {
            review_cursor,
            advance,
            additional_predecessors,
        } => {
            let cursor = crate::session::ReviewCursorV1::decode_token(review_cursor)?;
            let change = change_for_single_state_mutation(ready, &cursor.change_id)?;
            crate::session::workflow::validate_review_cursor_for_transition(
                review_cursor,
                change,
                &ready.document_projection,
            )
            .map_err(|error| invalid_input(error.to_string()))?;
            let predecessors = match advance {
                ChangeAdvanceV1::Parallel => {
                    if !additional_predecessors.is_empty() {
                        return Err(invalid_input(
                            "parallel capture cannot name replacement predecessors",
                        ));
                    }
                    Vec::new()
                }
                ChangeAdvanceV1::Replace => {
                    let mut predecessors = vec![cursor.revision.clone()];
                    predecessors.extend(additional_predecessors.iter().cloned());
                    predecessors.sort();
                    predecessors.dedup();
                    for predecessor in &predecessors {
                        require_exact_revision(ready, predecessor)?;
                        if !change.current_revisions.contains(&predecessor.revision_id) {
                            return Err(invalid_input(
                                "every replacement predecessor must be an exact current member",
                            ));
                        }
                    }
                    predecessors
                }
            };
            Ok((
                cursor.change_id,
                vec![graph_precondition(change)?],
                predecessors,
            ))
        }
    }
}

fn capture_receipt(
    repo: &Path,
    revision: RevisionRefV1,
    capture: Option<crate::session::CaptureResult>,
    revision_events_created: usize,
    revision_events_existing: usize,
    operation: ChangeOperationReceiptV1,
) -> Result<ChangeCaptureReceiptV1> {
    let ready = ready_for_mutation(repo)?;
    let change = change_for_mutation(&ready, &operation.change_id)?;
    let shown = crate::session::show_revision_for_change_reader(
        crate::session::RevisionShowOptions::new(repo)
            .with_revision_id(revision.revision_id.clone())
            .with_exact(true),
    )?;
    let source_request = shown
        .revision
        .git_provenance
        .as_ref()
        .map(|provenance| match &provenance.source {
            crate::model::RevisionSource::GitWorktree { .. }
            | crate::model::RevisionSource::GitStaged { .. }
            | crate::model::RevisionSource::GitUnstaged { .. } => {
                crate::session::ReviewSourceRequestV1::Worktree
            }
            crate::model::RevisionSource::GitCommitRange { .. }
            | crate::model::RevisionSource::GitRootCommit { .. } => {
                let commit_oid = match &provenance.target {
                    crate::model::ReviewEndpoint::GitCommit { commit_oid, .. } => {
                        commit_oid.clone()
                    }
                    _ => unreachable!("commit-backed capture has a commit target"),
                };
                crate::session::ReviewSourceRequestV1::Commit(commit_oid)
            }
        })
        .unwrap_or(crate::session::ReviewSourceRequestV1::Captured);
    let source_binding = match crate::session::review_source_binding(
        repo,
        &revision,
        source_request,
    ) {
        Err(ShoreError::WorkflowInputInvalid { reason })
            if capture.is_none() && reason.starts_with("review_cursor_source_changed:") =>
        {
            return Err(invalid_input(
                "operation_source_changed: the repeated capture operation no longer matches its previously captured exact Revision; restore the original source or use a new --operation-id",
            ));
        }
        result => result?,
    };
    let review_cursor = crate::session::select_review_cursor(
        change,
        &ready.document_projection,
        Some(&revision.revision_id),
        false,
        source_binding,
    )
    .map_err(|error| invalid_input(error.to_string()))?;
    let (base, target) =
        shown
            .revision
            .git_provenance
            .as_ref()
            .map_or((None, None), |provenance| {
                (
                    Some(provenance.base.clone()),
                    Some(provenance.target.clone()),
                )
            });
    let diffstat = crate::session::diffstat_from_files(&shown.snapshot.files);
    let events_created_by_type = capture
        .as_ref()
        .map(|capture| capture.events_created_by_type.clone())
        .unwrap_or_default();
    let mut diagnostics = capture
        .as_ref()
        .map(|capture| capture.diagnostics.clone())
        .unwrap_or_default();
    if capture.is_none() {
        diagnostics.push(crate::session::ProjectionDiagnostic {
            code: "operation_retry_retained_revision".to_owned(),
            message: "the repeated operation returned its previously captured exact Revision; current source bytes were not adopted"
                .to_owned(),
        });
    }
    Ok(ChangeCaptureReceiptV1 {
        schema: "pointbreak.change-capture-receipt.v1".to_owned(),
        version: 1,
        operation_id: operation.operation_id,
        change_id: operation.change_id,
        revision: ChangeCaptureRevisionV1 {
            id: revision.revision_id.clone(),
            revision_id: revision.revision_id,
            object_id: shown.revision.object_id,
            summary: shown.revision.summary,
            base,
            target,
            object_artifact_content_hash: revision.object_artifact_content_hash,
        },
        review_cursor,
        diffstat,
        events_created: revision_events_created,
        events_existing: revision_events_existing,
        events_created_by_type,
        diagnostics,
        revision_events_created,
        revision_events_existing,
        change_events: operation.events,
        complete: operation.complete,
    })
}

fn read_matching_plan(
    path: &Path,
    operation_id: &str,
    request_hash: &str,
) -> Result<ChangeOperationPlanV1> {
    let bytes =
        fs::read(path).map_err(|error| io_error("read Change operation plan", path, error))?;
    let plan: ChangeOperationPlanV1 = serde_json::from_slice(&bytes)?;
    if plan.schema != CHANGE_OPERATION_SCHEMA_V1
        || plan.operation_id != operation_id
        || plan.request_hash != request_hash
    {
        return Err(invalid_input(
            "operation id is already bound to different Change inputs",
        ));
    }
    Ok(plan)
}

fn ready_for_mutation(repo: &Path) -> Result<crate::session::ChangeReaderReadyV1> {
    let state = crate::session::change_reader_state_for_repo(repo)?;
    state
        .ready()
        .cloned()
        .ok_or_else(|| match state.capability.status {
            crate::session::StoreCapabilityStatus::MigrationRequired => invalid_input(
                "migration_required; Change writes require an explicit completed store migration",
            ),
            crate::session::StoreCapabilityStatus::MigrationInProgress { .. } => invalid_input(
                "migration_in_progress; Change writes refuse partial capability authority",
            ),
            crate::session::StoreCapabilityStatus::Ready { .. } => {
                invalid_input("complete Change authority is unavailable")
            }
        })
}

fn change_for_mutation<'a>(
    ready: &'a crate::session::ChangeReaderReadyV1,
    change_id: &ChangeId,
) -> Result<&'a crate::session::ChangeView> {
    ready
        .projection
        .changes
        .get(change_id)
        .ok_or_else(|| invalid_input("Change is unavailable"))
}

fn change_for_single_state_mutation<'a>(
    ready: &'a crate::session::ChangeReaderReadyV1,
    change_id: &ChangeId,
) -> Result<&'a crate::session::ChangeView> {
    let change = change_for_mutation(ready, change_id)?;
    if matches!(
        change.topology,
        crate::session::ChangeTopologyV1::ReplacementDivergent
            | crate::session::ChangeTopologyV1::CycleConflicted
            | crate::session::ChangeTopologyV1::Incomplete
    ) || !change.diagnostics.is_empty()
    {
        return Err(invalid_input(
            "Change graph is incomplete or conflicted; exact correction is required",
        ));
    }
    Ok(change)
}

fn require_exact_revision_id(
    ready: &crate::session::ChangeReaderReadyV1,
    revision_id: &RevisionId,
) -> Result<()> {
    match ready.document_projection.revision_refs.get(revision_id) {
        Some(references) if references.len() == 1 => Ok(()),
        Some(_) => Err(invalid_input(
            "Revision identity has more than one artifact representation",
        )),
        None => Err(invalid_input("exact Revision is unavailable")),
    }
}

fn require_exact_revision(
    ready: &crate::session::ChangeReaderReadyV1,
    revision: &RevisionRefV1,
) -> Result<()> {
    if ready
        .document_projection
        .revision_refs
        .get(&revision.revision_id)
        .is_some_and(|references| references.contains(revision))
    {
        Ok(())
    } else {
        Err(invalid_input(
            "exact Revision and object-artifact hash do not match authoritative state",
        ))
    }
}

fn graph_precondition(change: &crate::session::ChangeView) -> Result<ChangeGraphPreconditionV1> {
    Ok(ChangeGraphPreconditionV1 {
        change_id: change.change_id.clone(),
        graph_token: crate::session::change_graph_token(change)?,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_single_event_operation<P: EventPayload>(
    repo: &Path,
    operation_id: &str,
    request_hash: String,
    change_id: ChangeId,
    graph_preconditions: Vec<ChangeGraphPreconditionV1>,
    step: &str,
    payload: P,
    actor_id: Option<&ActorId>,
    signing: &EventSigningOptions,
) -> Result<ChangeOperationReceiptV1> {
    let write_store = resolve_change_write_store(repo)?;
    preflight_change_writer(write_store.backend().journal().as_ref())?;
    let plan_path = operation_plan_path(write_store.store_dir(), operation_id);
    let plan = if plan_path.exists() {
        let bytes = fs::read(&plan_path)
            .map_err(|error| io_error("read Change operation plan", &plan_path, error))?;
        let plan: ChangeOperationPlanV1 = serde_json::from_slice(&bytes)?;
        if plan.schema != CHANGE_OPERATION_SCHEMA_V1
            || plan.operation_id != operation_id
            || plan.request_hash != request_hash
        {
            return Err(invalid_input(
                "operation id is already bound to different Change inputs",
            ));
        }
        plan
    } else {
        let event = operation_event(
            operation_id,
            step,
            payload,
            writer_from_options(repo, actor_id),
            current_timestamp(),
            signing,
        )?;
        let plan = ChangeOperationPlanV1 {
            schema: CHANGE_OPERATION_SCHEMA_V1.to_owned(),
            operation_id: operation_id.to_owned(),
            request_hash,
            change_id,
            capture_revision: None,
            graph_preconditions,
            events: vec![event],
        };
        persist_operation_plan(&plan_path, &plan)?;
        plan
    };
    execute_operation_plan(repo, &write_store, plan)
}

fn execute_operation_plan(
    repo: &Path,
    write_store: &crate::session::store::resolution::WriteStore,
    plan: ChangeOperationPlanV1,
) -> Result<ChangeOperationReceiptV1> {
    execute_operation_plan_with_limit(repo, write_store, plan, None)
}

fn execute_operation_plan_with_limit(
    repo: &Path,
    write_store: &crate::session::store::resolution::WriteStore,
    plan: ChangeOperationPlanV1,
    interruption_after_event: Option<usize>,
) -> Result<ChangeOperationReceiptV1> {
    validate_graph_preconditions(repo, &plan)?;
    preflight_change_writer(write_store.backend().journal().as_ref())?;
    let event_store = write_store.event_store()?;
    let mut events = Vec::with_capacity(plan.events.len());
    for event in &plan.events {
        let outcome = event_store.record_change_event_once(event)?;
        events.push(event_receipt(event, outcome));
        if interruption_after_event == Some(events.len()) {
            return Err(invalid_input(format!(
                "injected interruption after Change event {}",
                events.len()
            )));
        }
    }
    Ok(ChangeOperationReceiptV1 {
        schema: CHANGE_OPERATION_SCHEMA_V1.to_owned(),
        operation_id: plan.operation_id,
        change_id: plan.change_id,
        events,
        complete: true,
    })
}

fn validate_graph_preconditions(repo: &Path, plan: &ChangeOperationPlanV1) -> Result<()> {
    let planned_keys = plan
        .events
        .iter()
        .map(|event| event.idempotency_key.as_str())
        .collect::<Vec<_>>();
    validate_graph_precondition_values(repo, &plan.graph_preconditions, &planned_keys)
}

fn validate_graph_precondition_values(
    repo: &Path,
    graph_preconditions: &[ChangeGraphPreconditionV1],
    planned_keys: &[&str],
) -> Result<()> {
    if graph_preconditions.is_empty() {
        return Ok(());
    }
    let ready = ready_for_mutation(repo)?;
    let planned_keys = planned_keys
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if planned_keys.iter().all(|key| {
        ready
            .events()
            .iter()
            .any(|event| event.idempotency_key == *key)
    }) {
        // A fully visible retry has no remaining semantic append. It must be
        // allowed to report Existing even if later independent graph claims
        // have arrived since the original operation completed.
        return Ok(());
    }
    let baseline_events = ready
        .events()
        .iter()
        .filter(|event| !planned_keys.contains(event.idempotency_key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let projection = crate::session::project_changes(&baseline_events)?;
    for expected in graph_preconditions {
        let current = projection
            .changes
            .get(&expected.change_id)
            .ok_or_else(|| invalid_input("Change graph disappeared before append"))?;
        if crate::session::change_graph_token(current)? != expected.graph_token {
            return Err(invalid_input(
                "Change graph changed after the operation was planned; refresh the exact cursor",
            ));
        }
    }
    Ok(())
}

fn operation_request_hash(
    family: &str,
    operation_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "operation": family,
        "operationId": operation_id,
        "inputs": inputs,
    }))
}

fn invalid_input(reason: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: reason.into(),
    }
}

fn validate_operation_id(operation_id: &str) -> Result<()> {
    if operation_id.starts_with("change-operation:")
        && operation_id.len() <= 256
        && operation_id
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
    {
        Ok(())
    } else {
        Err(ShoreError::WorkflowInputInvalid {
            reason: "operation id must be a bounded change-operation: identifier".to_owned(),
        })
    }
}

fn operation_plan_path(store_dir: &Path, operation_id: &str) -> PathBuf {
    store_dir.join("operations").join(format!(
        "{}.json",
        sha256_bytes_hex(operation_id.as_bytes())
    ))
}

fn capture_checkpoint_path(store_dir: &Path, operation_id: &str) -> PathBuf {
    store_dir.join("operations").join(format!(
        "{}.capture.json",
        sha256_bytes_hex(operation_id.as_bytes())
    ))
}

fn read_matching_checkpoint(
    path: &Path,
    operation_id: &str,
    request_hash: &str,
) -> Result<ChangeCaptureCheckpointV1> {
    let bytes =
        fs::read(path).map_err(|error| io_error("read Change capture checkpoint", path, error))?;
    let checkpoint: ChangeCaptureCheckpointV1 = serde_json::from_slice(&bytes)?;
    if checkpoint.schema != CHANGE_CAPTURE_CHECKPOINT_SCHEMA_V1
        || checkpoint.operation_id != operation_id
        || checkpoint.request_hash != request_hash
    {
        return Err(invalid_input(
            "operation id is already bound to different Change capture inputs",
        ));
    }
    Ok(checkpoint)
}

fn persist_capture_checkpoint(path: &Path, checkpoint: &ChangeCaptureCheckpointV1) -> Result<()> {
    let parent = path.parent().expect("capture checkpoint path has a parent");
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create Change operation directory", parent, error))?;
    let bytes = crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(checkpoint)?)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(&bytes)
                .map_err(|error| io_error("write Change capture checkpoint", path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync Change capture checkpoint", path, error))?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing =
                read_matching_checkpoint(path, &checkpoint.operation_id, &checkpoint.request_hash)?;
            if existing == *checkpoint {
                Ok(())
            } else {
                Err(invalid_input(
                    "operation id raced with a different exact capture source",
                ))
            }
        }
        Err(error) => Err(io_error("create Change capture checkpoint", path, error)),
    }
}

fn persist_operation_plan(path: &Path, plan: &ChangeOperationPlanV1) -> Result<()> {
    let parent = path.parent().expect("operation path has a parent");
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create Change operation directory", parent, error))?;
    let bytes = crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(plan)?)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(&bytes)
                .map_err(|error| io_error("write Change operation plan", path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync Change operation plan", path, error))?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let bytes = fs::read(path)
                .map_err(|error| io_error("read Change operation plan", path, error))?;
            let existing: ChangeOperationPlanV1 = serde_json::from_slice(&bytes)?;
            if existing == *plan {
                Ok(())
            } else {
                Err(ShoreError::WorkflowInputInvalid {
                    reason: "operation id raced with different Change inputs".to_owned(),
                })
            }
        }
        Err(error) => Err(io_error("create Change operation plan", path, error)),
    }
}

fn operation_nonce(operation_id: &str, step: &str) -> Result<[u8; 32]> {
    let hash = sha256_json_prefixed(&serde_json::json!({
        "operationId": operation_id,
        "step": step,
    }))?;
    let hex = hash.strip_prefix("sha256:").expect("canonical hash prefix");
    let mut nonce = [0_u8; 32];
    for (index, byte) in nonce.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .expect("canonical hash is lowercase hexadecimal");
    }
    Ok(nonce)
}

fn operation_event<P: EventPayload>(
    operation_id: &str,
    step: &str,
    payload: P,
    writer: crate::session::event::Writer,
    occurred_at: String,
    signing: &EventSigningOptions,
) -> Result<ShoreEvent> {
    let mut event = ShoreEvent::new(
        payload.event_type(),
        format!(
            "change-operation:{}",
            sha256_json_prefixed(&serde_json::json!({
                "operationId": operation_id,
                "step": step,
            }))?
        ),
        EventTarget::for_journal(JournalId::new("journal:default")),
        writer,
        payload,
        occurred_at,
    )?;
    sign_event_if_requested(&mut event, signing)?;
    Ok(event)
}

fn event_receipt(event: &ShoreEvent, outcome: EventWriteOutcome) -> ChangeOperationEventReceiptV1 {
    ChangeOperationEventReceiptV1 {
        event_type: event.event_type.as_str().to_owned(),
        idempotency_key: event.idempotency_key.clone(),
        event_id: event.event_id.clone(),
        outcome: match outcome {
            EventWriteOutcome::Created => ChangeOperationEventOutcomeV1::Created,
            EventWriteOutcome::Existing | EventWriteOutcome::ExistingDivergentSignature => {
                ChangeOperationEventOutcomeV1::Existing
            }
        },
    }
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> ShoreError {
    ShoreError::Message(format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChangeIdentityDescriptorV1;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };

    #[test]
    fn create_change_retries_with_one_claim_identity() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        let backend = store.backend().clone();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();

        let first = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:test-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x41; 32]),
        ))
        .unwrap();
        let retry = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:test-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x41; 32]),
        ))
        .unwrap();

        assert_eq!(first.change_id, retry.change_id);
        assert!(first.complete && retry.complete);
        assert_eq!(first.events.len(), 1);
        assert_eq!(
            first.events[0].outcome,
            ChangeOperationEventOutcomeV1::Created
        );
        assert_eq!(
            retry.events[0].outcome,
            ChangeOperationEventOutcomeV1::Existing
        );
        assert_eq!(first.events[0].event_id, retry.events[0].event_id);
    }

    #[test]
    fn initial_capture_refuses_l0_before_proposal_or_operation_state() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            root.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        git(root.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.path().join("sample.txt"), "base\n").unwrap();
        git(root.path(), &["add", "sample.txt"]);
        git(root.path(), &["commit", "--quiet", "-m", "base"]);
        std::fs::write(root.path().join("sample.txt"), "changed\n").unwrap();
        let before = crate::session::store_capability_for_repo(root.path())
            .unwrap()
            .cursor;

        let error = capture_change_revision(ChangeCaptureOptions::initial(
            "change-operation:inactive-initial",
            crate::session::CaptureOptions::new(root.path()),
            ChangeIdentityDescriptorV1::opaque_nonce([0x40; 32]),
        ))
        .unwrap_err();
        assert!(error.to_string().contains("migration_required"), "{error}");

        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        assert!(!store.store_dir().join("operations").exists());
        assert_eq!(
            crate::session::store_capability_for_repo(root.path())
                .unwrap()
                .cursor,
            before
        );
    }

    #[test]
    fn membership_relation_withdrawal_and_link_retries_are_exact() {
        let root = ready_repo();
        let ready = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .clone();
        let revisions = ready
            .document_projection
            .revision_refs
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        assert!(revisions.len() >= 2);

        let created = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:test-workflow-change",
            ChangeIdentityDescriptorV1::opaque_nonce([0x51; 32]),
        ))
        .unwrap();
        let first_join = join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:test-join-a",
            created.change_id.clone(),
            revisions[0].revision_id.clone(),
        ))
        .unwrap();
        let second_join = join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:test-join-b",
            created.change_id.clone(),
            revisions[1].revision_id.clone(),
        ))
        .unwrap();
        assert_eq!(
            join_revision_to_change(ChangeMembershipOptions::new(
                root.path(),
                "change-operation:test-join-a",
                created.change_id.clone(),
                revisions[0].revision_id.clone(),
            ))
            .unwrap()
            .events[0]
                .outcome,
            ChangeOperationEventOutcomeV1::Existing
        );

        let first_claim = membership_claim_for_event(root.path(), &first_join.events[0].event_id);
        let second_claim = membership_claim_for_event(root.path(), &second_join.events[0].event_id);
        assert_ne!(first_claim, second_claim);

        let relation = assert_change_revision_relation(ChangeRelationOptions::new(
            root.path(),
            "change-operation:test-relation",
            created.change_id.clone(),
            revisions[1].clone(),
            revisions[0].clone(),
        ))
        .unwrap();
        let relation_claim = relation_claim_for_event(root.path(), &relation.events[0].event_id);
        assert!(
            withdraw_change_revision_relation(ChangeRelationWithdrawalOptions::new(
                root.path(),
                "change-operation:test-relation-withdraw",
                relation_claim,
            ))
            .unwrap()
            .complete
        );
        assert!(
            withdraw_revision_from_change(ChangeMembershipWithdrawalOptions::new(
                root.path(),
                "change-operation:test-membership-withdraw",
                first_claim,
            ))
            .unwrap()
            .complete
        );

        let other = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:test-other-change",
            ChangeIdentityDescriptorV1::opaque_nonce([0x52; 32]),
        ))
        .unwrap();
        assert!(
            link_changes(ChangeLinkOptions::new(
                root.path(),
                "change-operation:test-link",
                created.change_id,
                other.change_id,
                ChangeLinkRelationV1::RelatedWork,
            ))
            .unwrap()
            .complete
        );
    }

    #[test]
    fn initial_replace_parallel_and_unchanged_retry_keep_revision_identity_exact() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "third\n").unwrap();
        let initial = capture_change_revision(ChangeCaptureOptions::initial(
            "change-operation:test-initial-capture",
            crate::session::CaptureOptions::new(root.path()).with_summary("initial Change state"),
            ChangeIdentityDescriptorV1::opaque_nonce([0x61; 32]),
        ))
        .unwrap();
        assert_eq!(initial.change_events.len(), 2);
        assert!(proposal_supersedes(root.path(), &initial.revision.revision_id).is_empty());

        let retry = capture_change_revision(ChangeCaptureOptions::initial(
            "change-operation:test-initial-capture",
            crate::session::CaptureOptions::new(root.path()).with_summary("initial Change state"),
            ChangeIdentityDescriptorV1::opaque_nonce([0x61; 32]),
        ))
        .unwrap();
        assert_eq!(retry.revision, initial.revision);
        assert!(
            retry
                .change_events
                .iter()
                .all(|event| event.outcome == ChangeOperationEventOutcomeV1::Existing)
        );

        std::fs::write(root.path().join("sample.txt"), "fourth\n").unwrap();
        let replacement = capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-replace-capture",
            crate::session::CaptureOptions::new(root.path()).with_summary("replacement state"),
            initial.review_cursor.token.clone(),
            ChangeAdvanceV1::Replace,
        ))
        .unwrap();
        assert_ne!(replacement.revision, initial.revision);
        assert!(proposal_supersedes(root.path(), &replacement.revision.revision_id).is_empty());

        std::fs::write(root.path().join("sample.txt"), "fifth\n").unwrap();
        let parallel = capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-parallel-capture",
            crate::session::CaptureOptions::new(root.path()).with_summary("parallel state"),
            replacement.review_cursor.token.clone(),
            ChangeAdvanceV1::Parallel,
        ))
        .unwrap();
        assert!(proposal_supersedes(root.path(), &parallel.revision.revision_id).is_empty());

        let state = crate::session::change_reader_state_for_repo(root.path()).unwrap();
        let change = &state.ready().unwrap().projection.changes[&initial.change_id];
        assert_eq!(change.current_revisions.len(), 2);
        assert!(
            change
                .current_revisions
                .contains(&replacement.revision.revision_id)
        );
        assert!(
            change
                .current_revisions
                .contains(&parallel.revision.revision_id)
        );

        std::fs::write(root.path().join("sample.txt"), "sixth\n").unwrap();
        let consolidation = capture_change_revision(
            ChangeCaptureOptions::advance(
                "change-operation:test-consolidation-capture",
                crate::session::CaptureOptions::new(root.path()).with_summary("consolidated state"),
                parallel.review_cursor.token.clone(),
                ChangeAdvanceV1::Replace,
            )
            .with_additional_predecessor(replacement.revision.exact_ref()),
        )
        .unwrap();
        assert_eq!(
            consolidation
                .change_events
                .iter()
                .filter(|event| event.event_type == "change_revision_relation_asserted")
                .count(),
            2
        );
        let state = crate::session::change_reader_state_for_repo(root.path()).unwrap();
        let change = &state.ready().unwrap().projection.changes[&initial.change_id];
        assert_eq!(
            change.current_revisions,
            [consolidation.revision.revision_id.clone()].into()
        );

        let before = state.ready().unwrap().events().len();
        let error = capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-identical-replace",
            crate::session::CaptureOptions::new(root.path()).with_summary("consolidated state"),
            consolidation.review_cursor.token,
            ChangeAdvanceV1::Replace,
        ))
        .unwrap_err();
        assert!(
            error.to_string().contains("did not create a new Revision"),
            "{error}"
        );
        let after = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();
        assert_eq!(before, after);
    }

    #[test]
    fn initial_capture_retry_names_a_changed_operation_source() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "captured\n").unwrap();
        let options = || {
            ChangeCaptureOptions::initial(
                "change-operation:test-initial-retry-source",
                crate::session::CaptureOptions::new(root.path()).with_summary("captured state"),
                ChangeIdentityDescriptorV1::opaque_nonce([0x62; 32]),
            )
        };
        capture_change_revision(options()).unwrap();

        std::fs::write(root.path().join("sample.txt"), "changed\n").unwrap();
        let error = capture_change_revision(options()).unwrap_err();
        assert!(
            error.to_string().contains("operation_source_changed"),
            "{error}"
        );
        assert!(!error.to_string().contains("review_cursor_source_changed"));
        assert!(error.to_string().contains("new --operation-id"));
    }

    #[test]
    fn relation_correction_can_add_a_current_fork_edge_to_a_historical_member() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "fork root\n").unwrap();
        let initial = capture_change_revision(ChangeCaptureOptions::initial(
            "change-operation:test-correction-root",
            crate::session::CaptureOptions::new(root.path()),
            ChangeIdentityDescriptorV1::opaque_nonce([0x63; 32]),
        ))
        .unwrap();

        std::fs::write(root.path().join("sample.txt"), "first successor\n").unwrap();
        let first = capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-correction-first",
            crate::session::CaptureOptions::new(root.path()),
            initial.review_cursor.token,
            ChangeAdvanceV1::Replace,
        ))
        .unwrap();

        std::fs::write(root.path().join("sample.txt"), "parallel successor\n").unwrap();
        let parallel = capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-correction-parallel",
            crate::session::CaptureOptions::new(root.path()),
            first.review_cursor.token,
            ChangeAdvanceV1::Parallel,
        ))
        .unwrap();

        let receipt = assert_change_revision_relation(ChangeRelationOptions::new(
            root.path(),
            "change-operation:test-correction-relation",
            initial.change_id.clone(),
            parallel.revision.exact_ref(),
            initial.revision.exact_ref(),
        ))
        .unwrap();
        assert!(receipt.complete);

        let state = crate::session::change_reader_state_for_repo(root.path()).unwrap();
        let change = &state.ready().unwrap().projection.changes[&initial.change_id];
        assert_eq!(change.current_revisions.len(), 2);
        assert!(
            change
                .current_revisions
                .contains(&first.revision.revision_id)
        );
        assert!(
            change
                .current_revisions
                .contains(&parallel.revision.revision_id)
        );
    }

    #[test]
    fn interrupted_initial_capture_converges_after_every_append() {
        for append_count in 1..=3 {
            let root = ready_repo();
            std::fs::write(root.path().join("sample.txt"), "interrupted\n").unwrap();
            let operation_id = format!("change-operation:test-interruption-{append_count}");
            let interrupted = capture_change_revision(
                ChangeCaptureOptions::initial(
                    operation_id.clone(),
                    crate::session::CaptureOptions::new(root.path())
                        .with_summary("interrupted Change state"),
                    ChangeIdentityDescriptorV1::opaque_nonce([0x71; 32]),
                )
                .with_interruption_after_append(append_count),
            )
            .unwrap_err();
            assert!(interrupted.to_string().contains("injected interruption"));

            let resumed = capture_change_revision(ChangeCaptureOptions::initial(
                operation_id,
                crate::session::CaptureOptions::new(root.path())
                    .with_summary("interrupted Change state"),
                ChangeIdentityDescriptorV1::opaque_nonce([0x71; 32]),
            ))
            .unwrap();
            assert!(resumed.complete);
            let state = crate::session::change_reader_state_for_repo(root.path()).unwrap();
            let ready = state.ready().unwrap();
            let change = &ready.projection.changes[&resumed.change_id];
            assert_eq!(change.members.len(), 1);
            assert!(change.members.contains(&resumed.revision.revision_id));
            assert_eq!(
                ready
                    .events()
                    .iter()
                    .filter(|event| {
                        event.event_type.as_str() == "work_object_proposed"
                            && event.subject_revision_id().ok().flatten().as_ref()
                                == Some(&resumed.revision.revision_id)
                    })
                    .count(),
                1
            );
        }
    }

    #[test]
    fn interrupted_capture_refuses_a_different_source_before_append() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "prepared\n").unwrap();
        let options = || {
            ChangeCaptureOptions::initial(
                "change-operation:test-source-race",
                crate::session::CaptureOptions::new(root.path()).with_summary("prepared state"),
                ChangeIdentityDescriptorV1::opaque_nonce([0x72; 32]),
            )
        };
        capture_change_revision(options().with_interruption_after_append(1)).unwrap_err();
        let before = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();

        std::fs::write(root.path().join("sample.txt"), "different\n").unwrap();
        let error = capture_change_revision(options()).unwrap_err();
        assert!(
            error.to_string().contains("capture source changed"),
            "{error}"
        );
        let after = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();
        assert_eq!(before, after);
    }

    #[test]
    fn source_change_during_capture_preflight_refuses_before_append() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "prepared\n").unwrap();
        let before = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();

        let error = super::super::capture::capture_change_review_with_preappend(
            crate::session::CaptureOptions::new(root.path()),
            |_| {
                std::fs::write(root.path().join("sample.txt"), "raced\n").unwrap();
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("source changed before append"));
        let after = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();
        assert_eq!(before, after);
    }

    #[test]
    fn exact_fact_cursor_writes_to_l2_and_stale_graph_refuses_before_append() {
        let root = ready_repo();
        std::fs::write(root.path().join("sample.txt"), "fact source\n").unwrap();
        let initial = capture_change_revision(ChangeCaptureOptions::initial(
            "change-operation:test-fact-cursor-initial",
            crate::session::CaptureOptions::new(root.path()).with_summary("fact source"),
            ChangeIdentityDescriptorV1::opaque_nonce([0x73; 32]),
        ))
        .unwrap();
        let fact = crate::session::record_observation(
            crate::session::ObservationAddOptions::new(root.path())
                .with_review_cursor(initial.review_cursor.token.clone())
                .with_track("track:author")
                .with_title("exact fact"),
        )
        .unwrap();
        assert_eq!(fact.revision_id, initial.revision.revision_id);
        crate::session::record_validation_check(
            crate::session::ValidationAddOptions::new(root.path())
                .with_review_cursor(initial.review_cursor.token.clone())
                .with_track("track:author")
                .with_check_name("exact check")
                .with_status(crate::model::ValidationStatus::Passed),
        )
        .unwrap();
        let request = crate::session::open_input_request(
            crate::session::InputRequestOpenOptions::new(root.path())
                .with_review_cursor(initial.review_cursor.token.clone())
                .with_track("track:author")
                .with_title("exact request")
                .with_reason_code(
                    crate::session::event::InputRequestReasonCode::ManualDecisionRequired,
                ),
        )
        .unwrap();
        crate::session::respond_input_request(
            crate::session::InputRequestRespondOptions::new(
                root.path(),
                request.input_request_id.clone(),
            )
            .with_outcome(crate::session::event::InputRequestResponseOutcome::Approved),
        )
        .unwrap();
        crate::session::record_assessment(
            crate::session::AssessmentAddOptions::new(root.path())
                .with_review_cursor(initial.review_cursor.token.clone())
                .with_track("track:reviewer")
                .with_assessment(crate::session::event::ReviewAssessment::Accepted),
        )
        .unwrap();
        assert_eq!(
            crate::session::list_observations(
                crate::session::ObservationListOptions::new(root.path())
                    .with_exact_revision_id(initial.revision.revision_id.clone()),
            )
            .unwrap()
            .observations
            .len(),
            1
        );
        assert_eq!(
            crate::session::list_validation_checks(
                crate::session::ValidationListOptions::new(root.path())
                    .with_exact_revision_id(initial.revision.revision_id.clone()),
            )
            .unwrap()
            .validation_checks
            .len(),
            1
        );
        assert_eq!(
            crate::session::list_input_requests(
                crate::session::InputRequestListOptions::new(root.path())
                    .with_exact_revision_id(initial.revision.revision_id.clone())
                    .with_status(crate::session::InputRequestStatusFilter::All),
            )
            .unwrap()
            .input_requests
            .len(),
            1
        );
        assert_eq!(
            crate::session::show_assessments(
                crate::session::AssessmentShowOptions::new(root.path())
                    .with_exact_revision_id(initial.revision.revision_id.clone()),
            )
            .unwrap()
            .assessments
            .len(),
            1
        );

        std::fs::write(root.path().join("sample.txt"), "parallel source\n").unwrap();
        capture_change_revision(ChangeCaptureOptions::advance(
            "change-operation:test-fact-cursor-parallel",
            crate::session::CaptureOptions::new(root.path()).with_summary("parallel source"),
            initial.review_cursor.token.clone(),
            ChangeAdvanceV1::Parallel,
        ))
        .unwrap();
        let before = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();
        let error = crate::session::record_observation(
            crate::session::ObservationAddOptions::new(root.path())
                .with_review_cursor(initial.review_cursor.token)
                .with_track("track:author")
                .with_title("stale fact"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("change_graph_stale"), "{error}");
        let after = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .events()
            .len();
        assert_eq!(before, after);
    }

    fn ready_repo() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            root.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        git(root.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.path().join("sample.txt"), "base\n").unwrap();
        git(root.path(), &["add", "sample.txt"]);
        git(root.path(), &["commit", "--quiet", "-m", "base"]);
        std::fs::write(root.path().join("sample.txt"), "first\n").unwrap();
        crate::session::capture_worktree_review(crate::session::CaptureOptions::new(root.path()))
            .unwrap();
        std::fs::write(root.path().join("sample.txt"), "second\n").unwrap();
        crate::session::capture_worktree_review(crate::session::CaptureOptions::new(root.path()))
            .unwrap();

        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        root
    }

    fn membership_claim_for_event(
        repo: &Path,
        event_id: &crate::model::EventId,
    ) -> ChangeMembershipClaimId {
        let state = crate::session::change_reader_state_for_repo(repo).unwrap();
        let event = state
            .ready()
            .unwrap()
            .events()
            .iter()
            .find(|event| &event.event_id == event_id)
            .unwrap();
        serde_json::from_value::<crate::session::event::ChangeMembershipAssertedPayload>(
            event.payload.clone(),
        )
        .unwrap()
        .membership_claim_id
    }

    fn relation_claim_for_event(
        repo: &Path,
        event_id: &crate::model::EventId,
    ) -> ChangeRevisionRelationClaimId {
        let state = crate::session::change_reader_state_for_repo(repo).unwrap();
        let event = state
            .ready()
            .unwrap()
            .events()
            .iter()
            .find(|event| &event.event_id == event_id)
            .unwrap();
        serde_json::from_value::<crate::session::event::ChangeRevisionRelationAssertedPayload>(
            event.payload.clone(),
        )
        .unwrap()
        .relation_claim_id
    }

    fn proposal_supersedes(repo: &Path, revision_id: &RevisionId) -> Vec<RevisionId> {
        let state = crate::session::change_reader_state_for_repo(repo).unwrap();
        let event = state
            .ready()
            .unwrap()
            .events()
            .iter()
            .find(|event| {
                if event.event_type != crate::session::event::EventType::WorkObjectProposed {
                    return false;
                }
                let payload: crate::session::event::WorkObjectProposedPayload =
                    serde_json::from_value(event.payload.clone()).unwrap();
                matches!(
                    payload.work_object,
                    crate::session::event::WorkObjectProposal::Revision { ref revision, .. }
                        if &revision.id == revision_id
                )
            })
            .unwrap();
        let payload: crate::session::event::WorkObjectProposedPayload =
            serde_json::from_value(event.payload.clone()).unwrap();
        let crate::session::event::WorkObjectProposal::Revision { supersedes, .. } =
            payload.work_object
        else {
            panic!("selected proposal is not a Revision")
        };
        supersedes
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
