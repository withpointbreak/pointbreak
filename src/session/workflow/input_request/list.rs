use std::path::{Path, PathBuf};

use super::view::{
    InputRequestProjectionOptions, InputRequestStatusFilter, InputRequestView,
    project_input_requests,
};
use crate::error::Result;
use crate::model::{RevisionId, TrackId};
use crate::session::derived_access::fact_reads::ExactRevisionFactReadRouteV1;
use crate::session::derived_access::history::DerivedHistoryAccess;
use crate::session::event::{AssertionMode, ShoreEvent};
use crate::session::observation::{
    CurrentRevisionContext, RevisionScope, RevisionSelection, resolve_revision, validated_track_id,
};
use crate::session::projection::body_content::{BodyRemovalLens, body_content_diagnostics};
use crate::session::projection::cosignature::CosignatureIndex;
use crate::session::signing::{RemovalPolicy, TrustSet};
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store::resolution::ReadStore;
use crate::session::{ArtifactRemovalProjection, PublicReadCommandContextV1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputRequestListOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    exact_revision_id: Option<RevisionId>,
    track: Option<String>,
    mode: Option<AssertionMode>,
    file: Option<String>,
    status: InputRequestStatusFilter,
    include_body: bool,
    trust_set: TrustSet,
    removal_policy: RemovalPolicy,
}

impl InputRequestListOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            exact_revision_id: None,
            track: None,
            mode: None,
            file: None,
            status: InputRequestStatusFilter::Open,
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

    pub fn with_track(mut self, track: impl Into<String>) -> Self {
        self.track = Some(track.into());
        self
    }

    pub fn with_mode(mut self, mode: AssertionMode) -> Self {
        self.mode = Some(mode);
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_status(mut self, status: InputRequestStatusFilter) -> Self {
        self.status = status;
        self
    }

    pub fn with_include_body(mut self, include_body: bool) -> Self {
        self.include_body = include_body;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputRequestListFilters {
    pub track_id: Option<TrackId>,
    pub mode: Option<AssertionMode>,
    pub file: Option<String>,
    pub status: InputRequestStatusFilter,
    pub include_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputRequestListResult {
    pub revision_id: RevisionId,
    pub filters: InputRequestListFilters,
    pub input_requests: Vec<InputRequestView>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

pub fn list_input_requests(options: InputRequestListOptions) -> Result<InputRequestListResult> {
    let (read_store, events) = super::super::capable_read_store_and_events(&options.repo)?;
    list_input_requests_from_events(options, &read_store, &events)
}

#[doc(hidden)]
pub fn list_input_requests_with_public_read_context(
    options: InputRequestListOptions,
    context: PublicReadCommandContextV1,
) -> Result<InputRequestListResult> {
    if options.revision_id.is_some()
        || options.exact_revision_id.is_none()
        || options.track.is_some()
        || options.mode.is_some()
        || options.file.is_some()
        || options.status != InputRequestStatusFilter::Open
        || options.include_body
    {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "public read context requires the exact qualified input-request shape"
                .to_owned(),
        });
    }
    context.require_repository(&options.repo)?;
    let revision_id = options
        .exact_revision_id
        .as_ref()
        .expect("qualified input-request shape has an exact Revision")
        .clone();
    let access = DerivedHistoryAccess::from_public_read_store(context.read_store().clone())
        .map_err(crate::error::ShoreError::Message)?;
    match access
        .exact_revision_fact_read_v1(&revision_id)
        .map_err(crate::error::ShoreError::Message)?
    {
        ExactRevisionFactReadRouteV1::Ready(derived) => {
            let result = list_input_requests_from_event_selection(
                options,
                context.read_store(),
                &derived.events,
                InputRequestDiagnosticsSource::Materialized(derived.diagnostics),
            )?;
            context.postflight()?;
            #[cfg(any(test, feature = "longitudinal-counting"))]
            super::super::record_derived_current_state();
            Ok(result)
        }
        ExactRevisionFactReadRouteV1::Off | ExactRevisionFactReadRouteV1::Unavailable => {
            let reader =
                super::super::change_read::public_read_change_reader_v1(context, &options.repo)?;
            let result =
                list_input_requests_from_events(options, reader.read_store(), reader.events())?;
            reader.postflight()?;
            Ok(result)
        }
    }
}

fn list_input_requests_from_events(
    options: InputRequestListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
) -> Result<InputRequestListResult> {
    let result = list_input_requests_from_event_selection(
        options,
        read_store,
        events,
        InputRequestDiagnosticsSource::AuthoritativeEvents,
    )?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    super::super::record_authoritative_replay_state();
    Ok(result)
}

enum InputRequestDiagnosticsSource {
    AuthoritativeEvents,
    Materialized(Vec<ProjectionDiagnostic>),
}

fn list_input_requests_from_event_selection(
    options: InputRequestListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
    diagnostics_source: InputRequestDiagnosticsSource,
) -> Result<InputRequestListResult> {
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
    let input_requests = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
        );
        project_input_requests(InputRequestProjectionOptions {
            backend: read_store.backend(),
            events,
            resolved: &resolved,
            track_filter: track_filter.clone(),
            mode_filter: options.mode,
            file_filter: options.file.as_deref(),
            status_filter: options.status,
            include_body: options.include_body,
            read_for_display: false,
            removal_lens: &removal_lens,
        })?
    };
    let mut diagnostics = match diagnostics_source {
        InputRequestDiagnosticsSource::AuthoritativeEvents => {
            SessionState::from_events(events)?.diagnostics
        }
        InputRequestDiagnosticsSource::Materialized(diagnostics) => diagnostics,
    };
    diagnostics.extend(body_content_diagnostics(
        input_requests
            .iter()
            .map(|r| (r.body_content_state, r.body_content_hash.as_deref()))
            .chain(input_requests.iter().flat_map(|r| {
                r.responses.iter().map(|resp| {
                    (
                        resp.reason_content_state,
                        resp.reason_content_hash.as_deref(),
                    )
                })
            })),
    ));

    Ok(InputRequestListResult {
        revision_id: resolved.revision_id,
        filters: InputRequestListFilters {
            track_id: track_filter,
            mode: options.mode,
            file: options.file,
            status: options.status,
            include_body: options.include_body,
        },
        input_requests,
        diagnostics,
    })
}
