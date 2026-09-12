//! Proof-first landing for one exact captured Revision.
//!
//! A safe landing is an ordered, idempotent publication: canonical proof
//! resource, structural commit association, then semantic attestation. The
//! structural low-level association remains available separately, but only
//! this workflow may return content-qualified landing language.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::git::{
    Ancestry, GitObjectPolicy, capture_commit_range_diff_files_with_policy, git_commit_endpoint,
    git_commit_parent_oids, git_is_ancestor_with_policy, git_worktree_root,
};
#[cfg(test)]
use crate::git::{capture_commit_range_diff_files, git_commit_tree_oid};
use crate::model::{
    ActorId, DiffFile, ReviewEndpoint, ReviewTargetRef, RevisionRefV1, RevisionSource, TargetRef,
};
use crate::session::acknowledgement::{DerivedWriteAggregate, EventWriteAcknowledgement};
use crate::session::event::{
    EventTarget, EventType, RelationProofStatusV1, RevisionRelationAttestationDraftV1,
    SemanticRevisionRelationV1, ShoreEvent, build_commit_association_id,
    build_revision_relation_attested,
};
use crate::session::evidence::{
    CanonicalProofInputV1, ProofCaptureModeV1, ProofGitAvailabilityV1, RelationProofAlgorithmV1,
    RelationProofManifestV1, canonical_candidate_diff_entries, canonical_diff_entries,
    evaluate_relation_proof_v1,
};
use crate::session::projection::{LegacyProjectionRefresh, publish_legacy_state_projection};
use crate::session::store::content::ContentArtifacts;
use crate::session::store::resolution::{prepare_write_landing, resolve_change_write_store};
use crate::session::{
    AssociateCommitOptions, AssociateCommitResult, BestEffortSkipSink, EventSigningOptions,
    EventWriteOutcome, LegacyProjectionStateV1, ProjectionDiagnostic, ReviewCursorV1,
    ReviewSourceBindingV1, RevisionShowOptions, RevisionShowResult, SessionState,
    WriteAcknowledgementV1, associate_commit, current_timestamp, show_revision_for_change_reader,
    sign_event_if_requested, validated_track_id, writer_from_options,
};
use crate::storage::{CreateOutcome, LocalStorage};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LandCommitOptions {
    repo: PathBuf,
    review_cursor: String,
    track: String,
    commit: String,
    allow_extension: bool,
    provenance_only: bool,
    candidate_parent: bool,
    expected_proof: Option<String>,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl LandCommitOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        review_cursor: impl Into<String>,
        track: impl Into<String>,
        commit: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            review_cursor: review_cursor.into(),
            track: track.into(),
            commit: commit.into(),
            allow_extension: false,
            provenance_only: false,
            candidate_parent: false,
            expected_proof: None,
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }

    fn object_policy(&self) -> GitObjectPolicy {
        if self.candidate_parent {
            GitObjectPolicy::Original
        } else {
            GitObjectPolicy::Configured
        }
    }

    pub fn with_allow_extension(mut self, value: bool) -> Self {
        self.allow_extension = value;
        self
    }

    pub fn with_provenance_only(mut self, value: bool) -> Self {
        self.provenance_only = value;
        self
    }

    pub fn with_candidate_parent(mut self, value: bool) -> Self {
        self.candidate_parent = value;
        self
    }

    pub fn with_expected_proof(mut self, hash: impl Into<String>) -> Self {
        self.expected_proof = Some(hash.into());
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

    pub fn sign_with_best_effort<S>(mut self, signer: S, skip_sink: BestEffortSkipSink) -> Self
    where
        S: EventSigner + Send + Sync + 'static,
    {
        self.signing = EventSigningOptions::sign_with_best_effort(signer, skip_sink);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LandCommitResultV1 {
    pub schema: String,
    pub revision: RevisionRefV1,
    pub commit_oid: String,
    pub commit_association_id: crate::model::CommitAssociationId,
    pub proof: RelationProofManifestV1,
    pub proof_created: bool,
    pub structural_association_created: bool,
    pub relation_attestation_id: crate::model::RevisionRelationAttestationId,
    pub relation_attestation_created: bool,
    pub message: String,
    pub acknowledgement: WriteAcknowledgementV1,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LandCommitPreviewV1 {
    pub schema: String,
    pub revision: RevisionRefV1,
    pub commit_oid: String,
    pub tree_oid: String,
    pub proof: RelationProofManifestV1,
    pub message: String,
}

struct PreparedLanding {
    shown: RevisionShowResult,
    cursor: ReviewCursorV1,
    preview: LandCommitPreviewV1,
}

pub fn preview_land_commit(options: LandCommitOptions) -> Result<LandCommitPreviewV1> {
    Ok(prepare_landing(&options)?.preview)
}

fn prepare_landing(options: &LandCommitOptions) -> Result<PreparedLanding> {
    if options.allow_extension && options.provenance_only {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "--allow-extension cannot be combined with --provenance-only".to_owned(),
        });
    }
    validated_track_id(&options.track)?;
    let cursor = ReviewCursorV1::decode_token(&options.review_cursor)?;
    if options.candidate_parent && (options.allow_extension || options.provenance_only) {
        return Err(unsupported_parent(
            "--candidate-parent cannot be combined with --allow-extension or --provenance-only",
        ));
    }
    if options.candidate_parent && cursor.source_binding != ReviewSourceBindingV1::Captured {
        return Err(unsupported_parent(
            "--candidate-parent requires a captured-source Review cursor",
        ));
    }
    let revision_id =
        super::exact_revision_from_transition_cursor(&options.repo, &options.review_cursor)?;
    if revision_id != cursor.revision.revision_id {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "Review cursor resolved to a different exact Revision".to_owned(),
        });
    }
    let shown = show_revision_for_change_reader(
        RevisionShowOptions::new(&options.repo)
            .with_revision_id(revision_id)
            .with_exact(true),
    )?;
    let revision = RevisionRefV1::new(
        shown.revision.revision_id.clone(),
        shown.revision.object_artifact_content_hash.clone(),
    )?;
    if revision != cursor.revision {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "Review cursor artifact binding no longer matches the exact Revision"
                .to_owned(),
        });
    }
    if options.candidate_parent
        && shown.snapshot_content_state != crate::session::SnapshotContentState::Present
    {
        return Err(unsupported_parent(
            "the frozen source artifact is not available",
        ));
    }

    let worktree_root = git_worktree_root(&options.repo)?;
    let worktree_root = worktree_root.as_path();
    let (commit_oid, commit_tree_oid) =
        git_commit_endpoint(worktree_root, &options.commit, options.object_policy())?;
    let association_id = build_commit_association_id(&revision.revision_id, &commit_oid)?;

    let (source, candidate, exact_endpoint) = if options.provenance_only {
        attribution_inputs(
            shown.revision.git_provenance.as_ref(),
            &shown.snapshot.files,
        )
    } else {
        proof_inputs(
            worktree_root,
            shown.revision.git_provenance.as_ref(),
            &shown.snapshot.files,
            &commit_oid,
            &commit_tree_oid,
            options.candidate_parent,
        )?
    };
    let algorithm = if options.provenance_only {
        RelationProofAlgorithmV1::AttributionOnly
    } else if exact_endpoint && source == candidate {
        RelationProofAlgorithmV1::ExactMaterialization
    } else if source.capture_mode == candidate.capture_mode
        && source.path_scope == candidate.path_scope
        && source.entries == candidate.entries
    {
        RelationProofAlgorithmV1::CanonicalEquivalentRewrite
    } else if source
        .entries
        .iter()
        .all(|entry| candidate.entries.contains(entry))
        && candidate
            .entries
            .iter()
            .any(|entry| !source.entries.contains(entry))
    {
        RelationProofAlgorithmV1::ContentPreservingExtension
    } else {
        RelationProofAlgorithmV1::CanonicalEquivalentRewrite
    };
    let proof = evaluate_relation_proof_v1(
        revision.clone(),
        association_id.clone(),
        algorithm,
        source,
        candidate,
    )?;
    match proof.result.proof_status {
        RelationProofStatusV1::Verified => {
            if proof.result.semantic_relation
                == SemanticRevisionRelationV1::ContentPreservingExtension
                && !options.allow_extension
            {
                if options.candidate_parent {
                    return Err(unsupported_parent(
                        "the candidate adds unreviewed content; capture and review a new Revision",
                    ));
                }
                return Err(ShoreError::WorkflowInputInvalid {
                    reason: "the candidate preserves the reviewed scope but adds unreviewed content; retry with --allow-extension or capture a new Revision".to_owned(),
                });
            }
        }
        RelationProofStatusV1::Asserted if options.provenance_only => {}
        RelationProofStatusV1::Indeterminate => {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "landing proof is indeterminate; use --provenance-only or the structural association command".to_owned(),
            });
        }
        _ => {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "landing proof refuted the reviewed-content relation; capture and review a new Revision or record only structural provenance".to_owned(),
            });
        }
    }

    Ok(PreparedLanding {
        shown,
        cursor,
        preview: LandCommitPreviewV1 {
            schema: "pointbreak.association-land-preview.v1".to_owned(),
            revision,
            commit_oid,
            tree_oid: commit_tree_oid,
            proof,
            message:
                "preview of the scoped Revision-to-commit relation; no landing records written"
                    .to_owned(),
        },
    })
}

