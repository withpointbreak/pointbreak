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
use crate::session::{ChangeDocumentProjectionV1, ChangeProjection, EventStore};

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
}

impl ChangeReaderReadyV1 {
    pub(crate) fn events(&self) -> &[ShoreEvent] {
        &self.events
    }

    pub(crate) fn event_set_hash(&self) -> &str {
        &self.event_set_hash
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
