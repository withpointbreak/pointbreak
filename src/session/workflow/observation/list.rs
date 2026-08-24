use std::path::{Path, PathBuf};

use super::target::{CurrentRevisionContext, RevisionScope, RevisionSelection, resolve_revision};
use super::util::validated_track_id;
use super::view::{ObservationProjectionOptions, ObservationView, project_observations};
use crate::error::Result;
use crate::model::{RevisionId, TrackId};
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
pub struct ObservationListOptions {
    repo: PathBuf,
    revision_id: Option<RevisionId>,
    exact_revision_id: Option<RevisionId>,
    track: Option<String>,
    file: Option<String>,
    tags: Vec<String>,
    include_body: bool,
    trust_set: TrustSet,
    removal_policy: RemovalPolicy,
}

impl ObservationListOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            exact_revision_id: None,
            track: None,
            file: None,
            tags: Vec::new(),
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

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
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
pub struct ObservationListFilters {
    pub track_id: Option<TrackId>,
    pub file: Option<String>,
    pub tags: Vec<String>,
    pub include_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationListResult {
    pub revision_id: RevisionId,
    pub filters: ObservationListFilters,
    pub observations: Vec<ObservationView>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

pub fn list_observations(options: ObservationListOptions) -> Result<ObservationListResult> {
    let (read_store, events) = super::super::capable_read_store_and_events(&options.repo)?;
    list_observations_from_events(options, &read_store, &events)
}

#[doc(hidden)]
pub fn list_observations_with_public_read_context(
    options: ObservationListOptions,
    context: PublicReadCommandContextV1,
) -> Result<ObservationListResult> {
    if options.revision_id.is_some()
        || options.exact_revision_id.is_none()
        || options.track.is_none()
        || options.file.is_some()
        || !options.tags.is_empty()
        || options.include_body
    {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "public read context requires the exact qualified observation shape".to_owned(),
        });
    }
    context.require_repository(&options.repo)?;
    let revision_id = options
        .exact_revision_id
        .as_ref()
        .expect("qualified observation shape has an exact Revision")
        .clone();
    let access = DerivedHistoryAccess::from_public_read_store(context.read_store().clone())
        .map_err(crate::error::ShoreError::Message)?;
    match access
        .exact_revision_fact_read_v1(&revision_id)
        .map_err(crate::error::ShoreError::Message)?
    {
        ExactRevisionFactReadRouteV1::Ready(derived) => {
            super::super::complete_current_derived_fact_projection_v1(context, |store| {
                list_observations_from_event_selection(
                    options,
                    store,
                    &derived.events,
                    ObservationDiagnosticsSource::Materialized(derived.diagnostics),
                )
            })
        }
        ExactRevisionFactReadRouteV1::Off => {
            let reader =
                super::super::change_read::public_read_change_reader_v1(context, &options.repo)?;
            let result =
                list_observations_from_events(options, reader.read_store(), reader.events())?;
            reader.postflight()?;
            Ok(result)
        }
        ExactRevisionFactReadRouteV1::Unavailable => {
            let repo = options.repo.clone();
            super::super::complete_unavailable_fact_fallback_v1(context, &repo, |store, events| {
                list_observations_from_event_selection(
                    options,
                    store,
                    events,
                    ObservationDiagnosticsSource::AuthoritativeEvents,
                )
            })
        }
    }
}

fn list_observations_from_events(
    options: ObservationListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
) -> Result<ObservationListResult> {
    let result = list_observations_from_event_selection(
        options,
        read_store,
        events,
        ObservationDiagnosticsSource::AuthoritativeEvents,
    )?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    super::super::record_authoritative_replay_state();
    Ok(result)
}

enum ObservationDiagnosticsSource {
    AuthoritativeEvents,
    Materialized(Vec<ProjectionDiagnostic>),
}

fn list_observations_from_event_selection(
    options: ObservationListOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
    diagnostics_source: ObservationDiagnosticsSource,
) -> Result<ObservationListResult> {
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
    let observations = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
        );
        project_observations(ObservationProjectionOptions {
            backend: read_store.backend(),
            events,
            resolved: &resolved,
            track_filter: track_filter.clone(),
            file_filter: options.file.as_deref(),
            tag_filters: &options.tags,
            include_body: options.include_body,
            read_for_display: false,
            removal_lens: &removal_lens,
        })?
    };
    let mut diagnostics = match diagnostics_source {
        ObservationDiagnosticsSource::AuthoritativeEvents => {
            SessionState::from_events(events)?.diagnostics
        }
        ObservationDiagnosticsSource::Materialized(diagnostics) => diagnostics,
    };
    diagnostics.extend(body_content_diagnostics(
        observations
            .iter()
            .map(|o| (o.body_content_state, o.body_content_hash.as_deref())),
    ));

    Ok(ObservationListResult {
        revision_id: resolved.revision_id,
        filters: ObservationListFilters {
            track_id: track_filter,
            file: options.file,
            tags: options.tags,
            include_body: options.include_body,
        },
        observations,
        diagnostics,
    })
}
