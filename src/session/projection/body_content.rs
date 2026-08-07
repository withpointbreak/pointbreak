//! Read-time resolution of note-shaped body content against recorded
//! removals, the body twin of the snapshot content seam.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::error::{Result, ShoreError};
use crate::session::body_artifact::{
    load_body_artifact, note_body_content_hash_from_path, parse_note_body_artifact,
};
use crate::session::projection::artifact_removal::{
    ArtifactRemovalProjection, RemovalOperativeStatus,
};
use crate::session::projection::cosignature::CosignatureIndex;
use crate::session::signing::{RemovalPolicy, TrustSet};
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::backend::StoreBackend;
use crate::session::store::content::ContentArtifacts;

/// Shared availability vocabulary for exact captured resources and
/// note-shaped fact bodies.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContentAvailabilityV1 {
    #[default]
    Available,
    Removed,
    Missing,
    Mismatch,
    NonTextual,
}

impl ContentAvailabilityV1 {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Wire+library state of a note-shaped body (observation body, input-request
/// body, response reason, assessment summary, validation summary, imported
/// note body). Body twin of `SnapshotContentState`: `SuppressedPresent` means
/// a removal is recorded but the bytes are still stored; `PhysicallyRemoved`
/// means the bytes have been swept from the store.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BodyContentState {
    #[default]
    Present,
    SuppressedPresent,
    PhysicallyRemoved,
}

impl BodyContentState {
    /// The serde skip predicate: `Present` is the default and stays off the wire.
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present)
    }

    /// Whether the body content is removed (suppressed or swept).
    pub fn is_removed(&self) -> bool {
        !self.is_present()
    }
}

/// Controls whether an exact fact-body projection returns text and whether it
/// must still validate external bytes for a display-facing selected read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BodyReadMode {
    pub include_body: bool,
    pub read_for_display: bool,
}

/// Resolved body content: the (state, text) pair the views consume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BodyContent {
    Present(Option<String>),
    SuppressedPresent {
        content_hash: String,
    },
    PhysicallyRemoved {
        content_hash: String,
    },
    Unavailable {
        content_hash: String,
        availability: ContentAvailabilityV1,
    },
}

impl BodyContent {
    pub(crate) fn state(&self) -> BodyContentState {
        match self {
            Self::Present(_) => BodyContentState::Present,
            Self::SuppressedPresent { .. } => BodyContentState::SuppressedPresent,
            Self::PhysicallyRemoved { .. } => BodyContentState::PhysicallyRemoved,
            Self::Unavailable { .. } => BodyContentState::Present,
        }
    }

    #[cfg(test)]
    pub(crate) fn availability(&self) -> ContentAvailabilityV1 {
        match self {
            Self::Present(_) => ContentAvailabilityV1::Available,
            Self::SuppressedPresent { .. } | Self::PhysicallyRemoved { .. } => {
                ContentAvailabilityV1::Removed
            }
            Self::Unavailable { availability, .. } => *availability,
        }
    }

    /// The removal key, borrowed from the removed variants; `None` when present.
    /// Surfaces whose payload carries no body hash (imported notes) render this
    /// as their `removed_body_content_hash` twin of the snapshot result field.
    pub(crate) fn removed_content_hash(&self) -> Option<&str> {
        match self {
            Self::Present(_) | Self::Unavailable { .. } => None,
            Self::SuppressedPresent { content_hash } | Self::PhysicallyRemoved { content_hash } => {
                Some(content_hash)
            }
        }
    }

    /// The rendered text: hydrated bytes when present, `None` for removed states.
    pub(crate) fn into_text(self) -> Option<String> {
        match self {
            Self::Present(text) => text,
            Self::SuppressedPresent { .. }
            | Self::PhysicallyRemoved { .. }
            | Self::Unavailable { .. } => None,
        }
    }
}

/// Reader-relative removal lens over note-body content hashes: the borrowed
/// bundle every body resolution reads (built once per store read, beside the
/// snapshot's own operative decision).
pub(crate) struct BodyRemovalLens<'a> {
    removal: &'a ArtifactRemovalProjection,
    trust_set: &'a TrustSet,
    policy: RemovalPolicy,
    cosig: &'a CosignatureIndex<'a>,
    /// Display-only availability failures discovered while resolving the exact
    /// selected revision.  This stays on the reader lens rather than expanding
    /// the long-standing public fact-view structs.
    unavailable: RefCell<BTreeSet<(String, ContentAvailabilityV1)>>,
}

