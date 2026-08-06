//! Explicit cross-Revision context continuity.
//!
//! Ports never move fact ownership. They name one exact origin fact and one
//! exact target Revision, while validation and assessment deliberately have no
//! representation in this workflow.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::crypto::EventSigner;
use crate::error::{Result, ShoreError};
use crate::model::{ActorId, ReviewTargetRef, RevisionRefV1, TargetRef};
use crate::session::event::{
    EventTarget, EventType, FactPortRelationV1, FactRefV1, ReviewFactPortDraftV1, ShoreEvent,
    build_review_fact_ported,
};
use crate::session::store::resolution::{prepare_write_landing, resolve_change_write_store};
use crate::session::{
    BestEffortSkipSink, EventSigningOptions, EventStore, EventWriteOutcome, ReviewCursorV1,
    RevisionShowOptions, SessionState, current_timestamp, show_revision_for_change_reader,
    sign_event_if_requested, validated_track_id, writer_from_options,
};
use crate::storage::{Durability, LocalStorage};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactPortOptions {
    repo: PathBuf,
    origin_revision: RevisionRefV1,
    origin_fact: FactRefV1,
    review_cursor: String,
    relation: FactPortRelationV1,
    target_fact: Option<FactRefV1>,
    rationale_content_hash: Option<String>,
    track: String,
    actor_id: Option<ActorId>,
    signing: EventSigningOptions,
}

impl FactPortOptions {
    pub fn new(
        repo: impl AsRef<Path>,
        origin_revision: RevisionRefV1,
        origin_fact: FactRefV1,
        review_cursor: impl Into<String>,
        relation: FactPortRelationV1,
        track: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            origin_revision,
            origin_fact,
            review_cursor: review_cursor.into(),
            relation,
            target_fact: None,
            rationale_content_hash: None,
            track: track.into(),
            actor_id: None,
            signing: EventSigningOptions::default(),
        }
    }

    pub fn with_target_fact(mut self, fact: FactRefV1) -> Self {
        self.target_fact = Some(fact);
        self
    }

    pub fn with_rationale_content_hash(mut self, hash: impl Into<String>) -> Self {
        self.rationale_content_hash = Some(hash.into());
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
pub struct FactPortResultV1 {
    pub schema: String,
    pub port_id: crate::model::ReviewFactPortId,
    pub origin_revision: RevisionRefV1,
    pub target_revision: RevisionRefV1,
    pub relation: FactPortRelationV1,
    pub event_id: crate::model::EventId,
    pub created: bool,
}

pub fn port_review_fact(options: FactPortOptions) -> Result<FactPortResultV1> {
    let track_id = validated_track_id(&options.track)?;
    let cursor = ReviewCursorV1::decode_token(&options.review_cursor)?;
    let target_revision_id =
        super::exact_revision_from_review_cursor(&options.repo, &options.review_cursor)?;
    let origin = show_revision_for_change_reader(
        RevisionShowOptions::new(&options.repo)
            .with_revision_id(options.origin_revision.revision_id.clone())
            .with_exact(true),
    )?;
    let actual_origin = RevisionRefV1::new(
        origin.revision.revision_id.clone(),
        origin.revision.object_artifact_content_hash.clone(),
    )?;
    if actual_origin != options.origin_revision {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "origin RevisionRef does not match the captured artifact binding".to_owned(),
        });
    }
    let target = show_revision_for_change_reader(
        RevisionShowOptions::new(&options.repo)
            .with_revision_id(target_revision_id)
            .with_exact(true),
    )?;
    let actual_target = RevisionRefV1::new(
        target.revision.revision_id.clone(),
        target.revision.object_artifact_content_hash.clone(),
    )?;
    if actual_target != cursor.revision {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "Review cursor target no longer matches its artifact binding".to_owned(),
        });
    }
    require_fact(&origin, &options.origin_fact, "origin")?;
    if let Some(target_fact) = &options.target_fact {
        require_fact(&target, target_fact, "target")?;
    }

    let write_store = resolve_change_write_store(&options.repo)?;
    let storage = LocalStorage::new(write_store.store_dir());
    prepare_write_landing(&write_store, &storage)?;
    let writer = writer_from_options(write_store.worktree_root(), options.actor_id.as_ref());
    let payload = build_review_fact_ported(
        ReviewFactPortDraftV1 {
            origin_revision: options.origin_revision.clone(),
            origin_fact: options.origin_fact.clone(),
            target_revision: actual_target.clone(),
            relation: options.relation,
            target_fact: options.target_fact,
            rationale_content_hash: options.rationale_content_hash,
            context_change_id: Some(cursor.change_id),
        },
        &writer.actor_id,
        &track_id,
    )?;
    let subject = match &options.origin_fact {
        FactRefV1::Observation { observation_id } => ReviewTargetRef::Observation {
            revision_id: options.origin_revision.revision_id.clone(),
            observation_id: observation_id.clone(),
        },
        FactRefV1::InputRequest { input_request_id } => ReviewTargetRef::InputRequest {
            revision_id: options.origin_revision.revision_id.clone(),
            input_request_id: input_request_id.clone(),
        },
    };
    let mut event = ShoreEvent::new(
        EventType::ReviewFactPorted,
        payload.port_id.as_str(),
        EventTarget::for_subject(
            origin.revision.journal_id,
            TargetRef::Review(subject),
            Some(track_id),
        )?,
        writer,
        payload.clone(),
        current_timestamp(),
    )?;
    sign_event_if_requested(&mut event, &options.signing)?;
    let event_store = EventStore::from_backend(write_store.backend());
    let outcome = event_store.record_change_event_once(&event)?;
    let state = SessionState::from_events(&event_store.list_change_events()?)?;
    storage.write_json_atomic(
        &write_store.store_dir().join("state.json"),
        &state,
        Durability::Projection,
    )?;
    Ok(FactPortResultV1 {
        schema: "pointbreak.review-fact-port.v1".to_owned(),
        port_id: payload.port_id,
        origin_revision: options.origin_revision,
        target_revision: actual_target,
        relation: options.relation,
        event_id: event.event_id,
        created: outcome == EventWriteOutcome::Created,
    })
}

