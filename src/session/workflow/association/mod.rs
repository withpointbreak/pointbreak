//! Library write path for the commit-range association/withdrawal family.
//!
//! The four write workflows build, sign, and record the association events the
//! CLI and capture auto-record delegate to, plus a projection read for listing.
//! Identity is track-free and writer-free (the builders take no track/writer
//! argument); the track rides on the envelope only. Withdrawals record
//! unconditionally — a missing referent is the expected cross-peer case, not an
//! error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::git::{git_commit_tree_oid, git_rev_parse_commit_oid};
use crate::model::{
    ActorId, CommitAssociationId, EventId, RefAssociationId, ReviewEndpoint, ReviewTargetRef,
    RevisionId, TargetRef,
};
use crate::session::event::{
    EventPayload, EventTarget, EventType, RevisionCommitAssociatedPayload,
    RevisionCommitWithdrawnPayload, RevisionRefAssociatedPayload, RevisionRefWithdrawnPayload,
    ShoreEvent, build_commit_association_id, build_commit_withdrawal_id, build_ref_association_id,
    build_ref_withdrawal_id,
};
use crate::session::observation::{
    CurrentRevisionContext, RevisionScope, RevisionSelection, resolve_revision, validated_track_id,
};
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store::resolution::{
    prepare_write_landing, resolve_read_store, resolve_write_store, resolve_write_validation_store,
};
use crate::session::{
    BestEffortSkipSink, CurrentCommitAssociation, CurrentRefAssociation, EventSigningOptions,
    EventStore, EventWriteOutcome, RevisionCommitRangeProjection, RevisionCommitRangeView,
    WithdrawnCommitAssociation, WithdrawnRefAssociation, current_timestamp,
    sign_event_if_requested, writer_from_options,
};
use crate::storage::{Durability, LocalStorage};

/// Which axis a listing or filter applies to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationAxis {
    Commit,
    Ref,
}

macro_rules! association_write_builders {
    ($name:ident) => {
        impl $name {
            pub fn with_revision_id(mut self, id: RevisionId) -> Self {
                self.revision_id = Some(id);
                self
            }
            pub fn with_track(mut self, track: impl Into<String>) -> Self {
                self.track = Some(track.into());
                self
            }

            pub fn with_actor_id(mut self, actor_id: ActorId) -> Self {
                self.actor_id = Some(actor_id);
                self
            }

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
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociateCommitOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    track: Option<String>,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
    commit: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawCommitOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    track: Option<String>,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
    commit_association_id: CommitAssociationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociateRefOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    track: Option<String>,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
    ref_name: String,
    head_oid: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawRefOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    track: Option<String>,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
    ref_association_id: RefAssociationId,
}

impl AssociateCommitOptions {
    pub fn new(repo: impl AsRef<Path>, commit: impl Into<String>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            track: None,
            actor_id: None,
            signing: EventSigningOptions::default(),
            commit: commit.into(),
        }
    }
}

association_write_builders!(AssociateCommitOptions);
association_write_builders!(WithdrawCommitOptions);
association_write_builders!(AssociateRefOptions);
association_write_builders!(WithdrawRefOptions);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociateCommitResult {
    pub revision_id: RevisionId,
    pub commit_association_id: CommitAssociationId,
    pub commit_oid: String,
    pub tree_oid: String,
    pub event_id: EventId,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawCommitResult {
    pub revision_id: RevisionId,
    pub commit_withdrawal_id: crate::model::CommitWithdrawalId,
    pub commit_association_id: CommitAssociationId,
    pub event_id: EventId,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociateRefResult {
    pub revision_id: RevisionId,
    pub ref_association_id: RefAssociationId,
    pub ref_name: String,
    pub head_oid: String,
    pub event_id: EventId,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawRefResult {
    pub revision_id: RevisionId,
    pub ref_withdrawal_id: crate::model::RefWithdrawalId,
    pub ref_association_id: RefAssociationId,
    pub event_id: EventId,
    pub events_created: usize,
    pub events_existing: usize,
    pub events_created_by_type: BTreeMap<String, usize>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAssociationsOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    axis: Option<AssociationAxis>,
    current_only: bool,
}

impl ListAssociationsOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            axis: None,
            current_only: false,
        }
    }

    pub fn with_revision_id(mut self, id: RevisionId) -> Self {
        self.revision_id = Some(id);
        self
    }
    pub fn with_axis(mut self, axis: AssociationAxis) -> Self {
        self.axis = Some(axis);
        self
    }

    pub fn current_only(mut self, current_only: bool) -> Self {
        self.current_only = current_only;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAssociationsResult {
    pub revision_id: RevisionId,
    pub anchored: bool,
    pub current_commits: Vec<CurrentCommitAssociation>,
    pub current_refs: Vec<CurrentRefAssociation>,
    pub withdrawn_commits: Vec<WithdrawnCommitAssociation>,
    pub withdrawn_refs: Vec<WithdrawnRefAssociation>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

impl WithdrawCommitOptions {
    pub fn new(repo: impl AsRef<Path>, commit_association_id: CommitAssociationId) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            track: None,
            actor_id: None,
            signing: EventSigningOptions::default(),
            commit_association_id,
        }
    }
}

impl AssociateRefOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        ref_name: impl Into<String>,
        head_oid: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            track: None,
            actor_id: None,
            signing: EventSigningOptions::default(),
            ref_name: ref_name.into(),
            head_oid: head_oid.into(),
        }
    }
}

