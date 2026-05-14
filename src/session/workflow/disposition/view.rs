use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Result, ShoreError};
use crate::model::{
    DispositionId, EventId, InterventionId, ObservationId, ReviewTargetRef, TrackId,
};
use crate::session::body_artifact::load_body_artifact;
use crate::session::disposition::util::sorted_unique;
use crate::session::event::{
    EventType, ReviewDisposition, ReviewDispositionRecordedPayload, ShoreEvent, Writer,
};
use crate::session::observation::ResolvedReviewUnit;

pub(crate) struct DispositionProjectionOptions<'a> {
    pub shore_dir: &'a Path,
    pub events: &'a [ShoreEvent],
    pub resolved: &'a ResolvedReviewUnit,
    pub track_filter: Option<TrackId>,
    pub include_summary: bool,
    pub include_all: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentDispositionView {
    pub status: CurrentDispositionStatus,
    pub dispositions: Vec<DispositionView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentDispositionStatus {
    None,
    Resolved,
    Ambiguous,
}

impl CurrentDispositionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Resolved => "resolved",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispositionRecordStatus {
    Current,
    Replaced,
}

impl DispositionRecordStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Replaced => "replaced",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionView {
    pub id: DispositionId,
    pub event_id: EventId,
    pub track_id: TrackId,
    pub target: ReviewTargetRef,
    pub disposition: ReviewDisposition,
    pub summary: Option<String>,
    pub summary_content_hash: Option<String>,
    pub status: DispositionRecordStatus,
    pub replaces: Vec<DispositionId>,
    pub related_observations: Vec<ObservationId>,
    pub related_interventions: Vec<InterventionId>,
    pub overrides: Vec<ReviewTargetRef>,
    pub created_at: String,
    pub writer: Writer,
}

pub(crate) fn project_dispositions(
    options: DispositionProjectionOptions<'_>,
) -> Result<(CurrentDispositionView, Vec<DispositionView>)> {
    let records = collect_disposition_records(options.events, options.resolved)?;
    let replaced_ids = records
        .values()
        .flat_map(|record| record.payload.replaces_disposition_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut all_views = Vec::new();

    for record in records.into_values() {
        if options
            .track_filter
            .as_ref()
            .is_some_and(|filter| filter != &record.track_id)
        {
            continue;
        }

        let view = disposition_view_from_event(
            options.shore_dir,
            record.event,
            record.payload,
            record.track_id,
            &replaced_ids,
            options.include_summary,
        )?;
        all_views.push(view);
    }

    sort_disposition_views(&mut all_views);
    let current_dispositions = all_views
        .iter()
        .filter(|view| view.status == DispositionRecordStatus::Current)
        .cloned()
        .collect::<Vec<_>>();
    let current_status = match current_dispositions.len() {
        0 => CurrentDispositionStatus::None,
        1 => CurrentDispositionStatus::Resolved,
        _ => CurrentDispositionStatus::Ambiguous,
    };
    let dispositions = if options.include_all {
        all_views
    } else {
        current_dispositions.clone()
    };

    Ok((
        CurrentDispositionView {
            status: current_status,
            dispositions: current_dispositions,
        },
        dispositions,
    ))
}

struct DispositionEventRecord<'a> {
    event: &'a ShoreEvent,
    payload: ReviewDispositionRecordedPayload,
    track_id: TrackId,
}

fn collect_disposition_records<'a>(
    events: &'a [ShoreEvent],
    resolved: &ResolvedReviewUnit,
) -> Result<BTreeMap<DispositionId, DispositionEventRecord<'a>>> {
    let mut records: BTreeMap<DispositionId, DispositionEventRecord<'a>> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewDispositionRecorded)
    {
        if event.target.review_unit_id.as_ref() != Some(&resolved.review_unit_id) {
            continue;
        }

        let payload: ReviewDispositionRecordedPayload =
            serde_json::from_value(event.payload.clone())?;
        let track_id =
            event.target.track_id.clone().ok_or_else(|| {
                ShoreError::Message("disposition event missing track id".to_owned())
            })?;
        let disposition_id = payload.disposition_id.clone();
        // Duplicate semantic events are reported by the state reducer diagnostics. This
        // read model keeps one stable representative so replacement/current projection
        // stays bounded even if duplicate payloads diverge.
        let replace_record = records.get(&disposition_id).is_none_or(|record| {
            // Event IDs are deterministic storage addresses, not causal order. Pick the
            // lowest one only as a stable representative for duplicate semantic facts.
            event.event_id.as_str() < record.event.event_id.as_str()
        });
        if replace_record {
            records.insert(
                disposition_id,
                DispositionEventRecord {
                    event,
                    payload,
                    track_id,
                },
            );
        }
    }

    Ok(records)
}

fn disposition_view_from_event(
    shore_dir: &Path,
    event: &ShoreEvent,
    payload: ReviewDispositionRecordedPayload,
    track_id: TrackId,
    replaced_ids: &BTreeSet<DispositionId>,
    include_summary: bool,
) -> Result<DispositionView> {
    let summary = if include_summary {
        disposition_summary(shore_dir, &payload)?
    } else {
        None
    };
    let status = if replaced_ids.contains(&payload.disposition_id) {
        DispositionRecordStatus::Replaced
    } else {
        DispositionRecordStatus::Current
    };
    let replaces = sorted_unique(payload.replaces_disposition_ids);
    let related_observations = sorted_unique(payload.related_observation_ids);
    let related_interventions = sorted_unique(payload.related_intervention_ids);

    Ok(DispositionView {
        id: payload.disposition_id,
        event_id: event.event_id.clone(),
        track_id,
        target: payload.target,
        disposition: payload.disposition,
        summary,
        summary_content_hash: payload.summary_content_hash,
        status,
        replaces,
        related_observations,
        related_interventions,
        overrides: payload.overrides,
        created_at: event.occurred_at.clone(),
        writer: event.writer.clone(),
    })
}

fn disposition_summary(
    shore_dir: &Path,
    payload: &ReviewDispositionRecordedPayload,
) -> Result<Option<String>> {
    if payload.summary.is_some() {
        return Ok(payload.summary.clone());
    }
    match payload.summary_artifact_path.as_deref() {
        Some(path) => load_body_artifact(shore_dir, path),
        None => Ok(None),
    }
}

fn sort_disposition_views(dispositions: &mut [DispositionView]) {
    dispositions.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
    });
}
