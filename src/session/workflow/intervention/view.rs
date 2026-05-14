use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Result, ShoreError};
use crate::model::{EventId, InterventionId, InterventionResolutionId, ReviewTargetRef, TrackId};
use crate::session::body_artifact::load_body_artifact;
use crate::session::event::{
    EventType, InterventionMode, InterventionReasonCode, InterventionRequestedPayload,
    InterventionResolutionOutcome, InterventionResolvedPayload, ShoreEvent, Writer,
};
use crate::session::observation::{ResolvedReviewUnit, target_matches_file};

pub(crate) struct InterventionProjectionOptions<'a> {
    pub shore_dir: &'a Path,
    pub events: &'a [ShoreEvent],
    pub resolved: &'a ResolvedReviewUnit,
    pub track_filter: Option<TrackId>,
    pub mode_filter: Option<InterventionMode>,
    pub file_filter: Option<&'a str>,
    pub status_filter: InterventionStatusFilter,
    pub include_body: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterventionView {
    pub id: InterventionId,
    pub event_id: EventId,
    pub track_id: TrackId,
    pub target: ReviewTargetRef,
    pub mode: InterventionMode,
    pub reason_code: InterventionReasonCode,
    pub title: String,
    pub body: Option<String>,
    pub body_content_hash: Option<String>,
    pub status: InterventionStatus,
    pub resolutions: Vec<InterventionResolutionView>,
    pub created_at: String,
    pub writer: Writer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterventionResolutionView {
    pub id: InterventionResolutionId,
    pub event_id: EventId,
    pub outcome: InterventionResolutionOutcome,
    pub reason: Option<String>,
    pub reason_content_hash: Option<String>,
    pub created_at: String,
    pub writer: Writer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterventionStatus {
    Open,
    Resolved,
    Ambiguous,
}

impl InterventionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterventionStatusFilter {
    Open,
    Resolved,
    Ambiguous,
    All,
}

impl InterventionStatusFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Ambiguous => "ambiguous",
            Self::All => "all",
        }
    }

    pub(super) fn matches(self, status: InterventionStatus) -> bool {
        match self {
            Self::Open => status == InterventionStatus::Open,
            Self::Resolved => status == InterventionStatus::Resolved,
            Self::Ambiguous => status == InterventionStatus::Ambiguous,
            Self::All => true,
        }
    }
}

pub(crate) fn project_interventions(
    options: InterventionProjectionOptions<'_>,
) -> Result<Vec<InterventionView>> {
    let request_records = collect_request_records(options.events)?;
    let resolutions = collect_resolution_views(options.events)?;
    let mut interventions = Vec::new();

    for record in request_records.into_values() {
        let event = record.event;
        if event.target.review_unit_id.as_ref() != Some(&options.resolved.review_unit_id) {
            continue;
        }

        if options
            .track_filter
            .as_ref()
            .is_some_and(|filter| filter != &record.track_id)
        {
            continue;
        }
        if options
            .mode_filter
            .is_some_and(|mode| mode != record.payload.mode)
        {
            continue;
        }
        if options
            .file_filter
            .is_some_and(|file| !target_matches_file(&record.payload.target, file))
        {
            continue;
        }

        let intervention_id = record.payload.intervention_id.clone();
        let resolutions = resolutions
            .get(&intervention_id)
            .cloned()
            .unwrap_or_default();
        let view = intervention_view_from_event(
            options.shore_dir,
            event,
            record.payload,
            record.track_id,
            resolutions,
            options.include_body,
        )?;
        if options.status_filter.matches(view.status) {
            interventions.push(view);
        }
    }

    sort_intervention_views(&mut interventions);
    Ok(interventions)
}

pub(super) struct InterventionRequestRecord<'a> {
    pub(super) event: &'a ShoreEvent,
    pub(super) payload: InterventionRequestedPayload,
    pub(super) track_id: TrackId,
}

struct InterventionResolutionRecord<'a> {
    event: &'a ShoreEvent,
    payload: InterventionResolvedPayload,
}

