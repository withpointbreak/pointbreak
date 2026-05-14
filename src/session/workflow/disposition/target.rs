use std::path::Path;

use crate::error::{Result, ShoreError};
use crate::model::{DispositionId, InterventionId, ObservationId, ReviewTargetRef, Side};
use crate::session::disposition::util::{sorted_unique, sorted_unique_targets};
use crate::session::event::{
    EventType, InterventionRequestedPayload, ReviewDisposition, ReviewDispositionRecordedPayload,
    ReviewObservationRecordedPayload, ShoreEvent,
};
use crate::session::observation::{
    ObservationTargetSelector, ResolvedReviewUnit, resolve_observation_target,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispositionTargetSelector {
    ReviewUnit,
    File {
        path: String,
    },
    Range {
        path: String,
        side: Side,
        start_line: u32,
        end_line: Option<u32>,
    },
    Observation {
        observation_id: ObservationId,
    },
    Intervention {
        intervention_id: InterventionId,
    },
    Disposition {
        disposition_id: DispositionId,
    },
}

impl DispositionTargetSelector {
    pub fn review_unit() -> Self {
        Self::ReviewUnit
    }

    pub fn file(path: impl Into<String>) -> Self {
        Self::File { path: path.into() }
    }

    pub fn range(
        path: impl Into<String>,
        side: Side,
        start_line: u32,
        end_line: Option<u32>,
    ) -> Self {
        Self::Range {
            path: path.into(),
            side,
            start_line,
            end_line,
        }
    }

    pub fn observation(observation_id: ObservationId) -> Self {
        Self::Observation { observation_id }
    }

    pub fn intervention(intervention_id: InterventionId) -> Self {
        Self::Intervention { intervention_id }
    }

    pub fn disposition(disposition_id: DispositionId) -> Self {
        Self::Disposition { disposition_id }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispositionRelationships {
    pub replaces_disposition_ids: Vec<DispositionId>,
    pub related_observation_ids: Vec<ObservationId>,
    pub related_intervention_ids: Vec<InterventionId>,
    pub overrides: Vec<DispositionOverrideSelector>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispositionOverrideSelector {
    Observation { observation_id: ObservationId },
    Intervention { intervention_id: InterventionId },
    Disposition { disposition_id: DispositionId },
}

impl DispositionOverrideSelector {
    pub fn observation(observation_id: ObservationId) -> Self {
        Self::Observation { observation_id }
    }

    pub fn intervention(intervention_id: InterventionId) -> Self {
        Self::Intervention { intervention_id }
    }

    pub fn disposition(disposition_id: DispositionId) -> Self {
        Self::Disposition { disposition_id }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDispositionTarget {
    pub target: ReviewTargetRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedDispositionRelationships {
    pub replaces_disposition_ids: Vec<DispositionId>,
    pub related_observation_ids: Vec<ObservationId>,
    pub related_intervention_ids: Vec<InterventionId>,
    pub overrides: Vec<ReviewTargetRef>,
}

pub(crate) fn resolve_disposition_target(
    repo: &Path,
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    selector: &DispositionTargetSelector,
) -> Result<ResolvedDispositionTarget> {
    let target = match selector {
        DispositionTargetSelector::ReviewUnit => {
            resolve_observation_target(repo, resolved, &ObservationTargetSelector::review_unit())?
        }
        DispositionTargetSelector::File { path } => {
            resolve_observation_target(repo, resolved, &ObservationTargetSelector::file(path))?
        }
        DispositionTargetSelector::Range {
            path,
            side,
            start_line,
            end_line,
        } => resolve_observation_target(
            repo,
            resolved,
            &ObservationTargetSelector::range(path, *side, *start_line, *end_line),
        )?,
        DispositionTargetSelector::Observation { observation_id } => {
            resolve_observation_ref(events, resolved, observation_id)?
        }
        DispositionTargetSelector::Intervention { intervention_id } => {
            resolve_intervention_ref(events, resolved, intervention_id)?
        }
        DispositionTargetSelector::Disposition { disposition_id } => {
            resolve_disposition_ref(events, resolved, disposition_id)?
        }
    };

    Ok(ResolvedDispositionTarget { target })
}

pub(crate) fn resolve_disposition_relationships(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    relationships: &DispositionRelationships,
    disposition: ReviewDisposition,
    summary: Option<&str>,
) -> Result<ResolvedDispositionRelationships> {
    if disposition == ReviewDisposition::Overridden {
        if summary.is_none_or(|summary| summary.trim().is_empty()) {
            return Err(ShoreError::Message(
                "summary is required for overridden disposition".to_owned(),
            ));
        }
        if relationships.overrides.is_empty() {
            return Err(ShoreError::Message(
                "override reference is required for overridden disposition".to_owned(),
            ));
        }
    }

    for observation_id in &relationships.related_observation_ids {
        resolve_observation_ref(events, resolved, observation_id)?;
    }
    for intervention_id in &relationships.related_intervention_ids {
        resolve_intervention_ref(events, resolved, intervention_id)?;
    }
    for disposition_id in &relationships.replaces_disposition_ids {
        resolve_disposition_ref(events, resolved, disposition_id)?;
    }

    let mut overrides = Vec::with_capacity(relationships.overrides.len());
    for override_selector in &relationships.overrides {
        overrides.push(resolve_override_ref(events, resolved, override_selector)?);
    }

    Ok(ResolvedDispositionRelationships {
        replaces_disposition_ids: sorted_unique(relationships.replaces_disposition_ids.clone()),
        related_observation_ids: sorted_unique(relationships.related_observation_ids.clone()),
        related_intervention_ids: sorted_unique(relationships.related_intervention_ids.clone()),
        overrides: sorted_unique_targets(overrides)?,
    })
}

fn resolve_override_ref(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    selector: &DispositionOverrideSelector,
) -> Result<ReviewTargetRef> {
    match selector {
        DispositionOverrideSelector::Observation { observation_id } => {
            resolve_observation_ref(events, resolved, observation_id)
        }
        DispositionOverrideSelector::Intervention { intervention_id } => {
            resolve_intervention_ref(events, resolved, intervention_id)
        }
        DispositionOverrideSelector::Disposition { disposition_id } => {
            resolve_disposition_ref(events, resolved, disposition_id)
        }
    }
}

fn resolve_observation_ref(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    observation_id: &ObservationId,
) -> Result<ReviewTargetRef> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewObservationRecorded)
    {
        if event.target.review_unit_id.as_ref() != Some(&resolved.review_unit_id) {
            continue;
        }

        let payload: ReviewObservationRecordedPayload =
            serde_json::from_value(event.payload.clone())?;
        if &payload.observation_id == observation_id {
            return Ok(ReviewTargetRef::Observation {
                review_unit_id: resolved.review_unit_id.clone(),
                observation_id: observation_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "unknown observation target: {}",
        observation_id.as_str()
    )))
}

fn resolve_intervention_ref(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    intervention_id: &InterventionId,
) -> Result<ReviewTargetRef> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::InterventionRequested)
    {
        if event.target.review_unit_id.as_ref() != Some(&resolved.review_unit_id) {
            continue;
        }

        let payload: InterventionRequestedPayload = serde_json::from_value(event.payload.clone())?;
        if &payload.intervention_id == intervention_id {
            return Ok(ReviewTargetRef::Intervention {
                review_unit_id: resolved.review_unit_id.clone(),
                intervention_id: intervention_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "unknown intervention target: {}",
        intervention_id.as_str()
    )))
}

fn resolve_disposition_ref(
    events: &[ShoreEvent],
    resolved: &ResolvedReviewUnit,
    disposition_id: &DispositionId,
) -> Result<ReviewTargetRef> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewDispositionRecorded)
    {
        if event.target.review_unit_id.as_ref() != Some(&resolved.review_unit_id) {
            continue;
        }

        let payload: ReviewDispositionRecordedPayload =
            serde_json::from_value(event.payload.clone())?;
        if &payload.disposition_id == disposition_id {
            return Ok(ReviewTargetRef::Disposition {
                review_unit_id: resolved.review_unit_id.clone(),
                disposition_id: disposition_id.clone(),
            });
        }
    }

    Err(ShoreError::Message(format!(
        "unknown disposition target: {}",
        disposition_id.as_str()
    )))
}