impl WithdrawRefOptions {
    pub fn new(repo: impl AsRef<Path>, ref_association_id: RefAssociationId) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            track: None,
            actor_id: None,
            signing: EventSigningOptions::default(),
            ref_association_id,
        }
    }
}

/// Canonicalize a ref to its full form so a branch yields one stable id
/// regardless of entry path: `feat/x` → `refs/heads/feat/x`, an already-full
/// `refs/...` ref is unchanged. Shared by the write path and the read filters so
/// a short name passed on either side matches the stored full `ref_name`.
pub(crate) fn normalize_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Build a `RevisionRefAssociated` event for a known review unit. The single
/// construction site for the ref-axis event: `associate_ref` and the capture-time
/// auto-record share it so they converge on one identity. `track_id` is
/// envelope-only — the id/key fold neither track nor writer.
pub(crate) fn build_ref_association_event(
    journal_id: &crate::model::JournalId,
    revision_id: &RevisionId,
    ref_name: &str,
    head_oid: &str,
    track_id: Option<crate::model::TrackId>,
    writer: crate::session::event::Writer,
    occurred_at: impl Into<String>,
) -> Result<ShoreEvent> {
    let full_ref = normalize_ref(ref_name);
    let ref_association_id = build_ref_association_id(revision_id, &full_ref, head_oid)?;
    let key = RevisionRefAssociatedPayload::idempotency_key(revision_id, &full_ref, head_oid);
    let payload = RevisionRefAssociatedPayload {
        ref_association_id,
        target: ReviewTargetRef::Revision {
            revision_id: revision_id.clone(),
        },
        ref_name: full_ref,
        head_oid: head_oid.to_owned(),
    };
    ShoreEvent::new(
        EventType::RevisionRefAssociated,
        key,
        EventTarget::for_subject(
            journal_id.clone(),
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            }),
            track_id,
        )?,
        writer,
        payload,
        occurred_at,
    )
}