pub(super) fn collect_request_records<'a>(
    events: &'a [ShoreEvent],
) -> Result<BTreeMap<InterventionId, InterventionRequestRecord<'a>>> {
    let mut records: BTreeMap<InterventionId, InterventionRequestRecord<'a>> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::InterventionRequested)
    {
        let payload: InterventionRequestedPayload = serde_json::from_value(event.payload.clone())?;
        let track_id =
            event.target.track_id.clone().ok_or_else(|| {
                ShoreError::Message("intervention event missing track id".to_owned())
            })?;
        let intervention_id = payload.intervention_id.clone();
        let replace_record = records.get(&intervention_id).is_none_or(|record| {
            // Event IDs are deterministic storage addresses, not causal order. Pick the
            // lowest one only as a stable representative for duplicate semantic facts.
            event.event_id.as_str() < record.event.event_id.as_str()
        });
        if replace_record {
            records.insert(
                intervention_id,
                InterventionRequestRecord {
                    event,
                    payload,
                    track_id,
                },
            );
        }
    }

    Ok(records)
}

pub(super) fn collect_resolution_views(
    events: &[ShoreEvent],
) -> Result<BTreeMap<InterventionId, Vec<InterventionResolutionView>>> {
    let mut resolution_records: BTreeMap<
        InterventionResolutionId,
        InterventionResolutionRecord<'_>,
    > = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::InterventionResolved)
    {
        let payload: InterventionResolvedPayload = serde_json::from_value(event.payload.clone())?;
        let resolution_id = payload.intervention_resolution_id.clone();
        let replace_record = resolution_records.get(&resolution_id).is_none_or(|record| {
            // Event IDs are deterministic storage addresses, not causal order. Pick the
            // lowest one only as a stable representative for duplicate semantic facts.
            event.event_id.as_str() < record.event.event_id.as_str()
        });
        if replace_record {
            resolution_records.insert(
                resolution_id,
                InterventionResolutionRecord { event, payload },
            );
        }
    }

    let mut resolutions: BTreeMap<InterventionId, Vec<InterventionResolutionView>> =
        BTreeMap::new();
    for record in resolution_records.into_values() {
        let event = record.event;
        let payload = record.payload;
        resolutions
            .entry(payload.intervention_id)
            .or_default()
            .push(InterventionResolutionView {
                id: payload.intervention_resolution_id,
                event_id: event.event_id.clone(),
                outcome: payload.outcome,
                reason: payload.reason,
                reason_content_hash: payload.reason_content_hash,
                created_at: event.occurred_at.clone(),
                writer: event.writer.clone(),
            });
    }

    for resolution_views in resolutions.values_mut() {
        sort_resolution_views(resolution_views);
    }

    Ok(resolutions)
}

pub(super) fn intervention_view_from_event(
    shore_dir: &Path,
    event: &ShoreEvent,
    payload: InterventionRequestedPayload,
    track_id: TrackId,
    resolutions: Vec<InterventionResolutionView>,
    include_body: bool,
) -> Result<InterventionView> {
    let body = if include_body {
        intervention_body(shore_dir, &payload)?
    } else {
        None
    };
    let status = status_for_resolutions(&resolutions);

    Ok(InterventionView {
        id: payload.intervention_id,
        event_id: event.event_id.clone(),
        track_id,
        target: payload.target,
        mode: payload.mode,
        reason_code: payload.reason_code,
        title: payload.title,
        body,
        body_content_hash: payload.body_content_hash,
        status,
        resolutions,
        created_at: event.occurred_at.clone(),
        writer: event.writer.clone(),
    })
}

fn intervention_body(
    shore_dir: &Path,
    payload: &InterventionRequestedPayload,
) -> Result<Option<String>> {
    if payload.body.is_some() {
        return Ok(payload.body.clone());
    }
    match payload.body_artifact_path.as_deref() {
        Some(path) => load_body_artifact(shore_dir, path),
        None => Ok(None),
    }
}

fn status_for_resolutions(resolutions: &[InterventionResolutionView]) -> InterventionStatus {
    match resolutions.len() {
        0 => InterventionStatus::Open,
        1 => InterventionStatus::Resolved,
        _ => InterventionStatus::Ambiguous,
    }
}

pub(super) fn sort_intervention_views(interventions: &mut [InterventionView]) {
    interventions.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
    });
}

fn sort_resolution_views(resolutions: &mut [InterventionResolutionView]) {
    resolutions.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
    });
}
