use std::path::{Path, PathBuf};

use super::{
    AssessmentProjectionOptions, AssessmentView, CurrentAssessmentView, project_assessments,
};
use crate::error::Result;
use crate::model::{RevisionId, TrackId};
use crate::session::event::ShoreEvent;
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
pub struct AssessmentShowOptions {
    pub(super) repo: PathBuf,
    pub(super) revision_id: Option<RevisionId>,
    pub(super) exact_revision_id: Option<RevisionId>,
    pub(super) track: Option<String>,
    pub(super) include_summary: bool,
    pub(super) include_all: bool,
    pub(super) trust_set: TrustSet,
    pub(super) removal_policy: RemovalPolicy,
}

impl AssessmentShowOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            exact_revision_id: None,
            track: None,
            include_summary: false,
            include_all: false,
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

    pub fn with_include_summary(mut self, include_summary: bool) -> Self {
        self.include_summary = include_summary;
        self
    }

    pub fn with_all(mut self, include_all: bool) -> Self {
        self.include_all = include_all;
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
pub struct AssessmentShowResult {
    pub revision_id: RevisionId,
    pub filters: AssessmentShowFilters,
    pub current: CurrentAssessmentView,
    pub assessments: Vec<AssessmentView>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssessmentShowFilters {
    pub track_id: Option<TrackId>,
    pub include_summary: bool,
    pub include_all: bool,
}

pub fn show_assessments(options: AssessmentShowOptions) -> Result<AssessmentShowResult> {
    let (read_store, events) = super::super::capable_read_store_and_events(&options.repo)?;
    show_assessments_from_events(options, &read_store, &events)
}

#[doc(hidden)]
pub fn show_assessments_with_public_read_context(
    options: AssessmentShowOptions,
    context: PublicReadCommandContextV1,
) -> Result<AssessmentShowResult> {
    if options.revision_id.is_some()
        || options.exact_revision_id.is_none()
        || options.track.is_none()
        || options.include_all
    {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "public read context requires the exact qualified assessment shape".to_owned(),
        });
    }
    let reader = super::super::change_read::public_read_change_reader_v1(context, &options.repo)?;
    let result = show_assessments_from_events(options, reader.read_store(), reader.events())?;
    reader.postflight()?;
    Ok(result)
}

fn show_assessments_from_events(
    options: AssessmentShowOptions,
    read_store: &ReadStore,
    events: &[ShoreEvent],
) -> Result<AssessmentShowResult> {
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
    let (current, assessments) = {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _phase = crate::bench_support::longitudinal::enter_derived_access_phase_v1(
            crate::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::RouteProjectionFold,
        );
        project_assessments(AssessmentProjectionOptions {
            backend: Some(read_store.backend()),
            events,
            resolved: &resolved,
            track_filter: track_filter.clone(),
            include_summary: options.include_summary,
            include_all: options.include_all,
            read_for_display: false,
            removal_lens: Some(&removal_lens),
        })?
    };
    #[cfg(any(test, feature = "longitudinal-counting"))]
    super::super::record_authoritative_replay_state();
    let mut diagnostics = SessionState::from_events(events)?.diagnostics;
    diagnostics.extend(body_content_diagnostics(
        assessments
            .iter()
            .map(|a| (a.summary_content_state, a.summary_content_hash.as_deref())),
    ));

    Ok(AssessmentShowResult {
        revision_id: resolved.revision_id,
        filters: AssessmentShowFilters {
            track_id: track_filter,
            include_summary: options.include_summary,
            include_all: options.include_all,
        },
        current,
        assessments,
        diagnostics,
    })
}
