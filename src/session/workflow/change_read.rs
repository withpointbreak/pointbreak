use std::path::Path;
use std::sync::Arc;

use crate::error::Result;
use crate::session::derived_access::semantic::change::build_change_semantic_generation;
use crate::session::event::ShoreEvent;
#[cfg(test)]
use crate::session::store::backend::StoreBackend;
#[cfg(test)]
use crate::session::store::capabilities::inspect_change_reader_journal_records;
use crate::session::store::capabilities::{
    JournalInspection, StoreCapabilityInspection, StoreCapabilityStatus,
};
use crate::session::store::resolution::resolve_change_read_store;
use crate::session::{AuthorityCursorV2, ChangeDocumentProjectionV1, ChangeProjection, EventStore};

/// The complete strict semantic input for a Change-capable reader.
///
/// This owns no path, backend, or derived-store handle. CLI, Inspector, and
/// extension adapters receive the same authoritative projections and must feed
/// them to `ChangeDocumentFacadeV1`; they cannot reconstruct policy from raw
/// events or from one another's wire documents.
#[derive(Clone, Debug)]
pub struct ChangeReaderReadyV1 {
    pub projection: ChangeProjection,
    pub document_projection: ChangeDocumentProjectionV1,
    events: Arc<Vec<ShoreEvent>>,
    event_set_hash: String,
    authority_cursor: AuthorityCursorV2,
}

impl ChangeReaderReadyV1 {
    pub(crate) fn events(&self) -> &[ShoreEvent] {
        &self.events
    }

    pub(crate) fn event_set_hash(&self) -> &str {
        &self.event_set_hash
    }

    /// Build the complete headless Change document facade from this one
    /// capability-validated generation. Inline proposal presentation is folded
    /// here so no adapter can pair semantic state with a different event set.
    pub fn document_facade(&self) -> Result<crate::documents::ChangeDocumentFacadeV1> {
        let presentations = crate::documents::change_presentation_projection(
            &self.projection,
            &self.document_projection,
            &self.events,
            &self.event_set_hash,
        )?;
        crate::documents::ChangeDocumentFacadeV1::new(
            self.projection.clone(),
            self.document_projection.clone(),
        )?
        .with_presentations(presentations)
    }

    /// Build the typed, review-domain event timeline from the same complete
    /// generation as the Change documents. Raw event payload JSON remains
    /// private to the projector.
    pub fn event_history_facade(
        &self,
        trust_set: &crate::session::TrustSet,
    ) -> Result<crate::documents::EventHistoryFacadeV1> {
        let source_change_projection_stamp = self.document_facade()?.projection_stamp().to_owned();
        crate::session::projection::event_history::project_event_history(
            &self.events,
            &self.document_projection,
            self.authority_cursor.clone(),
            source_change_projection_stamp,
            trust_set,
        )
    }
}

/// One complete-or-refuse capability snapshot for a cold or warm reader.
#[derive(Clone, Debug)]
pub struct ChangeReaderStateV1 {
    pub capability: StoreCapabilityInspection,
    ready: Option<ChangeReaderReadyV1>,
}

impl ChangeReaderStateV1 {
    pub fn ready(&self) -> Option<&ChangeReaderReadyV1> {
        self.ready.as_ref()
    }
}

/// Inspect one repo without crossing the legacy event-only read preflight.
pub fn change_reader_state_for_repo(repo: impl AsRef<Path>) -> Result<ChangeReaderStateV1> {
    let (_store, inspection) = resolve_change_read_store(repo)?;
    change_reader_state_from_inspection(&inspection)
}