impl<'a> BodyRemovalLens<'a> {
    pub(crate) fn new(
        removal: &'a ArtifactRemovalProjection,
        trust_set: &'a TrustSet,
        policy: RemovalPolicy,
        cosig: &'a CosignatureIndex<'a>,
    ) -> Self {
        Self {
            removal,
            trust_set,
            policy,
            cosig,
            unavailable: RefCell::new(BTreeSet::new()),
        }
    }

    fn record_unavailable(&self, content_hash: &str, availability: ContentAvailabilityV1) {
        if matches!(
            availability,
            ContentAvailabilityV1::Missing
                | ContentAvailabilityV1::Mismatch
                | ContentAvailabilityV1::NonTextual
        ) {
            self.unavailable
                .borrow_mut()
                .insert((content_hash.to_owned(), availability));
        }
    }

    pub(crate) fn availability_diagnostics(&self) -> Vec<ProjectionDiagnostic> {
        body_content_availability_diagnostics(
            self.unavailable
                .borrow()
                .iter()
                .map(|(hash, availability)| (*availability, Some(hash.as_str()))),
        )
    }
}

/// Resolve a note-shaped body against the reader's removal lens.
///
/// The removed-vs-missing decision lives here, at the layer that holds the
/// event set, so the storage byte readers stay event-unaware: an operative
/// removal renders as an explained removed state (split suppressed-vs-swept by
/// a store presence check, regardless of `include_body` — the state is
/// metadata about the store, not a hydration choice), while absent bytes
/// WITHOUT an operative removal keep the hard `import referenced artifacts`
/// error exactly as before.
///
/// Inline bodies always render and never consult the lens: inline bytes live
/// in the immutable event log, and content-targeted removal deliberately does
/// not cover event-payload bytes (the deferred tier in
/// `docs/adr/adr-0016-content-targeted-artifact-removal-and-compaction.md`),
/// so suppressing their render would overstate erasure.
pub(crate) fn resolve_body_content(
    backend: &StoreBackend,
    lens: &BodyRemovalLens<'_>,
    include_body: bool,
    inline: Option<String>,
    artifact_path: Option<&str>,
) -> Result<BodyContent> {
    resolve_body_content_for_read(
        backend,
        lens,
        include_body,
        inline,
        artifact_path,
        None,
        false,
    )
}

