use std::path::Path;

use crate::error::Result;
use crate::session::event::ShoreEvent;
use crate::session::state::{ProjectionDiagnostic, SessionState};
#[cfg(test)]
use crate::session::store::backend::StoreBackend;
use crate::session::store::resolution::{resolve_read_store, resolve_write_store};
use crate::session::{EventStore, SkippedEvent, sweep_stale_temp_files};
use crate::storage::{Durability, LocalStorage};

#[cfg(test)]
fn load_or_rebuild_session_state(repo: impl AsRef<Path>) -> Result<Option<SessionState>> {
    let Some((_backend, events)) = list_events_if_store_exists(repo)? else {
        return Ok(None);
    };

    Ok(Some(SessionState::from_events(&events)?))
}

pub fn rebuild_state(repo: impl AsRef<Path>) -> Result<SessionState> {
    // Resolve the store the same way read and write surfaces do, so the rebuilt
    // projection lands in (and is replayed from) the resolved store — the shared
    // common-dir store by default — never a stale worktree-local copy.
    let write_store = resolve_write_store(repo.as_ref())?;
    let store_dir = write_store.store_dir();
    let worktree_root = write_store.worktree_root();
    let storage = LocalStorage::new(store_dir);
    sweep_stale_temp_files(&storage, store_dir)?;

    let span = tracing::info_span!("session.rebuild_state", repo = %worktree_root.display());
    let _entered = span.enter();

    let state =
        SessionState::from_events(&EventStore::from_backend(write_store.backend()).list_events()?)?;
    storage.write_json_atomic(
        &store_dir.join("state.json"),
        &state,
        Durability::Projection,
    )?;
    Ok(state)
}

/// Diagnostic code carried in a write result when the legacy `state.json`
/// projection could not be replaced after the authoritative event was durable.
pub(crate) const LEGACY_STATE_PROJECTION_REFRESH_FAILED: &str =
    "legacy_state_projection_refresh_failed";

/// Best-effort publication of the legacy `state.json` projection. The
/// authoritative event is already durable when this runs, so a failed
/// replacement never fails the enclosing command: it is returned as one
/// diagnostic for the result's `diagnostics`, and the next successful
/// projection-producing write (or `rebuild_state`) regenerates the file.
#[must_use = "retain both the refresh state and its advisory diagnostic"]
pub(crate) struct LegacyProjectionRefresh {
    pub(crate) state: crate::session::LegacyProjectionStateV1,
    pub(crate) diagnostic: Option<ProjectionDiagnostic>,
}

pub(crate) fn publish_legacy_state_projection(
    storage: &LocalStorage,
    store_dir: &Path,
    state: &SessionState,
) -> LegacyProjectionRefresh {
    match storage.write_json_atomic(&store_dir.join("state.json"), state, Durability::Projection) {
        Ok(()) => LegacyProjectionRefresh {
            state: crate::session::LegacyProjectionStateV1::Refreshed,
            diagnostic: None,
        },
        Err(error) => LegacyProjectionRefresh {
            state: crate::session::LegacyProjectionStateV1::RefreshFailed,
            diagnostic: Some(ProjectionDiagnostic {
                code: LEGACY_STATE_PROJECTION_REFRESH_FAILED.to_owned(),
                message: format!(
                    "legacy state projection was not refreshed after durable truth: {error}"
                ),
            }),
        },
    }
}

pub fn read_events(repo: impl AsRef<Path>) -> Result<Vec<ShoreEvent>> {
    let read_store = resolve_read_store(repo.as_ref())?;
    EventStore::from_backend(read_store.backend()).list_events()
}

/// Render each skipped retired event as a `ProjectionDiagnostic`, carrying the
/// break record's canonical sentence as the message. The diagnostic class
/// strings pass through unchanged.
pub(crate) fn skipped_to_diagnostics(skipped: Vec<SkippedEvent>) -> Vec<ProjectionDiagnostic> {
    skipped
        .into_iter()
        .map(|s| ProjectionDiagnostic {
            code: s.code.to_owned(),
            message: s.record.to_string(),
        })
        .collect()
}

/// A read for human-facing surfaces: a retired/unsupported event is skipped and
/// surfaced as a `ProjectionDiagnostic` instead of aborting the whole read. The
/// strict [`read_events`] is unchanged and remains the reader for every surface
/// that must hard-fail on an unreadable event.
pub fn read_events_for_display(
    repo: impl AsRef<Path>,
) -> Result<(Vec<ShoreEvent>, Vec<ProjectionDiagnostic>)> {
    let read_store = resolve_read_store(repo.as_ref())?;
    let (events, skipped) = EventStore::from_backend(read_store.backend()).list_events_lenient()?;
    Ok((events, skipped_to_diagnostics(skipped)))
}

