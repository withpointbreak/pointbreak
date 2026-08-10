//! Product-domain contract for Change-aware derived reads.
#![cfg_attr(not(test), allow(dead_code))]
#![deny(private_bounds, private_interfaces)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::documents::{
    ChangeAttentionPresentationDocumentV2, ChangeListPresentationDocumentV1,
    ChangeQueryUnavailableDocumentV1, ReaderProfileDocumentV1, ReaderUpgradeRequiredDocumentV1,
};
use crate::error::{Result, ShoreError};
use crate::model::{ChangeId, InputRequestId, RevisionRefV1};
use crate::session::{ChangeLifecycleV1, ChangeTopologyV1};

const AUTHORITY_ERROR_SCHEMA: &str = "pointbreak.inspect-change-authority-error";
const PROJECTION_ERROR_SCHEMA: &str = "pointbreak.inspect-change-projection-error";
const ERROR_DOCUMENT_VERSION: u32 = 1;
const DEFAULT_PAGE_LIMIT: usize = 50;
const MAXIMUM_PAGE_LIMIT: usize = 100;
const MAXIMUM_SUMMARY_QUERY_BYTES: usize = 256;

/// Reserved shared runtime shell. The existing lifecycle, current-generation
/// slot, and hydration machinery move behind this one owner when that runtime
/// is extracted; this contract does not create another engine.
pub(crate) struct DerivedAccessRuntime {
    _reserved: std::convert::Infallible,
}

/// Thin product facade consumed by the Inspector binary.
#[doc(hidden)]
#[derive(Clone)]
pub struct DerivedChangeAccess {
    runtime: Arc<DerivedAccessRuntime>,
}

impl DerivedChangeAccess {
    pub fn resolve_for_inspector(_repo: impl AsRef<Path>) -> Result<Self> {
        Err(Self::runtime_not_connected())
    }

    pub fn profile(&self) -> Result<DerivedChangeOutcomeV1<ReaderProfileDocumentV1>> {
        let _runtime = &self.runtime;
        Err(Self::runtime_not_connected())
    }

    pub fn changes(
        &self,
        _request: &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangePageV1>> {
        let _runtime = &self.runtime;
        Err(Self::runtime_not_connected())
    }

    pub fn attention(
        &self,
        _request: &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedAttentionPageV1>> {
        let _runtime = &self.runtime;
        Err(Self::runtime_not_connected())
    }

    fn runtime_not_connected() -> ShoreError {
        ShoreError::Message("derived Change access runtime is not connected".to_owned())
    }
}

/// Independent authority, compatibility, and projection outcomes.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedChangeOutcomeV1<T> {
    Ready(T),
    AuthorityUnavailable(ChangeQueryUnavailableDocumentV1),
    AuthorityConflicted(DerivedAuthorityFailureDocumentV1),
    AuthorityInvalid(DerivedAuthorityFailureDocumentV1),
    ReaderUpgradeRequired(ReaderUpgradeRequiredDocumentV1),
    ProjectionUnavailable(DerivedProjectionUnavailableDocumentV1),
    Retryable(DerivedProjectionUnavailableDocumentV1),
}

impl<T> DerivedChangeOutcomeV1<T> {
    pub(crate) fn authority_conflicted(message: impl Into<String>) -> Self {
        Self::AuthorityConflicted(DerivedAuthorityFailureDocumentV1::new(
            DerivedAuthorityFailureCodeV1::AuthorityConflicted,
            message,
        ))
    }

    pub(crate) fn authority_invalid(message: impl Into<String>) -> Self {
        Self::AuthorityInvalid(DerivedAuthorityFailureDocumentV1::new(
            DerivedAuthorityFailureCodeV1::AuthorityInvalid,
            message,
        ))
    }

    pub(crate) fn projection_unavailable(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self::ProjectionUnavailable(DerivedProjectionUnavailableDocumentV1::new(
            code, message, false,
        ))
    }