fn change_reader_state_from_inspection(
    inspection: &JournalInspection,
) -> Result<ChangeReaderStateV1> {
    let capability = StoreCapabilityInspection {
        status: inspection.status.clone(),
        cursor: inspection.cursor.clone(),
        minimum_reader_profile: inspection.minimum_reader_profile.clone(),
    };
    let ready = if matches!(inspection.status, StoreCapabilityStatus::Ready { .. }) {
        let generation = build_change_semantic_generation(inspection)?;
        generation.validate()?;
        let events = inspection
            .event_entries
            .iter()
            .map(|entry| {
                EventStore::decode_qualification_entry(
                    entry.key_digest.clone(),
                    entry.bytes.clone(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Some(ChangeReaderReadyV1 {
            projection: generation.projection,
            document_projection: generation.document_projection,
            events: Arc::new(events),
            event_set_hash: inspection.cursor.event_set_hash.clone(),
            authority_cursor: inspection.cursor.clone(),
        })
    } else {
        None
    };
    Ok(ChangeReaderStateV1 { capability, ready })
}

#[cfg(test)]
fn change_reader_state_from_backend_for_test(
    backend: &StoreBackend,
) -> Result<ChangeReaderStateV1> {
    let inspection = inspect_change_reader_journal_records(backend.journal().as_ref())?;
    change_reader_state_from_inspection(&inspection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench_support::longitudinal::LongitudinalCountingScopeV1;
    use crate::model::{JournalId, ObservationId, ReviewTargetRef, RevisionId, TrackId};
    use crate::session::event::{EventTarget, EventType, ReviewObservationRecordedPayload, Writer};
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::workflow::{
        CaptureOptions, RevisionShowOptions, SnapshotContentState, capture_worktree_review,
        show_revision_for_change_reader,
    };

    #[test]
    fn capable_reader_is_complete_or_refuse_across_l0_m1_and_l2() {
        let l0_backend = StoreBackend::memory();

        let l0 = change_reader_state_from_backend_for_test(&l0_backend).unwrap();
        assert!(l0.ready().is_none());
        assert!(matches!(
            l0.capability.status,
            StoreCapabilityStatus::MigrationRequired
        ));

        let m1_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            m1_backend.journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .unwrap();
        let m1 = change_reader_state_from_backend_for_test(&m1_backend).unwrap();
        assert!(m1.ready().is_none());
        assert!(matches!(
            m1.capability.status,
            StoreCapabilityStatus::MigrationInProgress { .. }
        ));

        let l2_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            l2_backend.journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let l2 = change_reader_state_from_backend_for_test(&l2_backend).unwrap();
        assert!(matches!(
            l2.capability.status,
            StoreCapabilityStatus::Ready { .. }
        ));
        assert!(l2.ready().is_some());
        assert!(!l2.ready().unwrap().projection.changes.is_empty());
        let ready = l2.ready().unwrap();
        let history = ready
            .event_history_facade(&crate::session::TrustSet::default())
            .unwrap()
            .document();
        assert_eq!(history.authority_cursor, l2.capability.cursor);
        assert_eq!(
            history.source_change_projection_stamp,
            ready.document_facade().unwrap().projection_stamp()
        );
    }

    #[test]
    fn untouched_l0_with_a_recognized_retired_event_stays_typed() {
        let backend = StoreBackend::memory();
        backend
            .journal()
            .create_record_once(
                "legacy-retired-event",
                br#"{"eventType":"review_disposition_recorded"}"#,
            )
            .unwrap();

        let state = change_reader_state_from_backend_for_test(&backend).unwrap();
        assert!(state.ready().is_none());
        assert!(matches!(
            state.capability.status,
            StoreCapabilityStatus::MigrationRequired
        ));
        assert_eq!(state.capability.cursor.journal_record_count, 1);
        assert_eq!(state.capability.cursor.event_count, 0);
    }

    #[test]
    fn l2_exact_revision_reads_captured_content_without_legacy_preflight() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "--quiet"]);
        git(repo.path(), &["config", "user.name", "Pointbreak Test"]);
        git(
            repo.path(),
            &["config", "user.email", "pointbreak@example.test"],
        );
        git(repo.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.path().join("sample.txt"), "before\n").unwrap();
        git(repo.path(), &["add", "sample.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "base"]);
        std::fs::write(repo.path().join("sample.txt"), "after\n").unwrap();

        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let (store, inspection) = resolve_change_read_store(repo.path()).unwrap();
        assert!(matches!(
            inspection.status,
            StoreCapabilityStatus::MigrationRequired
        ));
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();

        let result = show_revision_for_change_reader(
            RevisionShowOptions::new(repo.path())
                .with_revision_id(capture.revision_id)
                .with_exact(true),
        )
        .unwrap();
        assert_eq!(result.snapshot_content_state, SnapshotContentState::Present);
        assert_eq!(
            result.revision.object_artifact_content_hash,
            capture.object_artifact_content_hash
        );
        assert_eq!(result.snapshot.files.len(), 1);
    }

    #[test]
    fn fact_only_event_advances_the_shared_change_document_stamp() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();

        let before = change_reader_state_from_backend_for_test(&backend).unwrap();
        let before_ready = before.ready().expect("L2 reader is ready");
        let revision_id = RevisionId::new("rev:sha256:presentation-test");
        let before_stamp = before_ready
            .document_facade()
            .unwrap()
            .list_document_for_inspector_with_presentations()
            .unwrap()
            .document
            .projection_stamp;

        EventStore::from_backend(&backend)
            .record_change_event_once(&external_observation_event(revision_id))
            .unwrap();

        let after = change_reader_state_from_backend_for_test(&backend).unwrap();
        let after_ready = after.ready().expect("L2 reader stays ready");
        assert_eq!(after_ready.projection, before_ready.projection);
        assert_eq!(
            after_ready.document_projection,
            before_ready.document_projection
        );
        assert_ne!(after_ready.event_set_hash(), before_ready.event_set_hash());

        let facade = after_ready.document_facade().unwrap();
        let list_stamp = facade
            .list_document_for_inspector_with_presentations()
            .unwrap()
            .document
            .projection_stamp;
        let review_attention_stamp = facade
            .attention_document_with_presentations(false)
            .unwrap()
            .document
            .projection_stamp;
        let inspect_attention_stamp = facade
            .attention_document_with_presentations(true)
            .unwrap()
            .document
            .projection_stamp;
        let change_id = facade
            .list_document_for_inspector_with_presentations()
            .unwrap()
            .document
            .changes
            .first()
            .expect("qualification fixture has a Change")
            .change_id
            .clone();
        let detail_stamp = facade
            .detail_document(&change_id)
            .unwrap()
            .detail
            .projection_stamp;

        assert_ne!(list_stamp, before_stamp);
        assert_eq!(review_attention_stamp, list_stamp);
        assert_eq!(inspect_attention_stamp, list_stamp);
        assert_eq!(detail_stamp, list_stamp);
    }

    #[test]
    fn ready_generation_remains_immutable_across_a_later_append() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();

        let before = change_reader_state_from_backend_for_test(&backend).unwrap();
        let before_ready = before.ready().expect("L2 reader is ready");
        let before_change_stamp = before_ready
            .document_facade()
            .unwrap()
            .projection_stamp()
            .to_owned();
        let before_history = before_ready
            .event_history_facade(&crate::session::TrustSet::default())
            .unwrap()
            .document();
        assert_eq!(
            before_history.source_change_projection_stamp,
            before_change_stamp
        );

        EventStore::from_backend(&backend)
            .record_change_event_once(&external_observation_event(RevisionId::new(
                "rev:sha256:immutable-generation-test",
            )))
            .unwrap();

        // A ready reader is one immutable generation. Re-projecting through it
        // after the append must not mix its old Change snapshot with new events.
        let retained_history = before_ready
            .event_history_facade(&crate::session::TrustSet::default())
            .unwrap()
            .document();
        assert_eq!(retained_history, before_history);
        assert_eq!(
            before_ready.document_facade().unwrap().projection_stamp(),
            before_change_stamp
        );

        // A subsequent load observes the append as one entirely new generation.
        let after = change_reader_state_from_backend_for_test(&backend).unwrap();
        let after_ready = after.ready().expect("L2 reader stays ready");
        let after_change_stamp = after_ready
            .document_facade()
            .unwrap()
            .projection_stamp()
            .to_owned();
        let after_history = after_ready
            .event_history_facade(&crate::session::TrustSet::default())
            .unwrap()
            .document();
        assert_ne!(
            after_history.authority_cursor,
            before_history.authority_cursor
        );
        assert_ne!(
            after_history.timeline_projection_stamp,
            before_history.timeline_projection_stamp
        );
        assert_eq!(
            after_history.entries.len(),
            before_history.entries.len() + 1
        );
        assert_eq!(
            after_history.source_change_projection_stamp,
            after_change_stamp
        );
        assert_ne!(after_change_stamp, before_change_stamp);
    }

    #[test]
    fn change_list_and_attention_presentations_read_zero_content_artifacts() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let revision_id = RevisionId::new("rev:sha256:presentation-test");
        EventStore::from_backend(&backend)
            .record_change_event_once(&external_observation_event(revision_id))
            .unwrap();

        let counting = LongitudinalCountingScopeV1::new("a".repeat(64)).unwrap();
        let guard = counting.enter();
        let state = change_reader_state_from_backend_for_test(&backend).unwrap();
        let facade = state
            .ready()
            .expect("L2 reader stays ready")
            .document_facade()
            .unwrap();
        serde_json::to_vec(
            &facade
                .list_document_for_inspector_with_presentations()
                .unwrap(),
        )
        .unwrap();
        serde_json::to_vec(&facade.attention_document_with_presentations(false).unwrap()).unwrap();
        serde_json::to_vec(&facade.attention_document_with_presentations(true).unwrap()).unwrap();
        drop(guard);

        let counters = counting.snapshot().counters;
        assert_eq!(counters.body_artifact_reads, 0);
        assert_eq!(counters.object_artifact_reads, 0);
    }

    fn external_observation_event(revision_id: RevisionId) -> ShoreEvent {
        let track_id = TrackId::new("track:presentation-test");
        let body_stem = "e".repeat(64);
        let payload = ReviewObservationRecordedPayload {
            observation_id: ObservationId::new("observation:sha256:presentation-test"),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            title: "fact-only presentation stamp".to_owned(),
            body: None,
            body_content_type: Default::default(),
            body_artifact_path: Some(format!("artifacts/notes/{body_stem}.json")),
            body_byte_size: Some(5_000),
            body_content_hash: Some(format!("sha256:{body_stem}")),
            tags: Vec::new(),
            confidence: None,
            supersedes_observation_ids: Vec::new(),
            responds_to_observation_ids: Vec::new(),
        };
        ShoreEvent::new(
            EventType::ReviewObservationRecorded,
            ReviewObservationRecordedPayload::idempotency_key(
                &revision_id,
                &track_id,
                "presentation-test",
            ),
            EventTarget::for_revision(
                JournalId::new("journal:qualification"),
                revision_id,
                Some(track_id),
            )
            .unwrap(),
            Writer::shore_local("presentation-test"),
            payload,
            "2026-08-04T00:01:00Z",
        )
        .unwrap()
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
