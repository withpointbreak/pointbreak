//! Versioned documents shared by the CLI and bundled inspect server.
//!
//! This module owns the serializable documents the `pointbreak review-*` commands
//! emit, the version handshake, and the small promoted inspect set. It includes
//! the shared envelopes ([`DiagnosticDocument`], [`EventWriteDocument`]), the
//! per-item view-document mappers, and the builders that wrap a
//! `pointbreak::session` result in its documented JSON shape.
//!
//! Consumers can produce the same JSON in-process by calling a builder and
//! serializing the returned document with `serde_json`. CLI and inspect
//! producers are thin callers over these builders, so each promoted contract
//! has one source of truth.
//!
//! Stdout serialization (`write_json`) stays in the CLI; this module exposes the
//! serializable documents, not terminal IO.

use std::collections::BTreeMap;

use crate::session::ProjectionDiagnostic;

mod assessment;
mod association;
mod association_comparison;
mod attention;
mod capture;
mod change;
mod event_history;
mod history;
mod identity;
mod input_request;
mod inspect;
mod observation;
mod reader_profile;
mod revision;
mod revision_interdiff;
mod revision_resource;
mod validation;
mod version;
mod view;

pub use assessment::{
    AssessmentAddBody, AssessmentShowBody, assessment_add_document, assessment_show_document,
};
pub use association::{
    AssociateCommitBody, AssociateRefBody, ListAssociationsBody, WithdrawCommitBody,
    WithdrawRefBody, associate_commit_document, associate_ref_document, list_associations_document,
    withdraw_commit_document, withdraw_ref_document,
};
pub use association_comparison::{
    ASSOCIATION_COMPARISON_SCHEMA, AssociationComparisonDocumentV1, AssociationComparisonRefV1,
    AssociationComparisonStateV1, AssociationProofAvailabilityV1,
};
pub use attention::{
    ATTENTION_LIST_SCHEMA, AttentionListBody, attention_list_document,
    derived_attention_list_document,
};
pub use capture::{CaptureBody, capture_document};
#[doc(hidden)]
pub use change::normalize_fact_presentations;
pub use change::{
    ATTENTION_LIST_SCHEMA_V2, ChangeAttentionDocumentV2, ChangeAttentionPresentationDocumentV2,
    ChangeClaimWithdrawalV1, ChangeDeclarationStateV1, ChangeDetailDocumentV1, ChangeDetailV1,
    ChangeDocumentFacadeV1, ChangeListDocumentV1, ChangeListPresentationDocumentV1,
    ChangeMemberRevisionV1, ChangePresentationV1, ChangeRevisionCurrencyV1, ChangeRevisionDetailV1,
    ChangeRevisionDocumentV1, ChangeRevisionPresentationDocumentV1, ChangeSummaryV1,
    CurrentRevisionPresentationV1, FactContentPresentationV1, FactContentV1, FactFamilyStateV1,
    FactInputResponseContentV1, FactPortApplicabilityV1, FactPortPresentationV1,
    FactPresentationV1, INSPECT_ATTENTION_SCHEMA_V2, INSPECT_CHANGES_PAGE_SCHEMA,
    REVIEW_CHANGE_LIST_SCHEMA, REVIEW_CHANGE_REVISION_SCHEMA, REVIEW_CHANGE_SCHEMA,
    RevisionQualificationV1, RevisionSummarySourceV1, UnavailableChangeMemberRevisionV1,
};
#[doc(hidden)]
pub use change::{
    ChangeAttentionPresentationV1, ChangeAttentionReasonPresentationV1, ChangeAttentionReasonV1,
    attention_presentation_for_change,
};
pub(crate) use change::{FactPortCarrierSourceV1, change_presentation_projection};
pub use event_history::{
    EventHistoryCompletionV1, EventHistoryDocumentV1, EventHistoryEntryV1, EventHistoryFacadeV1,
    EventHistoryOrderV1, EventHistorySubjectV1, EventHistorySummaryV1,
    INSPECT_EVENT_HISTORY_SCHEMA,
};
pub use history::{HistoryBody, derived_history_document, history_document};
pub use identity::{
    IDENTITY_WHOAMI_SCHEMA, IdentityWhoamiBody, IdentityWhoamiDocument, identity_whoami_document,
};
pub use input_request::{
    InputRequestFetchBody, InputRequestListBody, InputRequestOpenBody, InputRequestRespondBody,
    input_request_fetch_document, input_request_list_document, input_request_open_document,
    input_request_respond_document,
};
pub use inspect::{
    INSPECT_FRESHNESS_SCHEMA, INSPECT_STARTUP_SCHEMA, InspectFreshnessDocument,
    InspectStartupDocument, REVIEW_SNAPSHOT_SCHEMA, ReviewSnapshotDocument,
    promoted_inspect_document_registry, review_snapshot_document,
};
pub use observation::{
    ObservationAddBody, ObservationListBody, observation_add_document, observation_list_document,
};
pub use reader_profile::{
    ChangeQueryUnavailableDocumentV1, INSPECT_READER_PROFILE_SCHEMA,
    READER_UPGRADE_REQUIRED_SCHEMA, ReaderProfileAvailabilityV1, ReaderProfileDocumentV1,
    ReaderUpgradeRequiredDocumentV1, STORE_MIGRATION_IN_PROGRESS_SCHEMA,
    STORE_MIGRATION_REQUIRED_SCHEMA,
};
pub use revision::{
    RevisionListBody, RevisionShowBody, RevisionShowBodyV3, derived_revision_list_page_document,
    derived_revision_show_document, revision_list_document, revision_list_page_document,
    revision_show_document, revision_show_document_v3,
};
pub use revision_interdiff::{
    REVISION_INTERDIFF_SCHEMA, RevisionInterdiffAvailabilityV1, RevisionInterdiffDocumentV1,
    RevisionInterdiffRefV1,
};
pub use revision_resource::{
    ContentAvailabilityV1, REVISION_RESOURCE_SCHEMA, RevisionResourceAvailabilityV1,
    RevisionResourceDocumentV1, RevisionResourceProjectionV1, RevisionResourceRefV1,
};
pub use validation::{
    ValidationAddBody, ValidationListBody, validation_add_document, validation_list_document,
};
pub use version::{
    BuildIdentityV1, BuildSourceV1, VERSION_DISPLAY, VERSION_SCHEMA, VersionBody, version_document,
};
pub use view::{
    AssessmentViewDocument, CurrentAssessmentDocument, InputRequestAssertionModeDocument,
    InputRequestResponseViewDocument, InputRequestViewDocument, ObservationViewDocument,
    ValidationCheckViewDocument,
};