    pub(crate) fn retryable(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self::Retryable(DerivedProjectionUnavailableDocumentV1::new(
            code, message, true,
        ))
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedAuthorityFailureCodeV1 {
    AuthorityConflicted,
    AuthorityInvalid,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedAuthorityFailureDocumentV1 {
    schema: String,
    version: u32,
    code: DerivedAuthorityFailureCodeV1,
    message: String,
}

impl DerivedAuthorityFailureDocumentV1 {
    pub fn code(&self) -> DerivedAuthorityFailureCodeV1 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    fn new(code: DerivedAuthorityFailureCodeV1, message: impl Into<String>) -> Self {
        Self {
            schema: AUTHORITY_ERROR_SCHEMA.to_owned(),
            version: ERROR_DOCUMENT_VERSION,
            code,
            message: message.into(),
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedProjectionFailureCodeV1 {
    ProjectionAbsent,
    ProjectionRebuildRequired,
    ProjectionStale,
    ProjectionInvalid,
    ProjectionUnstable,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedProjectionUnavailableDocumentV1 {
    schema: String,
    version: u32,
    code: DerivedProjectionFailureCodeV1,
    message: String,
    retryable: bool,
}

impl DerivedProjectionUnavailableDocumentV1 {
    pub fn code(&self) -> DerivedProjectionFailureCodeV1 {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    fn new(
        code: DerivedProjectionFailureCodeV1,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            schema: PROJECTION_ERROR_SCHEMA.to_owned(),
            version: ERROR_DOCUMENT_VERSION,
            code,
            message: message.into(),
            retryable,
        }
    }
}

/// Authenticated and normalized page request. Token bytes and signatures stay
/// in the Inspector adapter.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedChangePageRequestV1 {
    Bare,
    Bounded(DerivedChangePageSelectionV1),
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageSelectionV1 {
    limit: usize,
    after: Option<DerivedChangePageContinuationV1>,
    summary_query: Option<String>,
    topology: Option<ChangeTopologyV1>,
    lifecycle: Option<ChangeLifecycleV1>,
    attention: Option<DerivedChangeAttentionFilterV1>,
    availability: Option<DerivedChangeAvailabilityFilterV1>,
}

impl DerivedChangePageSelectionV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        limit: usize,
        after: Option<DerivedChangePageContinuationV1>,
        summary_query: Option<String>,
        topology: Option<ChangeTopologyV1>,
        lifecycle: Option<ChangeLifecycleV1>,
        attention: Option<DerivedChangeAttentionFilterV1>,
        availability: Option<DerivedChangeAvailabilityFilterV1>,
    ) -> Result<Self> {
        if !(1..=MAXIMUM_PAGE_LIMIT).contains(&limit) {
            return Err(ShoreError::Message(
                "derived Change page limit must be between 1 and 100".to_owned(),
            ));
        }
        let summary_query = summary_query
            .map(|query| {
                let query = query.trim();
                if query.is_empty() {
                    return Err(ShoreError::Message(
                        "derived Change summary query is empty".to_owned(),
                    ));
                }
                if query.len() > MAXIMUM_SUMMARY_QUERY_BYTES {
                    return Err(ShoreError::Message(
                        "derived Change summary query exceeds 256 bytes".to_owned(),
                    ));
                }
                Ok(query.to_lowercase())
            })
            .transpose()?;
        Ok(Self {
            limit,
            after,
            summary_query,
            topology,
            lifecycle,
            attention,
            availability,
        })
    }

    pub fn default_page() -> Self {
        Self {
            limit: DEFAULT_PAGE_LIMIT,
            after: None,
            summary_query: None,
            topology: None,
            lifecycle: None,
            attention: None,
            availability: None,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn after(&self) -> Option<&DerivedChangePageContinuationV1> {
        self.after.as_ref()
    }

    pub fn summary_query(&self) -> Option<&str> {
        self.summary_query.as_deref()
    }

    pub fn topology(&self) -> Option<ChangeTopologyV1> {
        self.topology
    }

    pub fn lifecycle(&self) -> Option<ChangeLifecycleV1> {
        self.lifecycle
    }

    pub fn attention_filter(&self) -> Option<DerivedChangeAttentionFilterV1> {
        self.attention
    }

    pub fn availability_filter(&self) -> Option<DerivedChangeAvailabilityFilterV1> {
        self.availability
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageContinuationV1 {
    expected_projection_stamp: String,
    boundary: DerivedChangePageBoundaryV1,
}

impl DerivedChangePageContinuationV1 {
    pub fn new(
        expected_projection_stamp: impl Into<String>,
        boundary: DerivedChangePageBoundaryV1,
    ) -> Result<Self> {
        let expected_projection_stamp = expected_projection_stamp.into();
        if expected_projection_stamp.is_empty() {
            return Err(ShoreError::Message(
                "derived Change continuation has no projection stamp".to_owned(),
            ));
        }
        Ok(Self {
            expected_projection_stamp,
            boundary,
        })
    }

    pub fn expected_projection_stamp(&self) -> &str {
        &self.expected_projection_stamp
    }

    pub fn boundary(&self) -> &DerivedChangePageBoundaryV1 {
        &self.boundary
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageBoundaryV1 {
    last_change_id: Option<ChangeId>,
}

impl DerivedChangePageBoundaryV1 {
    pub fn page_one() -> Self {
        Self {
            last_change_id: None,
        }
    }

    pub fn after(last_change_id: ChangeId) -> Self {
        Self {
            last_change_id: Some(last_change_id),
        }
    }

    pub fn last_change_id(&self) -> Option<&ChangeId> {
        self.last_change_id.as_ref()
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedChangeAttentionFilterV1 {
    Clear,
    InProgress,
    Incomplete,
    Conflicted,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedChangeAvailabilityFilterV1 {
    Available,
    Incomplete,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageWindowV1 {
    pub projection_stamp: String,
    pub previous: Option<DerivedChangePageBoundaryV1>,
    pub next: Option<DerivedChangePageBoundaryV1>,
    pub last: Option<DerivedChangePageBoundaryV1>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedChangePageV1 {
    pub document: ChangeListPresentationDocumentV1,
    pub window: Option<DerivedChangePageWindowV1>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedAttentionPageV1 {
    pub document: ChangeAttentionPresentationDocumentV2,
    pub attention_presentations: BTreeMap<ChangeId, DerivedAttentionPresentationV1>,
    pub window: Option<DerivedChangePageWindowV1>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedAttentionPresentationV1 {
    pub primary_reason: DerivedAttentionReasonV1,
    pub reasons: Vec<DerivedAttentionReasonV1>,
    pub reason_presentations: Vec<DerivedAttentionReasonPresentationV1>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DerivedAttentionReasonV1 {
    Conflicted,
    Incomplete,
    NoCurrentRevision,
    UnresolvedOperativeRequests { request_ids: Vec<InputRequestId> },
    CurrentRevisionsNeedAssessment { revisions: Vec<RevisionRefV1> },
}

#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedAttentionReasonPresentationV1 {
    pub cause: DerivedAttentionReasonV1,
    pub ask: String,
    pub reason: String,
    pub evidence: String,
    pub next_action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{AUTHORITY_CURSOR_SCHEMA_V2, AuthorityCursorV2};

    fn empty_authority_cursor() -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
            journal_record_count: 0,
            event_count: 0,
            journal_record_set_hash: format!("sha256:{}", "0".repeat(64)),
            event_set_hash: format!("sha256:{}", "0".repeat(64)),
            capability_set_hash: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn inspector_http_status<T>(outcome: &DerivedChangeOutcomeV1<T>) -> u16 {
        match outcome {
            DerivedChangeOutcomeV1::Ready(_) => 200,
            DerivedChangeOutcomeV1::AuthorityUnavailable(_)
            | DerivedChangeOutcomeV1::AuthorityConflicted(_)
            | DerivedChangeOutcomeV1::AuthorityInvalid(_) => 409,
            DerivedChangeOutcomeV1::ReaderUpgradeRequired(_) => 426,
            DerivedChangeOutcomeV1::ProjectionUnavailable(_)
            | DerivedChangeOutcomeV1::Retryable(_) => 503,
        }
    }

    #[test]
    fn derived_change_outcomes_keep_failure_axes_distinct() {
        let ready = DerivedChangeOutcomeV1::Ready(());
        let unavailable = DerivedChangeOutcomeV1::<()>::AuthorityUnavailable(
            ChangeQueryUnavailableDocumentV1::MigrationRequired {
                schema: "pointbreak.store-migration-required".to_owned(),
                version: 1,
                authority_cursor: empty_authority_cursor(),
            },
        );
        let conflicted = DerivedChangeOutcomeV1::<()>::authority_conflicted("ambiguous authority");
        let invalid = DerivedChangeOutcomeV1::<()>::authority_invalid("invalid authority");
        let projection = DerivedChangeOutcomeV1::<()>::projection_unavailable(
            DerivedProjectionFailureCodeV1::ProjectionInvalid,
            "invalid projection",
        );
        let retryable = DerivedChangeOutcomeV1::<()>::retryable(
            DerivedProjectionFailureCodeV1::ProjectionUnstable,
            "projection moved",
        );
        let upgrade = DerivedChangeOutcomeV1::<()>::ReaderUpgradeRequired(
            ReaderUpgradeRequiredDocumentV1::new(
                "unsupported_reader_profile",
                Some("review_change_revision_v1".to_owned()),
            ),
        );

        assert_eq!(inspector_http_status(&ready), 200);
        assert_eq!(inspector_http_status(&unavailable), 409);
        assert_eq!(inspector_http_status(&conflicted), 409);
        assert_eq!(inspector_http_status(&invalid), 409);
        assert_eq!(inspector_http_status(&upgrade), 426);
        assert_eq!(inspector_http_status(&projection), 503);
        assert_eq!(inspector_http_status(&retryable), 503);

        let DerivedChangeOutcomeV1::AuthorityConflicted(document) = conflicted else {
            panic!("authority conflict changed axes");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap(),
            serde_json::json!({
                "schema": AUTHORITY_ERROR_SCHEMA,
                "version": 1,
                "code": "authority_conflicted",
                "message": "ambiguous authority",
            })
        );
        let DerivedChangeOutcomeV1::AuthorityInvalid(document) = invalid else {
            panic!("invalid authority changed axes");
        };
        assert_eq!(
            document.code(),
            DerivedAuthorityFailureCodeV1::AuthorityInvalid
        );
        let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = projection else {
            panic!("projection failure changed axes");
        };
        assert_eq!(
            serde_json::to_value(document).unwrap(),
            serde_json::json!({
                "schema": PROJECTION_ERROR_SCHEMA,
                "version": 1,
                "code": "projection_invalid",
                "message": "invalid projection",
                "retryable": false,
            })
        );
        let DerivedChangeOutcomeV1::Retryable(document) = retryable else {
            panic!("retryable projection state changed axes");
        };
        assert_eq!(
            document.code(),
            DerivedProjectionFailureCodeV1::ProjectionUnstable
        );
        assert!(document.is_retryable());
        assert_eq!(serde_json::to_value(document).unwrap()["retryable"], true);
    }

    #[test]
    fn derived_failure_codes_have_exact_wire_names() {
        for (code, expected) in [
            (
                DerivedAuthorityFailureCodeV1::AuthorityConflicted,
                "authority_conflicted",
            ),
            (
                DerivedAuthorityFailureCodeV1::AuthorityInvalid,
                "authority_invalid",
            ),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
        }
        for (code, expected) in [
            (
                DerivedProjectionFailureCodeV1::ProjectionAbsent,
                "projection_absent",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionRebuildRequired,
                "projection_rebuild_required",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionStale,
                "projection_stale",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionInvalid,
                "projection_invalid",
            ),
            (
                DerivedProjectionFailureCodeV1::ProjectionUnstable,
                "projection_unstable",
            ),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
        }
    }

    #[test]
    fn derived_change_selection_is_normalized_and_token_free() {
        let selection = DerivedChangePageSelectionV1::new(
            25,
            Some(
                DerivedChangePageContinuationV1::new(
                    "sha256:current",
                    DerivedChangePageBoundaryV1::page_one(),
                )
                .unwrap(),
            ),
            Some("  Mixed CASE  ".to_owned()),
            Some(ChangeTopologyV1::ParallelCurrent),
            Some(ChangeLifecycleV1::InProgress),
            Some(DerivedChangeAttentionFilterV1::InProgress),
            Some(DerivedChangeAvailabilityFilterV1::Available),
        )
        .unwrap();

        assert_eq!(selection.limit(), 25);
        assert_eq!(selection.summary_query(), Some("mixed case"));
        assert_eq!(
            selection.topology(),
            Some(ChangeTopologyV1::ParallelCurrent)
        );
        assert_eq!(
            selection
                .after()
                .expect("continuation")
                .boundary()
                .last_change_id(),
            None
        );
        assert!(DerivedChangePageSelectionV1::new(0, None, None, None, None, None, None).is_err());
        assert!(
            DerivedChangePageSelectionV1::new(101, None, None, None, None, None, None).is_err()
        );
        assert!(
            DerivedChangePageSelectionV1::new(
                50,
                None,
                Some("  ".to_owned()),
                None,
                None,
                None,
                None,
            )
            .is_err()
        );
        let unicode_boundary = "İ".repeat(128);
        assert_eq!(unicode_boundary.len(), MAXIMUM_SUMMARY_QUERY_BYTES);
        let normalized = DerivedChangePageSelectionV1::new(
            50,
            None,
            Some(unicode_boundary),
            None,
            None,
            None,
            None,
        )
        .expect("length is checked before Unicode lowercase expansion");
        assert!(normalized.summary_query().unwrap().len() > MAXIMUM_SUMMARY_QUERY_BYTES);
    }

    #[test]
    fn reserved_change_facade_cannot_synthesize_a_product_outcome() {
        let error = DerivedChangeAccess::resolve_for_inspector("unused-repository")
            .err()
            .expect("runtime extraction owns facade construction");
        assert!(error.to_string().contains("runtime is not connected"));
    }

    #[test]
    fn derived_failure_documents_do_not_change_the_shared_reader_registry() {
        let registry = crate::documents::change_revision_document_registry();
        assert!(
            !registry
                .iter()
                .any(|(schema, _)| *schema == AUTHORITY_ERROR_SCHEMA)
        );
        assert!(
            !registry
                .iter()
                .any(|(schema, _)| *schema == PROJECTION_ERROR_SCHEMA)
        );
    }
}
