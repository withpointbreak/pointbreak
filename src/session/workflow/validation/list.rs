use std::path::{Path, PathBuf};

use super::super::observation::{
    CurrentRevisionContext, RevisionScope, RevisionSelection, resolve_revision, validated_track_id,
};
use super::view::{
    ValidationCheckProjectionOptions, ValidationCheckView, project_validation_checks,
};
use crate::error::Result;
use crate::model::{RevisionId, TrackId, ValidationStatus};
use crate::session::derived_access::fact_reads::ExactRevisionFactReadRouteV1;
use crate::session::derived_access::history::DerivedHistoryAccess;
use crate::session::event::ShoreEvent;
use crate::session::projection::body_content::{BodyRemovalLens, body_content_diagnostics};
use crate::session::projection::cosignature::CosignatureIndex;
use crate::session::signing::{RemovalPolicy, TrustSet};
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store::resolution::ReadStore;
use crate::session::{ArtifactRemovalProjection, PublicReadCommandContextV1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationListOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    exact_revision_id: Option<RevisionId>,
    track: Option<String>,
    status: Option<ValidationStatus>,
    include_body: bool,
    trust_set: TrustSet,
    removal_policy: RemovalPolicy,
}

impl ValidationListOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            exact_revision_id: None,
            track: None,
            status: None,
            include_body: false,
            trust_set: TrustSet::default(),
            removal_policy: RemovalPolicy::default(),
        }
    }

    pub fn with_revision_id(mut self, id: RevisionId) -> Self {
        self.revision_id = Some(id);
        self
    }
    pub fn with_exact_revision_id(mut self, id: RevisionId) -> Self {
        self.exact_revision_id = Some(id);
        self
    }
    pub fn with_track(mut self, track: impl Into<String>) -> Self {
        self.track = Some(track.into());
        self
    }

    pub fn with_status(mut self, status: ValidationStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_include_body(mut self, include_body: bool) -> Self {
        self.include_body = include_body;
        self
    }

    /// Supply the reader's trust set for removal-state resolution
    /// (reader-relativity; the empty default reads every signer as untrusted).
    pub fn with_trust_set(mut self, trust_set: TrustSet) -> Self {
        self.trust_set = trust_set;
        self
    }

    /// Supply the render-time removal policy. A non-operative removal claim
    /// renders the bytes; an operative one renders the explained removed
    /// state. Render-only: it never gates the compact erasure sweep.
    pub fn with_removal_policy(mut self, removal_policy: RemovalPolicy) -> Self {
        self.removal_policy = removal_policy;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationListFilters {
    pub track_id: Option<TrackId>,
    pub status: Option<ValidationStatus>,
    pub include_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationListResult {
    pub revision_id: RevisionId,
    pub filters: ValidationListFilters,
    pub validation_checks: Vec<ValidationCheckView>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

pub fn list_validation_checks(options: ValidationListOptions) -> Result<ValidationListResult> {
    let (read_store, events) = super::super::capable_read_store_and_events(&options.repo)?;
    list_validation_checks_from_events(options, &read_store, &events)
}

#[doc(hidden)]
pub fn list_validation_checks_with_public_read_context(
    options: ValidationListOptions,
    context: PublicReadCommandContextV1,
) -> Result<ValidationListResult> {
    if options.revision_id.is_some()
        || options.exact_revision_id.is_none()
        || options.track.is_none()
        || options.status.is_some()
        || options.include_body
    {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "public read context requires the exact qualified validation shape".to_owned(),
        });
    }
    context.require_repository(&options.repo)?;
    let revision_id = options
        .exact_revision_id
        .as_ref()
        .expect("qualified validation shape has an exact Revision")
        .clone();
    let access = DerivedHistoryAccess::from_public_read_store(context.read_store().clone())
        .map_err(crate::error::ShoreError::Message)?;
    match access
        .exact_revision_fact_read_v1(&revision_id)
        .map_err(crate::error::ShoreError::Message)?
    {
        ExactRevisionFactReadRouteV1::Ready(derived) => {
            super::super::complete_current_derived_fact_projection_v1(context, |store| {
                list_validation_checks_from_event_selection(
                    options,
                    store,
                    &derived.events,
                    ValidationDiagnosticsSource::Materialized(derived.diagnostics),
                )
            })
        }
        ExactRevisionFactReadRouteV1::Off => {
            let reader =
                super::super::change_read::public_read_change_reader_v1(context, &options.repo)?;
            let result =
                list_validation_checks_from_events(options, reader.read_store(), reader.events())?;
            reader.postflight()?;
            Ok(result)
        }
        ExactRevisionFactReadRouteV1::Unavailable => {
            let repo = options.repo.clone();
            super::super::complete_unavailable_fact_fallback_v1(context, &repo, |store, events| {
                list_validation_checks_from_event_selection(
                    options,
                    store,
                    events,
                    ValidationDiagnosticsSource::AuthoritativeEvents,
                )
            })
        }
    }
}

fn list_validation_checks_from_events(
    options: ValidationListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
) -> Result<ValidationListResult> {
    let result = list_validation_checks_from_event_selection(
        options,
        read_store,
        events,
        ValidationDiagnosticsSource::AuthoritativeEvents,
    )?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    super::super::record_authoritative_replay_state();
    Ok(result)
}

enum ValidationDiagnosticsSource {
    AuthoritativeEvents,
    Materialized(Vec<ProjectionDiagnostic>),
}

fn list_validation_checks_from_event_selection(
    options: ValidationListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
    diagnostics_source: ValidationDiagnosticsSource,
) -> Result<ValidationListResult> {
    let selection = RevisionSelection::from_revision_options(
        options.revision_id.as_ref(),
        options.exact_revision_id.as_ref(),
    )?;
    let context = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::GitContextResolution,
        );
        CurrentRevisionContext::for_repo(&options.repo)?
    };
    let resolved = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteRevisionSelection,
        );
        resolve_revision(events, selection, &context, RevisionScope::default())?
    };
    let track_filter = options
        .track
        .as_deref()
        .map(validated_track_id)
        .transpose()?;
    let removal = ArtifactRemovalProjection::from_events(events)?;
    let cosig_index = CosignatureIndex::build(events)?;
    let removal_lens = BodyRemovalLens::new(
        &removal,
        &options.trust_set,
        options.removal_policy,
        &cosig_index,
    );
    let validation_checks = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
        );
        project_validation_checks(ValidationCheckProjectionOptions {
            backend: read_store.backend(),
            events,
            revision_id: &resolved.revision_id,
            track_filter: track_filter.clone(),
            status_filter: options.status,
            include_body: options.include_body,
            read_for_display: false,
            removal_lens: &removal_lens,
        })?
    };
    let mut diagnostics = match diagnostics_source {
        ValidationDiagnosticsSource::AuthoritativeEvents => {
            SessionState::from_events(events)?.diagnostics
        }
        ValidationDiagnosticsSource::Materialized(diagnostics) => diagnostics,
    };
    diagnostics.extend(body_content_diagnostics(
        validation_checks
            .iter()
            .map(|v| (v.summary_content_state, v.summary_content_hash.as_deref())),
    ));

    Ok(ValidationListResult {
        revision_id: resolved.revision_id,
        filters: ValidationListFilters {
            track_id: track_filter,
            status: options.status,
            include_body: options.include_body,
        },
        validation_checks,
        diagnostics,
    })
}