pub fn associate_commit(options: AssociateCommitOptions) -> Result<AssociateCommitResult> {
    let mut endpoint = None;
    let mut association_id = None;
    let outcome = record_association(
        &options.repo,
        options.revision_id.as_ref(),
        options.track.as_deref(),
        options.actor_id.as_ref(),
        &options.signing,
        |revision_id, worktree_root| {
            let commit_oid = git_rev_parse_commit_oid(worktree_root, &options.commit)?;
            let tree_oid = git_commit_tree_oid(worktree_root, &commit_oid)?;
            let commit_association_id = build_commit_association_id(revision_id, &commit_oid)?;
            let key = RevisionCommitAssociatedPayload::idempotency_key(revision_id, &commit_oid);
            let payload = RevisionCommitAssociatedPayload {
                commit_association_id: commit_association_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                commit: ReviewEndpoint::GitCommit {
                    commit_oid: commit_oid.clone(),
                    tree_oid: tree_oid.clone(),
                },
            };
            endpoint = Some((commit_oid, tree_oid));
            association_id = Some(commit_association_id);
            Ok((EventType::RevisionCommitAssociated, key, payload))
        },
    )?;
    let (commit_oid, tree_oid) = endpoint.expect("build closure resolved the commit endpoint");
    let mut diagnostics = outcome.diagnostics;
    if let Some(diagnostic) = commit_association_content_guard(
        &options.repo,
        &outcome.events,
        &outcome.revision_id,
        &commit_oid,
        &tree_oid,
    ) {
        diagnostics.push(diagnostic);
    }
    Ok(AssociateCommitResult {
        revision_id: outcome.revision_id,
        commit_association_id: association_id.expect("build closure set the association id"),
        commit_oid,
        tree_oid,
        event_id: outcome.event_id,
        events_created: outcome.events_created,
        events_existing: outcome.events_existing,
        events_created_by_type: outcome.events_created_by_type,
        diagnostics,
    })
}

/// The advisory guard's diagnostic code: the associated commit shares no
/// changed paths with the revision's captured snapshot — the classic
/// wrong-revision association (ADR-0014, 2026-07-09 amendment). Advisory and
/// never blocking: the association has already recorded when this is computed,
/// and it decorates only the write's result document, never the store.
pub const COMMIT_ASSOCIATION_CONTENT_MISMATCH_CODE: &str = "commit_association_content_mismatch";

/// One captured revision's content coordinates, scanned from the event set the
/// write already holds.
struct CaptureContent {
    object_id: crate::model::ObjectId,
    content_hash: String,
    /// `(commit_oid, tree_oid)` for a commit-anchored capture target.
    target: Option<(String, String)>,
}

/// Every captured revision's content coordinates, keyed by revision id. A
/// payload that fails to decode is skipped — the guard is advisory and must
/// never turn a malformed sibling event into a write failure.
fn revision_captures(events: &[ShoreEvent]) -> BTreeMap<RevisionId, CaptureContent> {
    use crate::session::event::{GitProvenance, WorkObjectProposal, WorkObjectProposedPayload};

    let mut captures = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
    {
        let Ok(payload) =
            serde_json::from_value::<WorkObjectProposedPayload>(event.payload.clone())
        else {
            continue;
        };
        let WorkObjectProposal::Revision {
            revision,
            object_artifact_content_hash,
            ..
        } = payload.work_object
        else {
            continue;
        };
        let target = match &revision.git_provenance {
            Some(GitProvenance {
                target:
                    ReviewEndpoint::GitCommit {
                        commit_oid,
                        tree_oid,
                    },
                ..
            }) => Some((commit_oid.clone(), tree_oid.clone())),
            _ => None,
        };
        captures.insert(
            revision.id.clone(),
            CaptureContent {
                object_id: revision.object_id,
                content_hash: object_artifact_content_hash,
                target,
            },
        );
    }
    captures
}

/// The path set a captured snapshot touches (old and new sides both count, so
/// renames overlap from either direction). `None` when the bound artifact is
/// unreadable — removed, suppressed, or not yet synced — which silences the
/// guard rather than failing the write.
fn snapshot_paths(
    repo: &Path,
    capture: &CaptureContent,
) -> Option<std::collections::HashSet<String>> {
    let artifact = crate::session::object_artifact::read_bound_object_artifact(
        repo,
        &capture.object_id,
        &capture.content_hash,
    )
    .ok()?;
    Some(
        artifact
            .snapshot
            .files
            .iter()
            .flat_map(|file| file.old_path.iter().chain(file.new_path.iter()))
            .cloned()
            .collect(),
    )
}

