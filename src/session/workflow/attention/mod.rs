//! Derived attention state over the review record (ADR-0019: attention and
//! notification without an executive controller). Read-side only: no write
//! workflow may consult this module, and its output never authorizes or blocks a
//! write. The projection surfaces *what is outstanding* — open asks, ambiguous
//! decisions, competing heads, stale decisions, failed checks, outstanding
//! follow-ups — as peer items; it never tie-breaks and never carries a
//! per-revision lifecycle stage. "Attention guides, never gates" (ADR-0019 D4).

mod items;

use std::path::{Path, PathBuf};

pub use items::{
    AttentionAssessmentRecord, AttentionDetail, AttentionFreshness, AttentionFreshnessState,
    AttentionItem, AttentionProjection, AttentionTier,
};
pub(crate) use items::{attention_from_events, scope_attention_items};

use crate::error::{Result, ShoreError};
use crate::model::RevisionId;
use crate::session::event::ShoreEvent;
use crate::session::projection::freshness::event_set_hash_for_events;
use crate::session::projection::skipped_to_diagnostics;
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::resolution::resolve_read_store;
use crate::session::{
    DerivedAttentionRoute, DerivedHistoryAccess, EventStore, PublicReadCommandContextV1,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionListOptions {
    repo: PathBuf,
    revision: Option<RevisionId>,
}

impl AttentionListOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision: None,
        }
    }

    pub fn with_revision(mut self, revision: RevisionId) -> Self {
        self.revision = Some(revision);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionListResult {
    pub event_set_hash: String,
    pub event_count: usize,
    pub revision: Option<RevisionId>,
    pub items: Vec<AttentionItem>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[doc(hidden)]
#[allow(
    clippy::large_enum_variant,
    reason = "the reviewed hidden bridge carries its single-use continuation by value"
)]
pub enum PublicReadAttentionRouteV1 {
    Authoritative {
        result: AttentionListResult,
        labeled_fallback: bool,
    },
    Derived {
        result: AttentionListResult,
        projection_stamp: String,
    },
    LabeledFallbackPending {
        fallback_hint: Option<String>,
        continuation: PublicReadAttentionFallbackV1,
    },
}

/// The one invocation's still-live authority for an Attention fallback.
///
/// ```compile_fail
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<pointbreak::session::PublicReadAttentionFallbackV1>();
/// ```
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<pointbreak::session::PublicReadAttentionFallbackV1>();
/// ```
#[doc(hidden)]
pub struct PublicReadAttentionFallbackV1 {
    options: AttentionListOptions,
    context: PublicReadCommandContextV1,
    access: DerivedHistoryAccess,
}

/// Resolve the repo's store, replay the event log leniently (an undecodable event
/// surfaces as a diagnostic rather than aborting the read, as `revision list`
/// does), and derive the attention projection over it. Pull-only: the envelope
/// carries the event-set hash and count so callers poll like every other surface.
pub fn list_attention(options: AttentionListOptions) -> Result<AttentionListResult> {
    let read_store = resolve_read_store(&options.repo)?;
    let store = EventStore::from_backend(read_store.backend());
    let (events, skipped) = store.list_events_lenient()?;
    list_attention_from_events(options, &events, skipped_to_diagnostics(skipped))
}

#[doc(hidden)]
pub fn list_attention_with_public_read_context(
    options: AttentionListOptions,
    context: PublicReadCommandContextV1,
) -> Result<PublicReadAttentionRouteV1> {
    if options.revision.is_none() {
        return Err(ShoreError::WorkflowInputInvalid {
            reason: "public read context requires the exact qualified attention shape".to_owned(),
        });
    }
    context.require_repository(&options.repo)?;
    let access = DerivedHistoryAccess::from_public_read_store(context.read_store().clone())
        .map_err(ShoreError::Message)?;
    match access
        .attention(options.revision.as_ref())
        .map_err(ShoreError::Message)?
    {
        DerivedAttentionRoute::Ready(derived) => {
            let result = AttentionListResult {
                event_set_hash: String::new(),
                event_count: derived.event_count,
                revision: options.revision,
                items: derived.items,
                diagnostics: derived.diagnostics,
            };
            context.postflight()?;
            Ok(PublicReadAttentionRouteV1::Derived {
                result,
                projection_stamp: derived.projection_stamp,
            })
        }
        DerivedAttentionRoute::Off if !access.is_active() => {
            complete_authoritative_attention(options, context, false)
        }
        DerivedAttentionRoute::Off | DerivedAttentionRoute::Unavailable(_) => {
            let fallback_hint = access
                .claim_authoritative_fallback_hint()
                .map(str::to_owned);
            Ok(PublicReadAttentionRouteV1::LabeledFallbackPending {
                fallback_hint,
                continuation: PublicReadAttentionFallbackV1 {
                    options,
                    context,
                    access,
                },
            })
        }
    }
}

#[doc(hidden)]
pub fn complete_public_read_attention_fallback_v1(
    continuation: PublicReadAttentionFallbackV1,
) -> Result<PublicReadAttentionRouteV1> {
    let PublicReadAttentionFallbackV1 {
        options,
        context,
        access,
    } = continuation;
    let result = complete_authoritative_attention(options, context, true);
    drop(access);
    result
}

