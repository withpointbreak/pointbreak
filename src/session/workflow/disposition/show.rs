use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::model::{ReviewUnitId, TrackId};
use crate::session::EventStore;
use crate::session::disposition::{
    CurrentDispositionView, DispositionProjectionOptions, DispositionView, project_dispositions,
};
use crate::session::observation::{resolve_review_unit, validated_track_id};
use crate::session::state::{ProjectionDiagnostic, SessionState};
use crate::session::store_init::ShoreStorePaths;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionShowOptions {
    pub(super) repo: PathBuf,
    pub(super) review_unit_id: Option<ReviewUnitId>,
    pub(super) track: Option<String>,
    pub(super) include_summary: bool,
    pub(super) include_all: bool,
}

impl DispositionShowOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            review_unit_id: None,
            track: None,
            include_summary: false,
            include_all: false,
        }
    }

    pub fn with_review_unit_id(mut self, id: ReviewUnitId) -> Self {
        self.review_unit_id = Some(id);
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionShowResult {
    pub review_unit_id: ReviewUnitId,
    pub filters: DispositionShowFilters,
    pub current: CurrentDispositionView,
    pub dispositions: Vec<DispositionView>,
    /// Diagnostics come from the full replayed event set, not only the filtered ReviewUnit.
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionShowFilters {
    pub track_id: Option<TrackId>,
    pub include_summary: bool,
    pub include_all: bool,
}

pub fn show_dispositions(options: DispositionShowOptions) -> Result<DispositionShowResult> {
    let paths = ShoreStorePaths::resolve(&options.repo)?;
    let shore_dir = paths.shore_dir();
    let events = EventStore::open(shore_dir).list_events()?;
    let resolved = resolve_review_unit(&events, options.review_unit_id.as_ref())?;
    let track_filter = options
        .track
        .as_deref()
        .map(validated_track_id)
        .transpose()?;
    let (current, dispositions) = project_dispositions(DispositionProjectionOptions {
        shore_dir,
        events: &events,
        resolved: &resolved,
        track_filter: track_filter.clone(),
        include_summary: options.include_summary,
        include_all: options.include_all,
    })?;
    // Reuse the state reducer for diagnostics so duplicate/corrupt-event policy stays
    // shared with state.json and other readers; row filtering is disposition-local.
    let diagnostics = SessionState::from_events(&events)?.diagnostics;

    Ok(DispositionShowResult {
        review_unit_id: resolved.review_unit_id,
        filters: DispositionShowFilters {
            track_id: track_filter,
            include_summary: options.include_summary,
            include_all: options.include_all,
        },
        current,
        dispositions,
        diagnostics,
    })
}