/// The advisory wrong-revision check for `associate_commit`: an association
/// whose commit touches none of the revision's captured paths earns a
/// `commit_association_content_mismatch` diagnostic, naming a better-matching
/// current revision when one exists. Every failure inside the guard degrades
/// to silence (`None`) — an unavailable git, an unreadable snapshot, or an
/// empty commit simply says nothing.
fn commit_association_content_guard(
    repo: &Path,
    events: &[ShoreEvent],
    revision_id: &RevisionId,
    commit_oid: &str,
    tree_oid: &str,
) -> Option<ProjectionDiagnostic> {
    let captures = revision_captures(events);
    let capture = captures.get(revision_id)?;

    // Exact landing: the commit is (or carries the tree of) the captured
    // target — nothing to compare.
    if let Some((target_commit, target_tree)) = &capture.target
        && (target_commit == commit_oid || target_tree == tree_oid)
    {
        return None;
    }

    let commit_paths: std::collections::HashSet<String> =
        crate::git::git_commit_changed_paths(repo, commit_oid)
            .ok()?
            .into_iter()
            .collect();
    if commit_paths.is_empty() {
        return None;
    }
    let captured_paths = snapshot_paths(repo, capture)?;
    if captured_paths.is_empty() || commit_paths.intersection(&captured_paths).next().is_some() {
        return None;
    }

    // Mismatch. Best-effort suggestion: the current (non-superseded) captured
    // revision whose snapshot overlaps the commit the most; ties break to the
    // smallest id for determinism.
    let supersession =
        crate::session::projection::supersession::SupersessionView::from_events(events).ok();
    let mut best: Option<(usize, &RevisionId)> = None;
    for (other_id, other_capture) in &captures {
        if other_id == revision_id {
            continue;
        }
        if let Some(view) = &supersession
            && !view.stale_by_superseding_revision(other_id).is_empty()
        {
            continue;
        }
        let Some(other_paths) = snapshot_paths(repo, other_capture) else {
            continue;
        };
        let overlap = commit_paths.intersection(&other_paths).count();
        if overlap > 0
            && best.is_none_or(|(best_overlap, best_id)| {
                overlap > best_overlap || (overlap == best_overlap && other_id < best_id)
            })
        {
            best = Some((overlap, other_id));
        }
    }

    let short_commit = &commit_oid[..commit_oid.len().min(12)];
    let mut message = format!(
        "commit {short_commit} touches none of the paths captured by revision {}",
        revision_id.as_str(),
    );
    match best {
        Some((_, better)) => {
            message.push_str(&format!(
                "; its paths overlap revision {} — verify the association targets \
                 the right revision, or withdraw it",
                better.as_str(),
            ));
        }
        None => {
            message.push_str("; verify the association targets the right revision, or withdraw it");
        }
    }
    Some(ProjectionDiagnostic {
        code: COMMIT_ASSOCIATION_CONTENT_MISMATCH_CODE.to_owned(),
        message,
    })
}

pub fn withdraw_commit(options: WithdrawCommitOptions) -> Result<WithdrawCommitResult> {
    let mut withdrawal_id = None;
    let outcome = record_association(
        &options.repo,
        options.revision_id.as_ref(),
        options.track.as_deref(),
        options.actor_id.as_ref(),
        &options.signing,
        |revision_id, _worktree_root| {
            let commit_withdrawal_id =
                build_commit_withdrawal_id(revision_id, &options.commit_association_id)?;
            let key =
                RevisionCommitWithdrawnPayload::idempotency_key(&options.commit_association_id);
            let payload = RevisionCommitWithdrawnPayload {
                commit_withdrawal_id: commit_withdrawal_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                commit_association_id: options.commit_association_id.clone(),
            };
            withdrawal_id = Some(commit_withdrawal_id);
            Ok((EventType::RevisionCommitWithdrawn, key, payload))
        },
    )?;
    Ok(WithdrawCommitResult {
        revision_id: outcome.revision_id,
        commit_withdrawal_id: withdrawal_id.expect("build closure set the withdrawal id"),
        commit_association_id: options.commit_association_id,
        event_id: outcome.event_id,
        events_created: outcome.events_created,
        events_existing: outcome.events_existing,
        events_created_by_type: outcome.events_created_by_type,
        diagnostics: outcome.diagnostics,
    })
}