/// Resolve body content for a strict or display read. Display reads degrade
/// only recognized content-availability failures; backend and policy failures
/// remain errors. `expected_content_hash` is the event's declared hash and is
/// checked independently of the locator before any text is returned.
pub(crate) fn resolve_body_content_for_read(
    backend: &StoreBackend,
    lens: &BodyRemovalLens<'_>,
    include_body: bool,
    inline: Option<String>,
    artifact_path: Option<&str>,
    expected_content_hash: Option<&str>,
    read_for_display: bool,
) -> Result<BodyContent> {
    if inline.is_some() {
        return Ok(BodyContent::Present(if include_body {
            inline
        } else {
            None
        }));
    }
    let Some(path) = artifact_path else {
        return Ok(BodyContent::Present(None));
    };
    // A path whose stem is not a well-formed content hash has no derivable
    // removal key (no claim can target it), so the lens is skipped and the
    // legacy load below keeps its exact behavior for such paths. A display
    // read still parses that external artifact when text is omitted.
    let locator_hash = note_body_content_hash_from_path(path);
    if locator_hash.is_err() && expected_content_hash.is_none() {
        let load = || load_body_artifact(backend, path);
        return match (include_body || read_for_display).then(load) {
            Some(Ok(text)) => Ok(BodyContent::Present(include_body.then_some(text).flatten())),
            Some(Err(error)) if read_for_display => {
                if let Some(availability) = body_artifact_error_availability(&error) {
                    lens.record_unavailable(path, availability);
                    Ok(BodyContent::Unavailable {
                        content_hash: path.to_owned(),
                        availability,
                    })
                } else {
                    Err(error)
                }
            }
            Some(Err(error)) => Err(error),
            None => Ok(BodyContent::Present(None)),
        };
    }
    let content_hash = expected_content_hash
        .map(str::to_owned)
        .unwrap_or_else(|| locator_hash.expect("validated above"));
    let status =
        lens.removal
            .operative_status(&content_hash, lens.trust_set, lens.policy, lens.cosig)?;
    if matches!(
        status,
        RemovalOperativeStatus::OperativePossession | RemovalOperativeStatus::OperativeTrusted
    ) {
        let present = ContentArtifacts::from_backend(backend)
            .get_if_exists(path)?
            .is_some();
        return Ok(if present {
            BodyContent::SuppressedPresent { content_hash }
        } else {
            BodyContent::PhysicallyRemoved { content_hash }
        });
    }
    // Exact display reads always validate an external artifact, even when the
    // caller deliberately requested no text.  This prevents an omitted body
    // from hiding a missing or mismatched selected-revision fact, while list and
    // overview paths keep `read_for_display` off and perform no body reads.
    if include_body || read_for_display {
        let expected = content_hash.as_str();
        let load = || {
            let bytes =
                ContentArtifacts::from_backend(backend).read_note_body_bytes(path, expected)?;
            Ok(parse_note_body_artifact(&bytes)?.body)
        };
        return match load() {
            Ok(text) => Ok(BodyContent::Present(include_body.then_some(text))),
            Err(error) if read_for_display => {
                if let Some(availability) = body_artifact_error_availability(&error) {
                    lens.record_unavailable(expected, availability);
                    Ok(BodyContent::Unavailable {
                        content_hash: expected.to_owned(),
                        availability,
                    })
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        };
    }
    Ok(BodyContent::Present(None))
}

fn body_artifact_error_availability(error: &ShoreError) -> Option<ContentAvailabilityV1> {
    match error {
        ShoreError::Json(_) => Some(ContentAvailabilityV1::Mismatch),
        ShoreError::Message(message) if message.starts_with("missing artifact ") => {
            Some(ContentAvailabilityV1::Missing)
        }
        ShoreError::Message(message)
            if message.contains("hash mismatch") || message.contains("locator hash mismatch") =>
        {
            Some(ContentAvailabilityV1::Mismatch)
        }
        ShoreError::Message(message)
            if message.starts_with("Unsupported note body artifact schema/version:") =>
        {
            Some(ContentAvailabilityV1::Mismatch)
        }
        _ => None,
    }
}

/// A removal is recorded for the body content, but its bytes are still stored:
/// the suppression is reversible and a compact would reclaim them.
const BODY_CONTENT_SUPPRESSED_PRESENT: &str = "body_content_suppressed_present";
/// A removal is recorded for the body content and its bytes have been swept
/// from the store.
const BODY_CONTENT_PHYSICALLY_REMOVED: &str = "body_content_physically_removed";

/// Fold body `(state, content_hash)` pairs into the explained removal
/// diagnostics, deduped per `(content_hash, state)` and emitted in
/// deterministic hash-sorted order. Present entries and hash-less entries
/// never surface. Every emitter goes through this mapper so codes and
/// messages have exactly one owner.
pub(crate) fn body_content_diagnostics<'a>(
    entries: impl IntoIterator<Item = (BodyContentState, Option<&'a str>)>,
) -> Vec<ProjectionDiagnostic> {
    let mut removed: BTreeSet<(String, BodyContentState)> = BTreeSet::new();
    for (state, hash) in entries {
        if state.is_removed()
            && let Some(hash) = hash
        {
            removed.insert((hash.to_owned(), state));
        }
    }
    removed
        .into_iter()
        .map(|(hash, state)| match state {
            BodyContentState::SuppressedPresent => ProjectionDiagnostic {
                code: BODY_CONTENT_SUPPRESSED_PRESENT.to_owned(),
                message: format!(
                    "body content {hash} is suppressed by a recorded removal; \
                     the bytes are still stored and a compact would reclaim them"
                ),
            },
            BodyContentState::PhysicallyRemoved => ProjectionDiagnostic {
                code: BODY_CONTENT_PHYSICALLY_REMOVED.to_owned(),
                message: format!(
                    "body content {hash} was removed and its bytes have been swept from the store"
                ),
            },
            BodyContentState::Present => unreachable!("present entries are filtered above"),
        })
        .collect()
}

/// Fold display-only body unavailability into stable diagnostics. Removed
/// content remains owned by `body_content_diagnostics` above.
pub(crate) fn body_content_availability_diagnostics<'a>(
    entries: impl IntoIterator<Item = (ContentAvailabilityV1, Option<&'a str>)>,
) -> Vec<ProjectionDiagnostic> {
    let unavailable = entries
        .into_iter()
        .filter(|(availability, _)| {
            matches!(
                availability,
                ContentAvailabilityV1::Missing
                    | ContentAvailabilityV1::Mismatch
                    | ContentAvailabilityV1::NonTextual
            )
        })
        .filter_map(|(availability, hash)| hash.map(|hash| (hash.to_owned(), availability)))
        .collect::<BTreeSet<_>>();
    unavailable
        .into_iter()
        .map(|(hash, availability)| {
            let suffix = match availability {
                ContentAvailabilityV1::Missing => "missing",
                ContentAvailabilityV1::Mismatch => "mismatch",
                ContentAvailabilityV1::NonTextual => "non_textual",
                ContentAvailabilityV1::Available | ContentAvailabilityV1::Removed => {
                    unreachable!("available and removed entries are filtered above")
                }
            };
            ProjectionDiagnostic {
                code: format!("body_content_{suffix}"),
                message: format!("body content {hash} is unavailable: {suffix}"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::JournalId;
    use crate::session::body_artifact::{
        BodyArtifactOutcome, note_body_content_hash_from_path, stage_body_artifact,
    };
    use crate::session::event::{
        ArtifactRemovedPayload, EventTarget, EventType, IngestProvenance, IngestVia, ShoreEvent,
        Writer,
    };
    use crate::session::projection::cosignature::CosignatureIndex;
    use crate::session::signing::{RemovalPolicy, TrustSet};
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::content::ContentArtifacts;

    fn external_body() -> String {
        "x".repeat(5000)
    }

    /// Stage `body` as an externalized note-body artifact; write the blob only
    /// when `write_blob`. Returns `(relative_path, content_hash)`.
    fn staged_note_body(backend: &StoreBackend, body: &str, write_blob: bool) -> (String, String) {
        match stage_body_artifact(body.as_bytes()).expect("stage body") {
            BodyArtifactOutcome::Artifact {
                relative_path,
                body_envelope,
                ..
            } => {
                if write_blob {
                    ContentArtifacts::from_backend(backend)
                        .put_note_body(
                            &relative_path,
                            &body_envelope.to_json_bytes().expect("envelope bytes"),
                        )
                        .expect("write blob");
                }
                let content_hash =
                    note_body_content_hash_from_path(&relative_path).expect("hash from path");
                (relative_path, content_hash)
            }
            BodyArtifactOutcome::Inline { .. } => panic!("fixture body must externalize"),
        }
    }

    /// A bare unsigned, locally-authored (`ingest = None`) removal for `content_hash`.
    fn removal_event(content_hash: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ArtifactRemoved,
            ArtifactRemovedPayload::idempotency_key(content_hash),
            EventTarget::for_journal(JournalId::new("journal:fixture")),
            Writer::shore_local("test"),
            ArtifactRemovedPayload {
                content_hash: content_hash.to_owned(),
            },
            "2026-06-19T00:00:00Z",
        )
        .unwrap()
    }

    /// Mark an event as ingested through a foreign-event seam (`ingest = Some`),
    /// which drops the local-possession arm.
    fn ingested(mut event: ShoreEvent) -> ShoreEvent {
        event.ingest = Some(IngestProvenance {
            via: IngestVia::IngestEvents,
            received_at: "2026-06-19T01:00:00Z".to_owned(),
        });
        event
    }

    /// Drive `resolve_body_content` over `events` with default trust/policy.
    fn resolve(
        backend: &StoreBackend,
        events: &[ShoreEvent],
        include_body: bool,
        inline: Option<String>,
        artifact_path: Option<&str>,
    ) -> crate::error::Result<BodyContent> {
        let removal = ArtifactRemovalProjection::from_events(events).expect("removal projection");
        let cosig = CosignatureIndex::build(events).expect("cosignature index");
        let trust = TrustSet::default();
        let lens = BodyRemovalLens::new(&removal, &trust, RemovalPolicy::default(), &cosig);
        resolve_body_content(backend, &lens, include_body, inline, artifact_path)
    }

    fn resolve_for_display(
        backend: &StoreBackend,
        events: &[ShoreEvent],
        include_body: bool,
        inline: Option<String>,
        artifact_path: Option<&str>,
    ) -> crate::error::Result<BodyContent> {
        let removal = ArtifactRemovalProjection::from_events(events).expect("removal projection");
        let cosig = CosignatureIndex::build(events).expect("cosignature index");
        let trust = TrustSet::default();
        let lens = BodyRemovalLens::new(&removal, &trust, RemovalPolicy::default(), &cosig);
        let expected = artifact_path.and_then(|path| note_body_content_hash_from_path(path).ok());
        resolve_body_content_for_read(
            backend,
            &lens,
            include_body,
            inline,
            artifact_path,
            expected.as_deref(),
            true,
        )
    }

    #[test]
    fn operative_removal_with_blob_on_disk_is_suppressed_present() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, hash) = staged_note_body(&backend, &body, true);
        let events = vec![removal_event(&hash)];

        let content = resolve(&backend, &events, true, None, Some(&path)).expect("resolves");

        assert_eq!(content.state(), BodyContentState::SuppressedPresent);
        assert_eq!(content.removed_content_hash(), Some(hash.as_str()));
        assert_eq!(content.into_text(), None);
    }

    #[test]
    fn operative_removal_with_blob_swept_is_physically_removed() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, hash) = staged_note_body(&backend, &body, false);
        let events = vec![removal_event(&hash)];

        let content = resolve(&backend, &events, true, None, Some(&path)).expect("resolves");

        assert_eq!(content.state(), BodyContentState::PhysicallyRemoved);
        assert_eq!(content.removed_content_hash(), Some(hash.as_str()));
        assert_eq!(content.into_text(), None);
    }

    #[test]
    fn non_operative_claim_with_blob_present_renders_body() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, hash) = staged_note_body(&backend, &body, true);
        let events = vec![ingested(removal_event(&hash))];

        let content = resolve(&backend, &events, true, None, Some(&path)).expect("resolves");

        assert_eq!(content.state(), BodyContentState::Present);
        assert_eq!(content.removed_content_hash(), None);
        assert_eq!(content.into_text(), Some(body));
    }

    #[test]
    fn non_operative_claim_over_absent_blob_keeps_missing_artifact_error() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, hash) = staged_note_body(&backend, &body, false);
        let events = vec![ingested(removal_event(&hash))];

        let err = resolve(&backend, &events, true, None, Some(&path)).unwrap_err();

        assert!(err.to_string().contains("import referenced artifacts"));
    }

    #[test]
    fn absent_blob_without_claim_keeps_missing_artifact_error() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, _hash) = staged_note_body(&backend, &body, false);

        let err = resolve(&backend, &[], true, None, Some(&path)).unwrap_err();

        assert!(err.to_string().contains("import referenced artifacts"));
    }

    #[test]
    fn display_read_maps_missing_body_to_typed_unavailability() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, _hash) = staged_note_body(&backend, &body, false);

        let content = resolve_for_display(&backend, &[], true, None, Some(&path))
            .expect("display read degrades a missing body");

        assert_eq!(content.state(), BodyContentState::Present);
        assert_eq!(content.availability(), ContentAvailabilityV1::Missing);
        assert_eq!(content.into_text(), None);
    }

    #[test]
    fn display_read_validates_and_maps_mismatched_body() {
        let backend = StoreBackend::memory();
        let expected = external_body();
        let (path, _hash) = staged_note_body(&backend, &expected, false);
        let wrong = format!("{}wrong", external_body());
        let envelope = crate::session::body_artifact::NoteBodyEnvelope::new(wrong);
        ContentArtifacts::from_backend(&backend)
            .put_note_body(&path, &envelope.to_json_bytes().expect("envelope bytes"))
            .expect("write corrupt blob");

        let content = resolve_for_display(&backend, &[], true, None, Some(&path))
            .expect("display read degrades a mismatched body");

        assert_eq!(content.state(), BodyContentState::Present);
        assert_eq!(content.availability(), ContentAvailabilityV1::Mismatch);
        assert_eq!(content.into_text(), None);
    }

    #[test]
    fn strict_read_rejects_mismatched_body() {
        let backend = StoreBackend::memory();
        let expected = external_body();
        let (path, _hash) = staged_note_body(&backend, &expected, false);
        let envelope = crate::session::body_artifact::NoteBodyEnvelope::new("wrong".repeat(2000));
        ContentArtifacts::from_backend(&backend)
            .put_note_body(&path, &envelope.to_json_bytes().expect("envelope bytes"))
            .expect("write corrupt blob");

        let error = resolve(&backend, &[], true, None, Some(&path))
            .expect_err("strict read must reject mismatched bytes");

        assert!(error.to_string().contains("content hash mismatch"));
    }

    #[test]
    fn inline_body_renders_even_when_its_hash_carries_an_operative_removal() {
        let backend = StoreBackend::memory();
        let inline = "a small inline body".to_owned();
        let hash = format!(
            "sha256:{}",
            crate::canonical_hash::sha256_bytes_hex(inline.as_bytes())
        );
        let events = vec![removal_event(&hash)];

        let content =
            resolve(&backend, &events, true, Some(inline.clone()), None).expect("resolves");

        assert_eq!(content.state(), BodyContentState::Present);
        assert_eq!(content.into_text(), Some(inline));
    }

    #[test]
    fn include_body_false_still_reports_removed_state_but_never_loads_bytes() {
        let backend = StoreBackend::memory();
        let body = external_body();
        let (path, hash) = staged_note_body(&backend, &body, false);

        let removed = resolve(&backend, &[removal_event(&hash)], false, None, Some(&path))
            .expect("removed state resolves without a read");
        assert_eq!(removed.state(), BodyContentState::PhysicallyRemoved);

        let untouched = resolve(&backend, &[], false, None, Some(&path))
            .expect("no claim and no read must not error");
        assert_eq!(untouched.state(), BodyContentState::Present);
        assert_eq!(untouched.into_text(), None);
    }

    #[test]
    fn non_content_addressed_path_skips_the_lens_and_loads_legacy() {
        let backend = StoreBackend::memory();
        ContentArtifacts::from_backend(&backend)
            .put_note_body(
                "artifacts/notes/abc.json",
                br#"{"schema":"shore.note-body","version":1,"body":"legacy body"}"#,
            )
            .expect("write legacy blob");

        let content = resolve(&backend, &[], true, None, Some("artifacts/notes/abc.json"))
            .expect("legacy path stays readable");

        assert_eq!(content.state(), BodyContentState::Present);
        assert_eq!(content.into_text(), Some("legacy body".to_owned()));
    }

    #[test]
    fn body_content_diagnostics_dedupes_and_orders_per_hash_and_state() {
        let entries = [
            (BodyContentState::PhysicallyRemoved, Some("sha256:aaa")),
            (BodyContentState::PhysicallyRemoved, Some("sha256:aaa")), // duplicate collapses
            (BodyContentState::SuppressedPresent, Some("sha256:bbb")),
            (BodyContentState::Present, Some("sha256:ccc")), // present never surfaces
            (BodyContentState::PhysicallyRemoved, None),     // no hash, skipped
        ];

        let diagnostics = body_content_diagnostics(entries);

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].code, "body_content_physically_removed");
        assert_eq!(
            diagnostics[0].message,
            "body content sha256:aaa was removed and its bytes have been swept from the store"
        );
        assert_eq!(diagnostics[1].code, "body_content_suppressed_present");
        assert_eq!(
            diagnostics[1].message,
            "body content sha256:bbb is suppressed by a recorded removal; \
             the bytes are still stored and a compact would reclaim them"
        );
    }

    #[test]
    fn body_content_state_serializes_snake_case_and_present_is_skipped() {
        #[derive(serde::Serialize)]
        struct Probe {
            #[serde(skip_serializing_if = "BodyContentState::is_present")]
            state: BodyContentState,
        }

        let suppressed = serde_json::to_string(&Probe {
            state: BodyContentState::SuppressedPresent,
        })
        .expect("serialize suppressed");
        assert_eq!(suppressed, r#"{"state":"suppressed_present"}"#);

        let removed = serde_json::to_string(&Probe {
            state: BodyContentState::PhysicallyRemoved,
        })
        .expect("serialize removed");
        assert_eq!(removed, r#"{"state":"physically_removed"}"#);

        let present = serde_json::to_string(&Probe {
            state: BodyContentState::default(),
        })
        .expect("serialize present");
        assert_eq!(present, "{}");
    }

    #[test]
    fn body_content_state_removed_predicate_matches_both_removed_states() {
        assert!(!BodyContentState::Present.is_removed());
        assert!(BodyContentState::SuppressedPresent.is_removed());
        assert!(BodyContentState::PhysicallyRemoved.is_removed());
    }
}