fn require_fact(
    revision: &crate::session::RevisionShowResult,
    fact: &FactRefV1,
    role: &str,
) -> Result<()> {
    let exists = match fact {
        FactRefV1::Observation { observation_id } => revision
            .observations
            .iter()
            .any(|observation| &observation.id == observation_id),
        FactRefV1::InputRequest { input_request_id } => revision
            .input_requests
            .iter()
            .any(|request| &request.id == input_request_id),
    };
    if exists {
        Ok(())
    } else {
        Err(ShoreError::WorkflowInputInvalid {
            reason: format!("{role} fact does not belong to the exact {role} Revision"),
        })
    }
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
        ChangeCreateOptions, ChangeMembershipOptions, CommitRangeSpec, ObservationAddOptions,
        ReviewSourceBindingV1, capture_review, create_change, join_revision_to_change,
        record_observation, select_review_cursor,
    };

    #[test]
    fn context_port_preserves_exact_origin_ownership_and_retries_idempotently() {
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

        std::fs::write(root.path().join("sample.txt"), "first\n").unwrap();
        git(root.path(), &["commit", "--quiet", "-am", "first"]);
        let first = capture_review(
            crate::session::CaptureOptions::new(root.path())
                .with_commit_range(CommitRangeSpec::new(&base)),
        )
        .unwrap();
        let observation = record_observation(
            ObservationAddOptions::new(root.path())
                .with_exact_revision_id(first.revision_id.clone())
                .with_track("track:author")
                .with_title("retain this context"),
        )
        .unwrap();

        std::fs::write(root.path().join("sample.txt"), "second\n").unwrap();
        git(root.path(), &["commit", "--quiet", "-am", "second"]);
        let second = capture_review(
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
            "change-operation:fact-port-create",
            ChangeIdentityDescriptorV1::opaque_nonce([0x72; 32]),
        ))
        .unwrap();
        join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:fact-port-join-first",
            change.change_id.clone(),
            first.revision_id.clone(),
        ))
        .unwrap();
        join_revision_to_change(ChangeMembershipOptions::new(
            root.path(),
            "change-operation:fact-port-join-second",
            change.change_id.clone(),
            second.revision_id.clone(),
        ))
        .unwrap();
        let ready = crate::session::change_reader_state_for_repo(root.path())
            .unwrap()
            .ready()
            .unwrap()
            .clone();
        let selected = select_review_cursor(
            &ready.projection.changes[&change.change_id],
            &ready.document_projection,
            Some(&second.revision_id),
            false,
            ReviewSourceBindingV1::Captured,
        )
        .unwrap();
        let origin =
            RevisionRefV1::new(first.revision_id, first.object_artifact_content_hash).unwrap();
        let origin_fact = FactRefV1::Observation {
            observation_id: observation.observation_id,
        };

        let first_port = port_review_fact(FactPortOptions::new(
            root.path(),
            origin.clone(),
            origin_fact.clone(),
            &selected.token,
            FactPortRelationV1::ContextOnly,
            "track:author",
        ))
        .unwrap();
        assert!(first_port.created);
        assert_eq!(first_port.origin_revision, origin);
        assert_eq!(first_port.target_revision.revision_id, second.revision_id);

        let retry = port_review_fact(FactPortOptions::new(
            root.path(),
            origin,
            origin_fact,
            selected.token,
            FactPortRelationV1::ContextOnly,
            "track:author",
        ))
        .unwrap();
        assert_eq!(retry.port_id, first_port.port_id);
        assert!(!retry.created);
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