#[cfg(test)]
fn list_events_if_store_exists(
    repo: impl AsRef<Path>,
) -> Result<Option<(StoreBackend, Vec<ShoreEvent>)>> {
    let read_store = resolve_read_store(repo.as_ref())?;
    // The store dir is the existence gate; the backend handle is what note-body
    // reads flow through.
    if !read_store.store_dir().exists() {
        return Ok(None);
    }

    let events = EventStore::from_backend(read_store.backend()).list_events()?;
    Ok(Some((read_store.backend().clone(), events)))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::error::SchemaBreakRecord;
    use crate::model::JournalId;
    use crate::session::event::{EventTarget, EventType, ReviewInitializedPayload, Writer};

    #[test]
    fn skipped_to_diagnostics_maps_code_and_message() {
        let skipped = vec![SkippedEvent {
            code: "unsupported_event_type",
            record: SchemaBreakRecord {
                retired: "review_disposition_recorded".to_owned(),
                broken_at: "0.1".to_owned(),
                anchor: "docs/assessment-model.md#legacy-disposition-events".to_owned(),
            },
        }];

        let diags = skipped_to_diagnostics(skipped);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "unsupported_event_type");
        assert!(diags[0].message.contains("review_disposition_recorded"));
        assert!(diags[0].message.contains("#legacy-disposition-events"));
    }

    #[cfg(unix)]
    #[test]
    fn publish_legacy_state_projection_reports_replacement_failure() {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path());
        let state = SessionState::from_events(&[review_initialized()]).unwrap();
        // A directory in the projection's place: the temp file still lands in
        // the parent, only the final rename onto `state.json` fails.
        std::fs::create_dir(dir.path().join("state.json")).unwrap();

        let refresh = publish_legacy_state_projection(&storage, dir.path(), &state);
        assert_eq!(
            refresh.state,
            crate::session::LegacyProjectionStateV1::RefreshFailed
        );
        let diagnostic = refresh
            .diagnostic
            .expect("a failed replacement must yield a diagnostic");
        assert_eq!(diagnostic.code, LEGACY_STATE_PROJECTION_REFRESH_FAILED);
        assert!(diagnostic.message.contains("rename temp file"));

        std::fs::remove_dir(dir.path().join("state.json")).unwrap();
        let refresh = publish_legacy_state_projection(&storage, dir.path(), &state);
        assert_eq!(
            refresh.state,
            crate::session::LegacyProjectionStateV1::Refreshed
        );
        assert!(refresh.diagnostic.is_none());
        let expected = dir.path().join("expected.json");
        storage
            .write_json_atomic(&expected, &state, Durability::Projection)
            .unwrap();
        assert_eq!(
            std::fs::read(expected).unwrap(),
            std::fs::read(dir.path().join("state.json")).unwrap()
        );
        assert!(dir.path().join("state.json").is_file());
    }

    #[test]
    fn load_or_rebuild_session_state_returns_none_when_shore_dir_absent() {
        let repo = tempfile::tempdir().expect("create repo");
        Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .unwrap();

        let state = load_or_rebuild_session_state(repo.path()).unwrap();

        assert!(state.is_none());
    }

    #[test]
    fn load_or_rebuild_session_state_rebuilds_from_events_when_store_present() {
        let repo = test_repo_with(vec![review_initialized()]);

        let state = load_or_rebuild_session_state(repo.path()).unwrap();
        let state = state.expect("state should be present when the resolved store exists");

        assert_eq!(state.event_count, 1);
    }

    fn test_repo_with(events: Vec<ShoreEvent>) -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("create repo");
        Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        // Write to the resolved store (the shared common-dir store), where the
        // read surfaces look — not the raw worktree-local `.pointbreak/data`.
        let store_dir = resolve_read_store(repo.path())
            .unwrap()
            .store_dir()
            .to_path_buf();
        std::fs::create_dir_all(store_dir.join("events")).unwrap();
        let store = EventStore::open(&store_dir);
        for event in events {
            store.record_event_once(&event).unwrap();
        }
        repo
    }

    fn review_initialized() -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            "review_initialized:session:default:work:default",
            EventTarget::for_journal(JournalId::new("journal:default")),
            Writer::shore_local("0.1.0"),
            ReviewInitializedPayload {},
            "2026-05-10T00:00:00Z",
        )
        .unwrap()
    }
}