/// Every CLI-emitted document schema and its current version.
const CLI_DOCUMENT_REGISTRY: &[(&str, u32)] = &[
    ("pointbreak.attention-list", 1),
    ("pointbreak.identity-attest", 1),
    ("pointbreak.identity-delegate", 1),
    (identity::IDENTITY_WHOAMI_SCHEMA, 1),
    ("pointbreak.key-discover", 1),
    ("pointbreak.key-enroll", 1),
    ("pointbreak.key-init", 1),
    ("pointbreak.key-list", 1),
    ("pointbreak.key-show", 1),
    ("pointbreak.key-use-ssh", 1),
    ("pointbreak.review-assessment-add", 1),
    ("pointbreak.review-assessment-show", 1),
    ("pointbreak.review-association-commit", 1),
    ("pointbreak.review-association-commit-withdrawn", 1),
    ("pointbreak.review-association-list", 1),
    ("pointbreak.review-association-ref", 1),
    ("pointbreak.review-association-ref-withdrawn", 1),
    ("pointbreak.change-capture-receipt.v1", 1),
    ("pointbreak.review-capture", 1),
    ("pointbreak.review-endorse", 1),
    ("pointbreak.review-history", 1),
    ("pointbreak.review-input-request-list", 1),
    ("pointbreak.review-input-request-open", 1),
    ("pointbreak.review-input-request-respond", 1),
    ("pointbreak.review-input-request-show", 1),
    ("pointbreak.review-observation-add", 1),
    ("pointbreak.review-observation-list", 1),
    ("pointbreak.review-revision", 2),
    ("pointbreak.review-revision-list", 1),
    ("pointbreak.review-validation-add", 1),
    ("pointbreak.review-validation-list", 1),
    ("pointbreak.store-compact", 1),
    ("pointbreak.store-derived-build", 1),
    ("pointbreak.store-derived-rebuild", 1),
    ("pointbreak.store-derived-status", 1),
    ("pointbreak.store-forget", 1),
    ("pointbreak.store-link", 1),
    ("pointbreak.store-link-preview", 1),
    ("pointbreak.store-list", 1),
    ("pointbreak.store-migrate", 1),
    ("pointbreak.store-mode", 1),
    ("pointbreak.store-paths", 1),
    ("pointbreak.store-remove", 1),
    ("pointbreak.store-status", 1),
    ("pointbreak.store-unlink", 1),
    (version::VERSION_SCHEMA, 1),
];