pub fn land_commit(options: LandCommitOptions) -> Result<LandCommitResultV1> {
    land_commit_with_after_association(options, || {})
}

fn land_commit_with_after_association(
    options: LandCommitOptions,
    after_association: impl FnOnce(),
) -> Result<LandCommitResultV1> {
    land_commit_with_hooks(options, || {}, after_association)
}

fn land_commit_with_hooks(
    options: LandCommitOptions,
    before_publication: impl FnOnce(),
    after_association: impl FnOnce(),
) -> Result<LandCommitResultV1> {
    let prepared = prepare_landing(&options)?;
    if (options.candidate_parent || options.expected_proof.is_some())
        && options.expected_proof.as_deref()
            != Some(prepared.preview.proof.evidence_sha256.as_str())
    {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "recording requires --expect-proof matching the current preview proof.evidenceSha256 (mandatory with --candidate-parent)".to_owned(),
        });
    }
    let expected_preview = prepared.preview.clone();
    let PreparedLanding {
        shown,
        cursor,
        preview,
    } = prepared;
    let LandCommitPreviewV1 {
        revision: _,
        commit_oid,
        tree_oid: commit_tree_oid,
        proof,
        ..
    } = preview;
    let revision = cursor.revision;
    let association_id = proof.association_id.clone();
    let track_id = validated_track_id(&options.track)?;
    let write_store = resolve_change_write_store(&options.repo)?;
    let worktree_root = write_store.worktree_root();
    let storage = LocalStorage::new(write_store.store_dir());
    prepare_write_landing(&write_store, &storage)?;

    before_publication();
    // Re-read the exact graph/artifact and resolve the original candidate spelling
    // immediately before the first publication. Git and the Journal are separate
    // authorities; this check does not promise a transaction across them.
    if options.candidate_parent && prepare_landing(&options)?.preview != expected_preview {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "landing inputs changed before publication; preview again and supply the new --expect-proof".to_owned(),
        });
    }

    let proof_outcome =
        ContentArtifacts::from_backend(write_store.backend()).put_relation_proof(&proof)?;
    let mut association_options = AssociateCommitOptions::new(&options.repo, &commit_oid)
        .with_review_cursor(options.review_cursor.clone())
        .with_track(options.track.clone())
        .with_object_policy(options.object_policy());
    if let Some(actor_id) = options.actor_id.clone() {
        association_options = association_options.with_actor_id(actor_id);
    }
    association_options = association_options.with_signing_options(options.signing.clone());
    let association = associate_commit(association_options)?;
    if association.commit_association_id != association_id {
        return Err(ShoreError::Message(
            "structural association identity differs from the proof binding".to_owned(),
        ));
    }

    after_association();

    let attestation = build_revision_relation_attested(RevisionRelationAttestationDraftV1 {
        revision: revision.clone(),
        commit_association_id: association_id.clone(),
        semantic_relation: proof.result.semantic_relation,
        proof_status: proof.result.proof_status,
        proof_method: if options.provenance_only {
            "attribution-only"
        } else {
            "canonical-git-diff"
        }
        .to_owned(),
        proof_algorithm_version: proof.algorithm_version.clone(),
        capture_scope: proof.source.path_scope.clone(),
        comparison_base_or_parent: proof.source.base_or_parent.clone(),
        endpoint_oids: vec![commit_oid.clone(), commit_tree_oid],
        evidence_content_hash: Some(proof.evidence_sha256.clone()),
        result_digest: proof.result_digest()?,
    })?;
    let writer = writer_from_options(worktree_root, options.actor_id.as_ref());
    let mut event = ShoreEvent::new(
        EventType::RevisionRelationAttested,
        attestation.relation_attestation_id.as_str(),
        EventTarget::for_subject(
            shown.revision.journal_id,
            TargetRef::Review(ReviewTargetRef::Revision {
                revision_id: revision.revision_id.clone(),
            }),
            Some(track_id),
        )?,
        writer,
        attestation.clone(),
        current_timestamp(),
    )?;
    sign_event_if_requested(&mut event, &options.signing)?;
    let event_store = write_store.event_store()?;
    let attestation_acknowledgement = event_store.record_change_event_once_acknowledged(&event)?;
    let attestation_outcome = attestation_acknowledgement.outcome;
    let events = event_store.list_change_events()?;
    let state = SessionState::from_events(&events)?;
    let projection_refresh =
        publish_legacy_state_projection(&storage, write_store.store_dir(), &state);
    let (acknowledgement, diagnostics) = landing_acknowledgement(
        proof_outcome,
        &association,
        attestation_acknowledgement,
        projection_refresh,
    );

    let message = match proof.result.semantic_relation {
        SemanticRevisionRelationV1::ExactMaterialization => {
            format!(
                "landed as an exact materialization of {}",
                revision.revision_id.as_str()
            )
        }
        SemanticRevisionRelationV1::EquivalentRewrite => {
            format!(
                "{} {}",
                if options.candidate_parent {
                    "recorded verified scoped equivalent rewrite of"
                } else {
                    "landed as a verified equivalent rewrite of"
                },
                revision.revision_id.as_str()
            )
        }
        SemanticRevisionRelationV1::ContentPreservingExtension => format!(
            "landed the reviewed scope of {} with {} unreviewed addition(s)",
            revision.revision_id.as_str(),
            proof.result.additions.len()
        ),
        SemanticRevisionRelationV1::LandingProvenance => format!(
            "recorded landing provenance for {} without a content-qualified relation",
            revision.revision_id.as_str()
        ),
        _ => unreachable!("accepted landing policy produces a closed relation set"),
    };
    Ok(LandCommitResultV1 {
        schema: "pointbreak.association-land.v1".to_owned(),
        revision,
        commit_oid,
        commit_association_id: association_id,
        proof,
        proof_created: proof_outcome == CreateOutcome::Created,
        structural_association_created: association.events_created > 0,
        relation_attestation_id: attestation.relation_attestation_id,
        relation_attestation_created: attestation_outcome == EventWriteOutcome::Created,
        message,
        acknowledgement,
        diagnostics,
    })
}

