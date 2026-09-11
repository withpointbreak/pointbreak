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
use crate::git::{capture_commit_range_diff_files, git_commit_tree_oid, git_rev_parse_commit_oid};
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
    RevisionShowOptions, SessionState, WriteAcknowledgementV1, associate_commit, current_timestamp,
    show_revision_for_change_reader, sign_event_if_requested, validated_track_id,
    writer_from_options,
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
            actor_id: None,
            signing: EventSigningOptions::default(),
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

pub fn land_commit(options: LandCommitOptions) -> Result<LandCommitResultV1> {
    land_commit_with_after_association(options, || {})
}

fn land_commit_with_after_association(
    options: LandCommitOptions,
    after_association: impl FnOnce(),
) -> Result<LandCommitResultV1> {
    if options.allow_extension && options.provenance_only {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "--allow-extension cannot be combined with --provenance-only".to_owned(),
        });
    }
    let track_id = validated_track_id(&options.track)?;
    let cursor = ReviewCursorV1::decode_token(&options.review_cursor)?;
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

    let write_store = resolve_change_write_store(&options.repo)?;
    let worktree_root = write_store.worktree_root();
    let storage = LocalStorage::new(write_store.store_dir());
    prepare_write_landing(&write_store, &storage)?;
    let commit_oid = git_rev_parse_commit_oid(worktree_root, &options.commit)?;
    let commit_tree_oid = git_commit_tree_oid(worktree_root, &commit_oid)?;
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

    let proof_outcome =
        ContentArtifacts::from_backend(write_store.backend()).put_relation_proof(&proof)?;
    let mut association_options = AssociateCommitOptions::new(&options.repo, &commit_oid)
        .with_review_cursor(options.review_cursor.clone())
        .with_track(options.track.clone());
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
                "landed as a verified equivalent rewrite of {}",
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
    let candidate_files = capture_commit_range_diff_files(
        repo,
        &base,
        candidate_commit,
        &path_scope_for_git(&path_scope),
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
        base_or_parent: Some(base),
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
            association.acknowledgement.derived = DerivedWriteAcknowledgementV1::new(
                DerivedWriteAvailabilityV1::Current,
                Some(DerivedVisibilityTokenV1 {
                    generation_id: "g1".into(),
                    epoch: 3,
                    head_sequence: 5,
                }),
            )
            .unwrap();
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
}