pub fn associate_ref(options: AssociateRefOptions) -> Result<AssociateRefResult> {
    let full_ref = normalize_ref(&options.ref_name);
    let mut association_id = None;
    let outcome = record_association(
        &options.repo,
        options.revision_id.as_ref(),
        options.track.as_deref(),
        options.actor_id.as_ref(),
        &options.signing,
        |revision_id, _worktree_root| {
            let ref_association_id =
                build_ref_association_id(revision_id, &full_ref, &options.head_oid)?;
            let key = RevisionRefAssociatedPayload::idempotency_key(
                revision_id,
                &full_ref,
                &options.head_oid,
            );
            let payload = RevisionRefAssociatedPayload {
                ref_association_id: ref_association_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                ref_name: full_ref.clone(),
                head_oid: options.head_oid.clone(),
            };
            association_id = Some(ref_association_id);
            Ok((EventType::RevisionRefAssociated, key, payload))
        },
    )?;
    Ok(AssociateRefResult {
        revision_id: outcome.revision_id,
        ref_association_id: association_id.expect("build closure set the association id"),
        ref_name: full_ref,
        head_oid: options.head_oid,
        event_id: outcome.event_id,
        events_created: outcome.events_created,
        events_existing: outcome.events_existing,
        events_created_by_type: outcome.events_created_by_type,
        diagnostics: outcome.diagnostics,
    })
}

pub fn withdraw_ref(options: WithdrawRefOptions) -> Result<WithdrawRefResult> {
    let mut withdrawal_id = None;
    let outcome = record_association(
        &options.repo,
        options.revision_id.as_ref(),
        options.track.as_deref(),
        options.actor_id.as_ref(),
        &options.signing,
        |revision_id, _worktree_root| {
            let ref_withdrawal_id =
                build_ref_withdrawal_id(revision_id, &options.ref_association_id)?;
            let key = RevisionRefWithdrawnPayload::idempotency_key(&options.ref_association_id);
            let payload = RevisionRefWithdrawnPayload {
                ref_withdrawal_id: ref_withdrawal_id.clone(),
                target: ReviewTargetRef::Revision {
                    revision_id: revision_id.clone(),
                },
                ref_association_id: options.ref_association_id.clone(),
            };
            withdrawal_id = Some(ref_withdrawal_id);
            Ok((EventType::RevisionRefWithdrawn, key, payload))
        },
    )?;
    Ok(WithdrawRefResult {
        revision_id: outcome.revision_id,
        ref_withdrawal_id: withdrawal_id.expect("build closure set the withdrawal id"),
        ref_association_id: options.ref_association_id,
        event_id: outcome.event_id,
        events_created: outcome.events_created,
        events_existing: outcome.events_existing,
        events_created_by_type: outcome.events_created_by_type,
        diagnostics: outcome.diagnostics,
    })
}

pub fn list_associations(options: ListAssociationsOptions) -> Result<ListAssociationsResult> {
    let read_store = resolve_read_store(&options.repo)?;
    let events = EventStore::from_backend(read_store.backend()).list_events()?;
    let resolved = resolve_revision(
        &events,
        RevisionSelection::from_revision_seed(options.revision_id.as_ref()),
        &CurrentRevisionContext::for_repo(&options.repo)?,
        RevisionScope::default(),
    )?;
    let view = RevisionCommitRangeProjection::from_events(&events)?
        .unit(&resolved.revision_id)
        .cloned()
        .unwrap_or_else(|| empty_view(resolved.revision_id.clone()));

    let include_commits = options.axis != Some(AssociationAxis::Ref);
    let include_refs = options.axis != Some(AssociationAxis::Commit);
    let include_withdrawn = !options.current_only;

    Ok(ListAssociationsResult {
        revision_id: view.revision_id,
        anchored: view.anchored,
        current_commits: if include_commits {
            view.current_commits
        } else {
            Vec::new()
        },
        current_refs: if include_refs {
            view.current_refs
        } else {
            Vec::new()
        },
        withdrawn_commits: if include_commits && include_withdrawn {
            view.withdrawn_commits
        } else {
            Vec::new()
        },
        withdrawn_refs: if include_refs && include_withdrawn {
            view.withdrawn_refs
        } else {
            Vec::new()
        },
        diagnostics: view.diagnostics,
    })
}