/// Proof storage has no derived write; only the two event calls contribute tokens.
fn landing_acknowledgement(
    proof: CreateOutcome,
    association: &AssociateCommitResult,
    attestation: EventWriteAcknowledgement,
    refresh: LegacyProjectionRefresh,
) -> (WriteAcknowledgementV1, Vec<ProjectionDiagnostic>) {
    let mut derived = DerivedWriteAggregate::default();
    let mut diagnostics = association.diagnostics.clone();
    derived.add(association.acknowledgement.derived.clone(), []);
    let attestation_created = derived.record(attestation) == EventWriteOutcome::Created;
    let proof_created = proof == CreateOutcome::Created;
    let legacy = if association.acknowledgement.legacy_projection_state
        == LegacyProjectionStateV1::RefreshFailed
    {
        LegacyProjectionStateV1::RefreshFailed
    } else {
        refresh.state
    };
    let acknowledgement = derived.finish(
        association.events_created + usize::from(proof_created) + usize::from(attestation_created),
        association.events_existing
            + usize::from(!proof_created)
            + usize::from(!attestation_created),
        legacy,
        &mut diagnostics,
    );
    diagnostics.extend(refresh.diagnostic);
    (acknowledgement, diagnostics)
}

fn attribution_inputs(
    provenance: Option<&crate::session::event::GitProvenance>,
    source_files: &[DiffFile],
) -> (CanonicalProofInputV1, CanonicalProofInputV1, bool) {
    let (capture_mode, path_scope, source_availability) = provenance.map_or_else(
        || {
            (
                ProofCaptureModeV1::CombinedWorktree,
                Vec::new(),
                ProofGitAvailabilityV1::Missing,
            )
        },
        |provenance| {
            let (mode, scope) = source_mode_and_scope(&provenance.source);
            (mode, scope, ProofGitAvailabilityV1::Available)
        },
    );
    let source = CanonicalProofInputV1 {
        capture_mode,
        base_or_parent: provenance.and_then(|value| endpoint_treeish(&value.base)),
        path_scope: canonical_scope(&path_scope),
        git_availability: source_availability,
        entries: canonical_diff_entries(source_files),
    };
    let candidate = CanonicalProofInputV1 {
        capture_mode,
        base_or_parent: None,
        path_scope: canonical_scope(&path_scope),
        git_availability: ProofGitAvailabilityV1::Missing,
        entries: Vec::new(),
    };
    (source, candidate, false)
}

fn proof_inputs(
    repo: &Path,
    provenance: Option<&crate::session::event::GitProvenance>,
    source_files: &[DiffFile],
    candidate_commit: &str,
    candidate_tree: &str,
    candidate_parent: bool,
) -> Result<(CanonicalProofInputV1, CanonicalProofInputV1, bool)> {
    let provenance = provenance.ok_or_else(|| ShoreError::WorkflowInputInvalid {
        reason: "the captured Revision has no Git provenance; use --provenance-only".to_owned(),
    })?;
    let (capture_mode, path_scope) = source_mode_and_scope(&provenance.source);
    let base =
        endpoint_treeish(&provenance.base).ok_or_else(|| ShoreError::WorkflowInputInvalid {
            reason:
                "the captured Revision has no immutable Git comparison base; use --provenance-only"
                    .to_owned(),
        })?;
    let candidate_base = if candidate_parent {
        parent_comparison_base(repo, provenance, candidate_commit)?
    } else {
        base.clone()
    };
    let candidate_files = capture_commit_range_diff_files_with_policy(
        repo,
        &candidate_base,
        candidate_commit,
        &path_scope_for_git(&path_scope),
        if candidate_parent {
            GitObjectPolicy::Original
        } else {
            GitObjectPolicy::Configured
        },
    )?;
    let source = CanonicalProofInputV1 {
        capture_mode,
        base_or_parent: Some(base.clone()),
        path_scope: canonical_scope(&path_scope),
        git_availability: ProofGitAvailabilityV1::Available,
        entries: canonical_diff_entries(source_files),
    };
    let candidate = CanonicalProofInputV1 {
        capture_mode,
        base_or_parent: Some(candidate_base),
        path_scope: canonical_scope(&path_scope),
        git_availability: ProofGitAvailabilityV1::Available,
        entries: canonical_candidate_diff_entries(&candidate_files, source_files),
    };
    let exact_endpoint = match &provenance.target {
        ReviewEndpoint::GitCommit {
            commit_oid,
            tree_oid,
        } => commit_oid == candidate_commit || tree_oid == candidate_tree,
        ReviewEndpoint::GitTree { tree_oid } | ReviewEndpoint::GitIndex { tree_oid } => {
            tree_oid == candidate_tree
        }
        // A mutable working-tree endpoint has no Git oid to compare directly.
        // Its full canonical entry set was frozen at capture, so equality with
        // the candidate diff is the exact-materialization proof.
        ReviewEndpoint::GitWorkingTree { .. } => true,
    };
    Ok((source, candidate, exact_endpoint))
}

fn unsupported_parent(reason: &str) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: format!(
            "{reason}; use an ordinary supported landing comparison or capture and review a new Revision"
        ),
    }
}

fn parent_comparison_base(
    repo: &Path,
    provenance: &crate::session::event::GitProvenance,
    candidate_commit: &str,
) -> Result<String> {
    let ReviewEndpoint::GitCommit {
        commit_oid: source_base,
        ..
    } = &provenance.base
    else {
        return Err(unsupported_parent(
            "--candidate-parent requires a captured Git commit base",
        ));
    };
    match &provenance.source {
        RevisionSource::GitWorktree { .. } => {}
        RevisionSource::GitCommitRange { .. } => {
            let ReviewEndpoint::GitCommit { commit_oid, .. } = &provenance.target else {
                return Err(unsupported_parent(
                    "--candidate-parent requires a single-commit source range",
                ));
            };
            if git_commit_parent_oids(repo, commit_oid)? != [source_base.clone()] {
                return Err(unsupported_parent(
                    "--candidate-parent requires a single-commit source whose sole parent is the captured base",
                ));
            }
        }
        _ => {
            return Err(unsupported_parent(
                "--candidate-parent supports only combined worktree or single-commit range captures",
            ));
        }
    }
    let parents = git_commit_parent_oids(repo, candidate_commit)?;
    let [parent] = parents.as_slice() else {
        return Err(unsupported_parent(
            "--candidate-parent requires a candidate with exactly one parent",
        ));
    };
    if git_is_ancestor_with_policy(repo, source_base, parent, GitObjectPolicy::Original)?
        != Ancestry::Ancestor
    {
        return Err(unsupported_parent(
            "the captured base must be a known ancestor of the candidate parent",
        ));
    }
    Ok(parent.clone())
}

fn source_mode_and_scope(source: &RevisionSource) -> (ProofCaptureModeV1, Vec<String>) {
    match source {
        RevisionSource::GitWorktree { pathspecs, .. } => {
            (ProofCaptureModeV1::CombinedWorktree, pathspecs.clone())
        }
        RevisionSource::GitCommitRange { pathspecs, .. } => {
            (ProofCaptureModeV1::CommitRange, pathspecs.clone())
        }
        RevisionSource::GitRootCommit { pathspecs, .. } => {
            (ProofCaptureModeV1::Root, pathspecs.clone())
        }
        RevisionSource::GitStaged { pathspecs, .. } => {
            (ProofCaptureModeV1::Staged, pathspecs.clone())
        }
        RevisionSource::GitUnstaged { pathspecs, .. } => {
            (ProofCaptureModeV1::Unstaged, pathspecs.clone())
        }
    }
}