fn complete_authoritative_attention(
    options: AttentionListOptions,
    context: PublicReadCommandContextV1,
    labeled_fallback: bool,
) -> Result<PublicReadAttentionRouteV1> {
    let reader = super::change_read::public_read_change_reader_v1(context, &options.repo)?;
    let result = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _projection_phase =
            crate::bench_support::longitudinal::enter_derived_access_phase_v1(
                crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
            );
        list_attention_from_events(options, reader.events(), Vec::new())?
    };
    reader.postflight()?;
    Ok(PublicReadAttentionRouteV1::Authoritative {
        result,
        labeled_fallback,
    })
}

fn list_attention_from_events(
    options: AttentionListOptions,
    events: &[ShoreEvent],
    skipped_diagnostics: Vec<ProjectionDiagnostic>,
) -> Result<AttentionListResult> {
    let event_set_hash = event_set_hash_for_events(events)?;
    let mut projection = attention_from_events(events, options.revision.as_ref())?;
    projection.diagnostics.extend(skipped_diagnostics);

    Ok(AttentionListResult {
        event_set_hash,
        event_count: events.len(),
        revision: options.revision,
        items: projection.items,
        diagnostics: projection.diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::store::resolution::resolve_change_read_backend;

    #[test]
    fn list_attention_over_empty_store_returns_well_formed_envelope() {
        let repo = tempfile::tempdir().expect("tempdir");
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .expect("git init");

        let result = list_attention(AttentionListOptions::new(repo.path())).expect("list");

        assert_eq!(result.event_count, 0);
        assert!(result.items.is_empty());
        assert!(result.diagnostics.is_empty());
        assert!(!result.event_set_hash.is_empty());
    }

    #[test]
    fn qualified_attention_rejects_shape_and_repository_misuse() {
        let first = capability_repo();
        let second = capability_repo();

        let shape_error = list_attention_with_public_read_context(
            AttentionListOptions::new(first.path()),
            crate::session::prepare_public_read_command_context_v1(first.path()).unwrap(),
        )
        .err()
        .expect("missing Revision must refuse")
        .to_string();
        assert!(shape_error.contains("exact qualified attention shape"));

        let repository_error = list_attention_with_public_read_context(
            AttentionListOptions::new(second.path())
                .with_revision(RevisionId::new(format!("rev:sha256:{}", "1".repeat(64)))),
            crate::session::prepare_public_read_command_context_v1(first.path()).unwrap(),
        )
        .err()
        .expect("different repository must refuse")
        .to_string();
        assert!(repository_error.contains("different repository"));
    }

    #[test]
    fn unavailable_attention_claims_one_hint_and_returns_single_use_continuations() {
        use crate::session::derived_access::product_contract::DerivedAccessProfile;

        let repo = capability_repo();
        let options = || {
            AttentionListOptions::new(repo.path())
                .with_revision(RevisionId::new(format!("rev:sha256:{}", "2".repeat(64))))
        };
        let context = || {
            crate::session::prepare_public_read_command_context_v1(repo.path())
                .unwrap()
                .with_derived_access_profile_for_test(DerivedAccessProfile::SqliteWalBodylessV1)
        };

        let first = list_attention_with_public_read_context(options(), context()).unwrap();
        let PublicReadAttentionRouteV1::LabeledFallbackPending {
            fallback_hint,
            continuation,
        } = first
        else {
            panic!("active unavailable Attention must return a pending fallback");
        };
        assert!(fallback_hint.is_some());
        drop(continuation);

        let second = list_attention_with_public_read_context(options(), context()).unwrap();
        let PublicReadAttentionRouteV1::LabeledFallbackPending {
            fallback_hint,
            continuation,
        } = second
        else {
            panic!("active unavailable Attention must remain pending");
        };
        assert!(fallback_hint.is_none());
        drop(continuation);
    }

    #[test]
    fn pending_attention_fallback_refuses_authority_movement_at_postflight() {
        use crate::session::derived_access::product_contract::DerivedAccessProfile;

        let repo = capability_repo();
        let context = crate::session::prepare_public_read_command_context_v1(repo.path())
            .unwrap()
            .with_derived_access_profile_for_test(DerivedAccessProfile::SqliteWalBodylessV1);
        let route = list_attention_with_public_read_context(
            AttentionListOptions::new(repo.path())
                .with_revision(RevisionId::new(format!("rev:sha256:{}", "3".repeat(64)))),
            context,
        )
        .unwrap();
        let PublicReadAttentionRouteV1::LabeledFallbackPending { continuation, .. } = route else {
            panic!("active unavailable Attention must return a pending fallback");
        };
        let store = resolve_change_read_backend(repo.path()).unwrap();
        let journal = store.backend().journal();
        let activation = "store_capability_activation:review_change_revision_v1:root";
        let bytes = journal.read_event_bytes(activation).unwrap().unwrap();
        journal.insert_raw(activation, &bytes).unwrap();

        let error = complete_public_read_attention_fallback_v1(continuation)
            .err()
            .expect("moved authority must refuse")
            .to_string();

        assert!(error.contains("changed"), "{error}");
    }

    fn capability_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "git init failed: {output:?}");
        let store = resolve_change_read_backend(repo.path()).unwrap();
        write_capability_fixture_for_test(
            store.backend().journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        repo
    }
}