/// Headless documents reserved by the Change-capable reader cohort. These are
/// public library contracts but are not advertised as active CLI/Inspector
/// routes until the later client cutover wires those routes atomically. The
/// bundled TypeScript readers consume the generated
/// `change_reader_profile_v1.json`; refresh it with
/// `just reader-profile-generate` whenever this registry changes.
const CHANGE_REVISION_DOCUMENT_REGISTRY: &[(&str, u32)] = &[
    (INSPECT_READER_PROFILE_SCHEMA, 1),
    (REVIEW_CHANGE_LIST_SCHEMA, 1),
    (INSPECT_CHANGES_PAGE_SCHEMA, 1),
    (REVIEW_CHANGE_SCHEMA, 1),
    (REVIEW_CHANGE_REVISION_SCHEMA, 1),
    ("pointbreak.review-revision", 3),
    (REVISION_RESOURCE_SCHEMA, 1),
    (ASSOCIATION_COMPARISON_SCHEMA, 1),
    (REVISION_INTERDIFF_SCHEMA, 1),
    (ATTENTION_LIST_SCHEMA_V2, 2),
    (INSPECT_ATTENTION_SCHEMA_V2, 2),
    (READER_UPGRADE_REQUIRED_SCHEMA, 1),
    (STORE_MIGRATION_REQUIRED_SCHEMA, 1),
    (STORE_MIGRATION_IN_PROGRESS_SCHEMA, 1),
];

pub fn change_revision_document_registry() -> &'static [(&'static str, u32)] {
    CHANGE_REVISION_DOCUMENT_REGISTRY
}

pub(crate) fn cli_document_registry() -> &'static [(&'static str, u32)] {
    CLI_DOCUMENT_REGISTRY
}

/// Compatibility registry for CLI documents and the exact promoted inspect
/// documents shipped with the bundled server.
pub fn document_registry() -> &'static [(&'static str, u32)] {
    static REGISTRY: std::sync::OnceLock<Vec<(&'static str, u32)>> = std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            cli_document_registry()
                .iter()
                .chain(promoted_inspect_document_registry())
                .copied()
                .collect()
        })
        .as_slice()
}

/// Envelope for a read/diagnostic document: `{ schema, version, <flattened
/// body>, diagnostics }`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticDocument<T> {
    schema: &'static str,
    version: u32,
    #[serde(flatten)]
    body: T,
    diagnostics: Vec<ProjectionDiagnostic>,
}

/// Envelope for an event-write document: the diagnostic envelope plus the
/// `eventsCreated`/`eventsExisting`/`eventsCreatedByType` write counts.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventWriteDocument<T> {
    schema: &'static str,
    version: u32,
    #[serde(flatten)]
    body: T,
    events_created: usize,
    events_existing: usize,
    events_created_by_type: BTreeMap<String, usize>,
    diagnostics: Vec<ProjectionDiagnostic>,
}

impl<T> DiagnosticDocument<T> {
    /// Wrap `body` in the diagnostic envelope under `schema` at version 1.
    pub fn new(schema: &'static str, body: T, diagnostics: Vec<ProjectionDiagnostic>) -> Self {
        Self::with_version(schema, 1, body, diagnostics)
    }

