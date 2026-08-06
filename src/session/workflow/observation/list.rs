use std::path::{Path, PathBuf};

use super::target::{CurrentRevisionContext, RevisionScope, RevisionSelection, resolve_revision};
use super::util::validated_track_id;
use super::view::{ObservationProjectionOptions, ObservationView, project_observations};
use crate::error::Result;
use crate::model::{RevisionId, TrackId};
use crate::session::ArtifactRemovalProjection;
use crate::session::projection::body_content::{BodyRemovalLens, body_content_diagnostics};
use crate::session::projection::cosignature::CosignatureIndex;
use crate::session::signing::{RemovalPolicy, TrustSet};
use crate::session::state::{ProjectionDiagnostic, SessionState};

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
    let resolved = resolve_revision(
        &events,
        RevisionSelection::from_revision_options(
            options.revision_id.as_ref(),
            options.exact_revision_id.as_ref(),
        )?,
        &CurrentRevisionContext::for_repo(&options.repo)?,
        RevisionScope::default(),
    )?;
    let track_filter = options
        .track
        .as_deref()
        .map(validated_track_id)
        .transpose()?;
    let removal = ArtifactRemovalProjection::from_events(&events)?;
    let cosig_index = CosignatureIndex::build(&events)?;
    let removal_lens = BodyRemovalLens::new(
        &removal,
        &options.trust_set,
        options.removal_policy,
        &cosig_index,
    );
    let observations = project_observations(ObservationProjectionOptions {
        backend: read_store.backend(),
        events: &events,
        resolved: &resolved,
        track_filter: track_filter.clone(),
        file_filter: options.file.as_deref(),
        tag_filters: &options.tags,
        include_body: options.include_body,
        removal_lens: &removal_lens,
    })?;
    let mut diagnostics = SessionState::from_events(&events)?.diagnostics;
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