fn empty_view(revision_id: RevisionId) -> RevisionCommitRangeView {
    RevisionCommitRangeView {
        revision_id,
        anchored: false,
        current_commits: Vec::new(),
        current_refs: Vec::new(),
        withdrawn_commits: Vec::new(),
        withdrawn_refs: Vec::new(),
        diagnostics: Vec::new(),
    }
}

struct AssociationWriteOutcome {
    revision_id: RevisionId,
    event_id: EventId,
    events_created: usize,
    events_existing: usize,
    events_created_by_type: BTreeMap<String, usize>,
    diagnostics: Vec<ProjectionDiagnostic>,
    /// The post-write event list the state re-projection already read; carried
    /// so `associate_commit`'s advisory content guard never re-reads the store.
    events: Vec<ShoreEvent>,
}

/// Shared scaffold: resolve the unit and write store, let the caller build the
/// payload (track-free), then build the envelope (track on it only), sign,
/// record unconditionally, and re-project state. Records always — withdrawals
/// never check their referent.
fn record_association<P, F>(
    repo: &Path,
    revision_id: Option<&RevisionId>,
    track: Option<&str>,
    actor_id: Option<&ActorId>,
    signing: &EventSigningOptions,
    build_payload: F,
) -> Result<AssociationWriteOutcome>
where
    P: EventPayload,
    F: FnOnce(&RevisionId, &Path) -> Result<(EventType, String, P)>,
{
    let write_store = resolve_write_store(repo)?;
    let worktree_root = write_store.worktree_root();
    let store_dir = write_store.store_dir();
    let storage = LocalStorage::new(store_dir);
    prepare_write_landing(&write_store, &storage)?;

    let event_store = EventStore::from_backend(write_store.backend());

    let validation_store = resolve_write_validation_store(repo)?;
    let validation_events = validation_store.validation_events()?;
    let resolved = resolve_revision(
        &validation_events,
        RevisionSelection::from_revision_seed(revision_id),
        &CurrentRevisionContext::for_repo(repo)?,
        RevisionScope::default(),
    )?;
    let track_id = validated_track_id(track.ok_or_else(|| ShoreError::WorkflowInputInvalid {
        reason: "track is required".to_owned(),
    })?)?;
    let revision_id = resolved.revision_id.clone();

    let (event_type, idempotency_key, payload) = build_payload(&revision_id, worktree_root)?;
    let writer = writer_from_options(worktree_root, actor_id);

    let mut event = ShoreEvent::new(
        event_type,
        idempotency_key,
        EventTarget::for_subject(
            resolved.journal_id,
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            }),
            Some(track_id),
        )?,
        writer,
        payload,
        current_timestamp(),
    )?;
    sign_event_if_requested(&mut event, signing)?;
    let event_id = event.event_id.clone();

    let outcome = event_store.record_event_once(&event)?;
    let mut events_created_by_type = BTreeMap::new();
    let (events_created, events_existing) = match outcome {
        EventWriteOutcome::Created => {
            events_created_by_type.insert(event.event_type.as_str().to_owned(), 1);
            (1, 0)
        }
        EventWriteOutcome::Existing | EventWriteOutcome::ExistingDivergentSignature => (0, 1),
    };

    let events = event_store.list_events()?;
    let state = SessionState::from_events(&events)?;
    storage.write_json_atomic(
        &store_dir.join("state.json"),
        &state,
        Durability::Projection,
    )?;

    Ok(AssociationWriteOutcome {
        revision_id,
        event_id,
        events_created,
        events_existing,
        events_created_by_type,
        diagnostics: state.diagnostics,
        events,
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::process::Command;

    use super::*;
    use crate::session::{CaptureOptions, capture_worktree_review};

    /// The store a workflow actually lands in for `repo` — the shared common-dir
    /// store by default. Reads that follow a workflow resolve here, not the raw
    /// worktree-local `.shore/data`.
    fn resolved_store_dir(repo: &std::path::Path) -> std::path::PathBuf {
        crate::git::git_common_dir(repo).unwrap().join("shore")
    }

    struct Repo {
        root: tempfile::TempDir,
    }

    impl Repo {
        fn with_capture() -> (Self, RevisionId) {
            let repo = Self {
                root: tempfile::tempdir().unwrap(),
            };
            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            std::fs::write(repo.path().join("src.txt"), "base\n").unwrap();
            repo.git(["add", "--all"]);
            repo.git(["commit", "-m", "base"]);
            std::fs::write(repo.path().join("src.txt"), "changed\n").unwrap();
            let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
            (repo, capture.revision_id)
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let status = Command::new("git")
                .args(
                    args.into_iter()
                        .map(|a| a.as_ref().to_owned())
                        .collect::<Vec<_>>(),
                )
                .current_dir(self.path())
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    #[test]
    fn associate_commit_records_and_is_idempotent() {
        let (repo, _unit) = Repo::with_capture();

        let first = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:codex"),
        )
        .unwrap();
        assert_eq!(first.events_created, 1);
        assert!(
            first
                .commit_association_id
                .as_str()
                .starts_with("assoc-commit:")
        );

        let again = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:codex"),
        )
        .unwrap();
        assert_eq!(again.events_existing, 1);
        assert_eq!(again.commit_association_id, first.commit_association_id);
        assert_eq!(again.event_id, first.event_id);
    }

    #[test]
    fn associate_commit_overlapping_content_raises_no_mismatch() {
        // The fixture's HEAD created `src.txt`, which the captured worktree diff
        // also touches — an ordinary landing shape, so the advisory guard stays
        // silent.
        let (repo, _unit) = Repo::with_capture();

        let result = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:codex"),
        )
        .unwrap();

        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == COMMIT_ASSOCIATION_CONTENT_MISMATCH_CODE),
            "overlapping paths must not flag: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn associate_commit_disjoint_content_flags_advisory_mismatch() {
        // A commit touching only an unrelated file shares no paths with the
        // captured snapshot (`src.txt`): the association still records — the
        // guard is advisory — but the result carries the mismatch diagnostic.
        let (repo, unit) = Repo::with_capture();
        std::fs::write(repo.path().join("unrelated.txt"), "elsewhere\n").unwrap();
        repo.git(["add", "unrelated.txt"]);
        repo.git(["commit", "-m", "unrelated"]);

        let result = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD")
                .with_revision_id(unit.clone())
                .with_track("agent:codex"),
        )
        .unwrap();

        assert_eq!(result.events_created, 1, "advisory: the write still lands");
        let mismatch = result
            .diagnostics
            .iter()
            .find(|d| d.code == COMMIT_ASSOCIATION_CONTENT_MISMATCH_CODE)
            .expect("disjoint content earns the advisory mismatch");
        assert!(
            mismatch.message.contains(unit.as_str()),
            "message names the associated revision: {}",
            mismatch.message
        );
    }

    #[test]
    fn associate_commit_mismatch_names_a_better_matching_revision() {
        // Two captures: the fixture's `src.txt` revision and a later
        // `unrelated.txt` revision. Landing the unrelated commit onto the
        // src.txt revision flags the mismatch and points at the sibling whose
        // snapshot the commit actually matches.
        let (repo, src_unit) = Repo::with_capture();
        repo.git(["add", "--all"]);
        repo.git(["commit", "-m", "land src change"]);
        std::fs::write(repo.path().join("unrelated.txt"), "elsewhere\n").unwrap();
        repo.git(["add", "unrelated.txt"]);
        // Staged, so the default worktree capture (diff HEAD → worktree) sees it.
        let unrelated_capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        repo.git(["commit", "-m", "unrelated"]);

        let result = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD")
                .with_revision_id(src_unit)
                .with_track("agent:codex"),
        )
        .unwrap();

        let mismatch = result
            .diagnostics
            .iter()
            .find(|d| d.code == COMMIT_ASSOCIATION_CONTENT_MISMATCH_CODE)
            .expect("disjoint content earns the advisory mismatch");
        assert!(
            mismatch
                .message
                .contains(unrelated_capture.revision_id.as_str()),
            "message suggests the overlapping sibling: {}",
            mismatch.message
        );
    }

    #[test]
    fn association_persists_a_full_event_log_rebuild() {
        // The state.json a write workflow persists is a rebuild of the whole
        // event log, not the batch the workflow loaded for itself: after
        // recording an association, the on-disk projection must equal a fresh
        // replay of every event in the store.
        let (repo, _unit) = Repo::with_capture();
        associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:codex"),
        )
        .unwrap();

        let store_dir = resolved_store_dir(repo.path());
        let events = EventStore::open(&store_dir).list_events().unwrap();
        let replay = SessionState::from_events(&events).unwrap();
        let persisted: SessionState =
            serde_json::from_slice(&std::fs::read(store_dir.join("state.json")).unwrap()).unwrap();

        assert_eq!(persisted, replay);
        assert_eq!(persisted.event_count, events.len());
    }

    #[test]
    fn withdraw_commit_records_even_when_referent_absent() {
        let (repo, _unit) = Repo::with_capture();

        let result = withdraw_commit(
            WithdrawCommitOptions::new(
                repo.path(),
                CommitAssociationId::new("assoc-commit:sha256:never-associated"),
            )
            .with_track("agent:codex"),
        )
        .unwrap();

        assert_eq!(result.events_created, 1);
        let listed = list_associations(ListAssociationsOptions::new(repo.path())).unwrap();
        assert!(
            listed
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "retraction_target_missing")
        );
    }

    #[test]
    fn associate_ref_stores_full_ref_and_list_filters_current() {
        let (repo, _unit) = Repo::with_capture();

        // A short branch name is normalized to the full ref.
        let associated = associate_ref(
            AssociateRefOptions::new(repo.path(), "feat/x", "oidH").with_track("agent:codex"),
        )
        .unwrap();
        assert_eq!(associated.ref_name, "refs/heads/feat/x");

        let current =
            list_associations(ListAssociationsOptions::new(repo.path()).current_only(true))
                .unwrap();
        // The capture also auto-records the current branch ref, so just assert
        // feat/x is present among the current refs.
        assert!(
            current
                .current_refs
                .iter()
                .any(|current_ref| current_ref.ref_name == "refs/heads/feat/x")
        );

        withdraw_ref(
            WithdrawRefOptions::new(repo.path(), associated.ref_association_id.clone())
                .with_track("agent:codex"),
        )
        .unwrap();

        let after = list_associations(ListAssociationsOptions::new(repo.path()).current_only(true))
            .unwrap();
        assert!(
            !after
                .current_refs
                .iter()
                .any(|current_ref| current_ref.ref_name == "refs/heads/feat/x"),
            "the withdrawn ref is gone (the auto-recorded branch ref may remain)"
        );
    }

    #[test]
    fn track_is_envelope_only_not_in_identity() {
        let (repo, _unit) = Repo::with_capture();

        let a = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:alice"),
        )
        .unwrap();
        let b = associate_commit(
            AssociateCommitOptions::new(repo.path(), "HEAD").with_track("agent:bob"),
        )
        .unwrap();

        assert_eq!(
            a.event_id, b.event_id,
            "track is not part of event identity"
        );
        assert_eq!(a.commit_association_id, b.commit_association_id);
    }
}