    /// Wrap `body` under `schema` at an explicit document `version` — for read
    /// documents that have shed or reshaped soft-shell fields (ADR-0029
    /// Decision 7 rides field removals on a version bump).
    pub fn with_version(
        schema: &'static str,
        version: u32,
        body: T,
        diagnostics: Vec<ProjectionDiagnostic>,
    ) -> Self {
        Self {
            schema,
            version,
            body,
            diagnostics,
        }
    }

    /// The typed body used to render a human companion without rebuilding the
    /// document envelope.
    pub fn body(&self) -> &T {
        &self.body
    }
}

impl<T> EventWriteDocument<T> {
    /// Wrap `body` in the event-write envelope under `schema` at version 1.
    pub fn new(
        schema: &'static str,
        body: T,
        events_created: usize,
        events_existing: usize,
        events_created_by_type: BTreeMap<String, usize>,
        diagnostics: Vec<ProjectionDiagnostic>,
    ) -> Self {
        Self {
            schema,
            version: 1,
            body,
            events_created,
            events_existing,
            events_created_by_type,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn write_compact<T: serde::Serialize>(document: &T) -> String {
        let mut buf = Vec::new();
        serde_json::to_writer(&mut buf, document).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn change_revision_cohort_documents_are_registered_at_frozen_versions() {
        assert_eq!(
            super::change_revision_document_registry(),
            &[
                ("pointbreak.inspect-reader-profile", 1),
                ("pointbreak.review-change-list", 1),
                ("pointbreak.inspect-changes-page", 1),
                ("pointbreak.review-change", 1),
                ("pointbreak.review-change-revision", 1),
                ("pointbreak.review-revision", 3),
                ("pointbreak.review-revision-resource", 1),
                ("pointbreak.review-association-comparison", 1),
                ("pointbreak.review-revision-interdiff", 1),
                ("pointbreak.attention-list", 2),
                ("pointbreak.inspect-attention", 2),
                ("pointbreak.reader-upgrade-required", 1),
                ("pointbreak.store-migration-required", 1),
                ("pointbreak.store-migration-in-progress", 1),
            ]
        );
    }

    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ChangeReaderProfileArtifact<'a> {
        minimum_reader_profile: &'a str,
        documents: BTreeMap<&'a str, u32>,
    }

    fn generated_change_reader_profile() -> Vec<u8> {
        let artifact = ChangeReaderProfileArtifact {
            minimum_reader_profile: crate::session::BULK_ADOPTION_MINIMUM_READER_PROFILE_V1,
            documents: super::change_revision_document_registry()
                .iter()
                .copied()
                .collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&artifact).unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn generated_change_reader_profile_is_current() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/documents/change_reader_profile_v1.json");
        let expected = generated_change_reader_profile();
        if std::env::var("POINTBREAK_UPDATE_CHANGE_READER_PROFILE").as_deref() == Ok("1") {
            fs::write(&path, &expected).unwrap();
        }
        assert_eq!(fs::read(path).unwrap(), expected);
    }

    #[test]
    fn event_write_document_preserves_field_order() {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            revision_id: &'static str,
            event_id: &'static str,
        }

        let doc = super::EventWriteDocument::new(
            "shore.test-write",
            Body {
                revision_id: "unit:1",
                event_id: "evt:1",
            },
            1,
            2,
            BTreeMap::new(),
            Vec::new(),
        );

        assert_eq!(
            write_compact(&doc),
            "{\"schema\":\"shore.test-write\",\"version\":1,\"revisionId\":\"unit:1\",\"eventId\":\"evt:1\",\"eventsCreated\":1,\"eventsExisting\":2,\"eventsCreatedByType\":{},\"diagnostics\":[]}"
        );
    }

    #[test]
    fn diagnostic_document_preserves_trailing_diagnostics() {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            revision_id: &'static str,
            count: usize,
        }

        let doc = super::DiagnosticDocument::new(
            "shore.test-read",
            Body {
                revision_id: "unit:1",
                count: 3,
            },
            Vec::new(),
        );

        assert_eq!(
            write_compact(&doc),
            "{\"schema\":\"shore.test-read\",\"version\":1,\"revisionId\":\"unit:1\",\"count\":3,\"diagnostics\":[]}"
        );
    }