fn endpoint_treeish(endpoint: &ReviewEndpoint) -> Option<String> {
    match endpoint {
        ReviewEndpoint::GitCommit { commit_oid, .. } => Some(commit_oid.clone()),
        ReviewEndpoint::GitTree { tree_oid } | ReviewEndpoint::GitIndex { tree_oid } => {
            Some(tree_oid.clone())
        }
        ReviewEndpoint::GitWorkingTree { .. } => None,
    }
}

fn canonical_scope(pathspecs: &[String]) -> Vec<String> {
    if pathspecs.is_empty() {
        vec![".".to_owned()]
    } else {
        pathspecs.to_vec()
    }
}

fn path_scope_for_git(pathspecs: &[String]) -> Vec<String> {
    pathspecs
        .iter()
        .filter(|path| path.as_str() != ".")
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::model::ChangeIdentityDescriptorV1;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::{
        ChangeCreateOptions, ChangeMembershipOptions, CommitRangeSpec, capture_review,
        create_change, join_revision_to_change, select_review_cursor,
    };

    #[test]
    fn exact_landing_publishes_proof_before_retry_stable_relation_state() {
        let (root, selected) = landing_fixture();
        let first = land_commit(LandCommitOptions::new(
            root.path(),
            &selected,
            "track:author",
            "HEAD",
        ))
        .unwrap();
        assert_eq!(
            first.proof.result.semantic_relation,
            SemanticRevisionRelationV1::ExactMaterialization
        );
        assert_eq!(
            first.acknowledgement.authority_outcome,
            crate::session::AuthorityWriteOutcomeV1::Created
        );
        assert!(first.diagnostics.is_empty());
        assert!(first.proof_created);
        assert!(first.structural_association_created);
        assert!(first.relation_attestation_created);

        let retry = land_commit(LandCommitOptions::new(
            root.path(),
            selected,
            "track:author",
            "HEAD",
        ))
        .unwrap();
        assert_eq!(first.proof, retry.proof);
        assert_eq!(
            retry.acknowledgement.authority_outcome,
            crate::session::AuthorityWriteOutcomeV1::Existing
        );
        assert!(!retry.proof_created);
        assert!(!retry.structural_association_created);
        assert!(!retry.relation_attestation_created);
    }

    fn landing_fixture() -> (tempfile::TempDir, String) {
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
        let base = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        std::fs::write(root.path().join("sample.txt"), "landed\n").unwrap();
        git(root.path(), &["commit", "--quiet", "-am", "candidate"]);

        let capture = capture_review(
            crate::session::CaptureOptions::new(root.path())
                .with_commit_range(CommitRangeSpec::new(base)),
        )
        .unwrap();
        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let change = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:landing-test-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x71; 32]),
        ))
        .unwrap();
        join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:landing-test-join",
            change.change_id.clone(),
            capture.revision_id.clone(),
        ))
        .unwrap();
        let ready = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .clone();
        let revision = ready.document_projection.revision_refs[&capture.revision_id]
            .first()
            .unwrap();
        let commit_binding = crate::session::review_source_binding(
            root.path(),
            revision,
            crate::session::ReviewSourceRequestV1::Commit("HEAD".to_owned()),
        )
        .unwrap();
        let selected = select_review_cursor(
            &ready.projection.changes[&change.change_id],
            &ready.document_projection,
            Some(&capture.revision_id),
            false,
            commit_binding,
        )
        .unwrap();

        (root, selected.token)
    }

    #[test]
    fn landing_acknowledgement_mixes_existing_association_with_new_proof_and_attestation() {
        let (root, selected) = landing_fixture();
        associate_commit(
            AssociateCommitOptions::new(root.path(), "HEAD")
                .with_review_cursor(selected.clone())
                .with_track("track:author"),
        )
        .unwrap();
        let result = land_commit(LandCommitOptions::new(
            root.path(),
            selected,
            "track:author",
            "HEAD",
        ))
        .unwrap();
        assert!(result.proof_created);
        assert!(!result.structural_association_created);
        assert!(result.relation_attestation_created);
        assert_eq!(
            result.acknowledgement.authority_outcome,
            crate::session::AuthorityWriteOutcomeV1::Mixed
        );
    }

    #[test]
    fn landing_acknowledgement_merges_only_compatible_event_tokens() {
        use crate::session::acknowledgement::EventWriteAcknowledgement;
        use crate::session::projection::LegacyProjectionRefresh;
        use crate::session::{
            DerivedVisibilityTokenV1, DerivedWriteAcknowledgementV1, DerivedWriteAvailabilityV1,
            LegacyProjectionStateV1,
        };
        let (root, selected) = landing_fixture();
        let mut association = associate_commit(
            AssociateCommitOptions::new(root.path(), "HEAD")
                .with_review_cursor(selected)
                .with_track("track:author"),
        )
        .unwrap();
        for (generation, epoch, expected) in [
            ("g1", 3, DerivedWriteAvailabilityV1::CatchingUp),
            ("g2", 3, DerivedWriteAvailabilityV1::Unavailable),
            ("g1", 4, DerivedWriteAvailabilityV1::Unavailable),
        ] {
            association.acknowledgement.derived =
                DerivedWriteAcknowledgementV1::current(DerivedVisibilityTokenV1 {
                    generation_id: "g1".into(),
                    epoch: 3,
                    head_sequence: 5,
                });
            let attestation = EventWriteAcknowledgement::new(
                EventWriteOutcome::Created,
                DerivedWriteAvailabilityV1::CatchingUp,
                Some(DerivedVisibilityTokenV1 {
                    generation_id: generation.into(),
                    epoch,
                    head_sequence: 9,
                }),
                Vec::new(),
            );
            let (ack, diagnostics) = landing_acknowledgement(
                CreateOutcome::Created,
                &association,
                attestation,
                LegacyProjectionRefresh {
                    state: LegacyProjectionStateV1::Refreshed,
                    diagnostic: None,
                },
            );
            assert_eq!(ack.derived.availability, expected);
            if expected == DerivedWriteAvailabilityV1::CatchingUp {
                assert_eq!(ack.derived.token.unwrap().head_sequence, 9);
            } else {
                assert!(ack.derived.token.is_none());
                assert!(
                    diagnostics
                        .iter()
                        .any(|d| d.code == "derived_write_token_conflict")
                );
            }
        }
    }

    #[test]
    fn landing_acknowledgement_preserves_each_refresh_failure() {
        use crate::session::LegacyProjectionStateV1;
        for (structural_fails, final_fails) in [(true, false), (false, true), (true, true)] {
            let (root, selected) = landing_fixture();
            let write_store = resolve_change_write_store(root.path()).unwrap();
            let path = write_store.store_dir().join("state.json");
            if structural_fails {
                std::fs::remove_file(&path).unwrap();
                std::fs::create_dir(&path).unwrap();
            }
            let result = land_commit_with_after_association(
                LandCommitOptions::new(root.path(), &selected, "track:author", "HEAD"),
                || {
                    if structural_fails && !final_fails {
                        std::fs::remove_dir(&path).unwrap();
                    }
                    if !structural_fails && final_fails {
                        std::fs::remove_file(&path).unwrap();
                        std::fs::create_dir(&path).unwrap();
                    }
                },
            )
            .expect("legacy refresh failure follows durable truth and is advisory");
            assert!(result.relation_attestation_created);
            assert_eq!(
                result.acknowledgement.legacy_projection_state,
                LegacyProjectionStateV1::RefreshFailed
            );
            let failures: Vec<_> = result
                .diagnostics
                .iter()
                .filter(|d| d.code == "legacy_state_projection_refresh_failed")
                .collect();
            assert_eq!(
                failures.len(),
                usize::from(structural_fails) + usize::from(final_fails)
            );
            let json = serde_json::to_value(&result).unwrap();
            assert_eq!(json["schema"], "pointbreak.association-land.v1");
            assert_eq!(json["message"], result.message);
            assert!(json["acknowledgement"].is_object());
            assert!(json["diagnostics"].is_array());
            if final_fails {
                std::fs::remove_dir(&path).unwrap();
            }
            let retry = land_commit(LandCommitOptions::new(
                root.path(),
                selected,
                "track:author",
                "HEAD",
            ))
            .unwrap();
            assert!(!retry.relation_attestation_created);
            assert_eq!(
                retry.acknowledgement.legacy_projection_state,
                LegacyProjectionStateV1::Refreshed
            );
        }
    }

    #[test]
    fn worktree_capture_lands_as_an_exact_materialization_after_commit() {
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
        std::fs::write(root.path().join("sample.txt"), "landed\n").unwrap();

        let capture = capture_review(crate::session::CaptureOptions::new(root.path())).unwrap();
        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let change = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:worktree-landing-test-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x72; 32]),
        ))
        .unwrap();
        join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:worktree-landing-test-join",
            change.change_id.clone(),
            capture.revision_id.clone(),
        ))
        .unwrap();
        git(root.path(), &["commit", "--quiet", "-am", "candidate"]);

        let ready = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .clone();
        let revision = ready.document_projection.revision_refs[&capture.revision_id]
            .first()
            .unwrap();
        let commit_binding = crate::session::review_source_binding(
            root.path(),
            revision,
            crate::session::ReviewSourceRequestV1::Commit("HEAD".to_owned()),
        )
        .unwrap();
        let selected = select_review_cursor(
            &ready.projection.changes[&change.change_id],
            &ready.document_projection,
            Some(&capture.revision_id),
            false,
            commit_binding,
        )
        .unwrap();
        let landed = land_commit(LandCommitOptions::new(
            root.path(),
            selected.token,
            "track:author",
            "HEAD",
        ))
        .unwrap();

        assert_eq!(
            landed.proof.result.semantic_relation,
            SemanticRevisionRelationV1::ExactMaterialization
        );
    }

    #[test]
    fn worktree_capture_with_untracked_files_lands_as_an_exact_materialization() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            root.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        git(root.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "--quiet", "-m", "base"]);

        std::fs::write(root.path().join("tracked.txt"), "landed\n").unwrap();
        std::fs::write(root.path().join("untracked.txt"), "new text\n").unwrap();
        std::fs::write(root.path().join("untracked.bin"), [0_u8, 159, 146, 150]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("untracked.txt", root.path().join("untracked-link")).unwrap();

        let capture = capture_review(
            crate::session::CaptureOptions::new(root.path())
                .with_worktree(crate::session::WorktreeSpec::new().with_include_untracked()),
        )
        .unwrap();
        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(root.path()).unwrap();
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let change = create_change(ChangeCreateOptions::new(
            root.path(),
            "change-operation:untracked-worktree-landing-test-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x73; 32]),
        ))
        .unwrap();
        join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:untracked-worktree-landing-test-join",
            change.change_id.clone(),
            capture.revision_id.clone(),
        ))
        .unwrap();
        git(root.path(), &["add", "--all"]);
        git(root.path(), &["commit", "--quiet", "-m", "candidate"]);

        let ready = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .clone();
        let revision = ready.document_projection.revision_refs[&capture.revision_id]
            .first()
            .unwrap();
        let commit_binding = crate::session::review_source_binding(
            root.path(),
            revision,
            crate::session::ReviewSourceRequestV1::Commit("HEAD".to_owned()),
        )
        .unwrap();
        let selected = select_review_cursor(
            &ready.projection.changes[&change.change_id],
            &ready.document_projection,
            Some(&capture.revision_id),
            false,
            commit_binding,
        )
        .unwrap();
        let landed = land_commit(LandCommitOptions::new(
            root.path(),
            selected.token,
            "track:author",
            "HEAD",
        ))
        .unwrap();

        assert_eq!(
            landed.proof.result.semantic_relation,
            SemanticRevisionRelationV1::ExactMaterialization,
            "source: {:#?}\ncandidate: {:#?}",
            landed.proof.source,
            landed.proof.candidate,
        );
    }

    fn captured_cursor(token: &str) -> String {
        let mut cursor = ReviewCursorV1::decode_token(token).unwrap();
        cursor.source_binding = crate::session::ReviewSourceBindingV1::Captured;
        cursor.encode_token().unwrap()
    }

    fn rewrite_fixture(combined: bool) -> (tempfile::TempDir, String, String, String) {
        let (root, original) = landing_fixture();
        let a = git_stdout(root.path(), &["rev-parse", "HEAD~1"]);
        let c = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        let token = if combined {
            git(root.path(), &["reset", "--mixed", &a]);
            let capture = capture_review(crate::session::CaptureOptions::new(root.path())).unwrap();
            let old = ReviewCursorV1::decode_token(&original).unwrap();
            let change = create_change(ChangeCreateOptions::new(
                root.path(),
                "change-operation:combined-create",
                ChangeIdentityDescriptorV1::opaque_nonce([0x74; 32]),
            ))
            .unwrap();
            join_revision_to_change(ChangeMembershipOptions::new(
                root.path(),
                "change-operation:combined-join",
                change.change_id.clone(),
                capture.revision_id.clone(),
            ))
            .unwrap();
            let ready = crate::session::change_reader_state_for_repo(root.path())
                .unwrap()
                .ready()
                .unwrap()
                .clone();
            let token = select_review_cursor(
                &ready.projection.changes[&change.change_id],
                &ready.document_projection,
                Some(&capture.revision_id),
                false,
                crate::session::ReviewSourceBindingV1::Captured,
            )
            .unwrap()
            .token;
            assert_ne!(
                old.revision,
                ReviewCursorV1::decode_token(&token).unwrap().revision
            );
            git(root.path(), &["reset", "--hard", &c]);
            token
        } else {
            captured_cursor(&original)
        };
        git(root.path(), &["checkout", "--detach", &a]);
        std::fs::write(root.path().join("upstream.txt"), "upstream\n").unwrap();
        git(root.path(), &["add", "upstream.txt"]);
        git(root.path(), &["commit", "--quiet", "-m", "upstream"]);
        let b = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        git(root.path(), &["cherry-pick", &c]);
        (root, token, a, b)
    }

    fn store_inventory(repo: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, out);
                } else {
                    out.insert(
                        path.strip_prefix(root).unwrap().to_owned(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let (store, _) =
            crate::session::store::resolution::resolve_change_read_store(repo).unwrap();
        let mut out = std::collections::BTreeMap::new();
        visit(store.store_dir(), store.store_dir(), &mut out);
        out
    }

    #[test]
    fn parent_preview_proves_disjoint_rewrites_without_writes_or_acknowledgement() {
        for combined in [false, true] {
            let (root, token, a, b) = rewrite_fixture(combined);
            let before = store_inventory(root.path());
            let preview = preview_land_commit(
                LandCommitOptions::new(root.path(), &token, "track:author", "HEAD")
                    .with_candidate_parent(true),
            )
            .unwrap();
            assert_eq!(
                preview.revision,
                ReviewCursorV1::decode_token(&token).unwrap().revision
            );
            assert_eq!(
                preview.proof.source.base_or_parent.as_deref(),
                Some(a.as_str())
            );
            assert_eq!(
                preview.proof.candidate.base_or_parent.as_deref(),
                Some(b.as_str())
            );
            assert_eq!(
                preview.proof.source.entries,
                preview.proof.candidate.entries
            );
            assert_eq!(
                preview.proof.result.semantic_relation,
                SemanticRevisionRelationV1::EquivalentRewrite
            );
            assert_eq!(
                preview.proof.result.proof_status,
                RelationProofStatusV1::Verified
            );
            let json = serde_json::to_value(&preview).unwrap();
            assert!(json.get("acknowledgement").is_none());
            assert!(json.get("proofCreated").is_none());
            assert_eq!(before, store_inventory(root.path()));
        }
    }

    #[test]
    fn parent_preview_rejects_changed_content_and_unsupported_candidates() {
        let (root, token, a, _) = rewrite_fixture(false);
        let options = LandCommitOptions::new(root.path(), &token, "track:author", "HEAD")
            .with_candidate_parent(true);
        let before = store_inventory(root.path());
        for policy in [
            options.clone().with_allow_extension(true),
            options.clone().with_provenance_only(true),
        ] {
            assert!(
                preview_land_commit(policy)
                    .unwrap_err()
                    .to_string()
                    .contains("--candidate-parent")
            );
        }
        let root_error = preview_land_commit(
            LandCommitOptions::new(root.path(), &token, "track:author", &a)
                .with_candidate_parent(true),
        )
        .unwrap_err();
        assert!(root_error.to_string().contains("exactly one parent"));
        std::fs::write(root.path().join("sample.txt"), "different\n").unwrap();
        git(root.path(), &["commit", "--amend", "--no-edit", "-a"]);
        assert!(
            preview_land_commit(options)
                .unwrap_err()
                .to_string()
                .contains("refuted")
        );
        assert_eq!(before, store_inventory(root.path()));
    }

    #[test]
    fn ordinary_preview_matches_existing_proof_and_has_no_store_effects() {
        let (root, token) = landing_fixture();
        let options = LandCommitOptions::new(root.path(), token, "track:author", "HEAD");
        let before = store_inventory(root.path());
        let preview = preview_land_commit(options.clone()).unwrap();
        assert_eq!(before, store_inventory(root.path()));
        let recorded = land_commit(options).unwrap();
        assert_eq!(preview.proof, recorded.proof);
    }

    #[test]
    fn parent_comparison_rejects_unsupported_sources_and_histories() {
        let (root, token, a, _) = rewrite_fixture(false);
        let options = LandCommitOptions::new(root.path(), &token, "track:author", "HEAD")
            .with_candidate_parent(true);
        let prepared = prepare_landing(&options).unwrap();
        let original = prepared.shown.revision.git_provenance.unwrap();
        let head = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        let tree = git_stdout(root.path(), &["rev-parse", "HEAD^{tree}"]);
        let merge = git_stdout(
            root.path(),
            &["commit-tree", &tree, "-p", &head, "-p", &a, "-m", "merge"],
        );
        assert!(
            parent_comparison_base(root.path(), &original, &merge)
                .unwrap_err()
                .to_string()
                .contains("exactly one parent")
        );
        let foreign_root = git_stdout(root.path(), &["commit-tree", &tree, "-m", "foreign root"]);
        let foreign_child = git_stdout(
            root.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                &foreign_root,
                "-m",
                "foreign child",
            ],
        );
        assert!(
            parent_comparison_base(root.path(), &original, &foreign_child)
                .unwrap_err()
                .to_string()
                .contains("known ancestor")
        );
        for source in [
            RevisionSource::GitStaged {
                mode: crate::model::StagedCaptureMode::BaseTreeToIndexTree,
                pathspecs: vec![],
            },
            RevisionSource::GitUnstaged {
                mode: crate::model::UnstagedCaptureMode::IndexTreeToWorkingTree,
                include_untracked: false,
                pathspecs: vec![],
            },
            RevisionSource::GitRootCommit {
                mode: crate::model::RootCommitCaptureMode::EmptyTreeToTargetTree,
                pathspecs: vec![],
            },
        ] {
            let mut provenance = original.clone();
            provenance.source = source;
            assert!(
                parent_comparison_base(root.path(), &provenance, &head)
                    .unwrap_err()
                    .to_string()
                    .contains("only combined worktree or single-commit")
            );
        }
        for base in [
            ReviewEndpoint::GitTree {
                tree_oid: tree.clone(),
            },
            ReviewEndpoint::GitIndex {
                tree_oid: tree.clone(),
            },
        ] {
            let mut provenance = original.clone();
            provenance.base = base;
            assert!(
                parent_comparison_base(root.path(), &provenance, &head)
                    .unwrap_err()
                    .to_string()
                    .contains("captured Git commit base")
            );
        }
        for target in [&head, &merge, &a] {
            let mut provenance = original.clone();
            provenance.target = ReviewEndpoint::GitCommit {
                commit_oid: target.clone(),
                tree_oid: tree.clone(),
            };
            assert!(
                parent_comparison_base(root.path(), &provenance, &head)
                    .unwrap_err()
                    .to_string()
                    .contains("single-commit source")
            );
        }
        let mut bound = ReviewCursorV1::decode_token(&token).unwrap();
        let original_commit = if let ReviewEndpoint::GitCommit { commit_oid, .. } = original.target
        {
            commit_oid
        } else {
            unreachable!()
        };
        bound.source_binding = crate::session::review_source_binding(
            root.path(),
            &bound.revision,
            crate::session::ReviewSourceRequestV1::Commit(original_commit),
        )
        .unwrap();
        assert!(
            preview_land_commit(
                LandCommitOptions::new(
                    root.path(),
                    bound.encode_token().unwrap(),
                    "track:author",
                    &head
                )
                .with_candidate_parent(true)
            )
            .unwrap_err()
            .to_string()
            .contains("captured-source")
        );
    }

    #[test]
    fn parent_comparison_preserves_canonical_kinds_modes_and_scope() {
        let (root, _) = landing_fixture();
        let repo = root.path();
        let old_link = git_stdout(repo, &["rev-parse", "HEAD~1"]);
        let new_link = git_stdout(repo, &["rev-parse", "HEAD"]);
        std::fs::create_dir(repo.join("scope")).unwrap();
        for (path, bytes) in [
            ("rename.txt", b"unique rename contents\n".as_slice()),
            ("delete.txt", b"deleted contents\n"),
            ("mode.txt", b"executable contents\n"),
            ("binary.dat", b"\0old binary"),
        ] {
            std::fs::write(repo.join("scope").join(path), bytes).unwrap();
        }
        git(repo, &["add", "scope"]);
        let old_blob = git_stdout(repo, &["hash-object", "-w", "scope/rename.txt"]);
        git(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "120000",
                &old_blob,
                "scope/link",
            ],
        );
        git(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000",
                &old_link,
                "scope/module",
            ],
        );
        git(repo, &["commit", "-m", "rich base"]);
        let a = git_stdout(repo, &["rev-parse", "HEAD"]);
        let a_tree = git_stdout(repo, &["rev-parse", "HEAD^{tree}"]);
        git(repo, &["mv", "scope/rename.txt", "scope/renamed.txt"]);
        git(repo, &["rm", "scope/delete.txt"]);
        std::fs::write(repo.join("scope/add.txt"), "new addition\n").unwrap();
        std::fs::write(repo.join("scope/binary.dat"), b"\0new binary").unwrap();
        git(repo, &["add", "scope/add.txt", "scope/binary.dat"]);
        git(repo, &["update-index", "--chmod=+x", "scope/mode.txt"]);
        let new_blob = git_stdout(repo, &["hash-object", "-w", "scope/add.txt"]);
        git(
            repo,
            &[
                "update-index",
                "--cacheinfo",
                "120000",
                &new_blob,
                "scope/link",
            ],
        );
        git(
            repo,
            &[
                "update-index",
                "--cacheinfo",
                "160000",
                &new_link,
                "scope/module",
            ],
        );
        git(repo, &["commit", "-m", "rich change"]);
        let c = git_stdout(repo, &["rev-parse", "HEAD"]);
        let c_tree = git_stdout(repo, &["rev-parse", "HEAD^{tree}"]);
        let add_outside = |commit: &str, parent: &str| {
            git(repo, &["read-tree", commit]);
            git(
                repo,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    "100644",
                    &new_blob,
                    "upstream.txt",
                ],
            );
            let tree = git_stdout(repo, &["write-tree"]);
            git_stdout(
                repo,
                &["commit-tree", &tree, "-p", parent, "-m", "advanced"],
            )
        };
        let b = add_outside(&a, &a);
        let candidate = add_outside(&c, &b);
        let candidate_tree = git_commit_tree_oid(repo, &candidate).unwrap();
        for scope in [vec![], vec!["scope".to_owned()]] {
            let files = capture_commit_range_diff_files(repo, &a, &c, &scope).unwrap();
            let provenance = crate::session::event::GitProvenance {
                source: RevisionSource::GitCommitRange {
                    mode: crate::model::CommitRangeCaptureMode::BaseTreeToTargetTree,
                    pathspecs: scope.clone(),
                },
                base: ReviewEndpoint::GitCommit {
                    commit_oid: a.clone(),
                    tree_oid: a_tree.clone(),
                },
                target: ReviewEndpoint::GitCommit {
                    commit_oid: c.clone(),
                    tree_oid: c_tree.clone(),
                },
            };
            let (source, compared, exact) = proof_inputs(
                repo,
                Some(&provenance),
                &files,
                &candidate,
                &candidate_tree,
                true,
            )
            .unwrap();
            assert_eq!(source.entries, compared.entries);
            assert!(!exact);
            assert_eq!(compared.path_scope, canonical_scope(&scope));
            assert_eq!(source.entries.len(), 7);
            use crate::session::evidence::{
                CanonicalChangeV1 as Change, CanonicalContentKindV1 as Kind,
            };
            for kind in [Kind::Text, Kind::Binary, Kind::Symlink, Kind::Submodule] {
                assert!(
                    source
                        .entries
                        .iter()
                        .any(|entry| entry.content_kind == kind)
                );
            }
            for change in [
                Change::Added,
                Change::Deleted,
                Change::Renamed,
                Change::ModeOnly,
            ] {
                assert!(source.entries.iter().any(|entry| entry.change == change));
            }
            // A candidate-only out-of-scope change is excluded only for scoped captures.
            git(repo, &["read-tree", &candidate]);
            git(
                repo,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    "100644",
                    &new_blob,
                    "extra.txt",
                ],
            );
            let expanded_tree = git_stdout(repo, &["write-tree"]);
            let expanded = git_stdout(
                repo,
                &["commit-tree", &expanded_tree, "-p", &b, "-m", "extra"],
            );
            let (_, expanded_input, _) = proof_inputs(
                repo,
                Some(&provenance),
                &files,
                &expanded,
                &expanded_tree,
                true,
            )
            .unwrap();
            assert_eq!(source.entries == expanded_input.entries, !scope.is_empty());
        }
    }

    fn git(repo: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success()
        );
    }

    fn git_stdout(repo: &Path, args: &[&str]) -> String {
        String::from_utf8(
            Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    }
    #[test]
    fn parent_record_rechecks_symbolic_candidate_before_publication() {
        let (root, token, _, _) = rewrite_fixture(false);
        let options = LandCommitOptions::new(root.path(), token, "track:author", "HEAD")
            .with_candidate_parent(true);
        let preview = preview_land_commit(options.clone()).unwrap();
        let before = store_inventory(root.path());
        let result = land_commit_with_hooks(
            options.with_expected_proof(preview.proof.evidence_sha256),
            || {
                git(
                    root.path(),
                    &["commit", "--amend", "-m", "changed identity"],
                );
            },
            || {},
        );
        assert!(
            result.is_err(),
            "candidate movement must refuse publication"
        );
        assert_eq!(before, store_inventory(root.path()));
    }
    #[test]
    fn parent_record_readback_binds_all_three_facts_and_requires_available_proof() {
        use crate::session::event::{
            RevisionCommitAssociatedPayload, RevisionRelationAttestedPayload,
        };
        use crate::session::evidence::{
            EvidenceAvailabilityV1, project_revision_relation_evidence_v1,
        };
        let (root, token, a, b) = rewrite_fixture(false);
        let options = LandCommitOptions::new(root.path(), token, "track:author", "HEAD")
            .with_candidate_parent(true);
        let preview = preview_land_commit(options.clone()).unwrap();
        let store = resolve_change_write_store(root.path()).unwrap();
        let events_before = store.event_store().unwrap().list_change_events().unwrap();
        let recorded =
            land_commit(options.with_expected_proof(&preview.proof.evidence_sha256)).unwrap();
        let proof_path = store.store_dir().join("artifacts/proofs").join(format!(
            "{}.json",
            preview
                .proof
                .evidence_sha256
                .strip_prefix("sha256:")
                .unwrap()
        ));
        let proof: RelationProofManifestV1 =
            serde_json::from_slice(&std::fs::read(&proof_path).unwrap()).unwrap();
        assert_eq!(proof, preview.proof);
        let events = store.event_store().unwrap().list_change_events().unwrap();
        let new_events: Vec<_> = events
            .iter()
            .filter(|e| !events_before.contains(e))
            .collect();
        assert_eq!(
            new_events.len(),
            2,
            "no capture, assessment or validation facts are added"
        );
        let association: RevisionCommitAssociatedPayload = serde_json::from_value(
            new_events
                .iter()
                .find(|e| e.event_type == EventType::RevisionCommitAssociated)
                .unwrap()
                .payload
                .clone(),
        )
        .unwrap();
        let attestation: RevisionRelationAttestedPayload = serde_json::from_value(
            new_events
                .iter()
                .find(|e| e.event_type == EventType::RevisionRelationAttested)
                .unwrap()
                .payload
                .clone(),
        )
        .unwrap();
        assert_eq!(association.commit_association_id, proof.association_id);
        assert_eq!(
            association.commit,
            ReviewEndpoint::GitCommit {
                commit_oid: recorded.commit_oid.clone(),
                tree_oid: preview.tree_oid.clone()
            }
        );
        assert_eq!(
            association.target,
            ReviewTargetRef::Revision {
                revision_id: recorded.revision.revision_id.clone()
            }
        );
        assert_eq!(attestation.revision, recorded.revision);
        assert_eq!(attestation.commit_association_id, proof.association_id);
        assert_eq!(
            attestation.comparison_base_or_parent.as_deref(),
            Some(a.as_str())
        );
        assert_eq!(proof.candidate.base_or_parent.as_deref(), Some(b.as_str()));
        let mut expected_endpoints = vec![recorded.commit_oid, preview.tree_oid];
        expected_endpoints.sort();
        assert_eq!(attestation.endpoint_oids, expected_endpoints);
        assert_eq!(
            attestation.evidence_content_hash.as_ref(),
            Some(&proof.evidence_sha256)
        );
        assert_eq!(attestation.result_digest, proof.result_digest().unwrap());
        let proofs = [(
            proof.evidence_sha256.clone(),
            (proof, EvidenceAvailabilityV1::Available),
        )]
        .into();
        let qualified = project_revision_relation_evidence_v1(
            recorded.revision.clone(),
            recorded.commit_association_id.clone(),
            std::slice::from_ref(&attestation),
            &proofs,
        )
        .unwrap();
        assert!(qualified.content_qualified);
        std::fs::remove_file(proof_path).unwrap();
        let unavailable = project_revision_relation_evidence_v1(
            recorded.revision,
            recorded.commit_association_id,
            &[attestation],
            &Default::default(),
        )
        .unwrap();
        assert!(!unavailable.content_qualified);
    }

    #[test]
    fn parent_record_rechecks_artifact_and_graph_before_publication() {
        for artifact in [true, false] {
            let (root, token, _, _) = rewrite_fixture(false);
            let options = LandCommitOptions::new(root.path(), &token, "track:author", "HEAD")
                .with_candidate_parent(true);
            let preview = preview_land_commit(options.clone()).unwrap();
            let mut after_mutation = None;
            let result = land_commit_with_hooks(
                options.with_expected_proof(&preview.proof.evidence_sha256),
                || {
                    if artifact {
                        let store = resolve_change_write_store(root.path()).unwrap();
                        let path =
                            crate::session::store::object_artifact::object_artifact_path_for_hash(
                                store.store_dir(),
                                &preview.revision.object_artifact_content_hash,
                            );
                        std::fs::remove_file(path).unwrap();
                    } else {
                        std::fs::write(root.path().join("sample.txt"), "parallel content\n")
                            .unwrap();
                        crate::session::capture_change_revision(
                            crate::session::ChangeCaptureOptions::advance(
                                "change-operation:landing-stale-graph",
                                crate::session::CaptureOptions::new(root.path()),
                                token.clone(),
                                crate::session::ChangeAdvanceV1::Parallel,
                            ),
                        )
                        .unwrap();
                    }
                    after_mutation = Some(store_inventory(root.path()));
                },
                || {},
            );
            let error = result.unwrap_err().to_string();
            assert!(
                if artifact {
                    error.contains("artifact") || error.contains("content")
                } else {
                    error.contains("change_graph_stale")
                },
                "{error}"
            );
            assert_eq!(after_mutation.unwrap(), store_inventory(root.path()));
        }
    }
    #[test]
    fn parent_comparison_refutes_same_hunk_when_upstream_changes_the_before_blob() {
        let (root, token) = landing_fixture();
        let repo = root.path();
        let base: String = (0..40).map(|i| format!("line {i}\n")).collect();
        std::fs::write(repo.join("sample.txt"), &base).unwrap();
        git(repo, &["add", "sample.txt"]);
        git(repo, &["commit", "-m", "long base"]);
        let a = git_stdout(repo, &["rev-parse", "HEAD"]);
        std::fs::write(
            repo.join("sample.txt"),
            base.replace("line 1\n", "reviewed change\n"),
        )
        .unwrap();
        git(repo, &["commit", "-am", "reviewed"]);
        let c = git_stdout(repo, &["rev-parse", "HEAD"]);
        let files = capture_commit_range_diff_files(repo, &a, &c, &[]).unwrap();
        let provenance = crate::session::event::GitProvenance {
            source: RevisionSource::GitCommitRange {
                mode: crate::model::CommitRangeCaptureMode::BaseTreeToTargetTree,
                pathspecs: vec![],
            },
            base: ReviewEndpoint::GitCommit {
                commit_oid: a.clone(),
                tree_oid: git_commit_tree_oid(repo, &a).unwrap(),
            },
            target: ReviewEndpoint::GitCommit {
                commit_oid: c.clone(),
                tree_oid: git_commit_tree_oid(repo, &c).unwrap(),
            },
        };
        git(repo, &["checkout", "--detach", &a]);
        std::fs::write(
            repo.join("sample.txt"),
            base.replace("line 30\n", "upstream change\n"),
        )
        .unwrap();
        git(repo, &["commit", "-am", "same file upstream"]);
        let b = git_stdout(repo, &["rev-parse", "HEAD"]);
        git(repo, &["cherry-pick", &c]);
        let candidate = git_stdout(repo, &["rev-parse", "HEAD"]);
        let reviewed_hunk = git_stdout(repo, &["diff", "--no-ext-diff", "--unified=0", &a, &c]);
        let candidate_hunk = git_stdout(
            repo,
            &["diff", "--no-ext-diff", "--unified=0", &b, &candidate],
        );
        assert_eq!(
            reviewed_hunk.split("@@").skip(1).collect::<Vec<_>>(),
            candidate_hunk.split("@@").skip(1).collect::<Vec<_>>()
        );
        let (source, compared, _) = proof_inputs(
            repo,
            Some(&provenance),
            &files,
            &candidate,
            &git_commit_tree_oid(repo, &candidate).unwrap(),
            true,
        )
        .unwrap();
        assert_ne!(source.entries[0].old_oid, compared.entries[0].old_oid);
        let proof = evaluate_relation_proof_v1(
            ReviewCursorV1::decode_token(&token).unwrap().revision,
            crate::model::CommitAssociationId::new("assoc-commit:sha256:before-blob"),
            RelationProofAlgorithmV1::CanonicalEquivalentRewrite,
            source,
            compared,
        )
        .unwrap();
        assert_eq!(proof.result.proof_status, RelationProofStatusV1::Refuted);
    }
    #[test]
    fn parent_preview_does_not_certify_replacement_bytes_as_the_candidate() {
        let (root, token, _, _) = rewrite_fixture(false);
        let good = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        std::fs::write(root.path().join("sample.txt"), "unreviewed candidate\n").unwrap();
        git(
            root.path(),
            &["commit", "--amend", "-am", "different content"],
        );
        let bad = git_stdout(root.path(), &["rev-parse", "HEAD"]);
        git(root.path(), &["replace", &bad, &good]);
        let before = store_inventory(root.path());
        let result = preview_land_commit(
            LandCommitOptions::new(root.path(), token, "track:author", bad)
                .with_candidate_parent(true),
        );
        assert!(
            result.is_err(),
            "replacement bytes must not qualify the original candidate OID"
        );
        assert_eq!(before, store_inventory(root.path()));
    }
    #[test]
    fn parent_original_policy_preserves_proof_and_structural_tree_under_replacements() {
        for replace_parent in [false, true] {
            let (root, token, _, parent) = rewrite_fixture(false);
            let candidate = git_stdout(root.path(), &["rev-parse", "HEAD"]);
            let options = LandCommitOptions::new(root.path(), token, "track:author", &candidate)
                .with_candidate_parent(true);
            let preview = preview_land_commit(options.clone()).unwrap();
            std::fs::write(root.path().join("sample.txt"), "other object bytes\n").unwrap();
            git(root.path(), &["commit", "--amend", "-am", "replacement"]);
            let replacement = git_stdout(root.path(), &["rev-parse", "HEAD"]);
            git(
                root.path(),
                &[
                    "replace",
                    if replace_parent { &parent } else { &candidate },
                    &replacement,
                ],
            );
            assert_eq!(preview, preview_land_commit(options.clone()).unwrap());
            let recorded =
                land_commit(options.with_expected_proof(&preview.proof.evidence_sha256)).unwrap();
            let store = resolve_change_write_store(root.path()).unwrap();
            let events = store.event_store().unwrap().list_change_events().unwrap();
            let associated = events
                .iter()
                .find(|e| {
                    e.event_type == EventType::RevisionCommitAssociated
                        && e.payload["commitAssociationId"].as_str()
                            == Some(recorded.commit_association_id.as_str())
                })
                .unwrap();
            assert_eq!(
                associated.payload["commit"]["treeOid"].as_str(),
                Some(preview.tree_oid.as_str())
            );
        }
    }
}