    #[test]
    fn validation_add_document_serializes_advisory_validation_add_schema() {
        use crate::documents::validation_add_document;
        use crate::model::{
            EventId, RevisionId, TrackId, ValidationCheckId, ValidationStatus, ValidationTarget,
        };
        use crate::session::ValidationAddResult;

        let revision_id = RevisionId::new("review-unit:sha256:one");
        let doc = validation_add_document(ValidationAddResult {
            revision_id: revision_id.clone(),
            validation_check_id: ValidationCheckId::new("validation:sha256:one"),
            event_id: EventId::new("evt:sha256:one"),
            track_id: TrackId::new("agent:codex"),
            target: ValidationTarget::Revision { revision_id },
            status: ValidationStatus::Passed,
            summary_content_hash: Some("sha256:summary".to_owned()),
            events_created: 1,
            events_existing: 0,
            events_created_by_type: BTreeMap::from([("validation_check_recorded".to_owned(), 1)]),
            diagnostics: Vec::new(),
        });

        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(value["schema"], "pointbreak.review-validation-add");
        assert_eq!(value["status"], "passed");
        assert_eq!(value["summaryContentHash"], "sha256:summary");
        assert!(value.get("accepted").is_none());
        assert!(value.get("gate").is_none());
    }

    #[test]
    fn view_document_principal_is_options_gated_and_agent_scoped() {
        use crate::documents::ValidationCheckViewDocument;
        use crate::model::ActorId;
        use crate::session::delegation_map_from_value;

        let map = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-05-01T00:00:00Z",
                    "validUntil": null
                }]
            }
        }))
        .unwrap();

        let agent_view = || {
            let mut view = validation_view();
            view.writer.actor_id = ActorId::new("actor:agent:claude-code");
            view
        };

        // Agent writer + map → resolved principal object beside writer.
        let resolved =
            ValidationCheckViewDocument::from(agent_view()).with_resolved_principal(Some(&map));
        let value = serde_json::to_value(&resolved).unwrap();
        assert_eq!(
            value["principal"]["actorId"],
            "actor:git-email:kevin@swiber.dev"
        );
        assert_eq!(value["principal"]["status"], "resolved");
        assert_eq!(value["principal"]["source"], "delegates");

        // Agent writer + no map → mirror posture.
        let no_map = ValidationCheckViewDocument::from(agent_view()).with_resolved_principal(None);
        assert_eq!(
            serde_json::to_value(&no_map).unwrap()["principal"],
            serde_json::json!({ "status": "none", "source": "none" })
        );

        // Human writer + map → no principal object (its own principal).
        let human = ValidationCheckViewDocument::from(validation_view())
            .with_resolved_principal(Some(&map));
        assert!(
            serde_json::to_value(&human)
                .unwrap()
                .get("principal")
                .is_none(),
            "human writers carry no principal object"
        );

        // Plain `From` (the unit-document / add path) carries no principal.
        let plain = ValidationCheckViewDocument::from(validation_view());
        assert!(
            serde_json::to_value(&plain)
                .unwrap()
                .get("principal")
                .is_none(),
            "the From path attaches no principal — unit docs stay principal-free"
        );
    }

    #[test]
    fn validation_view_document_has_expected_wire_keys() {
        use crate::documents::ValidationCheckViewDocument;

        let doc = ValidationCheckViewDocument::from(validation_view());
        let value = serde_json::to_value(&doc).unwrap();

        for key in [
            "id",
            "eventId",
            "trackId",
            "target",
            "checkName",
            "status",
            "trigger",
            "logArtifactContentHashes",
            "createdAt",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert!(value.get("accepted").is_none());
    }

    #[test]
    fn validation_document_serializes_superseded_by_revisions_camel_case_when_present() {
        use crate::documents::ValidationCheckViewDocument;
        use crate::model::RevisionId;

        let mut view = validation_view();
        view.superseded_by_revisions = [RevisionId::new("rev:sha256:successor")]
            .into_iter()
            .collect();

        let value = serde_json::to_value(ValidationCheckViewDocument::from(view)).unwrap();
        assert_eq!(
            value["supersededByRevisions"],
            serde_json::json!(["rev:sha256:successor"]),
        );
    }

    #[test]
    fn validation_document_omits_superseded_by_revisions_when_empty() {
        use crate::documents::ValidationCheckViewDocument;

        // validation_view() defaults to an empty set (a head-targeting check).
        let value =
            serde_json::to_value(ValidationCheckViewDocument::from(validation_view())).unwrap();
        assert!(
            value.get("supersededByRevisions").is_none(),
            "a current check must be byte-identical — the field is skip-when-empty",
        );
    }

    #[test]
    fn derived_revision_show_document_substitutes_only_the_identity_block() {
        use crate::documents::{
            derived_revision_show_document, revision_show_document, revision_show_document_v3,
        };
        use crate::model::RevisionRefV1;
        use crate::session::{
            CaptureOptions, RevisionShowOptions, capture_worktree_review, show_revision,
        };

        let repo = modified_repo();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let result = show_revision(
            RevisionShowOptions::new(repo.path())
                .with_revision_id(capture.revision_id.clone())
                .with_read_for_display(true),
        )
        .unwrap();

        let authoritative = serde_json::to_value(revision_show_document(result.clone())).unwrap();
        assert!(
            authoritative["eventSetHash"].is_string(),
            "the authoritative lane always serializes eventSetHash"
        );
        assert!(authoritative.get("projectionStamp").is_none());

        let derived = serde_json::to_value(derived_revision_show_document(
            result.clone(),
            "sha256:fixture-projection-stamp".to_owned(),
        ))
        .unwrap();
        assert_eq!(
            derived["projectionStamp"],
            "sha256:fixture-projection-stamp"
        );
        assert!(
            derived.get("eventSetHash").is_none(),
            "the derived lane never serializes the event-set hash"
        );
        let mut authoritative_rest = authoritative.clone();
        authoritative_rest
            .as_object_mut()
            .unwrap()
            .remove("eventSetHash");
        let mut derived_rest = derived.clone();
        derived_rest
            .as_object_mut()
            .unwrap()
            .remove("projectionStamp");
        assert_eq!(
            authoritative_rest, derived_rest,
            "the identity block is the only difference between the builders"
        );

        // The context-free v3 embedding always takes the authoritative
        // identity through the shared parts constructor.
        let revision_ref = RevisionRefV1::new(
            result.revision.revision_id.clone(),
            result.revision.object_artifact_content_hash.clone(),
        )
        .unwrap();
        let v3 = serde_json::to_value(
            revision_show_document_v3(result, revision_ref, Vec::new()).unwrap(),
        )
        .unwrap();
        assert!(v3["exactRevision"]["eventSetHash"].is_string());
        assert!(v3["exactRevision"].get("projectionStamp").is_none());
    }

    #[test]
    fn revision_show_document_includes_validation_checks_and_count() {
        use crate::documents::revision_show_document;
        use crate::model::ValidationStatus;
        use crate::session::{
            CaptureOptions, RevisionShowOptions, ValidationAddOptions, capture_worktree_review,
            record_validation_check, show_revision,
        };

        let repo = modified_repo();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        record_validation_check(
            ValidationAddOptions::new(repo.path())
                .with_revision_id(capture.revision_id.clone())
                .with_track("agent:codex")
                .with_check_name("cargo test")
                .with_status(ValidationStatus::Passed),
        )
        .unwrap();

        let result = show_revision(
            RevisionShowOptions::new(repo.path())
                .with_revision_id(capture.revision_id)
                .with_include_body(true),
        )
        .unwrap();
        let value = serde_json::to_value(revision_show_document(result)).unwrap();

        assert!(value["validationChecks"].is_array());
        assert_eq!(value["summary"]["validationCheckCount"], 1);
        let row = value["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["kind"] == "validation_evidence")
            .expect("validation row");
        assert_eq!(
            row["relatedValidationCheckIds"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn revision_show_v3_requires_the_exact_revision_and_artifact_hash() {
        use crate::documents::revision_show_document_v3;
        use crate::model::{RevisionId, RevisionRefV1};
        use crate::session::{
            CaptureOptions, RevisionShowOptions, capture_worktree_review, show_revision,
        };

        let repo = modified_repo();
        let capture = capture_worktree_review(CaptureOptions::new(repo.path())).unwrap();
        let result = show_revision(
            RevisionShowOptions::new(repo.path()).with_revision_id(capture.revision_id.clone()),
        )
        .unwrap();
        let artifact_hash = result.revision.object_artifact_content_hash.clone();
        let wrong_revision = RevisionRefV1::new(
            RevisionId::new("rev:sha256:substituted"),
            artifact_hash.clone(),
        )
        .unwrap();
        assert!(revision_show_document_v3(result, wrong_revision, Vec::new()).is_err());

        let result = show_revision(
            RevisionShowOptions::new(repo.path()).with_revision_id(capture.revision_id.clone()),
        )
        .unwrap();
        let wrong_hash = RevisionRefV1::new(
            capture.revision_id.clone(),
            format!("sha256:{}", "f".repeat(64)),
        )
        .unwrap();
        assert!(revision_show_document_v3(result, wrong_hash, Vec::new()).is_err());

        let result = show_revision(
            RevisionShowOptions::new(repo.path()).with_revision_id(capture.revision_id.clone()),
        )
        .unwrap();
        let exact = RevisionRefV1::new(capture.revision_id, artifact_hash).unwrap();
        assert!(revision_show_document_v3(result, exact, Vec::new()).is_ok());
    }

    fn validation_view() -> crate::session::ValidationCheckView {
        use crate::model::{
            EventId, RevisionId, TrackId, ValidationCheckId, ValidationStatus, ValidationTarget,
            ValidationTrigger,
        };
        use crate::session::event::Writer;

        let revision_id = RevisionId::new("review-unit:sha256:one");
        crate::session::ValidationCheckView {
            id: ValidationCheckId::new("validation:sha256:one"),
            event_id: EventId::new("evt:sha256:one"),
            track_id: TrackId::new("agent:codex"),
            target: ValidationTarget::Revision { revision_id },
            check_name: "cargo test".to_owned(),
            command: Some("cargo test --all".to_owned()),
            status: ValidationStatus::Passed,
            exit_code: Some(0),
            trigger: ValidationTrigger::Manual,
            source_fingerprint: Some("rev:sha256:head".to_owned()),
            summary: Some("tests passed".to_owned()),
            summary_content_type: Default::default(),
            summary_content_hash: Some("sha256:summary".to_owned()),
            summary_content_state: Default::default(),
            started_at: Some("2026-05-10T00:00:00Z".to_owned()),
            completed_at: Some("2026-05-10T00:01:00Z".to_owned()),
            log_artifact_content_hashes: vec!["sha256:log".to_owned()],
            created_at: "2026-05-10T00:01:01Z".to_owned(),
            writer: Writer::shore_local(env!("CARGO_PKG_VERSION")),
            superseded_by_revisions: std::collections::BTreeSet::new(),
        }
    }

    fn modified_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        repo.commit_all("base");
        repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
        repo
    }

    struct TestRepo {
        root: tempfile::TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp repo");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.email", "agent@example.com"]);
            repo.git(["config", "user.name", "Agent"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            repo
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.root.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent directories");
            }
            fs::write(path, contents).expect("write test fixture");
        }

        fn commit_all(&self, message: &str) {
            self.git(["add", "--all"]);
            self.git(["commit", "-m", message]);
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let args = args
                .into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect::<Vec<_>>();
            let output = Command::new("git")
                .args(&args)
                .current_dir(self.root.path())
                .output()
                .unwrap_or_else(|error| panic!("run git {:?}: {error}", args));

            assert!(
                output.status.success(),
                "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
